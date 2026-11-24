//! What needs no other domain: reading the settings, the templates in effect, turning one
//! off, reverting one, and the record of messages. Saving a template (checked against its
//! sample), the settings (the logo's asset usage), previews and sends are the workflow's,
//! `hypha-mail`.

use hypha_core::{
    AuthError, Ctx, ErrorCode, Language, NotificationId, Page, PageRequest, RepoError, Scope,
    retry_on_conflict,
};
use thiserror::Error;

use crate::{
    DefaultTemplate, HasNotificationSettings, HasNotificationTemplates, HasNotifications,
    NewTemplate, Notification, NotificationError, NotificationFilter, NotificationRepo,
    NotificationSettings, NotificationSettingsRepo, NotificationTemplateRepo, SettingsError,
    Template, TemplateError, TemplateKey, TemplateProblem, TemplateSources, default_template,
    default_template_in, keys,
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
    #[error("{0} isn't one of the store's languages")]
    LanguageNotListed(Language),
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
            CommandError::LanguageNotListed(_) => "language_not_listed",
        }
    }
}

/// The store's languages as templates need them, as plain values: the leaf can't see the
/// store.
#[derive(Debug, Clone, Copy)]
pub struct TemplateLanguages<'a> {
    /// The store's default language: its templates are the `''` rows.
    pub default: &'a Language,
    /// Its other languages, published or not.
    pub others: &'a [Language],
}

impl TemplateLanguages<'_> {
    /// The row `language` names: `None` (the `''` row) for none or the store's default, so
    /// no row is ever keyed by the default language itself. `Err` with the language when the
    /// store doesn't have it (`language_not_listed`).
    pub fn locale(&self, language: Option<&Language>) -> Result<Option<Language>, Language> {
        match language {
            None => Ok(None),
            Some(l) if l == self.default => Ok(None),
            Some(l) if self.others.contains(l) => Ok(Some(l.clone())),
            Some(l) => Err(l.clone()),
        }
    }
}

/// Where sources come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// The merchant's, saved for `locale` (`None`: the store's default language).
    Merchant { locale: Option<Language> },
    /// The build's default in `language`.
    Default { language: Language },
}

/// One place a send looks for sources, in `lookup_chain` order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// The merchant's row for this locale (`None`: the `''` row).
    Merchant(Option<Language>),
    /// The build's default written in this language.
    Default(Language),
}

/// Where an email in `language` takes its sources from, first to last, for a store whose
/// default language is `default`: the merchant's for the language, then for it without its
/// region; the build's for the language, then without its region; then the default
/// language's chain: the merchant's `''` row, the build's in the default language (and
/// without its region), and the English default, which every key has. A tag that is the
/// default language is its `''` row. For `de-AT` in an English store: the merchant's
/// `de-AT`, their `de`, the build's `de-AT`, its `de`, the merchant's `''`, English.
pub fn lookup_chain(language: &Language, default: &Language) -> Vec<Step> {
    let row = |tag: &Language| (tag != default).then(|| tag.clone());
    let base = language.without_region();
    let english = Language::new("en").ok();
    let mut steps = vec![Step::Merchant(row(language))];
    steps.extend(base.iter().map(|b| Step::Merchant(row(b))));
    steps.push(Step::Default(language.clone()));
    steps.extend(base.map(Step::Default));
    steps.push(Step::Merchant(None));
    steps.push(Step::Default(default.clone()));
    steps.extend(default.without_region().map(Step::Default));
    steps.extend(english.map(Step::Default));
    let mut chain: Vec<Step> = Vec::with_capacity(steps.len());
    for step in steps {
        if !chain.contains(&step) {
            chain.push(step);
        }
    }
    chain
}

/// A template as the store has it in one language: the merchant's sources or the next ones
/// its lookup chain finds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveTemplate {
    pub key: TemplateKey,
    /// The row it's for: `None` is the store's default language.
    pub locale: Option<Language>,
    /// What the editor shows: this row's sources when edited (even ones that stopped
    /// parsing), else what sends use.
    pub sources: TemplateSources,
    /// Whether it's sent: a key's switch, the default language's row's, for every language.
    pub enabled: bool,
    /// Whether this row's sources are the merchant's.
    pub customized: bool,
    /// Why this row's sources no longer parse; sends use the next in the chain meanwhile.
    pub problem: Option<TemplateProblem>,
    /// What sends use: the first sources in the chain that parse.
    sending: TemplateSources,
    /// Where they come from, and the language they're written in.
    origin: Origin,
    language: Language,
    /// The build's default a send falls back to when `sending` doesn't render, and its
    /// language.
    fallback: (DefaultTemplate, Language),
}

