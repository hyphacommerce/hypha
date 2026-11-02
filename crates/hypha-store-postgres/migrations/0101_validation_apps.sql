-- Each validation app's last answer about a cart (`checkout.validate`): what it was sent
-- (a digest), its messages or why it failed, and when; an app is asked again only when
-- what it would be sent changes.
ALTER TABLE carts ADD COLUMN app_checks jsonb NOT NULL DEFAULT '[]';

-- Per validation app: whether it's asked and whether its failure blocks checkout. An app
-- not listed is asked, and its failure doesn't block.
ALTER TABLE checkout_settings ADD COLUMN validation_apps jsonb NOT NULL DEFAULT '[]';
