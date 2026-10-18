//! Rendering and sending one message, with its record (design note 0028, §4, NT8): the
//! steps the handler, resends and the staff flows share.

use hypha_core::{Clock, Email, EventId, IdGenerator, NotificationId, OrderId, RepoError};
use hypha_media::{AssetRepo, HasAssets};
use hypha_notifications::{
    EffectiveTemplate, EmailSender, HasNotificationSettings, HasNotificationTemplates,
    HasNotifications, Mailbox, NewNotification, Notification, NotificationRepo,
    NotificationSettings, NotificationStatus, OutgoingEmail, RenderError, Rendered, TemplateKey,
    default_template, render, settings_or_default, template_in_effect,
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

/// `template` rendered in `layout` against `context`, both as sends use them
/// (`EffectiveTemplate::sending_sources`). When the merchant's version doesn't render
/// (a context rule an upgrade added, say), the defaults are used for this send rather
/// than leaving the customer without an email (Q5).
pub fn render_for_sending(
    layout: &EffectiveTemplate,
    template: &EffectiveTemplate,
    context: &MailContext,
) -> Result<Rendered, RenderError> {
    let first = render(
        layout.sending_sources().html(),
        &template.sending_sources(),
        context,
    );
    match first {
        Err(_) if layout.customized || template.customized => render(
            default_template(TemplateKey::Layout).html,
            &default_template(template.key).sources(),
            context,
        ),
        done => done,
    }
}

/// One message to send.
pub struct Message {
    pub template: TemplateKey,
    pub to: Email,
    /// The event it answers, so a second run finds it (NT8); `None` for direct sends
    /// and resends.
    pub event: Option<EventId>,
    pub order: Option<OrderId>,
    pub context: MailContext,
    /// Send even when the template is turned off: a resend staff asked for.
    pub even_if_disabled: bool,
}

/// Why a template that's off isn't sent, on its record.
const DISABLED: &str = "the template is turned off";

/// Sends `message` and records it (design note 0028, §4):
/// - answering an event, a message already `sent` or `skipped` for it is the answer: nothing more goes out;
///   a `sending` or `failed` one is tried again;
/// - a template that's off is recorded `skipped` (unless `even_if_disabled`);
/// - otherwise it's rendered, recorded `sending`, sent, and marked `sent` or `failed`.
///
/// A transient failure is `Err(Send)` after the record says `failed`, so the dispatcher
/// tries again; a permanent one is the `failed` record, `Ok`, and not retried.
pub async fn deliver<E, P>(
    env: &E,
    ports: &P,
    branding: &Branding,
    message: Message,
) -> Result<Notification, CommandError>
where
    E: MailEnv,
    P: HasNotificationTemplates + HasNotifications,
{
    let records = ports.notifications();
    let earlier = match message.event {
        Some(event) => records.find(event, message.template, &message.to).await?,
        None => None,
    };
    if let Some(done) = &earlier
        && matches!(
            done.status(),
            NotificationStatus::Sent | NotificationStatus::Skipped
        )
    {
        return Ok(done.clone());
    }
    let template = template_in_effect(ports, message.template).await?;
    let layout = template_in_effect(ports, TemplateKey::Layout).await?;
    let rendered = render_for_sending(&layout, &template, &message.context)?;

    let new = NewNotification {
        id: NotificationId::from_uuid(env.ids().new_uuid()),
        template: message.template,
        to: message.to.clone(),
        subject: rendered.subject.clone(),
        event: message.event,
        order: message.order,
    };
    if !template.enabled && !message.even_if_disabled {
        // One tried before it was turned off stays as it was.
        if let Some(earlier) = earlier {
            return Ok(earlier);
        }
        let mut skipped = Notification::skipped(new, DISABLED);
        records.insert(&mut skipped).await?;
        return Ok(skipped);
    }
    let mut record = match earlier {
        Some(mut earlier) => {
            earlier.retry()?;
            records.update(&mut earlier).await?;
            earlier
        }
        None => {
            let mut record = Notification::sending(new);
            // A `Duplicate` here is another run of the same delivery that got there
            // first: an error, so the dispatcher comes back and finds its record.
            records.insert(&mut record).await?;
            record
        }
    };

    let settings = &branding.settings;
    let email = OutgoingEmail {
        id: record.id(),
        from: Mailbox {
            name: Some(
                settings
                    .sender_name()
                    .unwrap_or(branding.store.name())
                    .to_string(),
            ),
            address: env.sender_address().clone(),
        },
        reply_to: settings.reply_to().cloned(),
        to: message.to,
        subject: rendered.subject,
        html: rendered.html,
        text: rendered.text,
    };
    let sender = env.sender();
    match sender.send(&email).await {
        Ok(sent) => {
            record.sent(sender.name(), sent.provider_message_id, env.clock().now())?;
            records.update(&mut record).await?;
            Ok(record)
        }
        Err(error) => {
            record.failed(sender.name(), error.message())?;
            records.update(&mut record).await?;
            if error.is_transient() {
                return Err(CommandError::Send(error));
            }
            Ok(record)
        }
    }
}

/// Whether `e` is a write that lost a race, worth running again on fresh data.
pub(crate) fn lost_a_race(e: &CommandError) -> bool {
    matches!(e, CommandError::Repo(RepoError::StaleVersion))
}
