use hypha_core::{AuthError, Ctx, ErrorCode, FulfillmentId, OrderId, RepoError, Scope};
use thiserror::Error;

use crate::{Fulfillment, FulfillmentError, FulfillmentRepo, Tracking};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Fulfillment(#[from] FulfillmentError),
    #[error("fulfilment {0} not found")]
    FulfillmentNotFound(FulfillmentId),
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Fulfillment(e) => e.code(),
            CommandError::FulfillmentNotFound(_) => "fulfillment_not_found",
        }
    }
}

/// A tracking number as a client sends it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackingInput {
    pub carrier: Option<String>,
    pub number: String,
    pub url: Option<String>,
}

impl TrackingInput {
    /// Checks each one (`Tracking::new`).
    pub fn parse_all(input: &[TrackingInput]) -> Result<Vec<Tracking>, FulfillmentError> {
        input
            .iter()
            .map(|t| Tracking::new(t.carrier.as_deref(), &t.number, t.url.as_deref()))
            .collect()
    }
}

pub async fn get_fulfillment<R: FulfillmentRepo>(
    ctx: &Ctx,
    fulfillments: &R,
    id: FulfillmentId,
) -> Result<Fulfillment, CommandError> {
    ctx.require(Scope::FulfillmentsRead)?;
    load(fulfillments, id).await
}

/// Every fulfilment of `order`, cancelled ones too, newest first. An order nobody knows
/// has none: the caller checks the order exists when it matters.
pub async fn list_order_fulfillments<R: FulfillmentRepo>(
    ctx: &Ctx,
    fulfillments: &R,
    order: OrderId,
) -> Result<Vec<Fulfillment>, CommandError> {
    ctx.require(Scope::FulfillmentsRead)?;
    Ok(fulfillments.list_for_order(order).await?)
}

/// Replaces a standing fulfilment's tracking (design note 0018, section 3), and says
/// whether it changed (the buyer may be told then). The same tracking writes nothing; a
/// cancelled fulfilment is refused (`fulfillment_cancelled`).
pub async fn update_tracking<R: FulfillmentRepo>(
    ctx: &Ctx,
    fulfillments: &R,
    id: FulfillmentId,
    tracking: &[TrackingInput],
) -> Result<(Fulfillment, bool), CommandError> {
    ctx.require(Scope::FulfillmentsWrite)?;
    let tracking = TrackingInput::parse_all(tracking)?;
    let mut fulfillment = load(fulfillments, id).await?;
    let changed = fulfillment.set_tracking(tracking)?;
    if changed {
        fulfillments.update(&mut fulfillment).await?;
    }
    Ok((fulfillment, changed))
}

async fn load<R: FulfillmentRepo>(
    fulfillments: &R,
    id: FulfillmentId,
) -> Result<Fulfillment, CommandError> {
    fulfillments
        .get(id)
        .await?
        .ok_or(CommandError::FulfillmentNotFound(id))
}
