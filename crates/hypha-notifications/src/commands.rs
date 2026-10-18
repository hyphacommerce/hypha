//! What needs no other domain (design note 0028, §11): reading the settings, the
//! templates in effect, turning one off, reverting one, and the record of messages.
//! Saving a template (checked against its sample), the settings (the logo's asset
//! usage), previews and sends are the workflow's, `hypha-mail`.

use hypha_core::{
    AuthError, Ctx, ErrorCode, NotificationId, Page, PageRequest, RepoError, Scope,
    retry_on_conflict,
};
use thiserror::Error;

use crate::{
    HasNotificationSettings, HasNotificationTemplates, HasNotifications, NewTemplate, Notification,
    NotificationError, NotificationFilter, NotificationRepo, NotificationSettings,
    NotificationSettingsRepo, NotificationTemplateRepo, SettingsError, Template, TemplateError,
    TemplateKey, TemplateProblem, TemplateSources, default_template, keys,
};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Template(#[from] TemplateError),
    #[error(transparent)]
    Settings(#[from] SettingsError),
    #[error(transparent)]
    Notification(#[from] NotificationError),
    #[error("notification {0} not found")]
    NotificationNotFound(NotificationId),
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Template(e) => e.code(),
            CommandError::Settings(e) => e.code(),
            CommandError::Notification(e) => e.code(),
            CommandError::NotificationNotFound(_) => "notification_not_found",
        }
    }
}

/// A template as the store has it: the merchant's sources or the build's default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveTemplate {
    pub key: TemplateKey,
    /// What the editor shows: the stored sources when edited, else the default's.
    pub sources: TemplateSources,
    pub enabled: bool,
    /// Whether the sources are the merchant's.
    pub customized: bool,
    /// Why the stored sources no longer parse (Q5); sends use the default meanwhile.
    pub problem: Option<TemplateProblem>,
}

impl EffectiveTemplate {
    /// `key` as `stored` (the default language's row, if any) makes it.
    pub fn of(key: TemplateKey, stored: Option<&Template>) -> Self {
        let edited = stored.and_then(Template::sources);
        Self {
            key,
            sources: edited
                .cloned()
                .unwrap_or_else(|| default_template(key).sources()),
            enabled: stored.is_none_or(Template::enabled),
            customized: edited.is_some(),
            problem: stored.and_then(Template::problem).cloned(),
        }
    }

    /// The sources a send uses: the merchant's while they parse, else the default's,
    /// so a template broken by an upgrade doesn't leave a customer without an email
    /// (Q5).
    pub fn sending_sources(&self) -> TemplateSources {
        match self.problem {
            Some(_) => default_template(self.key).sources(),
            None => self.sources.clone(),
        }
    }
}

/// The settings in effect: stored, else the defaults. No scope: for workflows that
/// send (the caller's scopes were checked there).
pub async fn settings_or_default<S: HasNotificationSettings>(
    ports: &S,
) -> Result<NotificationSettings, RepoError> {
    Ok(ports
        .notification_settings()
        .get()
        .await?
        .unwrap_or_default())
}

/// `key` in effect, in the default language. No scope, like `settings_or_default`.
pub async fn template_in_effect<S: HasNotificationTemplates>(
    ports: &S,
    key: TemplateKey,
) -> Result<EffectiveTemplate, RepoError> {
    let stored = ports.notification_templates().get(key, None).await?;
    Ok(EffectiveTemplate::of(key, stored.as_ref()))
}

pub async fn get_notification_settings<S: HasNotificationSettings>(
    ctx: &Ctx,
    ports: &S,
) -> Result<NotificationSettings, CommandError> {
    ctx.require(Scope::NotificationsRead)?;
    Ok(settings_or_default(ports).await?)
}

/// Every template in `TemplateKey::ALL` order, stored or default.
pub async fn list_notification_templates<S: HasNotificationTemplates>(
    ctx: &Ctx,
    ports: &S,
) -> Result<Vec<EffectiveTemplate>, CommandError> {
    ctx.require(Scope::NotificationsRead)?;
    let stored = ports.notification_templates().list().await?;
    Ok(TemplateKey::ALL
        .into_iter()
        .map(|key| {
            let mine = stored
                .iter()
                .find(|t| t.key() == key && t.locale().is_none());
            EffectiveTemplate::of(key, mine)
        })
        .collect())
}

