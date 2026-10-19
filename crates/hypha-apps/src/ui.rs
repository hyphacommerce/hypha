//! What installed apps contribute to admins and storefronts (design note 0029, §6–§8):
//! discovery per target, and finding one contribution to use (an action to run, a
//! block to fetch, a proxy to forward to, a session token to sign). Only apps installed
//! through OAuth, enabled, contribute, and only where their grant reads the target's
//! object (`Target::scope`).

use hypha_core::{AuthError, Ctx, Scope};

use crate::{
    App, AppError, AppRepo, CommandError,
    manifest::{AdminAction, AdminBlock, AdminPage, StorefrontElement},
    targets::{ExtensionContext, Surface, Target},
};

/// A page, with the target it fills (`apps.page`, or `settings.app` for `nav:
/// "settings"`).
#[derive(Debug, Clone, PartialEq)]
pub struct PageContribution {
    pub page: AdminPage,
    pub target: Target,
}

/// One app's admin contributions to the targets asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct AdminContributions {
    pub app: App,
    pub pages: Vec<PageContribution>,
    pub blocks: Vec<(AdminBlock, Target)>,
    pub actions: Vec<(AdminAction, Target)>,
    /// The manifest's settings form, when `settings.app` (or every target) was asked.
    pub settings: Option<crate::manifest::ManifestSettings>,
    /// Whether the manifest has an admin proxy (`/admin/apps/{app}/proxy/…`).
    pub proxy: bool,
}

/// One app's storefront contributions.
#[derive(Debug, Clone, PartialEq)]
pub struct StorefrontContributions {
    pub app: App,
    pub elements: Vec<(StorefrontElement, Target)>,
    /// Whether the manifest has a storefront proxy (`/storefront/apps/{app}/proxy/…`).
    pub proxy: bool,
}

/// The page's target: `nav: "settings"` is the app's settings page.
fn page_target(page: &AdminPage) -> Target {
    match page.nav.as_deref() {
        Some("settings") => Target::SettingsApp,
        _ => Target::AppsPage,
    }
}

/// Whether `app` may contribute to `target`: its grant reads the target's object.
fn granted(app: &App, target: Target) -> bool {
    match (app.grant(), target.scope()) {
        (Some(_), None) => true,
        (Some(grant), Some(scope)) => grant.has(scope),
        (None, _) => false,
    }
}

/// `target` read from a manifest, when this catalog knows it, it's on `surface` and
/// `wanted` (every target when `None`) is it.
fn wanted_target(name: &str, surface: Surface, wanted: Option<Target>) -> Option<Target> {
    Target::parse(name)
        .filter(|t| t.surface() == surface)
        .filter(|t| wanted.is_none_or(|w| w == *t))
}

/// Apps that contribute at all: installed through OAuth, enabled, with a manifest.
async fn contributing<R: AppRepo>(apps: &R) -> Result<Vec<App>, CommandError> {
    Ok(apps
        .list()
        .await?
        .into_iter()
        .filter(|a| a.is_installed() && a.enabled() && a.manifest().is_some())
        .collect())
}

/// Every installed app's admin contributions to `target` (every admin target when
/// `None`), by app key; apps with none are left out.
pub async fn admin_contributions<R: AppRepo>(
    ctx: &Ctx,
    apps: &R,
    target: Option<Target>,
) -> Result<Vec<AdminContributions>, CommandError> {
    ctx.require(Scope::AppsRead)?;
    let mut found = Vec::new();
    for app in contributing(apps).await? {
        let Some(manifest) = app.manifest() else {
            continue;
        };
        let doc = manifest.manifest.document().clone();
        let admin = doc.admin.unwrap_or_default();
        let fits = |t: Target| target.is_none_or(|w| w == t) && granted(&app, t);
        let pages: Vec<PageContribution> = admin
            .pages
            .into_iter()
            .map(|page| PageContribution {
                target: page_target(&page),
                page,
            })
            .filter(|p| fits(p.target))
            .collect();
        let blocks: Vec<(AdminBlock, Target)> = admin
            .blocks
            .into_iter()
            .filter_map(|b| {
                let t = wanted_target(&b.target, Surface::Admin, target)?;
                (t.takes_blocks() && granted(&app, t)).then_some((b, t))
            })
            .collect();
        let actions: Vec<(AdminAction, Target)> = admin
            .actions
            .into_iter()
            .filter_map(|a| {
                let t = wanted_target(&a.target, Surface::Admin, target)?;
                (t.takes_actions() && granted(&app, t)).then_some((a, t))
            })
            .collect();
        let settings = doc
            .settings
            .filter(|_| target.is_none_or(|t| t == Target::SettingsApp));
        if pages.is_empty() && blocks.is_empty() && actions.is_empty() && settings.is_none() {
            continue;
        }
        found.push(AdminContributions {
            proxy: admin.proxy.is_some(),
            app,
            pages,
            blocks,
            actions,
            settings,
        });
    }
    Ok(found)
}

