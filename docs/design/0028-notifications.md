# 0028: Notifications: email templates, branding and providers

- **Status:** Accepted (2026-10-18). Danilo set the frame: Jinja templates (not Liquid),
  edited in the admin as code with the store's branding applied to every template, a
  visual editor later; Resend as the first hosted provider, Mailpit for local
  development. The open decisions (§10) as recommended. §11 pins the contract the build
  follows.
- **Scope:** the emails Hypha sends (to shoppers and to staff), their templates and the
  store's branding, rendering, the record of what was sent, the `EmailSender` port with
  log, SMTP and Resend senders, triggers (domain events, and the staff flows' direct
  messages), routes, the admin's Settings → Notifications and the order's email card,
  Mailpit in development and e2e.
- **Not in scope:** the visual editor (§9, step 2), SMS and push, in-admin notifications
  (Medusa's `feed`; the same records will carry them later), marketing email and
  newsletters, customer-account emails (no accounts yet, 0008 §9), translated templates
  (reserved, Q2), bounce and complaint handling from the provider's webhooks (Q4),
  Cloudflare Email Service (a later sender, §5).
- **Related:** 0008 A10 (the `Notifier` port; invites and resets are logged today),
  0016 (the outbox, handlers, retries), 0018 §3 (`BuyerEmail`, the fulfilment's
  `notify` flag), 0019 §5.1 (in-process event handlers), 0013 (media: the logo), 0017
  (configuration), 0025/0026 (deployments and their secrets), 0027 (payment
  instructions in the confirmation).

## Why

Nothing leaves Hypha by email. Staff invites and password resets are logged with their
tokens (startup warns), the buyer's shipped, tracking and cancelled messages are logged
as one line, and there's no order confirmation at all, so a cash-on-delivery shopper is
never told what they ordered and a bank-transfer shopper never gets the bank details
outside the confirmation page. That blocks production use. Merchants also expect to
make the emails look like their shop without a developer, as Shopify lets them.

## Decisions

| | Decision |
| --- | --- |
| NT1 | **Templates are Jinja, rendered by `minijinja` in the API**, natively and on Workers (pure Rust, serde only). Not Liquid, not JSX: a stored template is text the API can render with no JavaScript runtime, so invites and resets work with the API alone. *Decided; hard to reverse (merchants' edited templates depend on the language)* |
| NT2 | **Two crates.** A leaf, `hypha-notifications`: templates, the store's notification settings (branding, sender, staff recipients), the record of each message, the `EmailSender` port, rendering. A workflow, `hypha-mail`: the context each template gets (built from orders, fulfilments, payments, the store, media), the event handler that sends, previews and test sends. *Hard to reverse (crate edges)* |
| NT3 | **A template's variables are a contract**, like event payloads (0016): documented per template, with a sample; fields are added, never renamed or removed. A reshaped context is a new version, and the old names stay available to templates written against it. *Hard to reverse (contract)* |
| NT4 | **Branding is store-wide and reaches every template through a shared `layout`** that the others extend (`{% extends "layout" %}`): logo, accent colour, sender name, reply-to, footer. The layout is itself an editable template, so a merchant can change the frame once; each template can still override any block or ignore the layout. That's Shopify's "customize email template" plus "edit code" in one mechanism |
| NT5 | **v1 editing is code**: per template, a subject line, an HTML body and an optional plain-text body, in a code editor with a live preview, "send a test to me", enable/disable (where allowed), and revert to default. A template that was never edited is not stored: the build's default is used, so improved defaults reach every store that kept them |
| NT6 | **Defaults are written in React Email** in a repo package (`packages/emails`) and built to Jinja templates that the Rust crate embeds (committed; a check fails while they're stale, like the themes). JSX gives good email HTML; the API only ever sees Jinja |
| NT7 | **Shopper and staff emails ride the outbox** (0016): a `Mailer` event handler (0019 §5.1) wants `order.placed`, `order.cancelled`, `fulfillment.created`, `fulfillment.updated`, renders and records each message `queued`; a send job (`send_notifications`) sends what's queued with its own lease and retry schedule (§4). The staff's choice to tell the buyer travels in the event (`notify_customer`, an added field, so still version 1; Shopify's fulfilment has the same flag). **Messages that carry a secret (invite, password reset) are sent directly** by the flow, after its commit, as today, so a token never lands in the outbox |
| NT8 | **Every message is recorded** (`ntf_…`): template, recipient, subject, status, attempts, the provider's message ID, the error, a note (a fallback to the default template); the event it answers and the order it's about. One message per (event, template, recipient): a handler that runs twice finds the record and queues nothing more. **Bodies are kept only until the message is sent or has failed** (Q1) |
| NT9 | **One `EmailSender` port; the operator picks the sender in `hypha.config.toml`**: `log` (the default, today's behaviour), `smtp` (native only, `lettre`; Mailpit in development) or `resend` (both hosts, over HTTPS). The from address is the operator's (a domain the provider verified); the merchant sets the sender's name and the reply-to. *Hard to reverse (configuration keys)* |
| NT10 | **New scopes `notifications:read` and `notifications:write`**: templates and settings are settings; resending a message is `notifications:write`. Reading the record of messages takes `orders:read` too: it names buyers and their orders |

## 1. What others do

- **Shopify:** Settings → Notifications lists customer notifications (order confirmation,
  shipping confirmation and update, cancellation, refunds, …), staff notifications (a
  "new order" email to chosen addresses, with a test send of fake data) and webhooks.
  "Customize email template" sets a logo and accent colour for all of them; each
  template has a subject and an HTML + Liquid code editor, preview, "send test email",
  and revert to default. The sender email must be confirmed before templates can be
  edited. The variables are documented and stable.
- **Medusa v2:** a Notification module with one provider per channel (`email`, `sms`,
  `feed`); the default provider logs. Code triggers sends (a subscriber on
  `order.placed` runs a workflow that loads the order and calls `createNotifications`
  with a template name and data). The Resend guide keeps templates as React Email
  components in the codebase; merchants can't edit them in the admin. The `feed` channel
  fills the admin's notification panel.

Hypha takes Shopify's merchant-facing model (branding, per-template code, preview, test,
revert, staff recipients), Medusa's provider port with a logging default, and its own
outbox for triggers.

## 2. Model (`hypha-notifications`)

**`TemplateKey`**, a closed enum (the API's names are contracts):

| Key | To | Triggered by | Can be disabled |
| --- | --- | --- | --- |
| `layout` | (shared frame) | | |
| `order_confirmation` | buyer | `order.placed` | yes |
| `order_shipped` | buyer | `fulfillment.created` with `notify_customer` | yes |
| `order_tracking_updated` | buyer | `fulfillment.updated` with `notify_customer` | yes |
| `order_cancelled` | buyer | `order.cancelled` with `notify_customer` | yes |
| `staff_new_order` | staff recipients | `order.placed` | yes (or no recipients) |
| `staff_invite` | invitee | the invite flow (direct) | no |
| `staff_password_reset` | staff member | the reset flow (direct) | no |

Payment instructions (bank details) are part of `order_confirmation`, from the order's
payment (0027 copies them at checkout), not a template of their own.

**`Template`** (aggregate root, `Versioned`, `Stamped`; stored only once edited):
- `key: TemplateKey`, unique; `locale` reserved (Q2);
- `subject`, `html`, `text: Option<String>`, each a Jinja source, size-capped (subject
  250 bytes, bodies 256 KiB);
- `enabled: bool` (refused as `false` for the keys that can't be disabled);
- `restore` re-parses the sources, so a template that stops parsing after an upgrade of
  minijinja shows as an error on the template, not a 500 on send (Q5).

**`NotificationSettings`** (one per store, like search settings):
- branding: `logo: Option<AssetId>` (an asset usage, so the asset can't be deleted while
  it's the logo), `accent: Colour` (`#rrggbb`), `footer` (plain text: address, legal);
- sender: `sender_name` (default: the store's name), `reply_to: Option<Email>`;
- `staff_recipients: Vec<Email>` (at most 10) for `staff_new_order`;
- `order_url: Option<String>`, a pattern with `{order}` and `{token}` for the link to the
  order's page (the API stays URL-agnostic; `hypha deploy` sets it when it connects the
  storefront: `https://shop…/order/{order}?token={token}`). Without one, emails
  have no order link.

**`Notification`** (`ntf_…`, aggregate root): `template: TemplateKey`, `to: Email`,
`subject` (rendered; empty when skipped), `status`, `attempts`, `provider` (`log`,
`smtp`, `resend`), `provider_message_id`, `error`, `note`, `event: Option<EventId>`,
`order: Option<OrderId>`, `sent_at`, `next_attempt_at`, `lease_until`, and while it
waits the rendered email (`QueuedEmail`: HTML, text, sender name, reply-to). Unique on
(event, template, to) where `event` is set (NT8). Its statuses and moves:

```text
queued ──lease──► sending ──► sent
   ▲                 │ └────► failed      (refused, or the retry schedule ran out)
   └──transient──────┘
skipped                                    (the template is off: never rendered)
sending (lease ended) ──lease──► sending   (the runner stopped: taken again)
```

`sent`, `failed` and `skipped` are final; resending makes a new record. A direct send
(an invite, a reset) is `sending` without a lease or a stored email while its request
sends it, then `sent` or `failed`.

Rendering (`render.rs`) builds a minijinja `Environment` per send from the template
sources: the stored or default `layout` and the template, autoescaping HTML in `html`
(not in `subject` and `text`), **strict undefined** (a misspelled variable is an error,
not an empty string), and minijinja's fuel limit so a template's loops can't run away
(the CPU it may burn on Workers). Filters: `money` (a `Money` with its currency's minor
units: `1.234,50 RSD`'s digits, Q3), `date` / `datetime` (in the store's time zone).
Without a `text` body the plain text is derived from the rendered HTML (links kept as
`text (url)`).

## 3. Contexts (`hypha-mail`)

Each template gets typed serde structs (`context.rs`), the contract of NT3:

- every template: `shop` (name, email, `url`), `brand` (`logo_url`, `accent`,
  `footer`), `layout` blocks (`title`, `preheader`, `content`);
- order templates: `order` (`number`, `id`, `url` from `order_url`, `email`, `placed_at`,
  `lines` with product title, variant title, quantity, unit price, line total, image
  URL; `subtotal`, `shipping`, `tax`, `total`; addresses; `shipping_method`;
  `payment` with method name, status and `instructions`), `customer` (name);
- shipped / tracking: `fulfillment` (`lines` shipped, `tracking` with carrier, number,
  url);
- cancelled: `cancellation` (`reason`);
- staff: `user` (email), `link` (the admin's invite or reset link, `server.admin_url`),
  `expires_at`; `staff_new_order` gets `order` and `admin_url` to it.

Each context has a `sample()` (a made-up order with two lines, a tracking number, a
bank transfer's instructions) for previews, test sends, the editor's variable list and
for checking a template on save: saving renders the template against its sample and
refuses `template_invalid` with the error's line (`details.line`).

## 4. Sending

1. **From events: queue, then send.** `Mailer` (in `hypha-mail`, listed in both hosts'
   `handlers.rs`) only renders and records: per event it loads the store's branding,
   the order, its newest payment and the fulfilment once, and per template the template
   and layout once; for each recipient without a record for (event, template, to) it
   writes the `Notification` as `queued` (due now, with the rendered email) or, when
   the template is off, as `skipped` without rendering it. No network, so it's fast and
   holds the dispatcher's connection only for its reads and writes; it fails only for a
   storage or data error (a missing order, a bad payload), which the dispatcher retries
   on its schedule, and a rerun finds the records and queues nothing more. Then it wakes
   the send job (`MailEnv::queued`).
2. **The send job** (`hypha_http::jobs::send_notifications`, the steps in `hypha-mail`'s
   `send.rs`), shaped like the dispatcher's webhooks (0016, EV9): it leases a batch of
   due messages (`NotificationRepo::lease_due`: `queued` whose `next_attempt_at` has
   come, or `sending` whose `lease_until` has passed; `FOR UPDATE SKIP LOCKED`, so
   concurrent runs never take the same one) for `jobs.send_notifications.lease`, lets
   the connection go, sends each (`send_leased`, no ports), then takes a connection to
   record the outcome (`record_attempt`): `sent`; a permanent refusal `failed`, final;
   a transient failure back to `queued` at the next wait of
   `jobs.send_notifications.retry` (1 min, 5 min, 30 min, 2 h, 6 h by default), and
   `failed` once those are used up. It never starts a send that couldn't finish within
   its lease or its host's time (`LONGEST_SEND`: the senders' 10-second timeout and
   room to connect; on Workers, the `waitUntil` deadline), so a lease doesn't run out
   mid-send. Natively it's its own job in `runner.rs`, woken when something is queued;
   on Workers it runs in a write's `waitUntil` after dispatch and search sync, and on
   the cron tick, whose `due` mark includes `NotificationRepo::next_due` (W7), so a
   quiet store's ticks touch Postgres no more than before.
3. **At least once, only after a crash.** A message is sent again only when the runner
   that leased it stopped (crashed, cut off) after the provider took it but before the
   outcome was recorded: its lease ends and the next lease takes it. Resend's
   `Idempotency-Key` is the message's ID, so Resend drops that repeat within its 24
   hours; SMTP has no such key, so an SMTP provider may deliver that message twice. A
   run that overlaps a lease still in force never takes it: no duplicates from slow
   runs. A late outcome from the stopped runner is a stale write and is dropped.
4. **Direct.** `Notifier<StaffEmail>` (`hypha-http`'s `StaffEnv`, around `HostEnv`)
   renders the staff template and sends at once (`send_now`), recording a
   `Notification` with no event (`sending`, then `sent` or `failed`; no stored body, so
   the token isn't kept). A failure is the flow's `notification_failed`, as today. Test
   sends go at once too, unrecorded.
5. **Resend.** `POST /admin/notifications/{id}/resend` renders the template again from
   current data and queues a new message (a new record, `event` empty, even if the
   template is off: staff asked), wakes the send job and answers `201` with it,
   `queued`; the admin says "Queued again", and the order's card shows how it went.
6. **A template that doesn't render.** `save_template` renders a template against its
   sample and against the sample with every value that may be absent absent
   (`MailContext::sparse`: no payment, address, order link, customer name, tracking,
   …; `sparse_fields`: present objects without their optional fields) and refuses
   `template_invalid` with the part and line, so one that only works when an order has
   a payment is refused on save. If a stored template still fails on a real order, the
   default renders that message instead (Q5), with a `tracing` warning and a note on
   the record ("sent with the default template: …"), shown in the order's Emails card.

`BuyerEmail` and the routes' `notify_buyer` go: the `notify` field the fulfilment and
cancel routes already take sets `notify_customer` on the event instead.

`staff_new_order` gets no `order.url`: the order link carries the buyer's access token,
and staff have `admin_url`.

## 5. The port and the senders

```rust
#[allow(async_fn_in_trait)]
pub trait EmailSender {
    /// "log", "smtp", "resend": recorded on the message.
    fn name(&self) -> &'static str;
    async fn send(&self, email: &OutgoingEmail) -> Result<Sent, SendError>;
}

pub struct OutgoingEmail {
    pub id: NotificationId, // the provider's idempotency key where it has one
    pub from: Mailbox,      // the operator's address, the merchant's sender name
    pub reply_to: Option<Email>,
    pub to: Email,
    pub subject: String,
    pub html: String,
    pub text: String,
}

pub enum SendError {
    /// Try again later (timeout, 5xx, rate limited).
    Transient(String),
    /// Never going to work (address refused, sender not verified).
    Permanent(String),
}
```

No `Send` bound, like every port. The sender is passed to commands and the handler
apart from the request's ports, like the `WebhookSender` and the search engine.

- **`log`**: today's behaviour, the default; startup warns that emails aren't sent.
- **`smtp`** (`hypha-host-axum`, `lettre` with rustls): any provider's SMTP, and
  Mailpit locally. Native only: Workers can't open SMTP connections through our host
  layer.
- **`resend`** (a portable crate, `hypha-mail-resend`, over an `HttpClient` port like
  `hypha-store-s3`): `POST https://api.resend.com/emails` with the
  `Idempotency-Key` header set to the message's ID. Both hosts give it their HTTP
  client.
- Later: Cloudflare Email Service through its Workers binding (public beta today), and
  senders from spores.

Configuration (0017), keys as contracts:

```toml
[email]
sender = "resend"            # "log" | "smtp" | "resend"
from = "shop@mail.example.com"

[email.smtp]
host = "localhost"
port = 1025
tls = "none"                 # "none" | "starttls" | "tls"
username = "…"               # password: HYPHA_SMTP_PASSWORD
```

`HYPHA_RESEND_API_KEY` is environment-only. `hypha init` asks for the sender (Resend's
key, or SMTP) and the from address; `deploy` passes the key as a secret to the API's
container or Worker.

## 6. Development and tests

- `compose.yaml` gains **Mailpit** (`axllent/mailpit`: SMTP on 1025, its inbox on
  http://localhost:8025). `.cargo/config.toml` and `pnpm dev` point the API at it with
  `sender = "smtp"`, so every email of the local stack is readable in Mailpit.
- The repository contract gains `notification_template_repo`, `notification_settings_repo`
  and `notification_repo` (the (event, template, to) key, `lease_due`, `next_due`);
  `hypha-testkit` gains an `InMemoryEmailSender` that keeps what it was given.
- The `Mailer`'s tests run on testkit; render tests pin every default template against
  its sample (strict undefined catches a default that names a variable its context
  lacks).
- `e2e.sh` runs with Mailpit and checks its API (`GET /api/v1/search?query=to:…`) after
  checkout, a fulfilment and an invite, then deletes the messages it read. CI adds the
  service.
- `hypha-mail-resend` is tested against a recorded request shape; no live calls in CI.

## 7. API and screens

- Admin routes:
  - `GET /admin/notification-settings`, `PATCH` (branding, sender name, reply-to, staff
    recipients, order URL);
  - `GET /admin/notification-templates` (every key: customized or default, enabled);
  - `GET /admin/notification-templates/{key}` (the sources in effect, `customized`, the
    variables with the sample), `PUT` (save: subject, html, text, enabled), `DELETE`
    (revert to default);
  - `POST /admin/notification-templates/{key}/preview` (unsaved sources, against the
    sample or an `order` ID: rendered subject, HTML and text, or `template_invalid`);
  - `POST /admin/notification-templates/{key}/test` (send the preview to the calling
    staff member's own address only);
  - `GET /admin/notifications?order=…&template=…&status=…` (a list, 0012; with
    `GET /admin/notifications/{id}`, `notifications:read` and `orders:read`), `POST
    /admin/notifications/{id}/resend` (queues it, `201`).
- Error codes: `template_invalid` (422, with `details.line`), `template_required`
  (disabling one that can't be), `email_not_configured` (409 for a test send with the
  `log` sender: there'd be nothing to receive), `notification_failed` (exists).
- **Settings → Notifications**: a branding card (logo from the media library, accent
  colour, sender name, reply-to, footer), staff recipients, and the templates in two
  groups (customer, staff) with their state. A template's page: subject field, HTML and
  plain-text code editors (CodeMirror 6, HTML with Jinja highlighting), the variables
  panel from the sample, a live preview (an iframe, desktop and mobile widths, against
  the sample or a chosen order), Send test, Save, Revert to default.
- The order page gains an **Emails** card: what was sent, when, its status and error,
  with Resend.
- The storefront's confirmation page accepts the token from the link (`?token=`) and
  moves it into its cookie, so the email's order link works on another device.

## 8. Events

None new in v1. `notification.sent` / `notification.failed` can follow for apps that
want them; the record is the source of truth meanwhile. The four trigger events gain
`notify_customer` where staff choose (an added field).

## 9. Build order

1. `hypha-notifications`: keys, templates, settings, messages, rendering with
   minijinja, the port; repositories, contract, Postgres migration.
2. `packages/emails`: the layout and the seven defaults in React Email, the build to
   Jinja, the stale check; embedded in the crate.
3. `hypha-mail`: contexts and samples, the `Mailer` handler, previews, test sends,
   resend; `notify_customer` on the events; `BuyerEmail` removed; `Notifier<StaffEmail>`
   through templates.
4. Senders: `log`, `smtp` with Mailpit in compose and `pnpm dev`, `hypha-mail-resend`;
   configuration, the CLI's prompts and secrets.
5. Routes, OpenAPI, SDK; e2e with Mailpit.
6. Admin: Settings → Notifications, the template editor, the order's Emails card; the
   storefront's `?token=`; the seed sets branding.
7. Later: the visual editor (`@react-email/editor` or Maily, MIT, both TipTap) for the
   content block, with custom blocks for order lines, addresses and buttons; a template
   then stores its editor document beside the Jinja it exports (a `format` field, so it
   needs no migration of meaning).

## 10. Open decisions

**Q1. Store rendered bodies?** Seeing exactly what a customer received helps support,
but it keeps personal data (addresses, order contents) for as long as the record lives,
and staff emails would store their tokens. Recommended: no; store the subject, and let
the Emails card show "preview as of now" (re-rendered). A queued message keeps its
rendered email only until it's sent or has failed (§4), and staff emails, sent
directly, never store theirs. Records are pruned after
`jobs.prune_notifications.retention` (default 90 days).

**Q2. Translations?** Recommended: not now, but reserve it: a template's identity is
(key, locale) with locale empty for the default, so per-language templates (a market's
language, 0021's room for i18n) are added rows, not a new table.

**Q3. Money and date formats?** Recommended: a store-wide format for now (the currency's
code, minor units and the store's decimal and thousands separators, set with branding),
with each value's raw `amount` and `currency` in the context for templates that want
their own. Locale-aware formatting comes with Q2.

**Q4. Bounces and complaints?** Resend reports them through webhooks. Recommended:
later; a route that marks the message `bounced` and an address list that skips
complained addresses, once a store sends enough to need it.

**Q5. A template that breaks after an upgrade?** minijinja is stable, but a default's
context could gain a rule. Recommended: a stored template that fails to parse or render
falls back to the default for that send, records the error on the message and shows a
warning on the template, rather than leaving the customer without an email.

**Q6. Who may edit templates?** Templates can show any variable in their context, all
of which staff with `orders:read` already see, and can't reach anything else (no
includes, no I/O). Recommended: `notifications:write` is enough; no extra review step.

## 11. Implementation contract

What the parts built in parallel agree on; where this and the sections above differ,
this wins.

**Crates.** `hypha-notifications` (leaf; depends on core, `minijinja`, `serde`):
`TemplateKey`, `Template`, `NotificationSettings`, `Notification`, their repositories
(`NotificationTemplateRepo`, `NotificationSettingsRepo`, `NotificationRepo`, with
`HasNotificationTemplates`, `HasNotificationSettings`, `HasNotifications`), the
`EmailSender` port, `render.rs`, errors, commands that need no other domain (list and
read settings, list templates, revert, list messages). The default templates are
embedded here (`defaults/<key>.subject.j2`, `.html.j2`, `.txt.j2`, and
`defaults/layout.html.j2`; `include_str!`). `hypha-mail` (workflow; depends on
notifications, orders, fulfillment, payments, stores, media, users, core): contexts and
samples, `Mailer` (the `EventHandler`), `save_template` (checks against the sample),
`preview`, `send_test`, `resend`, `update_settings` (the logo's asset usage),
`send_staff` (what `Notifier<StaffEmail>` calls). `hypha-mail-resend` (adapter, like
`hypha-store-s3`: the Resend sender over an `HttpClient` port). The SMTP and log senders
live in the hosts (`hypha-host-axum` for SMTP; `hypha-http` for log).

**Tables** (migration `0048_notifications.sql`):
- `notification_templates (key text, locale text NOT NULL DEFAULT '', subject text,
  html text, text text NULL, enabled boolean, version, created_at, updated_at,
  PRIMARY KEY (key, locale))`;
- `notification_settings (id smallint PRIMARY KEY CHECK (id = 1), version, logo_asset_id
  uuid NULL, accent text, footer text NULL, sender_name text NULL, reply_to text NULL,
  staff_recipients text[], order_url text NULL, created_at, updated_at)`; a missing row
  reads as the defaults; the logo is an asset usage (0013) like a category's image;
- `notifications (id uuid PK, version, template text, recipient text, subject text,
  status text CHECK IN ('queued','sending','sent','failed','skipped'), attempts int,
  provider text NULL, provider_message_id text NULL, error text NULL, note text NULL,
  event_id uuid NULL, order_id uuid NULL, sent_at timestamptz NULL, next_attempt_at
  timestamptz NULL, lease_until timestamptz NULL, email_sender_name text NULL,
  email_reply_to text NULL, email_html text NULL, email_text text NULL, created_at,
  updated_at)`, with checks: `sent_at` exactly when sent, `next_attempt_at` exactly when
  queued, `lease_until` only while sending, the email exactly while queued or leased;
  a unique index on `(event_id, template, recipient) WHERE event_id IS NOT NULL`, an
  index on `(order_id, id DESC)`, and partial indexes for the send job's leases and
  `next_due`: `(next_attempt_at) WHERE status = 'queued'`, `(lease_until) WHERE
  lease_until IS NOT NULL`.

**Template variables** (each template's context; `null` where the value is absent;
money is `{ "amount": i64 minor units, "currency": "EUR", "formatted": "€12.50" }` and
templates print `{{ x.formatted }}`; times are `{ "iso": "…", "formatted": "18 Oct
2026, 14:05" }` in the store's time zone, UTC without one):
- every template: `shop { name, email, url }` (`url`: the origin of `order_url`, or
  null), `brand { logo_url, accent, footer }`;
- `order_*` and `staff_new_order`: `order { id, number, url, email, placed_at, lines[] {
  title, variant, sku, quantity, unit_price, total }, subtotal, shipping, tax, total,
  taxes_included, shipping_address { name, company, line1, line2, city, province,
  postal_code, country, phone }, shipping_method, payment { method, status,
  instructions } }`, `customer { name }` (from the shipping address, else null);
- `order_shipped`, `order_tracking_updated`: `fulfillment { lines[] { title, variant,
  quantity }, tracking[] { carrier, number, url } }`;
- `order_cancelled`: `cancellation { reason }` (the reason's display text);
- `staff_new_order`: `admin_url` (the order in the admin, or null without
  `server.admin_url`);
- `staff_invite`, `staff_password_reset`: `user { email }`, `link`, `expires_at`.

The layout's blocks: `{% block preheader %}`, `{% block content %}`; templates start
with `{% extends "layout" %}`. Filters: none beyond minijinja's builtins in v1 (values
arrive formatted). Order links: `order_url`'s `{order}` and `{token}` are replaced with
the order's ID and access token (`HostEnv::order_token`, passed into `hypha-mail` as a
function so the workflow doesn't know how it's made).

**Routes** as §7, with scopes `notifications:read` / `notifications:write`, and the
fulfilment and cancel routes' existing `notify` field carried into the events as
`notify_customer`.

**Configuration**: `[email] sender = "log" | "smtp" | "resend"`, `from`, `[email.smtp]
host, port, tls ("none" | "starttls" | "tls"), username`; `[jobs.send_notifications]
enabled, interval (2s), batch (20), lease (2m, at least 20s), retry (["1m", "5m",
"30m", "2h", "6h"])`; `HYPHA_SMTP_PASSWORD`,
`HYPHA_RESEND_API_KEY` (environment only). The Workers host supports `log` and `resend`;
`smtp` there is a startup error. The CLI: `init` asks for the sender (Resend key or
none) and `from`; `deploy` passes the key as a secret; `pnpm dev` uses Mailpit.