impl EffectiveTemplate {
    /// `key` for `locale` (`None`: the store's default language, `default`) as `stored`
    /// (every row the store has, any key) makes it.
    pub fn resolve(
        key: TemplateKey,
        stored: &[Template],
        locale: Option<&Language>,
        default: &Language,
    ) -> Self {
        let row = |locale: Option<&Language>| {
            stored
                .iter()
                .find(|t| t.key() == key && t.locale() == locale)
        };
        let own = row(locale);
        let asked = locale.unwrap_or(default);
        let english = || (default_template(key), Language::new("en").ok());
        let mut sending = None;
        let mut fallback = None;
        for step in lookup_chain(asked, default) {
            match step {
                Step::Merchant(locale) => {
                    if sending.is_some() {
                        continue;
                    }
                    let usable = row(locale.as_ref())
                        .filter(|t| t.problem().is_none())
                        .and_then(Template::sources);
                    if let Some(sources) = usable {
                        let language = locale.clone().unwrap_or_else(|| default.clone());
                        sending = Some((sources.clone(), Origin::Merchant { locale }, language));
                    }
                }
                Step::Default(language) => {
                    let Some(built) = default_template_in(key, &language) else {
                        continue;
                    };
                    if sending.is_none() {
                        sending = Some((
                            built.sources(),
                            Origin::Default {
                                language: language.clone(),
                            },
                            language.clone(),
                        ));
                    }
                    if fallback.is_none() {
                        fallback = Some((built, language));
                    }
                }
            }
        }
        let (fallback_default, fallback_language) = match fallback {
            Some(found) => found,
            // The chain ends with English, which every key has.
            None => {
                let (built, en) = english();
                (built, en.unwrap_or_else(|| default.clone()))
            }
        };
        let (sending, origin, language) = sending.unwrap_or_else(|| {
            (
                fallback_default.sources(),
                Origin::Default {
                    language: fallback_language.clone(),
                },
                fallback_language.clone(),
            )
        });
        let edited = own.and_then(Template::sources);
        Self {
            key,
            locale: locale.cloned(),
            sources: edited.cloned().unwrap_or_else(|| sending.clone()),
            enabled: row(None).is_none_or(Template::enabled),
            customized: edited.is_some(),
            problem: own.and_then(Template::problem).cloned(),
            sending,
            origin,
            language,
            fallback: (fallback_default, fallback_language),
        }
    }

    /// The sources a send uses: the first in the chain that parse, so a template broken by
    /// an upgrade doesn't leave a customer without an email.
    pub fn sending_sources(&self) -> TemplateSources {
        self.sending.clone()
    }

    /// Where `sending_sources` come from.
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The language `sending_sources` are written in.
    pub fn language(&self) -> &Language {
        &self.language
    }

    /// Whether what sends use is the merchant's (so a failure to render falls back).
    pub fn sends_merchants(&self) -> bool {
        matches!(self.origin, Origin::Merchant { .. })
    }

    /// The build's default a send falls back to when the merchant's sources don't render,
    /// and the language it's written in.
    pub fn fallback(&self) -> (TemplateSources, &Language) {
        (self.fallback.0.sources(), &self.fallback.1)
    }

    /// The language `sources` (what the editor shows) are written in, in a store whose
    /// default language is `default`: this row's when the merchant edited it, else that of
    /// the sources its emails send.
    pub fn sources_language<'a>(&'a self, default: &'a Language) -> &'a Language {
        match (self.customized, &self.locale) {
            (true, Some(locale)) => locale,
            (true, None) => default,
            (false, _) => &self.language,
        }
    }

