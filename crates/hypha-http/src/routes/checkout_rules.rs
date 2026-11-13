//! Checkout rules: what a cart must meet to be checked out, set up by the merchant
//! without code, and the "Try an address" test that shows what every rule would say.

use std::collections::HashMap;

use axum::{Json, extract::State, http::StatusCode};
use hypha_addresses::Formats;
use hypha_apps::{HasApps, HasExtensionEndpoints};
use hypha_checkout::without_echoed_warnings;
use hypha_checkout_rules::{
    CheckoutRule, CheckoutSettings, RuleAppliesTo, RuleCondition, RuleDraft, RuleInput, RuleLine,
    RuleOutcome, RulePatch, UpdateCheckoutSettings, ValidationAppInput, VerifiedFacts,
    create_checkout_rule, delete_checkout_rule, evaluate, get_checkout_rule, get_checkout_settings,
    list_checkout_rules, names_categories, update_checkout_rule, update_checkout_settings,
};
use hypha_core::{
    AddressError, AddressFormats, ChannelId, CheckoutRuleId, CountryCode, MarketId, Message, Money,
    ProductId, Scope, ShippingOptionId, Stamped, VariantId, VerifyContext, verify_address,
};
use hypha_products::{HasProducts, ProductError, ProductRepo};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    address::{
        AddressVerificationResponse, MessageParamsDto, PostalAddressRequest, PostalAddressResponse,
    },
    carts::CartMessageResponse,
    stamp_times,
};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId},
    patch,
    state::{SharedState, Storage},
};

/// A checkout rule: where it applies, the conditions that must all hold, and what the
/// shopper is told when they do.
#[derive(Serialize, utoipa::ToSchema)]
pub struct CheckoutRuleResponse {
    id: CheckoutRuleId,
    /// For staff.
    name: String,
    enabled: bool,
    /// Its place in the list, from 0: rules are evaluated, and their messages shown, in
    /// this order.
    position: u32,
    applies_to: RuleAppliesTo,
    /// 1 to 10, all of which must hold.
    conditions: Vec<RuleCondition>,
    outcome: RuleOutcome,
    /// RFC 3339.
    created_at: String,
    updated_at: String,
}

impl CheckoutRuleResponse {
    fn new(rule: &CheckoutRule) -> Result<Self, ApiError> {
        let (created_at, updated_at) = stamp_times(rule.stamps())?;
        let draft = rule.draft();
        Ok(Self {
            id: rule.id(),
            name: draft.name.clone(),
            enabled: draft.enabled,
            position: rule.position(),
            applies_to: draft.applies_to.clone(),
            conditions: draft.conditions.clone(),
            outcome: draft.outcome.clone(),
            created_at,
            updated_at,
        })
    }
}

/// Every rule, in order: at most 100, so there's no paging.
#[derive(Serialize, utoipa::ToSchema)]
pub struct CheckoutRuleListResponse {
    items: Vec<CheckoutRuleResponse>,
}

/// The store's checkout rules, in the order they're evaluated.
#[utoipa::path(
    get,
    path = "/admin/checkout-rules",
    responses((status = 200, description = "OK", body = CheckoutRuleListResponse)),
    extensions(("x-hypha-scopes" = json!(["checkout_rules:read"]))),
)]
pub async fn list<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
) -> Result<Json<CheckoutRuleListResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let rules = list_checkout_rules(&ctx, &ports).await?;
    Ok(Json(CheckoutRuleListResponse {
        items: rules
            .iter()
            .map(CheckoutRuleResponse::new)
            .collect::<Result<_, _>>()?,
    }))
}

