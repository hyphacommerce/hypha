//! What happens to assets, as events (design note 0016). Contract types: a renamed or
//! reshaped field is a new `VERSION`, never an edit.

use hypha_core::{AssetId, Event};
use serde::Serialize;

/// The subject kind events about assets carry.
pub const ASSET: &str = "asset";

/// `asset.deleted`: the asset and its file are gone. What only refers to it (a custom
/// field's value, design note 0033, section 6.3) lets go of it; usages can't exist, as
/// an asset in use isn't deleted.
#[derive(Debug, Serialize)]
pub struct AssetDeleted {
    pub asset: AssetId,
}

impl Event for AssetDeleted {
    const TYPE: &'static str = "asset.deleted";
    const VERSION: u16 = 1;
}
