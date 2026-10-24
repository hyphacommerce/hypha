use std::collections::{HashMap, HashSet};

use hypha_core::{
    AuthError, Ctx, Currency, DiscountId, ErrorCode, HasBatch, HasClock, IdGenerator, MarketId,
    OrderId, Page, PageRequest, PendingEvent, Random, RepoError, Scope, StageEvents, Subject,
    Timestamp,
};
use thiserror::Error;

use crate::{
    code::{DiscountCode, Generation, MAX_ADDED, batch_label, normalize},
    discount::{
        ClassSet, Condition, Discount, DiscountDraft, Kind, Limits, MAX_LIVE_AUTOMATIC, Method,
        Status,
    },
    error::{CodeError, DiscountError, Problem},
    events::{DISCOUNT, DiscountCodesAdded, DiscountCodesRemoved, DiscountCodesUpdated},
    repo::{
        CodeFilter, CodeSelection, DiscountFilter, DiscountRepo, HasDiscounts, StageDiscounts,
        Usage, keys,
    },
};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Discount(#[from] DiscountError),
    #[error(transparent)]
    Code(#[from] CodeError),
    #[error("discount {0} not found")]
    NotFound(DiscountId),
    /// Another discount has the code (or the request lists it twice).
    #[error("the code {0} is taken")]
    CodeTaken(String),
    #[error("at most {MAX_LIVE_AUTOMATIC} automatic discounts can be active and not expired")]
    TooManyAutomatic,
    /// Codes belong to discounts by code.
    #[error("discount {0} is automatic: it has no codes")]
    AutomaticHasNoCodes(DiscountId),
    #[error("at most {MAX_ADDED} codes per call")]
    TooManyCodes,
    /// Drawing kept hitting codes that exist: a longer length or another prefix leaves
    /// more room.
    #[error("no room for {0} more codes of that length and prefix")]
    CodeSpaceExhausted(usize),
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Discount(e) => e.code(),
            CommandError::Code(e) => e.code(),
            CommandError::NotFound(_) => "discount_not_found",
            CommandError::CodeTaken(_) => "discount_code_taken",
            CommandError::TooManyAutomatic => "too_many_automatic_discounts",
            CommandError::AutomaticHasNoCodes(_) => "discount_automatic",
            CommandError::TooManyCodes => "too_many_codes",
            CommandError::CodeSpaceExhausted(_) => "code_space_exhausted",
        }
    }
}

/// What writing discounts needs: the repo, the clock, and a batch for a discount and its
/// codes together.
pub trait DiscountPorts:
    HasDiscounts + HasClock + HasBatch<Batch: StageDiscounts + StageEvents>
{
}

impl<T> DiscountPorts for T where
    T: HasDiscounts + HasClock + HasBatch<Batch: StageDiscounts + StageEvents> + ?Sized
{
}

/// A new code as a caller gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCode {
    pub code: String,
    pub usage_limit: Option<u32>,
}

pub struct CreateDiscount {
    pub draft: DiscountDraft,
    /// Codes added with it (a discount by code only), at most `MAX_ADDED`.
    pub codes: Vec<NewCode>,
    /// Each market's currency: what amounts are checked against.
    pub currencies: HashMap<MarketId, Currency>,
}

/// Creates a discount, with its first codes in the same write.
pub async fn create_discount<S: DiscountPorts>(
    ctx: &Ctx,
    ports: &S,
    ids: &impl IdGenerator,
    input: CreateDiscount,
) -> Result<Discount, CommandError> {
    ctx.require(Scope::DiscountsWrite)?;
    let mut discount = Discount::new(DiscountId::from_uuid(ids.new_uuid()), input.draft)?;
    discount.check_currencies(|m| input.currencies.get(&m).copied())?;
    if !input.codes.is_empty() && discount.method() == Method::Automatic {
        return Err(CommandError::AutomaticHasNoCodes(discount.id()));
    }
    let now = ports.now();
    check_automatic_room(ports, &discount, now, None).await?;
    let codes = new_codes(ports, discount.id(), &input.codes, None).await?;

    let mut batch = ports.batch();
    batch.insert_discount(&mut discount);
    if !codes.is_empty() {
        batch.add_discount_codes(&codes);
        batch.record_events(vec![codes_added(discount.id(), codes.len(), None)]);
    }
    ports
        .commit(batch)
        .await
        .map_err(|e| code_taken(e, &codes))?;
    Ok(discount)
}