fn yes() -> bool {
    true
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateCheckoutRuleRequest {
    name: String,
    /// On unless `false`.
    #[serde(default = "yes")]
    enabled: bool,
    /// Everywhere when absent.
    #[serde(default)]
    applies_to: RuleAppliesTo,
    conditions: Vec<RuleCondition>,
    outcome: RuleOutcome,
}

impl From<CreateCheckoutRuleRequest> for RuleDraft {
    fn from(r: CreateCheckoutRuleRequest) -> Self {
        RuleDraft {
            name: r.name,
            enabled: r.enabled,
            applies_to: r.applies_to,
            conditions: r.conditions,
            outcome: r.outcome,
        }
    }
}

/// Adds a rule, last in the list. Every broken rule of it is reported at once
/// (`422 invalid_checkout_rule`, `details.problems: [{path, code, message}]`); a store has
/// at most 100 (`409 too_many_checkout_rules`).
#[utoipa::path(
    post,
    path = "/admin/checkout-rules",
    request_body = CreateCheckoutRuleRequest,
    responses((status = 201, description = "Created", body = CheckoutRuleResponse)),
    extensions(("x-hypha-scopes" = json!(["checkout_rules:write"]))),
)]
pub async fn create<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreateCheckoutRuleRequest>,
) -> Result<(StatusCode, Json<CheckoutRuleResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let rule = create_checkout_rule(&ctx, &ports, &state.ids, req.into()).await?;
    Ok((StatusCode::CREATED, Json(CheckoutRuleResponse::new(&rule)?)))
}

/// A rule.
#[utoipa::path(
    get,
    path = "/admin/checkout-rules/{id}",
    params(("id" = CheckoutRuleId, Path)),
    responses((status = 200, description = "OK", body = CheckoutRuleResponse)),
    extensions(("x-hypha-scopes" = json!(["checkout_rules:read"]))),
)]
pub async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CheckoutRuleId>,
) -> Result<Json<CheckoutRuleResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let rule = get_checkout_rule(&ctx, &ports, id).await?;
    Ok(Json(CheckoutRuleResponse::new(&rule)?))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateCheckoutRuleRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = String, required = false)]
    name: Option<String>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = bool, required = false)]
    enabled: Option<bool>,
    /// Moves the rule to this place in the list, from 0 (past the end is the end); the
    /// others move to make room.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = u32, required = false)]
    position: Option<u32>,
    /// Replaced whole.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = RuleAppliesTo, required = false)]
    applies_to: Option<RuleAppliesTo>,
    /// Replaced whole.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = Vec<RuleCondition>, required = false)]
    conditions: Option<Vec<RuleCondition>>,
    /// Replaced whole.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = RuleOutcome, required = false)]
    outcome: Option<RuleOutcome>,
}

/// Changes a rule (JSON Merge Patch), checked as a whole; moving it renumbers the others
/// in the same commit.
#[utoipa::path(
    patch,
    path = "/admin/checkout-rules/{id}",
    params(("id" = CheckoutRuleId, Path)),
    request_body = UpdateCheckoutRuleRequest,
    responses((status = 200, description = "OK", body = CheckoutRuleResponse)),
    extensions(("x-hypha-scopes" = json!(["checkout_rules:write"]))),
)]
pub async fn update<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CheckoutRuleId>,
    body: JsonBody<UpdateCheckoutRuleRequest>,
) -> Result<Json<CheckoutRuleResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let patch = RulePatch {
        name: req.name,
        enabled: req.enabled,
        position: req.position,
        applies_to: req.applies_to,
        conditions: req.conditions,
        outcome: req.outcome,
    };
    let rule = update_checkout_rule(&ctx, &ports, id, patch).await?;
    Ok(Json(CheckoutRuleResponse::new(&rule)?))
}

