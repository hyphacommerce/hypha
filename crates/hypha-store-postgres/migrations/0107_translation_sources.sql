-- What's left to translate, as a read model beside `translations`: each resource's
-- fields that have source text, one row per field, with the SHA-256 hex of that text
-- (`digest`). A resource's translation into a language is missing a field when its
-- `fields` lack the key, outdated when the field's `source_digest` isn't `digest`. The
-- API keeps the rows in step with the resources (an event handler reads each changed
-- resource again); no foreign keys, as for `translations`.
CREATE TABLE translation_sources (
    resource_kind text COLLATE "C" NOT NULL,
    resource_id   uuid             NOT NULL,
    key           text COLLATE "C" NOT NULL,
    digest        text             NOT NULL,
    CONSTRAINT translation_sources_pkey PRIMARY KEY (resource_kind, resource_id, key)
);

-- The resources whose rows are to be read again by the API: at first every resource
-- there is (digests are computed by the API, not here), later every resource of a kind
-- a change reaches as a whole. `mark` orders them, and tells a resource queued again
-- while it was being read from the copy that was read.
CREATE SEQUENCE translation_source_marks;

CREATE TABLE translation_source_queue (
    resource_kind text   COLLATE "C" NOT NULL,
    resource_id   uuid               NOT NULL,
    mark          bigint             NOT NULL DEFAULT nextval('translation_source_marks'),
    CONSTRAINT translation_source_queue_pkey PRIMARY KEY (resource_kind, resource_id)
);

CREATE INDEX translation_source_queue_mark ON translation_source_queue (mark);

INSERT INTO translation_source_queue (resource_kind, resource_id)
          SELECT 'product', id FROM products
UNION ALL SELECT 'category', id FROM categories
UNION ALL SELECT 'custom_field_definition', id FROM custom_field_definitions
UNION ALL SELECT 'shipping_option', id FROM shipping_options
UNION ALL SELECT 'payment_method', id FROM payment_methods
UNION ALL SELECT 'discount', id FROM discounts
UNION ALL SELECT 'checkout_rule', id FROM checkout_rules
UNION ALL SELECT 'asset', id FROM assets WHERE alt IS NOT NULL
UNION ALL SELECT 'store', id FROM stores
UNION ALL SELECT 'search_facet', id FROM stores;