/// Refuses `discount` when it would be one automatic discount too many at `now`.
/// `before` is the stored version of a discount being changed.
async fn check_automatic_room<S: HasDiscounts>(
    ports: &S,
    discount: &Discount,
    now: Timestamp,
    before: Option<&Discount>,
) -> Result<(), CommandError> {
    if !discount.counts_as_live_automatic(now)
        || before.is_some_and(|b| b.counts_as_live_automatic(now))
    {
        return Ok(());
    }
    let live = ports.discounts().count_live_automatic(now).await?;
    if live >= MAX_LIVE_AUTOMATIC as u64 {
        return Err(CommandError::TooManyAutomatic);
    }
    Ok(())
}

/// A duplicate code from the write reads like one found before it: someone took it
/// meanwhile.
fn code_taken(e: RepoError, codes: &[DiscountCode]) -> CommandError {
    match e {
        RepoError::Duplicate(keys::DISCOUNT_CODE) => {
            CommandError::CodeTaken(codes.first().map(|c| c.code.clone()).unwrap_or_default())
        }
        e => e.into(),
    }
}

fn codes_added(discount: DiscountId, count: usize, batch: Option<String>) -> PendingEvent {
    PendingEvent::new(
        Subject::new(DISCOUNT, discount.as_uuid()),
        &DiscountCodesAdded {
            discount,
            count: count as u64,
            batch,
        },
    )
}

/// Checks new codes: each valid, listed once, and not taken.
async fn new_codes<S: HasDiscounts>(
    ports: &S,
    discount: DiscountId,
    given: &[NewCode],
    batch: Option<String>,
) -> Result<Vec<DiscountCode>, CommandError> {
    if given.len() > MAX_ADDED {
        return Err(CommandError::TooManyCodes);
    }
    let batch = batch_label(batch)?;
    let mut codes = Vec::with_capacity(given.len());
    let mut seen = HashSet::new();
    for c in given {
        let code = DiscountCode::new(discount, &c.code, c.usage_limit, batch.clone())?;
        if !seen.insert(code.normalized.clone()) {
            return Err(CommandError::CodeTaken(code.code));
        }
        codes.push(code);
    }
    if codes.is_empty() {
        return Ok(codes);
    }
    let normalized: Vec<String> = codes.iter().map(|c| c.normalized.clone()).collect();
    if let Some(taken) = ports.discounts().find_codes(&normalized).await?.first() {
        let code = codes
            .iter()
            .find(|c| c.normalized == taken.normalized)
            .map_or(taken.code.clone(), |c| c.code.clone());
        return Err(CommandError::CodeTaken(code));
    }
    Ok(codes)
}

pub async fn get_discount<S: HasDiscounts>(
    ctx: &Ctx,
    ports: &S,
    id: DiscountId,
) -> Result<Discount, CommandError> {
    ctx.require(Scope::DiscountsRead)?;
    ports
        .discounts()
        .get(id)
        .await?
        .ok_or(CommandError::NotFound(id))
}

/// A page of discounts, newest first. `filter.code` and `filter.q` are normalised here.
pub async fn list_discounts<S: HasDiscounts>(
    ctx: &Ctx,
    ports: &S,
    mut filter: DiscountFilter,
    page: PageRequest<DiscountId>,
) -> Result<Page<Discount>, CommandError> {
    ctx.require(Scope::DiscountsRead)?;
    filter.code = filter.code.as_deref().map(normalize).transpose()?;
    filter.q = filter
        .q
        .map(|q| q.trim().to_string())
        .filter(|q| !q.is_empty());
    Ok(ports.discounts().list(&filter, page).await?)
}

/// Changes to a discount, as JSON Merge Patch: `None` keeps a field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscountPatch {
    pub title: Option<String>,
    pub status: Option<Status>,
    pub starts_at: Option<Option<Timestamp>>,
    pub ends_at: Option<Option<Timestamp>>,
    pub conditions: Option<Vec<Condition>>,
    pub kind: Option<Kind>,
    pub combines_with: Option<ClassSet>,
    pub limits: Option<Limits>,
}

