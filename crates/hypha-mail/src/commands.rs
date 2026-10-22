//! What needs other domains: saving the settings with the logo's asset usage, saving a
//! template (checked against its sample), previews, test sends, resending a message, and
//! the staff flows' direct emails.

use hypha_core::{
    Actor, AssetId, Clock, Ctx, Email, HasBatch, HasOutbox, IdGenerator, NotificationId, Notifier,
    NotifyError, OrderId, OutboxRepo, RepoError, Scope, retry_on_conflict,
};
use hypha_customer_accounts::CustomerEmail;
use hypha_media::{
    AssetKind, AssetRepo, AssetStatus, HasAssets, StageAssetUsages, UsageOwner, UsageRole,
    Visibility, repo::keys as media_keys,
};
use hypha_notifications::{
    Colour, EffectiveTemplate, EmailSender, HasNotificationSettings, HasNotificationTemplates,
    HasNotifications, Mailbox, NewTemplate, Notification, NotificationRepo, NotificationSettings,
    NotificationSettingsRepo, NotificationStatus, NotificationTemplateRepo, OrderUrl,
    OutgoingEmail, Rendered, StageNotificationSettings, Template, TemplateKey, TemplateSources,
    keys, render, template_in_effect,
};
use hypha_payments::newest_payment;
use hypha_stores::StoreRepo;
use hypha_users::{HasUsers, StaffEmail, UserRepo};

use crate::{
    CommandError, MailEnv, MailPorts,
    context::{CodeMail, ErasureMail, MailContext, StaffMail, login_link, user_context},
    format::TimeValue,
    handler::{load_fulfillment, load_order, order_context},
    send::{
        Branding, Message, Sources, branding, lost_a_race, queue, render_for_sending, send_now,
    },
};

/// A change to the notification settings, as JSON Merge Patch reads: `None` keeps a field,
/// `Some(None)` clears it. Raw input: checked here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateSettings {
    /// An image from the media library: ready, public.
    pub logo: Option<Option<AssetId>>,
    /// `#rrggbb`.
    pub accent: Option<String>,
    pub footer: Option<Option<String>>,
    pub sender_name: Option<Option<String>>,
    pub reply_to: Option<Option<String>>,
    pub staff_recipients: Option<Vec<String>>,
    /// An http(s) URL with `{order}` (and optionally `{token}`).
    pub order_url: Option<Option<String>>,
}

/// `UpdateSettings` checked: every value through its constructor.
struct Checked {
    logo: Option<Option<AssetId>>,
    accent: Option<Colour>,
    footer: Option<Option<String>>,
    sender_name: Option<Option<String>>,
    reply_to: Option<Option<Email>>,
    staff_recipients: Option<Vec<Email>>,
    order_url: Option<Option<OrderUrl>>,
}

fn check_settings(input: UpdateSettings) -> Result<Checked, CommandError> {
    let email = |text: &str| Email::new(text).map_err(hypha_notifications::SettingsError::from);
    Ok(Checked {
        logo: input.logo,
        accent: input.accent.as_deref().map(Colour::new).transpose()?,
        footer: input.footer,
        sender_name: input.sender_name,
        reply_to: input
            .reply_to
            .map(|r| r.as_deref().map(email).transpose())
            .transpose()?,
        staff_recipients: input
            .staff_recipients
            .map(|list| list.iter().map(|e| email(e)).collect::<Result<Vec<_>, _>>())
            .transpose()?,
        order_url: input
            .order_url
            .map(|u| u.as_deref().map(OrderUrl::new).transpose())
            .transpose()?,
    })
}

