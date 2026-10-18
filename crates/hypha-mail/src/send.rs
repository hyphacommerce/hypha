//! Rendering, queueing and sending messages, with their records (design note 0028, §4,
//! NT8): the steps the handler, resends, the send job and the staff flows share.
//!
//! Messages about events (and resends) are rendered and recorded `queued`, without any
//! network: that's what the `Mailer` handler and `resend` do. The send job then leases
//! what's due (`lease_due`), sends each with no connection held (`send_leased`), and
//! records how it went (`record_attempt`), the way the dispatcher delivers webhooks.
//! Staff invites and resets are sent at once by their request (`send_now`): the flow
//! waits for them, and their token never lands in a stored body.

use hypha_core::{Clock, Email, EventId, IdGenerator, NotificationId, OrderId, RepoError};
use hypha_media::{AssetRepo, HasAssets};
use hypha_notifications::{
    EffectiveTemplate, EmailSender, HasNotificationSettings, HasNotificationTemplates,
    HasNotifications, LONGEST_SEND, Mailbox, NewNotification, Notification, NotificationRepo,
    NotificationSettings, OutgoingEmail, QueuedEmail, RenderError, Rendered, SendError,
    SendSettings, Sent, TemplateKey, default_template, keys, render, settings_or_default,
    template_in_effect,
};
use hypha_stores::{Store, StoreRepo};

use crate::{
    CommandError, MailEnv,
    context::{BrandContext, MailContext, ShopContext},
};

/// What every email of the store shares: the store, its notification settings and what
/// templates see of them.
#[derive(Debug, Clone)]
pub struct Branding {
    pub store: Store,
    pub settings: NotificationSettings,
    pub shop: ShopContext,
    pub brand: BrandContext,
}

impl Branding {
    /// The sender name emails go out under: the merchant's, else the store's name.
    pub fn sender_name(&self) -> &str {
        self.settings.sender_name().unwrap_or(self.store.name())
    }

    /// `rendered` as the send job will send it, with the sender name and reply-to in
    /// effect now.
    pub fn queued_email(&self, rendered: Rendered) -> QueuedEmail {
        QueuedEmail {
            sender_name: Some(self.sender_name().to_string()),
            reply_to: self.settings.reply_to().cloned(),
            html: rendered.html,
            text: rendered.text,
        }
    }
}

/// Loads the store (`MailEnv::store`), the settings in effect and the logo's URL.
pub async fn branding<E, P>(env: &E, ports: &P) -> Result<Branding, CommandError>
where
    E: MailEnv,
    P: StoreRepo + HasNotificationSettings + HasAssets,
{
    let id = env.store();
    let store = ports
        .get(id)
        .await?
        .ok_or(CommandError::StoreNotFound(id))?;
    let settings = settings_or_default(ports).await?;
    let logo_url = match settings.logo() {
        Some(logo) => match ports.assets().get(logo).await? {
            Some(asset) => env.asset_url(&asset).await,
            // Its usage keeps it from being deleted; gone anyway, the email has none.
            None => None,
        },
        None => None,
    };
    Ok(Branding {
        shop: ShopContext::of(&store, &settings),
        brand: BrandContext::of(&settings, logo_url),
        store,
        settings,
    })
}

/// A template with the layout it's rendered in, both as the store has them, loaded once
/// for every recipient of a message.
#[derive(Debug, Clone)]
pub struct Sources {
    pub layout: EffectiveTemplate,
    pub template: EffectiveTemplate,
}

impl Sources {
    /// `key` and the layout, in effect.
    pub async fn load<P: HasNotificationTemplates>(
        ports: &P,
        key: TemplateKey,
    ) -> Result<Self, RepoError> {
        Ok(Self {
            layout: template_in_effect(ports, TemplateKey::Layout).await?,
            template: template_in_effect(ports, key).await?,
        })
    }
}

/// A rendering, with a note for the record when it isn't the merchant's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendering {
    pub rendered: Rendered,
    /// "sent with the default template: …" when the merchant's version didn't render
    /// and the default was used instead (Q5).
    pub note: Option<String>,
}

/// The note on a message rendered with the defaults because the merchant's version
/// failed with `error`.
fn fallback_note(error: &RenderError) -> String {
    format!("sent with the default template: {error}")
}