impl DiscountPatch {
    /// `draft` with the patch's fields in place of its own.
    pub fn apply_to(self, mut draft: DiscountDraft) -> DiscountDraft {
        if let Some(title) = self.title {
            draft.title = title;
        }
        if let Some(status) = self.status {
            draft.status = status;
        }
        if let Some(starts_at) = self.starts_at {
            draft.starts_at = starts_at;
        }
        if let Some(ends_at) = self.ends_at {
            draft.ends_at = ends_at;
        }
        if let Some(conditions) = self.conditions {
            draft.conditions = conditions;
        }
        if let Some(kind) = self.kind {
            draft.kind = kind;
        }
        if let Some(combines_with) = self.combines_with {
            draft.combines_with = combines_with;
        }
        if let Some(limits) = self.limits {
            draft.limits = limits;
        }
        draft
    }
}

/// What a discount being written would read as, writing nothing: the discount it would
/// be (for the caller to summarise with the names it may show), or every rule it breaks (amounts checked against `currencies`): a new discount's draft, or a
/// saved one's with a patch over it.
pub async fn summarize<S: HasDiscounts>(
    ctx: &Ctx,
    ports: &S,
    draft: SummaryDraft,
    currencies: &HashMap<MarketId, Currency>,
) -> Result<Result<Discount, Vec<Problem>>, CommandError> {
    ctx.require(Scope::DiscountsRead)?;
    let (id, draft) = match draft {
        // It has no ID yet, and a summary doesn't show one.
        SummaryDraft::New(draft) => (DiscountId::from_uuid(Default::default()), draft),
        SummaryDraft::Saved(id, patch) => {
            let saved = ports
                .discounts()
                .get(id)
                .await?
                .ok_or(CommandError::NotFound(id))?;
            (id, patch.apply_to(saved.draft()))
        }
    };
    let checked = Discount::new(id, draft).and_then(|d| {
        d.check_currencies(|m| currencies.get(&m).copied())
            .map(|()| d)
    });
    let e = match checked {
        Ok(discount) => return Ok(Ok(discount)),
        Err(e) => e,
    };
    let path = match &e {
        DiscountError::Invalid(problems) => return Ok(Err(problems.clone())),
        DiscountError::MarketNotFound { path, .. }
        | DiscountError::CurrencyMismatch { path, .. } => path.clone(),
        DiscountError::MethodFixed => return Err(e.into()),
    };
    Ok(Err(vec![Problem {
        path,
        code: e.code(),
        message: e.to_string(),
    }]))
}

/// What `summarize` reads: a new discount, or changes to a saved one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SummaryDraft {
    New(DiscountDraft),
    Saved(DiscountId, DiscountPatch),
}

pub async fn update_discount<S: HasDiscounts + HasClock>(
    ctx: &Ctx,
    ports: &S,
    id: DiscountId,
    patch: DiscountPatch,
    currencies: &HashMap<MarketId, Currency>,
) -> Result<Discount, CommandError> {
    ctx.require(Scope::DiscountsWrite)?;
    let mut discount = ports
        .discounts()
        .get(id)
        .await?
        .ok_or(CommandError::NotFound(id))?;
    let before = discount.clone();
    discount.update(patch.apply_to(discount.draft()))?;
    if !discount.is_changed() {
        return Ok(discount);
    }
    discount.check_currencies(|m| currencies.get(&m).copied())?;
    check_automatic_room(ports, &discount, ports.now(), Some(&before)).await?;
    ports.discounts().update(&mut discount).await?;
    Ok(discount)
}

/// Deletes a discount with its codes and usage records. Orders keep their copies of what
/// it took off.
pub async fn delete_discount<S: HasDiscounts>(
    ctx: &Ctx,
    ports: &S,
    id: DiscountId,
) -> Result<(), CommandError> {
    ctx.require(Scope::DiscountsWrite)?;
    let mut discount = ports
        .discounts()
        .get(id)
        .await?
        .ok_or(CommandError::NotFound(id))?;
    discount.delete();
    ports.discounts().delete(&mut discount).await?;
    Ok(())
}

