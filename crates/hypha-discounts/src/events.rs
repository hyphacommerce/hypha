//! What happens to discounts, as events. Contract types: a reshaped field is a new
//! `VERSION`, never an edit.

use hypha_core::{DiscountId, Event};
use serde::Serialize;

pub const DISCOUNT: &str = "discount";

/// `discount.created`.
#[derive(Debug, Serialize)]
pub struct DiscountCreated {
    pub discount: DiscountId,
}

impl Event for DiscountCreated {
    const TYPE: &'static str = "discount.created";
    const VERSION: u16 = 1;
}

/// `discount.updated`: `fields` names what changed (`title`, `status`, `starts_at`,
/// `ends_at`, `conditions`, `kind`, `combines_with`, `limits`).
#[derive(Debug, Serialize)]
pub struct DiscountUpdated {
    pub discount: DiscountId,
    pub fields: Vec<&'static str>,
}

impl Event for DiscountUpdated {
    const TYPE: &'static str = "discount.updated";
    const VERSION: u16 = 1;
}

/// `discount.deleted`: orders keep their copies of what it took off.
#[derive(Debug, Serialize)]
pub struct DiscountDeleted {
    pub discount: DiscountId,
}

impl Event for DiscountDeleted {
    const TYPE: &'static str = "discount.deleted";
    const VERSION: u16 = 1;
}

/// `discount.codes_added`: how many, and the batch label they were added under, not the
/// codes themselves (a set can hold thousands).
#[derive(Debug, Serialize)]
pub struct DiscountCodesAdded {
    pub discount: DiscountId,
    pub count: u64,
    pub batch: Option<String>,
}

impl Event for DiscountCodesAdded {
    const TYPE: &'static str = "discount.codes_added";
    const VERSION: u16 = 1;
}

/// `discount.codes_removed`: how many, and the batch when a whole batch went.
#[derive(Debug, Serialize)]
pub struct DiscountCodesRemoved {
    pub discount: DiscountId,
    pub count: u64,
    pub batch: Option<String>,
}

impl Event for DiscountCodesRemoved {
    const TYPE: &'static str = "discount.codes_removed";
    const VERSION: u16 = 1;
}
