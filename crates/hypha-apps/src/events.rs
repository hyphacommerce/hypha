//! What happens to apps installed through OAuth, as events. Contract types: a reshaped
//! field is a new `VERSION`, never an edit. They are about the app (subject `app`, its
//! `appid_…`) and go to that app only, whatever it subscribed to (`App::wants_event`).

use hypha_core::Event;
use serde::Serialize;

pub const APP: &str = "app";

/// `app.installed`: staff approved the app's first authorization (or one after an
/// uninstall).
#[derive(Debug, Serialize)]
pub struct AppInstalled {
    pub app: String,
    /// The granted scopes.
    pub scopes: Vec<&'static str>,
}

impl Event for AppInstalled {
    const TYPE: &'static str = "app.installed";
    const VERSION: u16 = 1;
}

/// `app.grant_changed`: staff approved other scopes, or narrowed the grant.
#[derive(Debug, Serialize)]
pub struct AppGrantChanged {
    pub app: String,
    /// The grant's scopes now.
    pub scopes: Vec<&'static str>,
    pub added: Vec<&'static str>,
    pub removed: Vec<&'static str>,
}

impl Event for AppGrantChanged {
    const TYPE: &'static str = "app.grant_changed";
    const VERSION: u16 = 1;
}

/// `app.uninstalled`: the store removed the app; its tokens no longer work. The last
/// webhook it gets.
#[derive(Debug, Serialize)]
pub struct AppUninstalled {
    pub app: String,
}

impl Event for AppUninstalled {
    const TYPE: &'static str = "app.uninstalled";
    const VERSION: u16 = 1;
}