/// The discount for a codes command: it exists and goes by codes.
async fn coded<S: HasDiscounts>(ports: &S, id: DiscountId) -> Result<Discount, CommandError> {
    let discount = ports
        .discounts()
        .get(id)
        .await?
        .ok_or(CommandError::NotFound(id))?;
    if discount.method() != Method::Codes {
        return Err(CommandError::AutomaticHasNoCodes(id));
    }
    Ok(discount)
}

/// A page of a discount's codes, by code.
pub async fn list_codes<S: HasDiscounts>(
    ctx: &Ctx,
    ports: &S,
    id: DiscountId,
    mut filter: CodeFilter,
    page: PageRequest<String>,
) -> Result<Page<DiscountCode>, CommandError> {
    ctx.require(Scope::DiscountsRead)?;
    ports
        .discounts()
        .get(id)
        .await?
        .ok_or(CommandError::NotFound(id))?;
    filter.q = match filter.q.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(q) => Some(normalize(q)?),
    };
    Ok(ports.discounts().code_page(id, &filter, page).await?)
}

/// Adds listed codes (at most `MAX_ADDED`), all or none.
pub async fn add_codes<S: DiscountPorts>(
    ctx: &Ctx,
    ports: &S,
    id: DiscountId,
    codes: Vec<NewCode>,
    batch: Option<String>,
) -> Result<Vec<DiscountCode>, CommandError> {
    ctx.require(Scope::DiscountsWrite)?;
    coded(ports, id).await?;
    let codes = new_codes(ports, id, &codes, batch.clone()).await?;
    if codes.is_empty() {
        return Ok(codes);
    }
    let mut write = ports.batch();
    write.add_discount_codes(&codes);
    write.record_events(vec![codes_added(id, codes.len(), batch_label(batch)?)]);
    ports
        .commit(write)
        .await
        .map_err(|e| code_taken(e, &codes))?;
    Ok(codes)
}

/// How many times drawing may hit taken codes before giving up.
const DRAWS: usize = 10;

/// Generates `generation.count` new codes, each `usage_limit` uses, under `batch`. Codes
/// drawn twice or taken already are drawn again.
pub async fn generate_codes<S: DiscountPorts>(
    ctx: &Ctx,
    ports: &S,
    random: &impl Random,
    id: DiscountId,
    generation: Generation,
    usage_limit: Option<u32>,
    batch: Option<String>,
) -> Result<Vec<DiscountCode>, CommandError> {
    ctx.require(Scope::DiscountsWrite)?;
    let generation = generation.check()?;
    if usage_limit == Some(0) {
        return Err(CodeError::UsageLimit.into());
    }
    let batch = batch_label(batch)?;
    coded(ports, id).await?;
    let mut codes: Vec<DiscountCode> = Vec::with_capacity(generation.count);
    let mut seen: HashSet<String> = HashSet::new();
    for _ in 0..DRAWS {
        let missing = generation.count - codes.len();
        if missing == 0 {
            break;
        }
        let mut drawn = Vec::with_capacity(missing);
        for _ in 0..missing {
            let code = generation.draw(random);
            if seen.insert(code.clone()) {
                drawn.push(code);
            }
        }
        // The taken ones are drawn again, in batches of what a lookup takes.
        let mut taken = HashSet::new();
        for chunk in drawn.chunks(MAX_ADDED) {
            for found in ports.discounts().find_codes(chunk).await? {
                taken.insert(found.normalized);
            }
        }
        for code in drawn.into_iter().filter(|c| !taken.contains(c)) {
            codes.push(DiscountCode::new(id, &code, usage_limit, batch.clone())?);
        }
    }
    if codes.len() < generation.count {
        return Err(CommandError::CodeSpaceExhausted(
            generation.count - codes.len(),
        ));
    }
    let mut write = ports.batch();
    write.add_discount_codes(&codes);
    write.record_events(vec![codes_added(id, codes.len(), batch)]);
    ports
        .commit(write)
        .await
        .map_err(|e| code_taken(e, &codes))?;
    Ok(codes)
}

