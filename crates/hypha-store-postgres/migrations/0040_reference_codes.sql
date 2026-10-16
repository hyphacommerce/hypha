-- Design note 0021 (SU1, SU2): currencies and countries are checked against core's ISO
-- tables (crates/hypha-core/src/tables), and `restore` re-runs the check. Until now any
-- three (or two) capital letters passed (0011), so this lists every stored code that isn't
-- in the tables and fails naming them, rather than leaving rows that stop loading.
--
-- A one-off check, not a constraint: the tables live in code, so a new currency or country
-- still needs no migration (0011). The lists are the tables as of this migration; a test
-- in this crate fails if a code here leaves them, since removing one needs its own check.
DO $$
DECLARE
    -- ISO 4217 list one (2026-09-17), without funds, metals, bond units and test codes.
    currencies text[] := ARRAY[
        'AED', 'AFN', 'ALL', 'AMD', 'AOA', 'ARS', 'AUD', 'AWG', 'AZN', 'BAM', 'BBD',
        'BDT', 'BHD', 'BIF', 'BMD', 'BND', 'BOB', 'BRL', 'BSD', 'BTN', 'BWP', 'BYN',
        'BZD', 'CAD', 'CDF', 'CHF', 'CLP', 'CNY', 'COP', 'CRC', 'CUP', 'CVE', 'CZK',
        'DJF', 'DKK', 'DOP', 'DZD', 'EGP', 'ERN', 'ETB', 'EUR', 'FJD', 'FKP', 'GBP',
        'GEL', 'GHS', 'GIP', 'GMD', 'GNF', 'GTQ', 'GYD', 'HKD', 'HNL', 'HTG', 'HUF',
        'IDR', 'ILS', 'INR', 'IQD', 'IRR', 'ISK', 'JMD', 'JOD', 'JPY', 'KES', 'KGS',
        'KHR', 'KMF', 'KPW', 'KRW', 'KWD', 'KYD', 'KZT', 'LAK', 'LBP', 'LKR', 'LRD',
        'LSL', 'LYD', 'MAD', 'MDL', 'MGA', 'MKD', 'MMK', 'MNT', 'MOP', 'MRU', 'MUR',
        'MVR', 'MWK', 'MXN', 'MYR', 'MZN', 'NAD', 'NGN', 'NIO', 'NOK', 'NPR', 'NZD',
        'OMR', 'PAB', 'PEN', 'PGK', 'PHP', 'PKR', 'PLN', 'PYG', 'QAR', 'RON', 'RSD',
        'RUB', 'RWF', 'SAR', 'SBD', 'SCR', 'SDG', 'SEK', 'SGD', 'SHP', 'SLE', 'SOS',
        'SRD', 'SSP', 'STN', 'SVC', 'SYP', 'SZL', 'THB', 'TJS', 'TMT', 'TND', 'TOP',
        'TRY', 'TTD', 'TWD', 'TZS', 'UAH', 'UGX', 'USD', 'UYU', 'UZS', 'VED', 'VES',
        'VND', 'VUV', 'WST', 'XAF', 'XCD', 'XCG', 'XOF', 'XPF', 'YER', 'ZAR', 'ZMW',
        'ZWG'
    ];
    -- ISO 3166-1 alpha-2, the officially assigned codes.
    countries text[] := ARRAY[
        'AD', 'AE', 'AF', 'AG', 'AI', 'AL', 'AM', 'AO', 'AQ', 'AR', 'AS', 'AT',
        'AU', 'AW', 'AX', 'AZ', 'BA', 'BB', 'BD', 'BE', 'BF', 'BG', 'BH', 'BI',
        'BJ', 'BL', 'BM', 'BN', 'BO', 'BQ', 'BR', 'BS', 'BT', 'BV', 'BW', 'BY',
        'BZ', 'CA', 'CC', 'CD', 'CF', 'CG', 'CH', 'CI', 'CK', 'CL', 'CM', 'CN',
        'CO', 'CR', 'CU', 'CV', 'CW', 'CX', 'CY', 'CZ', 'DE', 'DJ', 'DK', 'DM',
        'DO', 'DZ', 'EC', 'EE', 'EG', 'EH', 'ER', 'ES', 'ET', 'FI', 'FJ', 'FK',
        'FM', 'FO', 'FR', 'GA', 'GB', 'GD', 'GE', 'GF', 'GG', 'GH', 'GI', 'GL',
        'GM', 'GN', 'GP', 'GQ', 'GR', 'GS', 'GT', 'GU', 'GW', 'GY', 'HK', 'HM',
        'HN', 'HR', 'HT', 'HU', 'ID', 'IE', 'IL', 'IM', 'IN', 'IO', 'IQ', 'IR',
        'IS', 'IT', 'JE', 'JM', 'JO', 'JP', 'KE', 'KG', 'KH', 'KI', 'KM', 'KN',
        'KP', 'KR', 'KW', 'KY', 'KZ', 'LA', 'LB', 'LC', 'LI', 'LK', 'LR', 'LS',
        'LT', 'LU', 'LV', 'LY', 'MA', 'MC', 'MD', 'ME', 'MF', 'MG', 'MH', 'MK',
        'ML', 'MM', 'MN', 'MO', 'MP', 'MQ', 'MR', 'MS', 'MT', 'MU', 'MV', 'MW',
        'MX', 'MY', 'MZ', 'NA', 'NC', 'NE', 'NF', 'NG', 'NI', 'NL', 'NO', 'NP',
        'NR', 'NU', 'NZ', 'OM', 'PA', 'PE', 'PF', 'PG', 'PH', 'PK', 'PL', 'PM',
        'PN', 'PR', 'PS', 'PT', 'PW', 'PY', 'QA', 'RE', 'RO', 'RS', 'RU', 'RW',
        'SA', 'SB', 'SC', 'SD', 'SE', 'SG', 'SH', 'SI', 'SJ', 'SK', 'SL', 'SM',
        'SN', 'SO', 'SR', 'SS', 'ST', 'SV', 'SX', 'SY', 'SZ', 'TC', 'TD', 'TF',
        'TG', 'TH', 'TJ', 'TK', 'TL', 'TM', 'TN', 'TO', 'TR', 'TT', 'TV', 'TW',
        'TZ', 'UA', 'UG', 'UM', 'US', 'UY', 'UZ', 'VA', 'VC', 'VE', 'VG', 'VI',
        'VN', 'VU', 'WF', 'WS', 'YE', 'YT', 'ZA', 'ZM', 'ZW'
    ];
    unknown text;
