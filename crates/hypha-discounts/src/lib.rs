//! Discounts: automatic or by code, reducing items, the order or shipping, and the pure
//! engine that applies them to a cart to the cent. A leaf crate on core: carts hand it
//! prices, categories and the buyer as plain values.
//!
//! Prices from price lists come first (a sale is a price, not a discount); discounts are
//! what a cart takes off those prices.

pub mod code;
pub mod commands;
pub mod context;
pub mod discount;
pub mod engine;
pub mod error;
pub mod events;
pub mod json;
pub mod repo;
pub mod schedule;
pub mod summary;

pub use code::{DiscountCode, Generation, normalize};
pub use commands::{
    CommandError, CreateDiscount, DiscountPatch, DiscountPorts, NewCode, add_codes,
    create_discount, delete_discount, find_live_code, generate_codes, get_discount, list_codes,
    list_discounts, list_usage, remove_codes, update_discount,
};
pub use context::{DiscountContext, Promotion, promotions};
pub use discount::{
    Allocation, Class, ClassSet, Condition, Discount, DiscountDraft, DiscountSnapshot, ItemSet,
    Items, Kind, Limits, MAX_CODES_PER_CART, MAX_IDS, MAX_LIVE_AUTOMATIC, MAX_REWARDS, MAX_TIERS,
    Measure, Method, PerMarket, Phase, Reward, Status, Target, Threshold, Tier, Value,
};
pub use engine::{
    Application, Buyer, CodeOutcome, CodeState, DiscountInput, Line, Live, Needed, Outcome, Share,
    ShippingGroup, ShippingOutcome, apply, spread,
};
pub use error::{CodeError, DiscountError, Problem};
pub use events::{
    DiscountCodesAdded, DiscountCodesRemoved, DiscountCreated, DiscountDeleted, DiscountEnded,
    DiscountStarted, DiscountUpdated,
};
pub use repo::{
    CodeFilter, CodeSelection, DiscountFilter, DiscountRepo, HasDiscounts, StageDiscounts, Usage,
    UsageClaim, keys,
};
pub use schedule::{announce_windows, boundaries, next_boundary};
pub use summary::summary;
