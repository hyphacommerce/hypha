//! External apps (design note 0016, section 8): one record per app that Hypha signs
//! calls to, owning its signing secret, its webhook endpoint and the events it
//! subscribes to. Shipping's rate apps are apps too: their rate settings live in
//! `hypha-shipping` under the same key. Apps installed through OAuth (design note
//! 0029) also keep the manifest they were installed from and the grant staff approved;
//! the OAuth flow itself is `hypha-oauth`'s.
//!
//! A leaf domain: it depends on core only.

pub mod app;
pub mod blocks;
pub mod commands;
pub mod endpoints;
pub mod error;
pub mod events;
pub mod fetch;
pub mod manifest;
pub mod repo;
pub mod spore_settings;
pub mod targets;
pub mod ui;

pub use blocks::{BlockCheck, BlockProblem, MAX_BLOCK_DEPTH, check_block};
pub use targets::{ContextKey, ExtensionContext, Surface, Target};
pub use ui::{
    AdminContributions, PageContribution, StorefrontContributions, admin_contributions,
    find_action, find_block, find_proxy, may_use_on, scopes_for, session_app, spore_contributes,
    spore_for_ui, storefront_contributions,
};

pub use app::{
    App, AppSnapshot, EventPattern, Grant, GrantSnapshot, InstalledManifest, NewApp, NewSpore,
    Webhook, event_scope, normalize,
};
pub use commands::{
    CommandError, CreateApp, SECRET_PREFIX, SporeRegistration, UpdateApp, WebhookInput, create_app,
    delete_app, get_app, get_event, list_apps, list_deliveries, new_secret, open_secret,
    register_spore, retry_delivery, rotate_app_secret, spore_enabled, spore_in, spore_of,
    spore_registered, update_app,
};
pub use endpoints::{
    EndpointInput, ExtensionEndpoint, ExtensionEndpointRepo, HasExtensionEndpoints, KnownPoint,
    endpoint_of, find_endpoint, list_extension_endpoints, set_extension_endpoints,
};
pub use error::AppError;
pub use events::{APP, AppGrantChanged, AppInstalled, AppUninstalled};
pub use fetch::{FetchError, ManifestFetcher, fetch_manifest};
pub use hypha_core::{AppKey, AppKeyError, AppKind};
pub use manifest::{
    AppManifest, MAX_MANIFEST_BYTES, ManifestDocument, ManifestError, ManifestProblem,
    ManifestSource, PRIVATE_CLIENT_PREFIX, is_private_client, private_client_id,
};
pub use repo::{AppRepo, HasApps, StageApps, keys};
pub use spore_settings::{
    HasSporeSettings, ORDER_MARKER, SECRET_MARKER, SettingsProblem, SettingsReadError,
    SettingsSchema, SettingsView, SporeSettings, SporeSettingsRepo, get_spore_settings,
    read_spore_settings, save_spore_settings,
};
