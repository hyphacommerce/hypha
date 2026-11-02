use hypha_core::{
    AppKey, AppKeyError, AppPoints, AuthError, CheckoutRuleId, Ctx, ErrorCode, IdGenerator,
    RepoError, Scope,
};

use crate::{
    error::CheckoutRuleError,
    repo::{CheckoutRuleRepo, CheckoutSettingsRepo, HasCheckoutRules, HasCheckoutSettings},
    rule::{CheckoutRule, MAX_RULES, RuleAppliesTo, RuleCondition, RuleDraft, RuleOutcome},
    settings::{CheckoutSettings, VERIFY_POINT},
};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Rule(#[from] CheckoutRuleError),
    #[error("checkout rule {0} not found")]
    NotFound(CheckoutRuleId),
    #[error("a store has at most {max} checkout rules")]
    TooMany { max: usize },
    #[error(transparent)]
    AppKey(#[from] AppKeyError),
    #[error("{0} isn't installed, enabled and granted to answer address.verify")]
    VerificationAppUnavailable(AppKey),
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Rule(e) => e.code(),
            CommandError::NotFound(_) => "checkout_rule_not_found",
            CommandError::TooMany { .. } => "too_many_checkout_rules",
            CommandError::AppKey(e) => e.code(),
            CommandError::VerificationAppUnavailable(_) => "verification_app_unavailable",
        }
    }
}

/// What changes in a rule; `None` keeps it. A part given is replaced whole.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RulePatch {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    /// Its place in the list, from 0; past the end is the end. The other rules move to
    /// make room.
    pub position: Option<u32>,
    pub applies_to: Option<RuleAppliesTo>,
    pub conditions: Option<Vec<RuleCondition>>,
    pub outcome: Option<RuleOutcome>,
}

/// Every rule, in the order checkout evaluates them.
pub async fn list_checkout_rules<S: HasCheckoutRules>(
    ctx: &Ctx,
    ports: &S,
) -> Result<Vec<CheckoutRule>, CommandError> {
    ctx.require(Scope::CheckoutRulesRead)?;
    Ok(ports.checkout_rules().list().await?)
}

pub async fn get_checkout_rule<S: HasCheckoutRules>(
    ctx: &Ctx,
    ports: &S,
    id: CheckoutRuleId,
) -> Result<CheckoutRule, CommandError> {
    ctx.require(Scope::CheckoutRulesRead)?;
    ports
        .checkout_rules()
        .get(id)
        .await?
        .ok_or(CommandError::NotFound(id))
}

/// A new rule, last in the list.
pub async fn create_checkout_rule<S: HasCheckoutRules>(
    ctx: &Ctx,
    ports: &S,
    ids: &impl IdGenerator,
    draft: RuleDraft,
) -> Result<CheckoutRule, CommandError> {
    ctx.require(Scope::CheckoutRulesWrite)?;
    let rules = ports.checkout_rules().list().await?;
    let position = rules
        .iter()
        .map(CheckoutRule::position)
        .max()
        .map_or(0, |p| p.saturating_add(1));
    let mut rule = CheckoutRule::new(CheckoutRuleId::from_uuid(ids.new_uuid()), position, draft)?;
    if rules.len() >= MAX_RULES {
        return Err(CommandError::TooMany { max: MAX_RULES });
    }
    ports.checkout_rules().insert(&mut rule).await?;
    Ok(rule)
}

/// Changes a rule; moving it renumbers the others in the same commit.
pub async fn update_checkout_rule<S: HasCheckoutRules>(
    ctx: &Ctx,
    ports: &S,
    id: CheckoutRuleId,
    patch: RulePatch,
) -> Result<CheckoutRule, CommandError> {
    ctx.require(Scope::CheckoutRulesWrite)?;
    let mut rules = ports.checkout_rules().list().await?;
    let at = rules
        .iter()
        .position(|r| r.id() == id)
        .ok_or(CommandError::NotFound(id))?;
    let rule = &mut rules[at];
    let current = rule.draft().clone();
    let draft = RuleDraft {
        name: patch.name.unwrap_or(current.name),
        enabled: patch.enabled.unwrap_or(current.enabled),
        applies_to: patch.applies_to.unwrap_or(current.applies_to),
        conditions: patch.conditions.unwrap_or(current.conditions),
        outcome: patch.outcome.unwrap_or(current.outcome),
    };
    rule.update(draft)?;
    if let Some(to) = patch.position {
        let moved = rules.remove(at);
        let to = usize::try_from(to).unwrap_or(usize::MAX).min(rules.len());
        rules.insert(to, moved);
    }
    // Dense positions in list order, so the next move lands where the admin shows it.
    for (position, rule) in rules.iter_mut().enumerate() {
        rule.set_position(u32::try_from(position).unwrap_or(u32::MAX));
    }
    let mut changed: Vec<CheckoutRule> = rules
        .into_iter()
        .filter(|r| r.id() == id || r.has_changes())
        .collect();
    ports.checkout_rules().update_many(&mut changed).await?;
    changed
        .into_iter()
        .find(|r| r.id() == id)
        .ok_or(CommandError::NotFound(id))
}

pub async fn delete_checkout_rule<S: HasCheckoutRules>(
    ctx: &Ctx,
    ports: &S,
    id: CheckoutRuleId,
) -> Result<(), CommandError> {
    ctx.require(Scope::CheckoutRulesWrite)?;
    let mut rule = ports
        .checkout_rules()
        .get(id)
        .await?
        .ok_or(CommandError::NotFound(id))?;
    rule.mark_deleted();
    ports.checkout_rules().delete(&mut rule).await?;
    Ok(())
}

/// The store's checkout settings; the defaults (nothing asked) until saved.
pub async fn get_checkout_settings<S: HasCheckoutSettings>(
    ctx: &Ctx,
    ports: &S,
) -> Result<CheckoutSettings, CommandError> {
    ctx.require(Scope::CheckoutRulesRead)?;
    Ok(ports.checkout_settings().get().await?.unwrap_or_default())
}

/// Changes only what's `Some`, like a JSON Merge Patch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateCheckoutSettings {
    /// The app verifying addresses (`Some(None)`: none).
    pub verification_app: Option<Option<String>>,
}

/// Changes the settings. A verification app must be installed, enabled and granted what
/// `address.verify` needs, as `apps` says (`verification_app_unavailable`); an app named
/// already stays named while it's checked, so an outage doesn't block other changes.
pub async fn update_checkout_settings<S: HasCheckoutSettings>(
    ctx: &Ctx,
    ports: &S,
    apps: &impl AppPoints,
    input: UpdateCheckoutSettings,
) -> Result<CheckoutSettings, CommandError> {
    ctx.require(Scope::CheckoutRulesWrite)?;
    let stored = ports.checkout_settings().get().await?;
    let is_new = stored.is_none();
    let mut settings = stored.unwrap_or_default();
    if let Some(app) = input.verification_app {
        let app = app.map(|key| AppKey::new(&key)).transpose()?;
        if let Some(key) = &app
            && settings.verification_app() != Some(key)
            && !apps.answers(key, VERIFY_POINT).await?
        {
            return Err(CommandError::VerificationAppUnavailable(key.clone()));
        }
        settings.set_verification_app(app);
    }
    if is_new {
        ports.checkout_settings().insert(&mut settings).await?;
    } else {
        ports.checkout_settings().update(&mut settings).await?;
    }
    Ok(settings)
}