/// Changes the store's notification settings: the settings and the logo's asset usage
/// (`UsageOwner::notification_settings`, `UsageRole::logo`) in one batch, so a logo in use
/// can't be deleted from the library (as a category's image). The logo must exist, be
/// ready, public and an image. Retried on a lost race; the first save stores them.
pub async fn update_settings<S>(
    ctx: &Ctx,
    ports: &S,
    input: UpdateSettings,
) -> Result<NotificationSettings, CommandError>
where
    S: HasNotificationSettings
        + HasAssets
        + HasBatch<Batch: StageNotificationSettings + StageAssetUsages>,
{
    ctx.require(Scope::NotificationsWrite)?;
    let checked = check_settings(input)?;
    if let Some(Some(logo)) = checked.logo {
        check_logo(ports, logo).await?;
    }
    let checked = &checked;
    retry_on_conflict(
        move || async move {
            let stored = ports.notification_settings().get().await?;
            let first = stored.is_none();
            let mut settings = stored.unwrap_or_default();
            if let Some(logo) = checked.logo {
                settings.set_logo(logo);
            }
            if let Some(accent) = &checked.accent {
                settings.set_accent(accent.clone());
            }
            if let Some(footer) = &checked.footer {
                settings.set_footer(footer.clone())?;
            }
            if let Some(name) = &checked.sender_name {
                settings.set_sender_name(name.clone())?;
            }
            if let Some(reply_to) = &checked.reply_to {
                settings.set_reply_to(reply_to.clone());
            }
            if let Some(recipients) = &checked.staff_recipients {
                settings.set_staff_recipients(recipients.clone())?;
            }
            if let Some(order_url) = &checked.order_url {
                settings.set_order_url(order_url.clone());
            }
            let mut batch = ports.batch();
            if first {
                batch.insert_notification_settings(&mut settings);
            } else {
                batch.update_notification_settings(&mut settings);
            }
            if let Some(logo) = checked.logo {
                let usages: Vec<(AssetId, UsageRole)> =
                    logo.map(|a| (a, UsageRole::logo())).into_iter().collect();
                batch.replace_asset_usages(&UsageOwner::notification_settings(), &usages);
            }
            match ports.commit(batch).await {
                Ok(()) => Ok(settings),
                // Another request stored them first: a lost race.
                Err(RepoError::Duplicate(keys::SETTINGS)) => {
                    Err(CommandError::Repo(RepoError::StaleVersion))
                }
                // Deleted since it was checked: as if it was never there.
                Err(RepoError::Reference(key)) if key == media_keys::USAGE_ASSET => {
                    Err(match checked.logo.flatten() {
                        Some(logo) => CommandError::AssetNotFound(logo),
                        None => RepoError::Reference(key).into(),
                    })
                }
                Err(e) => Err(e.into()),
            }
        },
        lost_a_race,
    )
    .await
}

async fn check_logo<S: HasAssets>(ports: &S, id: AssetId) -> Result<(), CommandError> {
    let asset = ports
        .assets()
        .get(id)
        .await?
        .ok_or(CommandError::AssetNotFound(id))?;
    if asset.status() != AssetStatus::Ready {
        return Err(CommandError::AssetNotReady(id));
    }
    if asset.visibility() != Visibility::Public {
        return Err(CommandError::AssetNotPublic(id));
    }
    if asset.kind() != AssetKind::Image {
        return Err(CommandError::AssetKindNotAllowed {
            asset: id,
            kind: asset.kind(),
        });
    }
    Ok(())
}

/// A template's sources as the editor sends them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateInput {
    /// Empty for the layout.
    pub subject: String,
    pub html: String,
    pub text: Option<String>,
}

impl TemplateInput {
    fn sources(self, key: TemplateKey) -> Result<TemplateSources, CommandError> {
        Ok(TemplateSources::new(
            key,
            self.subject,
            self.html,
            self.text,
        )?)
    }
}

/// Saves a template's sources and whether it's on. The sources must parse, and render in
/// the layout in effect against the template's sample, and against it with every value that
/// may be absent absent (`MailContext::checks`): `template_invalid` with the part and line
/// otherwise, so a template that only works when an order has a payment or a shipping
/// address is refused here rather than failing on a real order. Saving the layout renders
/// every message template in it, so a layout that breaks one is refused. Turning off one
/// that can't be is `template_required`.
pub async fn save_template<S: HasNotificationTemplates>(
    ctx: &Ctx,
    ports: &S,
    key: TemplateKey,
    input: TemplateInput,
    enabled: bool,
) -> Result<EffectiveTemplate, CommandError> {
    ctx.require(Scope::NotificationsWrite)?;
    let sources = input.sources(key)?;
    if !enabled && !key.can_disable() {
        return Err(hypha_notifications::TemplateError::Required(key).into());
    }
    check_against_samples(ports, key, &sources).await?;
    let sources = &sources;
    retry_on_conflict(
        move || async move {
            let repo = ports.notification_templates();
            let template = match repo.get(key, None).await? {
                Some(mut template) => {
                    template.set_sources(Some(sources.clone()));
                    template.set_enabled(enabled)?;
                    repo.update(&mut template).await?;
                    template
                }
                None => {
                    let mut template = Template::new(NewTemplate {
                        key,
                        locale: None,
                        sources: Some(sources.clone()),
                        enabled,
                    })?;
                    match repo.insert(&mut template).await {
                        Err(RepoError::Duplicate(keys::TEMPLATE_KEY)) => {
                            return Err(CommandError::Repo(RepoError::StaleVersion));
                        }
                        done => done?,
                    }
                    template
                }
            };
            Ok(EffectiveTemplate::of(key, Some(&template)))
        },
        lost_a_race,
    )
    .await
}

