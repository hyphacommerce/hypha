//! What happens to the store, as events. Contract types: a reshaped field is a new
//! `VERSION`, never an edit.

use hypha_core::{Event, StoreId};
use serde::Serialize;

pub const STORE: &str = "store";

/// `store.updated`: `fields` names what changed (`name`, `contact_email`, `description`,
/// `legal_name`, `legal_address`, `timezone`, `default_language`, `languages`,
/// `currency`, `setup`), one event per write. The values aren't in it: read the store.
#[derive(Debug, Serialize)]
pub struct StoreUpdated {
    pub store: StoreId,
    pub fields: Vec<&'static str>,
}

impl Event for StoreUpdated {
    const TYPE: &'static str = "store.updated";
    const VERSION: u16 = 1;
}
