//! Sales channels (design note 0020): where products are sold (a storefront, shopping
//! agents, an app), and which products each sells. A leaf crate on core: products,
//! carts, search and auth reach channels without depending on each other.
//!
//! A channel is *where*; a market is *to whom* and *at what price* (CH1). A storefront
//! request may sell a product only if both allow it.

pub mod channel;
pub mod commands;
pub mod error;
pub mod events;
pub mod repo;

pub use channel::{Channel, ChannelKind, ChannelSnapshot};
pub use commands::{
    CommandError, CreateChannel, UpdateChannel, channels_of_products, create_channel, get_channel,
    is_published, list_channels, product_channels, publish_products, set_product_channels,
    unpublish_products, update_channel,
};
pub use error::ChannelError;
pub use events::{ChannelChanged, ProductsPublished, ProductsUnpublished};
pub use repo::{ChannelRepo, HasChannels, StagePublications, keys};
