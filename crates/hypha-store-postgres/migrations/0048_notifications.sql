-- Notifications (the hypha-notifications crate, design note 0028): the templates a
-- merchant edited, the store's notification settings, and the record of every email.

-- A template is stored only once edited or turned off: a key without a row uses the
-- build's default. `locale` is reserved for translations (Q2); '' is the default
-- language. A template only turned off keeps no sources: subject and html are both
-- NULL then, so it still gets the build's improved defaults.
CREATE TABLE notification_templates (
    key        text        NOT NULL,
    locale     text        NOT NULL DEFAULT '',
    version    bigint      NOT NULL CHECK (version > 0),
    subject    text,
    html       text,
    text       text,
    enabled    boolean     NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT notification_templates_pkey PRIMARY KEY (key, locale),
    CHECK ((subject IS NULL) = (html IS NULL)),
    CHECK (html IS NOT NULL OR text IS NULL)
);

-- One row per store (`id` is always 1); a missing row reads as the defaults. The logo is
-- an asset usage (owner `notification_settings`), written with the row, which keeps the
-- asset from being deleted while it's the logo.
CREATE TABLE notification_settings (
    id               smallint    CONSTRAINT notification_settings_pkey PRIMARY KEY
                                 DEFAULT 1 CHECK (id = 1),
    version          bigint      NOT NULL CHECK (version > 0),
    logo_asset_id    uuid,
    accent           text        NOT NULL,
    footer           text,
    sender_name      text,
    reply_to         text,
    staff_recipients text[]      NOT NULL DEFAULT '{}',
    order_url        text,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now()
);

-- No foreign keys to orders or events: the record outlives what it was about (an
-- event pruned from the outbox, an order deleted in a test store). Bodies aren't kept
-- (Q1).
CREATE TABLE notifications (
    id                  uuid        CONSTRAINT notifications_pkey PRIMARY KEY,
    version             bigint      NOT NULL CHECK (version > 0),
    template            text        NOT NULL,
    recipient           text        NOT NULL,
    subject             text        NOT NULL,
    status              text        NOT NULL
                                    CHECK (status IN ('sending', 'sent', 'failed', 'skipped')),
    attempts            int         NOT NULL CHECK (attempts >= 0),
    provider            text,
    provider_message_id text,
    error               text,
    event_id            uuid,
    order_id            uuid,
    sent_at             timestamptz,
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now(),
    CHECK ((status = 'sent') = (sent_at IS NOT NULL))
);

-- One message per (event, template, recipient) (NT8): a handler that runs twice finds
-- the first and sends nothing more. Its name is the repo key `notification_message`.
CREATE UNIQUE INDEX notification_message ON notifications (event_id, template, recipient)
    WHERE event_id IS NOT NULL;
-- An order's emails, newest first (its Emails card).
CREATE INDEX notifications_by_order ON notifications (order_id, id DESC);