/// `sources` rendered against `context` as sends use them
/// (`EffectiveTemplate::sending_sources`). When the merchant's version doesn't render (a
/// context rule an upgrade added, a value that's null on a real order), the defaults are
/// used for this message rather than leaving the customer without an email (Q5); that's
/// logged and noted on the rendering, so staff see it on the record.
pub fn render_for_sending(
    sources: &Sources,
    context: &MailContext,
) -> Result<Rendering, RenderError> {
    let Sources { layout, template } = sources;
    let first = render(
        layout.sending_sources().html(),
        &template.sending_sources(),
        context,
    );
    match first {
        Err(error) if layout.customized || template.customized => {
            tracing::warn!(
                template = template.key.as_str(),
                %error,
                "email: the store's template didn't render; sending the default"
            );
            let rendered = render(
                default_template(TemplateKey::Layout).html,
                &default_template(template.key).sources(),
                context,
            )?;
            Ok(Rendering {
                rendered,
                note: Some(fallback_note(&error)),
            })
        }
        done => done.map(|rendered| Rendering {
            rendered,
            note: None,
        }),
    }
}

/// Who a message goes to and what it's about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub template: TemplateKey,
    pub to: Email,
    /// The event it answers, so a second run finds it (NT8); `None` for direct sends
    /// and resends.
    pub event: Option<EventId>,
    pub order: Option<OrderId>,
}

impl Message {
    fn record(&self, env: &impl MailEnv, subject: String) -> NewNotification {
        NewNotification {
            id: NotificationId::from_uuid(env.ids().new_uuid()),
            template: self.template,
            to: self.to.clone(),
            subject,
            event: self.event,
            order: self.order,
        }
    }
}

/// Why a template that's off isn't sent, on its record.
pub(crate) const DISABLED: &str = "the template is turned off";

/// Stores `record` (queued or skipped). Answering an event, a `Duplicate` of its (event,
/// template, recipient) is another run of the same delivery that got there first: its
/// record is the answer.
pub(crate) async fn record_once<P: HasNotifications>(
    ports: &P,
    mut record: Notification,
) -> Result<Notification, CommandError> {
    let records = ports.notifications();
    match records.insert(&mut record).await {
        Ok(()) => Ok(record),
        Err(RepoError::Duplicate(keys::NOTIFICATION_MESSAGE)) => {
            let found = match record.event() {
                Some(event) => records.find(event, record.template(), record.to()).await?,
                None => None,
            };
            found.ok_or(CommandError::Repo(RepoError::Duplicate(
                keys::NOTIFICATION_MESSAGE,
            )))
        }
        Err(e) => Err(e.into()),
    }
}

/// Records `message` as `queued` with `rendering`, due now, for the send job.
pub(crate) async fn queue<E: MailEnv, P: HasNotifications>(
    env: &E,
    ports: &P,
    branding: &Branding,
    message: &Message,
    rendering: &Rendering,
) -> Result<Notification, CommandError> {
    let record = Notification::queued(
        message.record(env, rendering.rendered.subject.clone()),
        branding.queued_email(rendering.rendered.clone()),
        env.clock().now(),
    )
    .with_note(rendering.note.as_deref());
    record_once(ports, record).await
}

/// Records `message` as `skipped` (its template is off), without rendering it: nothing
/// it says is kept, so its subject is empty.
pub(crate) async fn skip<E: MailEnv, P: HasNotifications>(
    env: &E,
    ports: &P,
    message: &Message,
) -> Result<Notification, CommandError> {
    record_once(
        ports,
        Notification::skipped(message.record(env, String::new()), DISABLED),
    )
    .await
}

/// The sender emails go out with: the operator's address and `name`.
fn mailbox(env: &impl MailEnv, name: Option<&str>) -> Mailbox {
    Mailbox {
        name: name.map(str::to_string),
        address: env.sender_address().clone(),
    }
}

/// Sends `message` at once (staff invites and resets, design note 0028, §4): rendered
/// with `sources` against `context`, recorded `sending`, sent, then `sent` or `failed`
/// (no retry: the request that asked for it reports the failure). The record is
/// returned either way; a storage error is `Err`.
pub async fn send_now<E, P>(
    env: &E,
    ports: &P,
    branding: &Branding,
    sources: &Sources,
    message: Message,
    context: &MailContext,
) -> Result<Notification, CommandError>
where
    E: MailEnv,
    P: HasNotifications,
{
    let rendering = render_for_sending(sources, context)?;
    let rendered = rendering.rendered;
    let mut record = Notification::sending(message.record(env, rendered.subject.clone()))
        .with_note(rendering.note.as_deref());
    let records = ports.notifications();
    records.insert(&mut record).await?;
    let email = OutgoingEmail {
        id: record.id(),
        from: mailbox(env, Some(branding.sender_name())),
        reply_to: branding.settings.reply_to().cloned(),
        to: message.to,
        subject: rendered.subject,
        html: rendered.html,
        text: rendered.text,
    };
    let sender = env.sender();
    match sender.send(&email).await {
        Ok(sent) => record.sent(sender.name(), sent.provider_message_id, env.clock().now())?,
        Err(error) => record.failed(sender.name(), error.message())?,
    }
    records.update(&mut record).await?;
    Ok(record)
}