BEGIN
    SELECT string_agg(listing, '; ' ORDER BY listing) INTO unknown
    FROM (
        SELECT format('%s %s', place, string_agg(DISTINCT quote_literal(code), ', ')) AS listing
        FROM (
            SELECT 'currency', 'variants.price_currency', price_currency FROM variants
            UNION ALL
            SELECT 'currency', 'markets.currency', currency FROM markets
            UNION ALL
            SELECT 'currency', 'price_lists.currency', currency FROM price_lists
            UNION ALL
            SELECT 'currency', 'price_list_prices.currency', currency FROM price_list_prices
            UNION ALL
            SELECT 'currency', 'orders.currency', currency FROM orders
            UNION ALL
            SELECT 'currency', 'carts.quote_currency', quote_currency FROM carts
            UNION ALL
            SELECT 'currency', 'shipping_option_tiers.currency', currency FROM shipping_option_tiers
            UNION ALL
            SELECT 'currency', 'search_documents.currency', currency FROM search_documents
            UNION ALL
            SELECT 'country', 'market_countries.country', country FROM market_countries
            UNION ALL
            SELECT 'country', 'tax_regions.country', country FROM tax_regions
            UNION ALL
            SELECT 'country', 'shipping_zone_countries.country', country FROM shipping_zone_countries
            UNION ALL
            SELECT 'country', 'carts.country', country FROM carts
            UNION ALL
            SELECT 'country', 'carts.shipping_country', shipping_country FROM carts
            UNION ALL
            SELECT 'country', 'customer_addresses.country', country FROM customer_addresses
            UNION ALL
            SELECT 'country', 'orders.country', country FROM orders
            UNION ALL
            SELECT 'country', 'orders.shipping_country', shipping_country FROM orders
            UNION ALL
            SELECT 'country', 'locations.address_country', address_country FROM locations
        ) AS stored (kind, place, code)
        WHERE code IS NOT NULL
          AND code <> ALL (CASE kind WHEN 'currency' THEN currencies ELSE countries END)
        GROUP BY place
    ) AS per_column;

    IF unknown IS NOT NULL THEN
        RAISE EXCEPTION 'stored codes that are not ISO 4217 currencies or ISO 3166-1 countries: %', unknown
            USING HINT = 'Change or delete those rows, then start Hypha again (design note 0021).';
    END IF;
END
$$;