/// Every installed app's storefront elements for `target` (every storefront target
/// when `None`), and which apps have a storefront proxy. Under `stores:read`, which
/// every publishable key has: what's installed is the store's setup.
pub async fn storefront_contributions<R: AppRepo>(
    ctx: &Ctx,
    apps: &R,
    target: Option<Target>,
) -> Result<Vec<StorefrontContributions>, CommandError> {
    ctx.require(Scope::StoresRead)?;
    let mut found = Vec::new();
    for app in contributing(apps).await? {
        let Some(manifest) = app.manifest() else {
            continue;
        };
        let storefront = manifest
            .manifest
            .document()
            .storefront
            .clone()
            .unwrap_or_default();
        let elements: Vec<(StorefrontElement, Target)> = storefront
            .elements
            .into_iter()
            .filter_map(|e| {
                let t = wanted_target(&e.target, Surface::Storefront, target)?;
                granted(&app, t).then_some((e, t))
            })
            .collect();
        let proxy = storefront.proxy.is_some();
        if elements.is_empty() && !proxy {
            continue;
        }
        found.push(StorefrontContributions {
            app,
            elements,
            proxy,
        });
    }
    Ok(found)
}

/// The app under `key` if it contributes: installed through OAuth (`app_not_installed`
/// otherwise) and enabled (a disabled app's contributions are `app_not_installed` too:
/// it's switched off).
async fn installed<R: AppRepo>(apps: &R, key: &str) -> Result<App, CommandError> {
    let not_found = || CommandError::AppNotFound(key.to_string());
    let parsed = crate::AppKey::new(key).map_err(|_| not_found())?;
    let app = apps.get(&parsed).await?.ok_or_else(not_found)?;
    if !(app.is_installed() && app.enabled() && app.manifest().is_some()) {
        return Err(AppError::NotInstalled.into());
    }
    Ok(app)
}

/// Whether the spore `app` (design note 0030) contributes to `target` for discovery: its
/// grant (its declared scopes) reads the target's object, as an app's must.
pub fn spore_contributes(app: &App, target: Target) -> bool {
    app.enabled() && granted(app, target)
}

/// The spore under `key`, for using one of its blocks or actions (design note 0030,
/// §7): the caller needs `apps:read`; a key that's no spore of this store is
/// `app_not_found`; one staff disabled is `app_disabled`.
pub async fn spore_for_ui<R: AppRepo>(ctx: &Ctx, apps: &R, key: &str) -> Result<App, CommandError> {
    ctx.require(Scope::AppsRead)?;
    let not_found = || CommandError::AppNotFound(key.to_string());
    let parsed = crate::AppKey::new(key).map_err(|_| not_found())?;
    if parsed.kind() != crate::AppKind::Spore {
        return Err(not_found());
    }
    let app = apps.get(&parsed).await?.ok_or_else(not_found)?;
    if !app.enabled() {
        return Err(AppError::Disabled(parsed).into());
    }
    Ok(app)
}

/// `target`'s object may be read by the app (its grant) and by the caller, and
/// `context` fits it: what using a block or action needs, an app's or a spore's.
pub fn may_use_on(
    ctx: &Ctx,
    app: &App,
    target: Target,
    context: &ExtensionContext,
) -> Result<(), CommandError> {
    may_use(ctx, app, target)?;
    context.check(target).map_err(CommandError::InvalidContext)
}

/// `target`'s object may be read by the app (its grant) and by the caller.
fn may_use(ctx: &Ctx, app: &App, target: Target) -> Result<(), CommandError> {
    if let Some(scope) = target.scope() {
        if !granted(app, target) {
            return Err(AuthError::AppNotAuthorized(scope).into());
        }
        ctx.require(scope)?;
    }
    Ok(())
}

