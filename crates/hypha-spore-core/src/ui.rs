//! What a spore adds to admins: blocks and actions on the same targets apps use, answered
//! in-process. The HTTP layer asks installed apps (over HTTP) and spores (here) and merges
//! the two, so a frontend can't tell which answered. Blocks are trees in
//! `packages/app-blocks`' format v1, checked like an app's (`hypha_apps::check_block`). No
//! embedded pages for spores.

use hypha_apps::{ExtensionContext, Target};
use hypha_core::{Ctx, UserId};
use serde_json::Value;
use thiserror::Error;

/// A block a spore declares: its ID (unique among its blocks) and the admin target it
/// fills (one that takes blocks: `order.payment`, `product.details`, …).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiBlock {
    pub id: &'static str,
    pub target: Target,
}

/// An action a spore declares: a button on `target` (one that takes actions).
#[derive(Debug, Clone, PartialEq)]
pub struct UiAction {
    pub id: &'static str,
    pub target: Target,
    pub label: &'static str,
    /// A JSON Schema for a form shown before running it, when it takes input.
    pub input_schema: Option<Value>,
    /// The payment point it stands for (`payments.capture`, `.void`, `.refund`), as an
    /// app's manifest names one: admins run it through the payment's own route
    /// (`POST /admin/payments/{id}/capture`), which reaches the spore's provider, and
    /// show it only when the payment's status allows. `None` for an action the spore
    /// runs itself (`SporeUi::action`).
    pub point: Option<&'static str>,
}

/// A web component a spore adds to storefronts: its tag, the storefront target it's for
/// (`checkout.payment` for a payment widget), and the path of the spore's own storefront
/// route that serves its ES module (`/storefront/spores/stripe/elements/payment.js`), which
/// storefronts load from the API's public address (`server.public_url`, which `hypha
/// deploy` sets; unset, the address the storefront reached the API at, which must then be
/// one a browser can load too). The route must answer `Access-Control-Allow-Origin: *`: a
/// module loads cross-origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiElement {
    pub id: &'static str,
    pub target: Target,
    pub tag: &'static str,
    pub path: &'static str,
}

/// One request for a block or an action.
pub struct UiCall<'a> {
    pub target: Target,
    /// The objects it's used on, checked against the target.
    pub context: &'a ExtensionContext,
    /// The spore acting for the caller: `Actor::App` (the spore's app), with the
    /// spore's scopes as far as the caller holds them, as an app's session token
    /// would carry. What a spore passes to the commands it calls.
    pub ctx: &'a Ctx,
    /// The staff member asking, for a session.
    pub staff: Option<UserId>,
}

/// What an action answered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiAnswer {
    /// Plain text to show (a toast).
    pub message: Option<String>,
    /// Reload what the action was pressed on.
    pub refresh: bool,
    /// Where to go in the admin.
    pub navigate: Option<String>,
}

/// Why a block or action gave no answer.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum UiError {
    /// It has no such block or action.
    #[error("no such block or action")]
    NotFound,
    /// It failed, in words staff may see.
    #[error("{0}")]
    Failed(String),
}

impl hypha_core::ErrorCode for UiError {
    fn code(&self) -> &'static str {
        match self {
            UiError::NotFound => "app_extension_not_found",
            UiError::Failed(_) => "app_unavailable",
        }
    }
}

/// A spore's admin UI, implemented next to `Spore`: an empty
/// `impl<P> SporeUi<P> for MySpore {}` for a spore with none. `P` is the host's
/// ports; bound it on what the blocks read (`P: HasOrders + HasPayments`), as event
/// handlers do.
#[allow(async_fn_in_trait)]
pub trait SporeUi<P> {
    /// Its blocks.
    fn blocks() -> Vec<UiBlock> {
        Vec::new()
    }

    /// Its actions.
    fn actions() -> Vec<UiAction> {
        Vec::new()
    }

    /// Its storefront elements.
    fn elements() -> Vec<UiElement> {
        Vec::new()
    }

    /// The block `id`'s tree for `call` (format v1: `{"version": 1, "root": …}`).
    async fn block(&self, id: &str, call: UiCall<'_>, ports: &P) -> Result<Value, UiError> {
        let _ = (id, call, ports);
        Err(UiError::NotFound)
    }

    /// Runs the action `id` for `call`, with the form's values when it has a form.
    async fn action(
        &self,
        id: &str,
        call: UiCall<'_>,
        input: Option<Value>,
        ports: &P,
    ) -> Result<UiAnswer, UiError> {
        let _ = (id, call, input, ports);
        Err(UiError::NotFound)
    }
}

/// What one spore contributes (`SporeRuntime::contributions`).
#[derive(Debug, Clone, PartialEq)]
pub struct SporeContributions {
    pub name: &'static str,
    pub title: &'static str,
    pub blocks: Vec<UiBlock>,
    pub actions: Vec<UiAction>,
    pub elements: Vec<UiElement>,
}
