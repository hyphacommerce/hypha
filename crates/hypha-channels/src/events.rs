//! What happens to sales channels, as events. Contract types: a renamed or reshaped field
//! is a new `VERSION`, never an edit.

use hypha_core::{ChannelId, Event, ProductId};
use serde::Serialize;

pub const CHANNEL: &str = "channel";

/// `channel.changed`: created or renamed.
#[derive(Debug, Serialize)]
pub struct ChannelChanged {
    pub channel: ChannelId,
}

impl Event for ChannelChanged {
    const TYPE: &'static str = "channel.changed";
    const VERSION: u16 = 1;
}

/// `channel.products_published`: these products are sold through the channel now.
#[derive(Debug, Serialize)]
pub struct ProductsPublished {
    pub channel: ChannelId,
    pub products: Vec<ProductId>,
}

impl Event for ProductsPublished {
    const TYPE: &'static str = "channel.products_published";
    const VERSION: u16 = 1;
}

/// `channel.products_unpublished`: these products aren't sold through the channel any
/// more.
#[derive(Debug, Serialize)]
pub struct ProductsUnpublished {
    pub channel: ChannelId,
    pub products: Vec<ProductId>,
}

impl Event for ProductsUnpublished {
    const TYPE: &'static str = "channel.products_unpublished";
    const VERSION: u16 = 1;
}