/// `selection` checked and normalised, with how many of the discount's codes it selects
/// (those with `active` other than `unless_active`, when given) and its batch label.
async fn selected<S: HasDiscounts>(
    ports: &S,
    id: DiscountId,
    selection: CodeSelection,
    unless_active: Option<bool>,
) -> Result<(CodeSelection, u64, Option<String>), CommandError> {
    Ok(match selection {
        CodeSelection::Codes(codes) => {
            if codes.len() > MAX_ADDED {
                return Err(CommandError::TooManyCodes);
            }
            let normalized = codes
                .iter()
                .map(|c| normalize(c))
                .collect::<Result<Vec<_>, _>>()?;
            let count = ports
                .discounts()
                .find_codes(&normalized)
                .await?
                .iter()
                .filter(|c| c.discount == id && unless_active.is_none_or(|a| c.active != a))
                .count() as u64;
            (CodeSelection::Codes(normalized), count, None)
        }
        CodeSelection::Batch(batch) => {
            let batch = batch_label(Some(batch))?.ok_or(CodeError::Batch)?;
            let filter = CodeFilter {
                q: None,
                batch: Some(batch.clone()),
                active: unless_active.map(|a| !a),
            };
            let count = ports.discounts().count_codes(id, &filter).await?;
            (CodeSelection::Batch(batch.clone()), count, Some(batch))
        }
    })
}

/// Makes listed codes or a batch active or inactive; returns how many changed. An
/// inactive code is refused by carts like an unknown one, and a cart holding it shows it
/// `invalid`; its uses so far stay.
pub async fn set_codes_active<S: DiscountPorts>(
    ctx: &Ctx,
    ports: &S,
    id: DiscountId,
    selection: CodeSelection,
    active: bool,
) -> Result<u64, CommandError> {
    ctx.require(Scope::DiscountsWrite)?;
    coded(ports, id).await?;
    let (selection, count, batch) = selected(ports, id, selection, Some(active)).await?;
    if count == 0 {
        return Ok(0);
    }
    let mut write = ports.batch();
    write.set_discount_codes_active(id, selection, active);
    write.record_events(vec![PendingEvent::new(
        Subject::new(DISCOUNT, id.as_uuid()),
        &DiscountCodesUpdated {
            discount: id,
            count,
            batch,
            active,
        },
    )]);
    ports.commit(write).await?;
    Ok(count)
}

/// Removes listed codes or a batch; returns how many went.
pub async fn remove_codes<S: DiscountPorts>(
    ctx: &Ctx,
    ports: &S,
    id: DiscountId,
    selection: CodeSelection,
) -> Result<u64, CommandError> {
    ctx.require(Scope::DiscountsWrite)?;
    coded(ports, id).await?;
    let (selection, count, batch) = selected(ports, id, selection, None).await?;
    if count == 0 {
        return Ok(0);
    }
    let mut write = ports.batch();
    write.remove_discount_codes(id, selection);
    write.record_events(vec![PendingEvent::new(
        Subject::new(DISCOUNT, id.as_uuid()),
        &DiscountCodesRemoved {
            discount: id,
            count,
            batch,
        },
    )]);
    ports.commit(write).await?;
    Ok(count)
}

/// A page of the orders a discount was used on. It names orders and the customers who
/// placed them, so it needs reading both besides the discount.
pub async fn list_usage<S: HasDiscounts>(
    ctx: &Ctx,
    ports: &S,
    id: DiscountId,
    page: PageRequest<OrderId>,
) -> Result<Page<Usage>, CommandError> {
    ctx.require(Scope::DiscountsRead)?;
    ctx.require(Scope::CustomersRead)?;
    ctx.require(Scope::OrdersRead)?;
    ports
        .discounts()
        .get(id)
        .await?
        .ok_or(CommandError::NotFound(id))?;
    Ok(ports.discounts().usage_page(id, page).await?)
}

/// The live discount `normalized` belongs to, if the code is active and its discount is
/// live at `at`: what a cart accepts a code by. Unknown, inactive and not live read alike,
/// so codes can't be told apart by their errors.
pub async fn find_live_code<S: HasDiscounts>(
    ports: &S,
    normalized: &str,
    at: Timestamp,
) -> Result<Option<(DiscountCode, Discount)>, RepoError> {
    let Some(code) = ports
        .discounts()
        .find_codes(&[normalized.to_string()])
        .await?
        .into_iter()
        .find(|c| c.active)
    else {
        return Ok(None);
    };
    let Some(discount) = ports.discounts().get(code.discount).await? else {
        return Ok(None);
    };
    Ok((discount.is_live(at) && discount.method() == Method::Codes).then_some((code, discount)))
}
