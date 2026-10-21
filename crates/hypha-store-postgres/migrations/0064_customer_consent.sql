-- Marketing consent per channel and the erasure state on customers, and the cart's
-- opt-in box (design note 0032, §5 and §7.3). A consent's source is `checkout`,
-- `storefront`, `admin`, `import`, `system` or `app:<appid_…>`.
ALTER TABLE customers
    ADD COLUMN email_marketing_state  text   NOT NULL DEFAULT 'not_subscribed',
    ADD COLUMN email_marketing_level  text   NOT NULL DEFAULT 'unknown',
    ADD COLUMN email_marketing_at     bigint,
    ADD COLUMN email_marketing_source text,
    ADD COLUMN sms_marketing_state    text   NOT NULL DEFAULT 'not_subscribed',
    ADD COLUMN sms_marketing_level    text   NOT NULL DEFAULT 'unknown',
    ADD COLUMN sms_marketing_at       bigint,
    ADD COLUMN sms_marketing_source   text,
    ADD COLUMN erasure_due_at         bigint,
    ADD COLUMN erased_at              bigint;

ALTER TABLE carts
    ADD COLUMN marketing_opt_in boolean NOT NULL DEFAULT false;