    /// This template with unsaved `sources` (a preview's) in place of what the editor
    /// shows, sent as they are and written in the language of the sources they replace
    /// (`sources_language`): a preview of a language's tab that still shows another
    /// language's sources renders as that language's emails send.
    pub fn with_sources(self, sources: TemplateSources, default: &Language) -> Self {
        let language = self.sources_language(default).clone();
        Self {
            origin: Origin::Merchant {
                locale: self.locale.clone(),
            },
            sending: sources.clone(),
            sources,
            language,
            ..self
        }
    }
}

/// A template in the list: in the default language, and the other languages the merchant
/// edited it in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedTemplate {
    pub template: EffectiveTemplate,
    /// The locales with the merchant's own sources, in the order they're stored.
    pub languages: Vec<Language>,
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

/// `key` in effect for `locale` (`None`: the store's default language, `default`). No
/// scope, like `settings_or_default`.
pub async fn template_in_effect<S: HasNotificationTemplates>(
    ports: &S,
    key: TemplateKey,
    locale: Option<&Language>,
    default: &Language,
) -> Result<EffectiveTemplate, RepoError> {
    let stored = ports.notification_templates().list().await?;
    Ok(EffectiveTemplate::resolve(key, &stored, locale, default))
}

pub async fn get_notification_settings<S: HasNotificationSettings>(
    ctx: &Ctx,
    ports: &S,
) -> Result<NotificationSettings, CommandError> {
    ctx.require(Scope::NotificationsRead)?;
    Ok(settings_or_default(ports).await?)
}

/// Every template in `TemplateKey::ALL` order, in the default language as stored or
/// built, with the other languages the merchant edited it in.
pub async fn list_notification_templates<S: HasNotificationTemplates>(
    ctx: &Ctx,
    ports: &S,
    default: &Language,
) -> Result<Vec<ListedTemplate>, CommandError> {
    ctx.require(Scope::NotificationsRead)?;
    let stored = ports.notification_templates().list().await?;
    Ok(TemplateKey::ALL
        .into_iter()
        .map(|key| ListedTemplate {
            template: EffectiveTemplate::resolve(key, &stored, None, default),
            languages: stored
                .iter()
                .filter(|t| t.key() == key && t.customized())
                .filter_map(|t| t.locale().cloned())
                .collect(),
        })
        .collect())
}

/// `key` for `language` (none: the store's default): its row's sources, else what its
/// chain sends. `language_not_listed` for a language the store doesn't have.
pub async fn get_notification_template<S: HasNotificationTemplates>(
    ctx: &Ctx,
    ports: &S,
    key: TemplateKey,
    language: Option<&Language>,
    languages: TemplateLanguages<'_>,
) -> Result<EffectiveTemplate, CommandError> {
    ctx.require(Scope::NotificationsRead)?;
    let locale = languages
        .locale(language)
        .map_err(CommandError::LanguageNotListed)?;
    Ok(template_in_effect(ports, key, locale.as_ref(), languages.default).await?)
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

/// Drops the merchant's sources for `language` (none: the store's default), keeping
/// whether it's on: it sends what its chain finds next again. Reverting a template that
/// wasn't edited changes nothing.
pub async fn revert_notification_template<S: HasNotificationTemplates>(
    ctx: &Ctx,
    ports: &S,
    key: TemplateKey,
    language: Option<&Language>,
    languages: TemplateLanguages<'_>,
) -> Result<EffectiveTemplate, CommandError> {
    ctx.require(Scope::NotificationsWrite)?;
    let locale = languages
        .locale(language)
        .map_err(CommandError::LanguageNotListed)?;
    let locale = locale.as_ref();
    retry_on_conflict(
        move || async move {
            let repo = ports.notification_templates();
            if let Some(mut template) = repo.get(key, locale).await? {
                template.set_sources(None);
                write(repo, template, true).await?;
            }
            Ok(template_in_effect(ports, key, locale, languages.default).await?)
        },
        lost_a_race,
    )
    .await
}

/// Turns a template on or off in every language (`template_required` for the ones that
/// can't be off), keeping its sources: the switch is the default language's row's.
pub async fn set_notification_template_enabled<S: HasNotificationTemplates>(
    ctx: &Ctx,
    ports: &S,
    key: TemplateKey,
    enabled: bool,
    default: &Language,
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
            write(repo, template, stored).await?;
            Ok(template_in_effect(ports, key, None, default).await?)
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