/// Deletes a rule. The others keep their order.
#[utoipa::path(
    delete,
    path = "/admin/checkout-rules/{id}",
    params(("id" = CheckoutRuleId, Path)),
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["checkout_rules:write"]))),
)]
pub async fn remove<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CheckoutRuleId>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    delete_checkout_rule(&ctx, &ports, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- trying an address ----------

/// A line of the cart to try: its product, categories and weight count.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TestLineRequest {
    variant: VariantId,
    quantity: u32,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TestCheckoutRulesRequest {
    /// Checked against its country's format first, as a cart's would be.
    address: PostalAddressRequest,
    /// Rules for markets apply only with one.
    #[serde(default)]
    market: Option<MarketId>,
    /// Rules for channels apply only with one.
    #[serde(default)]
    channel: Option<ChannelId>,
    /// The chosen shipping option: rules for options apply only with one.
    #[serde(default)]
    shipping_option: Option<ShippingOptionId>,
    #[serde(default)]
    lines: Vec<TestLineRequest>,
    /// The cart's subtotal after discounts: subtotal conditions don't hold without one.
    #[serde(default)]
    subtotal: Option<Money>,
    #[serde(default)]
    email: Option<String>,
    /// A rule not saved yet (or `rule_id`'s, edited), tried in place of `rule_id` or last,
    /// even while it's off.
    #[serde(default)]
    rule: Option<CreateCheckoutRuleRequest>,
    #[serde(default)]
    rule_id: Option<CheckoutRuleId>,
    /// Also asks the store's verification app, which costs a lookup: needs
    /// `checkout_rules:write`. Its answer is in `verification`, its warning among the
    /// messages, and rules on verdicts and its flags are tried with it.
    #[serde(default)]
    verify: bool,
}

/// What's wrong with a field of the address tried.
#[derive(Serialize, utoipa::ToSchema)]
pub struct TestAddressProblem {
    field: String,
    code: String,
    message: String,
    params: MessageParamsDto,
}

/// What the address's text tells.
#[derive(Serialize, utoipa::ToSchema)]
pub struct TestAddressFlags {
    po_box: bool,
    military: bool,
    parcel_locker: bool,
    non_latin: bool,
}

/// A message the cart would show, and whether the draft rule gave it.
#[derive(Serialize, utoipa::ToSchema)]
pub struct TestMessageResponse {
    #[serde(flatten)]
    message: CartMessageResponse,
    /// The request's `rule` gave it (its `source.rule` is `rule_id`, or a placeholder for
    /// a rule not saved yet).
    draft: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct TestCheckoutRulesResponse {
    /// The address's format problems: a cart refuses it (`422 invalid_address`), so rules
    /// aren't tried until there are none.
    problems: Vec<TestAddressProblem>,
    /// The address as a cart would keep it (the region as its code); `null` with problems.
    address: Option<PostalAddressResponse>,
    flags: Option<TestAddressFlags>,
    /// What the verification app said, with `verify`; `null` without it, or without a
    /// verification app.
    verification: Option<AddressVerificationResponse>,
    /// What the cart would show: the format's warnings, the verification's, then the
    /// rules' messages in order.
    messages: Vec<TestMessageResponse>,
}

/// What the cart's messages would be for an address, and optionally its lines, market,
/// channel, chosen option and subtotal: the format's warnings and every enabled rule's
/// message, with a draft rule tried in place of a saved one. Writes nothing.
#[utoipa::path(
    post,
    path = "/admin/checkout-rules/test",
    request_body = TestCheckoutRulesRequest,
    responses((status = 200, description = "OK", body = TestCheckoutRulesResponse)),
    extensions(("x-hypha-scopes" = json!(["checkout_rules:read"]))),
)]
pub async fn test<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<TestCheckoutRulesRequest>,
) -> Result<Json<TestCheckoutRulesResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let mut rules = list_checkout_rules(&ctx, &ports).await?;
    if req.verify {
        ctx.require(Scope::CheckoutRulesWrite)?;
    }
    // The draft, in place of the rule it edits or last, tried even while it's off; its
    // ID marks its messages.
    let draft_id = req.rule.as_ref().map(|_| {
        req.rule_id
            .unwrap_or(CheckoutRuleId::from_uuid(Uuid::nil()))
    });
    if let (Some(draft), Some(id)) = (req.rule, draft_id) {
        let at = rules.iter().position(|r| r.id() == id);
        let position = at.and_then(|i| u32::try_from(i).ok()).unwrap_or(u32::MAX);
        let mut draft = RuleDraft::from(draft);
        draft.enabled = true;
        let rule = CheckoutRule::new(id, position, draft)
            .map_err(hypha_checkout_rules::CommandError::from)?;
        match at {
            Some(i) => rules[i] = rule,
            None => rules.push(rule),
        }
    }

    let checked = match Formats.validate(req.address.into()) {
        Ok(checked) => checked,
        Err(AddressError::Invalid(problems)) => {
            return Ok(Json(TestCheckoutRulesResponse {
                problems: problems
                    .iter()
                    .map(|p| TestAddressProblem {
                        field: p.field.as_str().to_string(),
                        code: p.code.as_str().to_string(),
                        message: p.message.clone(),
                        params: MessageParamsDto::from(&p.params),
                    })
                    .collect(),
                address: None,
                flags: None,
                verification: None,
                messages: Vec::new(),
            }));
        }
        // A wrong country refuses alone, as a cart's address does.
        Err(e) => return Err(ApiError::of(StatusCode::UNPROCESSABLE_ENTITY, &e)),
    };

    let mut lines = Vec::with_capacity(req.lines.len());
    for line in &req.lines {
        let product = ports
            .products()
            .get_by_variant(line.variant)
            .await?
            .ok_or_else(|| {
                ApiError::of(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    &ProductError::VariantNotFound(line.variant),
                )
            })?;
        let weight = product
            .variant(line.variant)
            .filter(|v| v.requires_shipping())
            .and_then(|v| v.weight());
        lines.push((product.id(), line.quantity, weight));
    }
    let categories = if names_categories(&rules) {
        let products: Vec<ProductId> = lines.iter().map(|(p, _, _)| *p).collect();
        hypha_cart::product_categories(&ports, &products).await?
    } else {
        HashMap::new()
    };

    let verification = if req.verify {
        let verifier = state.verifier(&ports, None);
        verify_address(&verifier, &checked.address, None, VerifyContext::Cart).await?
    } else {
        None
    };
    let address = checked.address.parts().clone();
    let input = RuleInput {
        country: CountryCode::new(&address.country)?,
        address: Some(address),
        flags: checked.flags,
        verification: verification.as_ref().map(|v| VerifiedFacts {
            verdict: Some(v.verdict),
            po_box: v.flags.po_box,
            residential: v.flags.residential,
            business: v.flags.business,
            vacant: v.flags.vacant,
        }),
        email: req.email,
        market: req.market,
        channel: req.channel,
        shipping: vec![req.shipping_option],
        lines: lines
            .into_iter()
            .map(|(product, quantity, shipping_weight)| RuleLine {
                product,
                categories: categories.get(&product).cloned().unwrap_or_default(),
                quantity,
                shipping_weight,
            })
            .collect(),
        subtotal: req.subtotal,
    };
    // The cart's messages in the cart's order, with the same built-in warnings left out
    // where an error already points.
    let mut messages: Vec<Message> = checked
        .warnings
        .iter()
        .map(Message::format_warning)
        .collect();
    messages.extend(
        verification
            .as_ref()
            .and_then(Message::verification_warning),
    );
    messages.extend(evaluate(&input, &rules));
    let messages = without_echoed_warnings(messages)
        .iter()
        .map(|m| TestMessageResponse {
            message: CartMessageResponse::from(m),
            draft: matches!(m.source(), hypha_core::MessageSource::Rule { rule } if Some(*rule) == draft_id),
        })
        .collect();
    let flags = checked.flags;
    Ok(Json(TestCheckoutRulesResponse {
        problems: Vec::new(),
        address: Some(PostalAddressResponse::from(&checked.address)),
        flags: Some(TestAddressFlags {
            po_box: flags.po_box,
            military: flags.military,
            parcel_locker: flags.parcel_locker,
            non_latin: flags.non_latin,
        }),
        verification: verification
            .as_ref()
            .map(AddressVerificationResponse::new)
            .transpose()?,
        messages,
    }))
}