/// Renders `sources` (of `key`) against the samples (`MailContext::checks`): a message
/// in the layout in effect, or the layout around every message template in effect.
async fn check_against_samples<S: HasNotificationTemplates>(
    ports: &S,
    key: TemplateKey,
    sources: &TemplateSources,
) -> Result<(), CommandError> {
    if key.is_layout() {
        for message in TemplateKey::ALL.into_iter().filter(|k| !k.is_layout()) {
            let template = template_in_effect(ports, message).await?;
            for context in MailContext::checks(message) {
                render(sources.html(), &template.sending_sources(), &context)?;
            }
        }
    } else {
        let layout = template_in_effect(ports, TemplateKey::Layout).await?;
        for context in MailContext::checks(key) {
            render(layout.sending_sources().html(), sources, &context)?;
        }
    }
    Ok(())
}

/// The context a preview of `key` renders with: the store's branding on the sample, or
/// `order` as it is now (for the order templates; the layout shows an order
/// confirmation).
async fn preview_context<E: MailEnv, P: MailPorts>(
    env: &E,
    ports: &P,
    branding: &Branding,
    key: TemplateKey,
    order: Option<OrderId>,
) -> Result<MailContext, CommandError> {
    let as_order = match key {
        TemplateKey::Layout => Some(TemplateKey::OrderConfirmation),
        TemplateKey::StaffInvite
        | TemplateKey::StaffPasswordReset
        | TemplateKey::CustomerLoginCode
        | TemplateKey::CustomerErasureRequested => None,
        other => Some(other),
    };
    match (order, as_order) {
        (Some(order), Some(template)) => {
            let order = load_order(ports, order).await?;
            let payment = newest_payment(ports, order.id()).await?;
            order_context(
                env,
                ports,
                branding,
                template,
                &order,
                payment.as_ref(),
                None,
            )
            .await
        }
        _ => {
            Ok(MailContext::sample(key)
                .with_branding(branding.shop.clone(), branding.brand.clone()))
        }
    }
}

/// Renders unsaved sources the way they'd send, with what `preview` renders them
/// against, and the branding (for a test send's sender).
async fn render_preview<E: MailEnv, P: MailPorts>(
    env: &E,
    ports: &P,
    key: TemplateKey,
    input: TemplateInput,
    order: Option<OrderId>,
) -> Result<(Rendered, Branding), CommandError> {
    let sources = input.sources(key)?;
    let branding = branding(env, ports).await?;
    let context = preview_context(env, ports, &branding, key, order).await?;
    let rendered = if key.is_layout() {
        let confirmation = template_in_effect(ports, TemplateKey::OrderConfirmation).await?;
        render(sources.html(), &confirmation.sending_sources(), &context)?
    } else {
        let layout = template_in_effect(ports, TemplateKey::Layout).await?;
        render(layout.sending_sources().html(), &sources, &context)?
    };
    Ok((rendered, branding))
}

/// Renders unsaved sources of `key`: against the sample with the store's branding, or
/// against a real `order` (which needs `orders:read` too; the shipping templates use its
/// newest fulfilment). The layout previews as an order confirmation in it.
/// `template_invalid` with the part and line when they don't render.
pub async fn preview<E: MailEnv, P: MailPorts>(
    ctx: &Ctx,
    env: &E,
    ports: &P,
    key: TemplateKey,
    input: TemplateInput,
    order: Option<OrderId>,
) -> Result<Rendered, CommandError> {
    ctx.require(Scope::NotificationsRead)?;
    if order.is_some() {
        ctx.require(Scope::OrdersRead)?;
    }
    Ok(render_preview(env, ports, key, input, order).await?.0)
}

/// The sender every email goes out under: the operator's address with the merchant's
/// sender name, else the store's name.
fn from(env: &impl MailEnv, branding: &Branding) -> Mailbox {
    Mailbox {
        name: Some(branding.sender_name().to_string()),
        address: env.sender_address().clone(),
    }
}

/// The test sender's name: what the `log` sender calls itself.
const LOG_SENDER: &str = "log";