pub async fn get_notification_template<S: HasNotificationTemplates>(
    ctx: &Ctx,
    ports: &S,
    key: TemplateKey,
) -> Result<EffectiveTemplate, CommandError> {
    ctx.require(Scope::NotificationsRead)?;
    Ok(template_in_effect(ports, key).await?)
}

/// Writes `template` as it now is: deleted when it's only the default (nothing left
/// to keep), else inserted or updated. `stored` says whether it was loaded.
async fn write<R: NotificationTemplateRepo>(
    repo: &R,
    mut template: Template,
    stored: bool,
) -> Result<Template, RepoError> {
    match (stored, template.is_default()) {
        (true, true) => repo.delete(&template).await?,
        (true, false) => repo.update(&mut template).await?,
        (false, false) => match repo.insert(&mut template).await {
            // Another request stored it first: a lost race, tried again on fresh data.
            Err(RepoError::Duplicate(keys::TEMPLATE_KEY)) => {
                return Err(RepoError::StaleVersion);
            }
            done => done?,
        },
        (false, true) => {}
    }
    Ok(template)
}

fn lost_a_race(e: &CommandError) -> bool {
    matches!(e, CommandError::Repo(RepoError::StaleVersion))
}

/// Goes back to the default's sources, keeping whether it's on. Reverting a template
/// that wasn't edited changes nothing.
pub async fn revert_notification_template<S: HasNotificationTemplates>(
    ctx: &Ctx,
    ports: &S,
    key: TemplateKey,
) -> Result<EffectiveTemplate, CommandError> {
    ctx.require(Scope::NotificationsWrite)?;
    retry_on_conflict(
        move || async move {
            let repo = ports.notification_templates();
            let Some(mut template) = repo.get(key, None).await? else {
                return Ok(EffectiveTemplate::of(key, None));
            };
            template.set_sources(None);
            let template = write(repo, template, true).await?;
            Ok(EffectiveTemplate::of(key, Some(&template)))
        },
        lost_a_race,
    )
    .await
}

/// Turns a template on or off (`template_required` for the ones that can't be off),
/// keeping its sources.
pub async fn set_notification_template_enabled<S: HasNotificationTemplates>(
    ctx: &Ctx,
    ports: &S,
    key: TemplateKey,
    enabled: bool,
) -> Result<EffectiveTemplate, CommandError> {
    ctx.require(Scope::NotificationsWrite)?;
    retry_on_conflict(
        move || async move {
            let repo = ports.notification_templates();
            let (template, stored) = match repo.get(key, None).await? {
                Some(mut template) => {
                    template.set_enabled(enabled)?;
                    (template, true)
                }
                None => {
                    let template = Template::new(NewTemplate {
                        key,
                        locale: None,
                        sources: None,
                        enabled,
                    })?;
                    (template, false)
                }
            };
            let template = write(repo, template, stored).await?;
            Ok(EffectiveTemplate::of(key, Some(&template)))
        },
        lost_a_race,
    )
    .await
}

/// A page of messages, newest first: an order's emails (`filter.order`), or every one
/// of a template or status. Records name buyers' addresses and their orders, so reading
/// them takes `orders:read` too.
pub async fn list_notifications<S: HasNotifications>(
    ctx: &Ctx,
    ports: &S,
    filter: NotificationFilter,
    page: PageRequest<NotificationId>,
) -> Result<Page<Notification>, CommandError> {
    ctx.require(Scope::NotificationsRead)?;
    ctx.require(Scope::OrdersRead)?;
    Ok(ports.notifications().list(filter, page).await?)
}

/// One message's record; `orders:read` too, as `list_notifications`.
pub async fn get_notification<S: HasNotifications>(
    ctx: &Ctx,
    ports: &S,
    id: NotificationId,
) -> Result<Notification, CommandError> {
    ctx.require(Scope::NotificationsRead)?;
    ctx.require(Scope::OrdersRead)?;
    ports
        .notifications()
        .get(id)
        .await?
        .ok_or(CommandError::NotificationNotFound(id))
}