/// How checkout treats one validation app.
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ValidationAppDto {
    /// `app_<name>`.
    app: String,
    /// Whether it's asked: on for an app installed for `checkout.validate` until set.
    enabled: bool,
    /// Whether checkout stops (a `validation_unavailable` error on the cart) when the app
    /// can't be asked, rather than going on without it: off until set.
    block_on_failure: bool,
}

/// Which apps checkout asks about an address and a cart.
#[derive(Serialize, utoipa::ToSchema)]
pub struct CheckoutSettingsResponse {
    /// The app verifying shipping addresses (`address.verify`): asked when a cart's
    /// address is set and when staff change an order's. `null`: none are verified.
    verification_app: Option<String>,
    /// The app suggesting addresses while someone types one (`address.suggest` and
    /// `address.resolve`), for the storefront's and the admin's address forms. `null`:
    /// forms don't suggest.
    autocomplete_app: Option<String>,
    /// Every app answering `checkout.validate` now (installed, enabled, granted
    /// `carts:read`; at most five), by key, with how checkout treats it. Each is asked when
    /// a cart's address is set, when an option is chosen and at checkout.
    validation_apps: Vec<ValidationAppDto>,
}

impl CheckoutSettingsResponse {
    async fn new<P: HasApps + HasExtensionEndpoints>(
        ports: &P,
        s: &CheckoutSettings,
    ) -> Result<Self, ApiError> {
        let validation_apps = crate::checkout_validate::validation_apps(ports, s)
            .await?
            .into_iter()
            .map(|v| ValidationAppDto {
                app: v.app.as_str().to_string(),
                enabled: v.enabled,
                block_on_failure: v.block_on_failure,
            })
            .collect();
        Ok(Self {
            verification_app: s.verification_app().map(|a| a.as_str().to_string()),
            autocomplete_app: s.autocomplete_app().map(|a| a.as_str().to_string()),
            validation_apps,
        })
    }
}