/// Sends `preview`'s rendering to the calling staff member's own address only, its subject
/// marked `[Test]`. Not recorded: it's about no one's order. Refused with the `log` sender
/// (`email_not_configured`: there'd be nothing to receive) and for callers that aren't a
/// staff member (`staff_only`). Returns where it went.
pub async fn send_test<E, P>(
    ctx: &Ctx,
    env: &E,
    ports: &P,
    key: TemplateKey,
    input: TemplateInput,
    order: Option<OrderId>,
) -> Result<Email, CommandError>
where
    E: MailEnv,
    P: MailPorts + HasUsers,
{
    ctx.require(Scope::NotificationsWrite)?;
    if order.is_some() {
        ctx.require(Scope::OrdersRead)?;
    }
    if env.sender().name() == LOG_SENDER {
        return Err(CommandError::EmailNotConfigured);
    }
    let Actor::User(user) = ctx.actor() else {
        return Err(CommandError::StaffOnly);
    };
    let user = ports
        .users()
        .get(user)
        .await?
        .ok_or(CommandError::StaffOnly)?;
    let (rendered, branding) = render_preview(env, ports, key, input, order).await?;
    let to = user.email().clone();
    let email = OutgoingEmail {
        id: NotificationId::from_uuid(env.ids().new_uuid()),
        from: from(env, &branding),
        reply_to: branding.settings.reply_to().cloned(),
        to: to.clone(),
        subject: format!("[Test] {}", rendered.subject),
        html: rendered.html,
        text: rendered.text,
    };
    env.sender().send(&email).await?;
    Ok(to)
}

/// Queues a message again ("the customer didn't get it"), rendered from the order as it is
/// now with the templates in effect, as a new `queued` record without an event, even if the
/// template has since been turned off: staff asked. The send job sends it (woken now). A
/// shipping email is about the fulfilment its event named (while the event is kept), else
/// the order's newest. Invites and resets can't be resent (their token isn't kept; the flow
/// sends a new one): `notification_not_resendable`.
pub async fn resend<E, P>(
    ctx: &Ctx,
    env: &E,
    ports: &P,
    id: NotificationId,
) -> Result<Notification, CommandError>
where
    E: MailEnv,
    P: MailPorts + HasOutbox,
{
    ctx.require(Scope::NotificationsWrite)?;
    let earlier = ports
        .notifications()
        .get(id)
        .await?
        .ok_or(CommandError::NotificationNotFound(id))?;
    let template = earlier.template();
    if earlier.to().is_placeholder() {
        return Err(CommandError::RecipientErased);
    }
    let not_resendable = CommandError::NotResendable { template };
    let order = match (template, earlier.order()) {
        (
            TemplateKey::OrderConfirmation
            | TemplateKey::OrderShipped
            | TemplateKey::OrderTrackingUpdated
            | TemplateKey::OrderCancelled
            | TemplateKey::StaffNewOrder,
            Some(order),
        ) => order,
        _ => return Err(not_resendable),
    };
    let branding = branding(env, ports).await?;
    let order = load_order(ports, order).await?;
    let fulfillment = match (template, earlier.event()) {
        (TemplateKey::OrderShipped | TemplateKey::OrderTrackingUpdated, Some(event)) => {
            let named = ports
                .outbox()
                .get_event(event.as_uuid())
                .await?
                .and_then(|e| e.data["fulfillment"].as_str()?.parse().ok());
            match named {
                Some(fulfillment) => Some(load_fulfillment(ports, fulfillment).await?),
                None => None,
            }
        }
        _ => None,
    };
    let payment = newest_payment(ports, order.id()).await?;
    let context = order_context(
        env,
        ports,
        &branding,
        template,
        &order,
        payment.as_ref(),
        fulfillment.as_ref(),
    )
    .await?;
    let sources = Sources::load(ports, template).await?;
    let rendering = render_for_sending(&sources, &context)?;
    let message = Message {
        template,
        to: earlier.to().clone(),
        event: None,
        order: Some(order.id()),
    };
    let queued = queue(env, ports, &branding, &message, &rendering).await?;
    env.queued();
    Ok(queued)
}

