-- What a previewed or applied write wrote: each row of an aggregate (a table with a
-- `version`), with the version it found. A preview and its apply compare it beside their
-- events, so an apply over a record someone changed since the preview is stale even when
-- its events read the same (an event names what changed, not what it was changed from).
--
-- Only while the transaction says so (`SET LOCAL hypha.log_writes = 'on'`, which previews
-- and applies do): the trigger's WHEN checks it, so every other write runs no function. The log is the
-- transaction-local setting `hypha.writes`, one JSON array per line
-- (`["products", "<key>", "update", 4]`), so a write rolled back to its savepoint takes
-- its lines with it, as it takes its rows.
CREATE FUNCTION hypha_note_write() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    found_row jsonb;
    row_key   text := '';
    found     bigint;
BEGIN
    IF TG_OP = 'UPDATE' THEN
        -- A write that keeps the version (a use recorded, stamps written after the
        -- version moved) changes no aggregate.
        IF OLD.version = NEW.version THEN
            RETURN NULL;
        END IF;
    END IF;
    IF TG_OP = 'INSERT' THEN
        found_row := to_jsonb(NEW);
    ELSE
        found_row := to_jsonb(OLD);
        found := OLD.version;
    END IF;
    FOR i IN 0 .. TG_NARGS - 1 LOOP
        IF i > 0 THEN
            row_key := row_key || '/';
        END IF;
        row_key := row_key || coalesce(found_row ->> TG_ARGV[i], '');
    END LOOP;
    PERFORM set_config(
        'hypha.writes',
        coalesce(current_setting('hypha.writes', true), '')
            || json_build_array(TG_TABLE_NAME, row_key, lower(TG_OP), found)::text
            || E'\n',
        true
    );
    RETURN NULL;
END
$$;

-- Every aggregate's table, with its key's columns. A new one gets the same trigger (a
-- test checks every table with a bigint `version` has it).
DO $$
DECLARE
    t record;
BEGIN
    FOR t IN
        SELECT * FROM (VALUES
            ('agent_proposals', 'id'), ('agents', 'id'), ('api_keys', 'id'),
            ('apps', 'key'), ('assets', 'id'), ('built_in_settings', 'key'),
            ('carts', 'id'), ('catalogs', 'id'), ('categories', 'id'), ('channels', 'id'),
            ('custom_field_definitions', 'id'),
            ('custom_field_values', 'definition_id, owner_id'),
            ('customer_groups', 'id'), ('customers', 'id'), ('fulfillments', 'id'),
            ('identities', 'id'), ('inventory_items', 'id'), ('invites', 'id'),
            ('locations', 'id'), ('markets', 'id'), ('notification_settings', 'id'),
            ('notification_templates', 'key, locale'), ('notifications', 'id'),
            ('oauth_codes', 'hash'), ('oauth_refresh_tokens', 'id'),
            ('oauth_requests', 'id'), ('orders', 'id'), ('password_resets', 'id'),
            ('payment_methods', 'id'), ('payments', 'id'), ('price_lists', 'id'),
            ('products', 'id'), ('roles', 'id'), ('search_settings', 'id'),
            ('sessions', 'id'), ('shipping_app_providers', 'key'),
            ('shipping_zones', 'id'), ('stores', 'id'), ('tax_classes', 'id'),
            ('tax_regions', 'id'), ('users', 'id')
        ) AS tables (name, key_columns)
    LOOP
        EXECUTE format(
            'CREATE TRIGGER hypha_note_write AFTER INSERT OR UPDATE OR DELETE ON %I
                 FOR EACH ROW WHEN (current_setting(''hypha.log_writes'', true) = ''on'')
                 EXECUTE FUNCTION hypha_note_write(%s)',
            t.name,
            (SELECT string_agg(quote_literal(trim(c)), ', ')
                 FROM unnest(string_to_array(t.key_columns, ',')) AS c)
        );
    END LOOP;
END
$$;