/// The store's checkout settings; the defaults (nothing asked) until saved.
#[utoipa::path(
    get,
    path = "/admin/checkout-settings",
    responses((status = 200, description = "OK", body = CheckoutSettingsResponse)),
    extensions(("x-hypha-scopes" = json!(["checkout_rules:read"]))),
)]
pub async fn settings<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
) -> Result<Json<CheckoutSettingsResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let settings = get_checkout_settings(&ctx, &ports).await?;
    Ok(Json(
        CheckoutSettingsResponse::new(&ports, &settings).await?,
    ))
}

/// JSON Merge Patch: absent keeps, `null` clears.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateCheckoutSettingsRequest {
    /// An app installed, enabled and granted what `address.verify` needs
    /// (`customer_data:address`), whose manifest declares it (`422
    /// verification_app_unavailable` otherwise); `null` for none.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>)]
    verification_app: Option<Option<String>>,
    /// An app installed, enabled and granted what `address.suggest` and `address.resolve`
    /// need (`customer_data:address`), answering both (`422 autocomplete_app_unavailable`
    /// otherwise); `null` for none.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>)]
    autocomplete_app: Option<Option<String>>,
    /// How checkout treats each validation app named, replacing what was set: an app left
    /// out goes back to the default (asked, not blocking). Each answering
    /// `checkout.validate` (`422 validation_app_unavailable` otherwise), once (`422
    /// duplicate_validation_app`).
    #[serde(default, deserialize_with = "patch::non_null")]
    validation_apps: Option<Vec<ValidationAppDto>>,
}

/// Changes the store's checkout settings (`checkout_settings.updated`).
#[utoipa::path(
    patch,
    path = "/admin/checkout-settings",
    request_body = UpdateCheckoutSettingsRequest,
    responses((status = 200, description = "OK", body = CheckoutSettingsResponse)),
    extensions(("x-hypha-scopes" = json!(["checkout_rules:write"]))),
)]
pub async fn update_settings<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<UpdateCheckoutSettingsRequest>,
) -> Result<Json<CheckoutSettingsResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let settings = update_checkout_settings(
        &ctx,
        &ports,
        &state.app_endpoints(&ports),
        UpdateCheckoutSettings {
            verification_app: req.verification_app,
            autocomplete_app: req.autocomplete_app,
            validation_apps: req.validation_apps.map(|apps| {
                apps.into_iter()
                    .map(|a| ValidationAppInput {
                        app: a.app,
                        enabled: a.enabled,
                        block_on_failure: a.block_on_failure,
                    })
                    .collect()
            }),
        },
    )
    .await?;
    Ok(Json(
        CheckoutSettingsResponse::new(&ports, &settings).await?,
    ))
}