/// Sends a staff flow's email at once: an invite or a password reset, rendered from its
/// template with `link` (the admin's page with the token: `server.admin_url` + `/invite` or
/// `/reset-password` + `?token=`; the bare token without an admin URL), recorded without an
/// event (the token isn't). What a host's `Notifier<StaffEmail>` calls; any failure, a
/// refusal included, is the flow's `notification_failed`.
pub async fn send_staff<E, P>(
    env: &E,
    ports: &P,
    message: StaffEmail,
) -> Result<Notification, NotifyError>
where
    E: MailEnv,
    P: StoreRepo
        + HasAssets
        + HasNotificationTemplates
        + HasNotificationSettings
        + HasNotifications,
{
    let failed = |e: CommandError| NotifyError(e.to_string());
    let (template, path, to, token, expires_at) = match message {
        StaffEmail::Invite {
            to,
            token,
            expires_at,
        } => (TemplateKey::StaffInvite, "/invite", to, token, expires_at),
        StaffEmail::PasswordReset {
            to,
            token,
            expires_at,
        } => (
            TemplateKey::StaffPasswordReset,
            "/reset-password",
            to,
            token,
            expires_at,
        ),
    };
    let branding = branding(env, ports).await.map_err(failed)?;
    let link = match env.admin_url() {
        Some(base) => format!("{base}{path}?token={token}"),
        None => format!("token {token} (set HYPHA_ADMIN_URL for a link)"),
    };
    let context = MailContext::Staff(StaffMail {
        shop: branding.shop.clone(),
        brand: branding.brand.clone(),
        user: user_context(&to),
        link,
        expires_at: TimeValue::of(expires_at, branding.store.timezone()),
    });
    let message = Message {
        template,
        to,
        event: None,
        order: None,
    };
    let sources = Sources::load(ports, template)
        .await
        .map_err(|e| failed(e.into()))?;
    let record = send_now(env, ports, &branding, &sources, message, &context)
        .await
        .map_err(failed)?;
    match record.status() {
        NotificationStatus::Sent => Ok(record),
        _ => Err(NotifyError(
            record
                .error()
                .unwrap_or("the email wasn't sent")
                .to_string(),
        )),
    }
}

/// Sends a customer's account email at once, like `send_staff`: the sign-in code (a secret,
/// not kept in the record) or the confirmation of an erasure request (which should arrive
/// while the person is there). What a host's `Notifier<CustomerEmail>` calls.
pub async fn send_customer<E, P>(
    env: &E,
    ports: &P,
    message: CustomerEmail,
) -> Result<Notification, NotifyError>
where
    E: MailEnv,
    P: StoreRepo
        + HasAssets
        + HasNotificationTemplates
        + HasNotificationSettings
        + HasNotifications,
{
    let failed = |e: CommandError| NotifyError(e.to_string());
    let branding = branding(env, ports).await.map_err(failed)?;
    let (template, to, context) = match message {
        CustomerEmail::LoginCode {
            to,
            code,
            expires_at,
        } => {
            let left = expires_at
                .unix_secs()
                .saturating_sub(env.clock().now().unix_secs())
                .max(0);
            let link = login_link(branding.shop.url.as_deref(), to.as_str(), &code);
            let context = MailContext::Code(CodeMail {
                shop: branding.shop.clone(),
                brand: branding.brand.clone(),
                email: to.as_str().to_string(),
                link,
                code,
                expires_at: TimeValue::of(expires_at, branding.store.timezone()),
                expires_in_minutes: u64::try_from(left / 60).unwrap_or(0).max(1),
            });
            (TemplateKey::CustomerLoginCode, to, context)
        }
        CustomerEmail::ErasureRequested { to, due_at } => {
            let context = MailContext::Erasure(ErasureMail {
                shop: branding.shop.clone(),
                brand: branding.brand.clone(),
                due_at: TimeValue::of(due_at, branding.store.timezone()),
            });
            (TemplateKey::CustomerErasureRequested, to, context)
        }
    };
    let message = Message {
        template,
        to,
        event: None,
        order: None,
    };
    let sources = Sources::load(ports, template)
        .await
        .map_err(|e| failed(e.into()))?;
    let record = send_now(env, ports, &branding, &sources, message, &context)
        .await
        .map_err(failed)?;
    match record.status() {
        NotificationStatus::Sent => Ok(record),
        _ => Err(NotifyError(
            record
                .error()
                .unwrap_or("the email wasn't sent")
                .to_string(),
        )),
    }
}

/// `send_staff` as a `Notifier<StaffEmail>`: the environment with the ports to record
/// in, for the staff flows' `env` (`hypha_users::invite`, `request_password_reset`).
pub struct StaffMailer<'a, E, P> {
    pub env: &'a E,
    pub ports: &'a P,
}

impl<E, P> Notifier<StaffEmail> for StaffMailer<'_, E, P>
where
    E: MailEnv,
    P: StoreRepo
        + HasAssets
        + HasNotificationTemplates
        + HasNotificationSettings
        + HasNotifications,
{
    async fn notify(&self, message: StaffEmail) -> Result<(), NotifyError> {
        send_staff(self.env, self.ports, message).await.map(drop)
    }
}