/// The app, the URL and the target of its action `id`, for running it on `context`
/// (`POST /admin/app-extensions/actions/{app}/{id}`): the caller needs `apps:read` and
/// to read the target's object, the app's grant to read it too. An action that names
/// an extension point instead of a URL runs through that point's route
/// (`app_action_has_point`).
pub async fn find_action<R: AppRepo>(
    ctx: &Ctx,
    apps: &R,
    key: &str,
    id: &str,
    context: &ExtensionContext,
) -> Result<(App, String, Target), CommandError> {
    ctx.require(Scope::AppsRead)?;
    let app = installed(apps, key).await?;
    let not_found = || CommandError::ExtensionNotFound {
        app: app.key().clone(),
        id: id.to_string(),
    };
    let action = app
        .manifest()
        .and_then(|m| m.manifest.document().admin.as_ref())
        .and_then(|a| a.actions.iter().find(|a| a.id == id))
        .cloned()
        .ok_or_else(not_found)?;
    let target = wanted_target(&action.target, Surface::Admin, None)
        .filter(|t| t.takes_actions())
        .ok_or_else(not_found)?;
    may_use(ctx, &app, target)?;
    context
        .check(target)
        .map_err(CommandError::InvalidContext)?;
    let url = match (action.url, action.point) {
        (Some(url), _) => url,
        (None, point) => {
            return Err(CommandError::ActionHasPoint {
                app: app.key().clone(),
                id: action.id,
                point: point.unwrap_or_default(),
            });
        }
    };
    Ok((app, url, target))
}

/// The app and its block `id` for fetching it, under the same rules as `find_action`.
pub async fn find_block<R: AppRepo>(
    ctx: &Ctx,
    apps: &R,
    key: &str,
    id: &str,
    context: &ExtensionContext,
) -> Result<(App, AdminBlock, Target), CommandError> {
    ctx.require(Scope::AppsRead)?;
    let app = installed(apps, key).await?;
    let not_found = || CommandError::ExtensionNotFound {
        app: app.key().clone(),
        id: id.to_string(),
    };
    let block = app
        .manifest()
        .and_then(|m| m.manifest.document().admin.as_ref())
        .and_then(|a| a.blocks.iter().find(|b| b.id == id))
        .cloned()
        .ok_or_else(not_found)?;
    let target = wanted_target(&block.target, Surface::Admin, None)
        .filter(|t| t.takes_blocks())
        .ok_or_else(not_found)?;
    may_use(ctx, &app, target)?;
    context
        .check(target)
        .map_err(CommandError::InvalidContext)?;
    Ok((app, block, target))
}

/// The app and its proxy URL on `surface` (`app_proxy_not_found` without one). The
/// admin proxy needs `apps:read`, the storefront's `stores:read` (every publishable
/// key).
pub async fn find_proxy<R: AppRepo>(
    ctx: &Ctx,
    apps: &R,
    key: &str,
    surface: Surface,
) -> Result<(App, String), CommandError> {
    ctx.require(match surface {
        Surface::Admin => Scope::AppsRead,
        Surface::Storefront => Scope::StoresRead,
    })?;
    let app = installed(apps, key).await?;
    let doc = app.manifest().map(|m| m.manifest.document());
    let url = match surface {
        Surface::Admin => doc.and_then(|d| d.admin.as_ref()?.proxy.clone()),
        Surface::Storefront => doc.and_then(|d| d.storefront.as_ref()?.proxy.clone()),
    };
    let url = url.ok_or_else(|| CommandError::ProxyNotFound(app.key().clone()))?;
    Ok((app, url))
}

/// The app a staff member opens an embedded page of (a session token, §6.3), and the
/// scopes the token carries: the app's grant as far as the caller holds it.
pub async fn session_app<R: AppRepo>(
    ctx: &Ctx,
    apps: &R,
    key: &str,
) -> Result<(App, Vec<Scope>), CommandError> {
    ctx.require(Scope::AppsRead)?;
    let app = installed(apps, key).await?;
    let scopes = scopes_for(ctx, &app);
    Ok((app, scopes))
}

/// `app`'s grant as far as `ctx` holds it: what a session token or a forwarded call
/// says the app may do for this caller.
pub fn scopes_for(ctx: &Ctx, app: &App) -> Vec<Scope> {
    app.grant()
        .map(|g| {
            g.scopes()
                .iter()
                .copied()
                .filter(|s| ctx.has_scope(*s))
                .collect()
        })
        .unwrap_or_default()
}