/// Leases the messages due now for the send job (`SendSettings::batch` of them, for
/// `SendSettings::lease`). Short: get ports for it, then let them go.
pub async fn lease_due<P: HasNotifications>(
    ports: &P,
    clock: &impl Clock,
    settings: &SendSettings,
) -> Result<Vec<Notification>, RepoError> {
    let now = clock.now();
    ports
        .notifications()
        .lease_due(now, now.plus(settings.lease), settings.batch)
        .await
}

/// Whether `leased` can still be sent and recorded within its lease. The rest of a batch
/// that's run out of time is left: its leases end, and the next lease takes them.
pub fn has_time(leased: &Notification, clock: &impl Clock) -> bool {
    leased
        .lease_until()
        .is_some_and(|until| clock.now().plus(LONGEST_SEND) <= until)
}

/// How one send went: the sender's name and what it said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub provider: &'static str,
    pub result: Result<Sent, SendError>,
}

/// Sends a leased message: no ports, so no connection waits on the provider. The
/// message's ID is the provider's idempotency key where it has one (Resend: 24 hours),
/// so a lease taken again after a runner stopped mid-send doesn't deliver twice there.
pub async fn send_leased<E: MailEnv>(env: &E, leased: &Notification) -> Attempt {
    let sender = env.sender();
    let provider = sender.name();
    let Some(queued) = leased.email() else {
        // Leasing keeps the email (`Notification::lease`); a record without one has
        // nothing to send.
        return Attempt {
            provider,
            result: Err(SendError::Permanent("nothing to send".to_string())),
        };
    };
    let email = OutgoingEmail {
        id: leased.id(),
        from: mailbox(env, queued.sender_name.as_deref()),
        reply_to: queued.reply_to.clone(),
        to: leased.to().clone(),
        subject: leased.subject().to_string(),
        html: queued.html.clone(),
        text: queued.text.clone(),
    };
    Attempt {
        provider,
        result: sender.send(&email).await,
    }
}

/// Records how `attempt` at `leased` went: `sent`; `failed` for a refusal, or for a
/// transient failure once the schedule is used up; else queued again for the
/// schedule's next attempt. `StaleVersion` when the record changed since it was leased
/// (its lease ran out and another run took it).
pub async fn record_attempt<P: HasNotifications>(
    ports: &P,
    clock: &impl Clock,
    settings: &SendSettings,
    mut leased: Notification,
    attempt: Attempt,
) -> Result<Notification, CommandError> {
    let now = clock.now();
    match attempt.result {
        Ok(sent) => leased.sent(attempt.provider, sent.provider_message_id, now)?,
        Err(SendError::Transient(error)) => match settings.next_attempt(leased.attempts(), now) {
            Some(next) => leased.retry_at(attempt.provider, &error, next)?,
            None => leased.failed(attempt.provider, &error)?,
        },
        Err(SendError::Permanent(error)) => leased.failed(attempt.provider, &error)?,
    }
    ports.notifications().update(&mut leased).await?;
    Ok(leased)
}

/// What a run of `send_due` did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SendRun {
    /// The records as each attempt left them, in the lease's order.
    pub attempted: Vec<Notification>,
    /// Leased but not attempted: too little of the lease left (they wait for it to end).
    pub left: usize,
}

/// One step of the send job with one set of ports for all of it: lease, send, record.
/// For tests and a host whose connections aren't pooled; a pooled host runs the parts
/// itself so no connection waits on the provider (`hypha_http::jobs`).
pub async fn send_due<E, P>(
    env: &E,
    ports: &P,
    settings: &SendSettings,
) -> Result<SendRun, CommandError>
where
    E: MailEnv,
    P: HasNotifications,
{
    let leased = lease_due(ports, env.clock(), settings).await?;
    let mut run = SendRun::default();
    for message in leased {
        if !has_time(&message, env.clock()) {
            run.left += 1;
            continue;
        }
        let attempt = send_leased(env, &message).await;
        run.attempted
            .push(record_attempt(ports, env.clock(), settings, message, attempt).await?);
    }
    Ok(run)
}

/// Whether `e` is a write that lost a race, worth running again on fresh data.
pub(crate) fn lost_a_race(e: &CommandError) -> bool {
    matches!(e, CommandError::Repo(RepoError::StaleVersion))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fallback_note_names_the_error() {
        let error = RenderError::TooComplex(hypha_notifications::TemplatePart::Html);
        assert_eq!(
            fallback_note(&error),
            "sent with the default template: the html takes too long to render"
        );
    }
}
