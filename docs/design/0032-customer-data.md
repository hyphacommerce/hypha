# 0032: Customer data: notes, groups, consent and privacy

- **Status:** Accepted with changes (2026-10-20); Danilo's answers are in §11.
  - Email stays mandatory, and erasure writes a placeholder.
  - Tags are deferred, likely to become list custom fields.
  - Custom fields get their own note (0033) and are built **before** this one.
  - Staff roles under `customer_data:*` are deferred.
- **Scope:** what a customer record holds beyond 0005 (a staff note, marketing consent,
  and what it needs from typed custom fields, which have their own note), customer groups, how all of it reaches apps (events, the
  Mailchimp-style sync), and personal data over a customer's life (redaction, deletion,
  export, erasure).
- **Not in scope:** customer accounts and login (their own note, after this one; §9
  says what this note reserves for them), segments (dynamic, query-based lists: §8
  reserves them), merging customers, B2B companies (§10 sketches them), discounts.
- **Related:** 0033 (custom fields, built first), 0005 (customers: one per email, the address book), 0002 §3 (custom fields,
  never built), 0029 AP10 / §4.2 (personal-data scopes and redaction), 0016 (events,
  webhooks), 0008 §9 (customer accounts), 0003 §6 (tax exemption, deferred), 0007 (orders
  snapshot the buyer).

## Why

A customer today is an email, a name, a phone and an address book. Merchants need three
things on top of that, and each is a pattern Shopify got mostly right:

1. **Personal data handled carefully.** Apps and keys already see names, emails, phones
   and addresses only with `customer_data:*` (0029 AP10). What's missing is everything
   over time: removing a customer, giving them their data, erasing them while the
   orders stay, and telling apps to do the same.
2. **Labels and fields that apps read.** A merchant tags a customer `vip`, or a quiz app
   writes `custom.skin_type`, and Mailchimp or Klaviyo syncs them into an audience.
   Tags are the integration glue across Shopify's app ecosystem.
3. **Groups**, so customers can be treated differently: wholesale prices, payment terms,
   and later B2B companies.

## Decisions

| | Decision |
| --- | --- |
| CD1 | **Tags: deferred** (Q4). Labels that apps sync are expected to be a list custom field (§2 says what 0033 must give them) rather than a separate tag system |
| CD2 | A **staff note** on the customer (at most 5,000 characters). It's free text about a person, so it's personal data (CD9) |
| CD3 | **Typed custom fields** (0002 §3) in their **own note, built before this one** (Q5): generic over owner types, with customers among the first. §4 lists what customers need from it: definitions as data, a namespace per owner (`custom`, `app_<name>`, `spore_<name>`), per-surface visibility, and a `personal` flag. There's no untyped `metadata` blob |
| CD4 | **Marketing consent** per channel (email, SMS) as a small state machine: state, opt-in level, when, and from where. **The newest consent time wins**, not the newest write, so a delayed sync can't overwrite a newer opt-out |
| CD5 | **Customer groups** (`cgrp_…`): named, static, many-to-many, written as sets like publications and category membership, with events. In v1 they're filters and a stable ID other domains can point at. Prices per group come with accounts (§6) |
| CD6 | **Labels, groups and segments are separate things.** Labels (a list custom field) are for integrations. Groups are entities that rules refer to (prices, payment methods). Segments, later, are saved queries. None stands in for another |
| CD10 | **Email stays mandatory** (Q1, 0005 C2 unchanged). Erasure replaces it with an address that can never receive mail (§7.3) |
| CD7 | **Delete** only a customer with no orders (`customer_has_orders`, 409). Otherwise **erase**: personal data is removed from the customer and from its orders, carts and email records, while orders, totals and countries stay. This is Shopify's model |
| CD8 | Erasure is a **request with a cancel window** (10 days by default, `customers.erasure_delay`), run by a job, and announced to apps (`customer.redacted`) so they delete their copies. **Export** answers a data request with one JSON document |
| CD9 | Personal data stays decided in one place (`redact.rs`). The note and personal custom fields are a fifth family, `customer_data:profile` |

## 1. What others do

The research behind this note was done on 2026-10-20 against current docs. The main
sources are linked inline.

**Shopify** ([Customer](https://shopify.dev/docs/api/admin-graphql/latest/objects/Customer))
- **Tags:** a list on the customer, at most 250 tags of 255 characters each,
  case-insensitive. `tagsAdd`/`tagsRemove` are deltas; `tags` on update overwrites. There
  are `customer.tags_added`/`_removed` webhooks, and the list filters `tag:`/`tag_not:`.
  Mailchimp's sync copies chosen tags one way
  ([Mailchimp](https://mailchimp.com/help/shopify-sync-customer-tags/)). Shopify warns
  that any app able to read customers may show tags to the customer.
- **A note** (5,000 characters, the limit on a merge).
- **Metafields** with definitions: a type, validations, and access per surface (`admin`,
  `storefront`, `customerAccount`). The namespace `$app` is reserved for the app that
  owns it. Merchant namespaces are writable by every app with the scope, which gives no
  isolation. There's a legacy of untyped metafields with no definition
  ([ownership](https://shopify.dev/docs/apps/build/custom-data/ownership)).
- **Consent:** `emailMarketingConsent` was moved onto contact-point objects in 2025
  (`defaultEmailAddress.marketingState`). The fields are:
  - state: `NOT_SUBSCRIBED` (never asked), `PENDING`, `SUBSCRIBED`, `UNSUBSCRIBED`, and
    the system-only `REDACTED` and `INVALID`
  - opt-in level: `SINGLE_OPT_IN`, `CONFIRMED_OPT_IN`, `UNKNOWN`
  - `consentUpdatedAt`
  - source

  "The consent state reflects the record with the most recent date." There's one
  webhook per channel. Klaviyo syncs both ways
  ([consent](https://shopify.dev/docs/api/admin-graphql/latest/objects/CustomerEmailMarketingConsentState)).
- **Segments** are saved queries in a small language (`customer_tags CONTAINS 'vip' AND
  amount_spent > 100`). They're dynamic, evaluated asynchronously, and have joined/left
  webhooks (one subscription per segment). They replaced "saved searches" in 2022, a
  painful migration: a stored query language is a contract.
- **B2B:** `Company` → `CompanyLocation` (catalogs, payment terms, tax, checkout-to-draft)
  → `CompanyContact` (a customer, with a role per location). One customer belongs to one
  company, which forces `+suffix` emails for shared inboxes.
- **Privacy:** protected customer data. Name, email, phone and address are each approved
  separately for apps; unapproved fields come back `null` with no explanation.
  - **Mandatory webhooks** for apps: `customers/data_request`, `customers/redact`,
    `shop/redact`.
  - **Erasure** is a request with a 10-day cancel window. "The profile and order history
    remain"; consent becomes `REDACTED`.
  - **Delete** works only before the first order.
  - **Merge** is a previewed async job with listed blockers
    ([privacy](https://shopify.dev/docs/apps/build/privacy-law-compliance)).

**Medusa v2** ([customer module](https://github.com/medusajs/medusa/tree/develop/packages/modules/customer/src/models))
- No tags.
- `metadata` JSON on everything. Typed data means writing your own module plus a link.
- **Groups:** many-to-many, with a pivot that has its own `created_by`. Groups drive
  prices and promotions through a string rule attribute (`customer.groups.id`), whose
  spelling Medusa's own docs disagree on.
- Events carry only `{id}`. There are no membership or address events.
- **Guest and account:** two rows per email, which needs an order transfer.
- **B2B starter:** `Company` and `Employee` modules linked to a customer group, so the
  company's prices come from its group.

**Saleor** ([metadata](https://docs.saleor.io/api-usage/metadata), [customer types](https://docs.saleor.io/developer/users/customer-types))
- `metadata` (public) and `privateMetadata` (staff and apps with permission). Apps use
  the private one as their own storage.
- 3.23.26 (August 2026) added `CustomerType`: exactly one per customer, with typed attributes. Saleor
  added typed fields after years of untyped metadata.
- No customer groups.
- An account deletion flow keeps orders.

**Vendure** ([custom fields](https://docs.vendure.io/guides/developer-guide/custom-fields/))
- `customFields` with `public`/`internal`/`requiresPermission` flags. There's one column
  per field, so every change is a migration.
- Many-to-many `CustomerGroup`s drive promotions (core prices don't vary by group).
- A history timeline: notes, group added/removed, address changes.

**Commerce Layer** ([customers](https://docs.commercelayer.io/core/v/api-reference/customers/object))
- Tags and metadata.
- `reference` + `reference_origin` (an external ID and the system that owns it).
- One group per customer. A group makes a market private, and so chooses its price list.
- **Anonymization** as an async, cancellable state machine (`anonymization_status`).
  Placed orders are never deleted.

**BigCommerce and commercetools**
- **BigCommerce:**
  - **Groups:** one group per customer, carrying price lists and category access.
  - **Segments:** added later (many-to-many, for promotions), so it ended up with two
    concepts.
  - **Attributes:** typed, at most 50, and their type can't be changed.
- **commercetools:**
  - **Groups:** a single customer group became `customerGroupAssignments`
    (many-to-many).
  - **Prices:** price selection uses a fallback ladder (group, channel, country) with a
    deterministic tie-break.

**What we take:**
- Shopify's tags and their limits, delta writes, and events that carry the delta.
- Typed custom fields with per-surface visibility and owner namespaces (Shopify), with
  apps confined to their own namespace (fixing Shopify's gap). No untyped blob (Saleor
  and BigCommerce both retrofitted typed data).
- Consent as a timestamped state machine where the newest consent wins (Shopify).
- Many-to-many groups from the start (the ones that started single, commercetools and
  BigCommerce, changed later).
- Delete only before the first order, otherwise erase in place with a cancel window,
  and tell apps (Shopify, Commerce Layer).
- Specific events for each kind of change (Vendure's history list, against Medusa's
  `{id}`-only events).
- Our explicit `redacted` list over Shopify's silent `null`s.

**What we don't take:**
- Medusa's two rows per email (0005 already declined it).
- Stringly price rules (`customer.groups.id`).
- One group per customer.
- Column-per-field custom fields (Vendure).
- Merchant fields that any app may overwrite (Shopify).

## 2. Tags (CD1: deferred)

There's no separate tag system for now (Q4). What a merchant or an email app uses tags
for (`vip`, `newsletter`, `wholesale`) should be a **list custom field**, for example
`custom.tags` (`list.text`). For that to work as well as Shopify's tags, the custom
fields note must give list fields:

- **Item deltas:** `add` and `remove` items as well as replacing the list. They must be
  idempotent, so adding an item that's already there records nothing. Bulk tagging and
  concurrent apps need this: two apps each adding a label must not overwrite each other.
- **Optional case-insensitive uniqueness** of items (Shopify's tags ignore case).
- **Events that carry the delta** for non-personal fields: `{owner, key, added,
  removed}`. An email app syncing labels needs the change itself, not just a key.
- **A `contains` filter** on the owner's list (`GET /admin/customers?field[custom.tags]=vip`)
  and a store-wide "values in use" listing for autocomplete.
- **A bulk route** that sets one field on up to 250 owners in one call.

If that turns out awkward, a dedicated tag system can still come later. It would follow
Shopify: at most 250 tags of 255 characters, case-insensitive, `tags_added`/`_removed`
events.

## 3. The note (CD2)

- `Customer.note: Option<String>`, at most 5,000 characters, set through `update_customer`.
- `customer.updated` names the field `note` and never carries its text.
- Notes say things like "asked us not to call, recovering from surgery". That's personal
  data in the plain sense, so the note is redacted without `customer_data:profile`
  (§7.1).

## 4. Custom fields (CD3): what customers need from their note

Custom fields get their own design note (Q5), generic over owner types (products,
variants, orders, customers, …), built before this one. This section is the customer
side's starting point for it, not the final model.

**Definition** (`cfd_…`, crate `hypha-custom-fields`, a leaf):

| Field | |
| --- | --- |
| `owner_type` | `customer` (later `product`, `variant`, `order`, …) |
| `namespace`, `key` | `custom.skin_type`. A namespace is `custom` (the merchant's), `app_<name>` or `spore_<name>`; keys are `[a-z0-9_]{1,64}`. Both are immutable |
| `name`, `description` | For the admin's form |
| `type` | `text`, `multiline_text`, `integer`, `decimal` (a string, like money), `boolean`, `date`, `date_time`, `url`, `money`, `choice`, `reference` (a prefixed ID of a given kind), or `list.<one of these>`. Immutable, like Shopify's |
| `validations` | `min`, `max`, `max_length`, `pattern`, `choices`, `list_max`. Tightening one fails while stored values break it (`custom_field_values_invalid`, with a count), so `restore` never meets a value it rejects (CLAUDE.md, "tightening a rule") |
| `visibility` | `admin` (default) or `storefront` (public read). `customer_account` (read or read_write by the customer) is reserved for accounts |
| `personal` | Whether the value is about the person (a birthday, a skin type). Personal values are redacted without `customer_data:profile` |

**Ownership** (fixes Shopify's merchant-namespace gap):
- `custom.*` is created and written by staff and by callers with `customers:write`.
- An app's namespace is reserved by its identity. Only `Actor::App` for that app (or a
  spore, for `spore_<name>`) creates its definitions and writes their values.
  - Other callers read them under the normal rules.
  - Staff can write them only if the definition says `admin: read_write`, which is
    Shopify's knob.
- When an app is uninstalled, its definitions and values are kept until the app is
  removed, then they're deleted with it, as storage links do (`shipping_provider_app`).

**Values:**
- Stored in `custom_field_values (owner_type, owner_id, definition_id, value jsonb)`,
  outside the aggregate, so editing a field never conflicts with editing the customer's
  name (0002 §3).
- `set_custom_fields(owner, {"custom.skin_type": "dry", "app_mailchimp.member_id":
  null})` checks every value against its definition, reports every problem with its
  path (`custom_field_unknown`, `custom_field_invalid`), and writes them in one batch
  with the event `customer.custom_fields_updated {customer, keys}`. The event names the
  keys, not the values.
- `null` removes a value.
- A value with no definition is refused. There are no untyped fields.

**Reads:**
- Customer responses get `custom_fields: {"custom.skin_type": "dry", …}`, filtered by
  visibility and redaction.
- The list filters by a field whose definition says `filterable` (an expression index
  per definition is later; v1 filters `choice` and `boolean` fields through a GIN index
  on `value`).

**External IDs need no new field.** Mailchimp's member ID is
`app_mailchimp.member_id` (Commerce Layer's `reference` + `reference_origin`, with the
namespace saying which system owns it).

**Limits:**
- 100 definitions per owner type per namespace.
- Values up to 16 KiB.
- Lists up to 128 items.

These are constants (they're a contract with apps), like the per-product limits.

**Spores** register definitions in code (`Spore::custom_fields()`). They're created at
startup like the spore's app, under `spore_<name>`.

## 5. Marketing consent (CD4)

```rust
pub struct MarketingConsent {
    state: ConsentState,          // not_subscribed | pending | subscribed | unsubscribed | redacted
    opt_in_level: OptInLevel,     // single | confirmed | unknown
    consented_at: Option<Timestamp>,
    source: ConsentSource,        // checkout | storefront | admin | import | app(AppKey) | spore
}
```

- **Fields:** `Customer.email_marketing` and `Customer.sms_marketing`. SMS needs a phone
  (`consent_needs_phone`).
- **States:**
  - `not_subscribed` is the starting state and can't be set: "never asked" stays
    distinct from "said no".
  - `redacted` is set only by erasure.
- **Writes:** `set_marketing_consent(id, channel, {state, opt_in_level, consented_at?})`.
  - The source comes from the `Ctx`, never from input: an app is `app(<its key>)` and
    staff are `admin`.
  - **The newest `consented_at` wins** (defaulting to now). A write older than the
    stored one changes nothing and says so (`applied: false`, with the current state).
    So an email provider replaying a week-old subscribe can't undo yesterday's
    unsubscribe.
- **Changing the email resets email consent** to `not_subscribed`, unless the same
  request sets it. Consent was given for an address, not a person (Q7).
- **Checkout:**
  - The cart gets `marketing_opt_in: bool` (`PUT /storefront/carts/{id}/marketing`),
    rendered as the usual "Email me with news and offers" box.
  - Placing the order sets `subscribed` / `single` / `checkout` when it's ticked. An
    unticked box never unsubscribes anyone.
  - Double opt-in is the email provider's job: it sets `pending`, sends its
    confirmation, and writes `subscribed` / `confirmed` back. Hypha sends no marketing
    email.
- **Event:** `customer.marketing_consent_updated {customer, channel, state,
  opt_in_level}`. These states aren't personal data in the AP10 sense, and they are
  what a sync needs.
- **Unsubscribe links in Hypha's own emails** are for transactional mail, so there's no
  link. Note 0028's templates stay transactional.

## 6. Customer groups (CD5)

- **`CustomerGroup`** (`cgrp_…`, in `hypha-customers`):
  - `name` (unique among groups, case-insensitive), `handle` (core's `Handle`, unique),
    `description`.
  - Like `CustomerGroup` in Medusa and Vendure. It's a plain aggregate, not a hierarchy.
- **Membership** is set-based, like publications and category membership (0020, 0022):
  - `add_to_group(group, customers)` and `remove_from_group(group, customers)`, staged
    with `StageCustomerGroupMembers` and events
    `customer_group.members_added`/`_removed {group, customers: [...]}`.
  - A customer is in any number of groups. `GET /admin/customers?group=cgrp_…` filters,
    and a customer's response lists its group IDs.
- **Events:** `customer_group.created`, `.updated`, `.deleted`.
- **Deleting a group** drops its memberships (staged with their removal events), unless
  something refers to it. Once prices or payment methods point at groups,
  `RepoError::Reference` → `group_in_use`, as with assets in use.
- **What groups drive.** In v1, only filters, events and their IDs: nothing a shopper
  sees, because nothing on the storefront knows who the shopper is until accounts
  exist. Planned after accounts:
  - **Prices:** `CatalogContext` gains `CustomerGroup(id)`.
    - The comment at `hypha-markets/src/catalog.rs:10` already expects B2B contexts.
    - A cart with a customer in a group resolves the group's catalog in the cart's
      market before the market's own.
    - With several groups, the lowest price wins, ties going to the oldest group
      (commercetools' ladder, made deterministic).
    - This is typed: no string rule attributes.
  - **Payment and shipping methods offered to groups:** a method's "markets and
    channels" list (0027) gains "groups".
  - **Tax exemption per group** (0003 §6).
- **Why groups and not labels for this:** a price list pointing at the label
  `"wholesale"` breaks the day someone retypes it. A group has an ID, a rename keeps it, and
  deleting it is refused while it's in use.

## 7. Personal data

### 7.1 Redaction (CD9)

`redact.rs` stays the one place that decides. It gains a fifth family:

| Scope | Unlocks |
| --- | --- |
| `customer_data:name`, `:email`, `:phone`, `:address` | as today (0029 §4.2) |
| **`customer_data:profile`** | the staff note and every custom field marked `personal` |

- **Tags, groups, consent states and non-personal custom fields** need only
  `customers:read`.
- **`Family::Customer`** removes `note` and the personal keys inside `custom_fields`, and
  lists them in `redacted` (`"custom_fields.custom.birthday"`).
- **Webhooks and extension requests** go through the same rules per the app's grant, as
  now.
- The route test `no_admin_route_shows_personal_data_without_its_scopes` gains the
  note and a personal custom field.
- **Staff roles (Q2, deferred):** a staff session still sees everything. Roles are
  already named scope sets, so a later step can make them obey `customer_data:*` too:
  - `redact.rs` drops its staff exception.
  - The existing roles get every scope, so nothing changes until a merchant narrows
    one.
  - The benefit is data minimisation: GDPR wants access on a need-to-know basis, and a
    support contractor or a packing station that reads orders needn't see notes or
    emails.

### 7.2 Delete (CD7)

- `DELETE /admin/customers/{id}` works only while the customer has no orders. Otherwise
  it's `409 customer_has_orders` and the admin offers erasure. The response says
  `can_delete` (Shopify's `canDelete`).
- It removes the row, its addresses, memberships and custom field values, and
  unlinks carts (`customer` cleared, email kept: a cart without an order is the
  shopper's own data).
- Event: `customer.deleted {customer}`.
- **Needs** `orders.customer_id` indexed, which 0007 already planned. The same index
  gives the orders list a `customer` filter, replacing the admin's
  `/admin/orders?q=<email>` workaround on the customer page.

### 7.3 Erasure (CD8)

**Request:**
- `POST /admin/customers/{id}/erasure` (scope `customers:erase`, a new scope only the
  owner role has by default) sets `erasure_due_at = now + customers.erasure_delay`.
- It records `customer.erasure_requested`.
- `DELETE …/erasure` cancels it within the window (`customer.erasure_cancelled`).
- The customer page shows the countdown.

**The job:** the `erase_customers` job runs in `jobs.rs` like the others, natively and
on the Workers cron. It's a workflow crate, `hypha-privacy`, since it composes
customers, orders, carts, notifications and custom fields. For each due customer it
writes one batch:

| Where | What happens |
| --- | --- |
| Customer | Names, phone, note, addresses and personal custom fields are removed. The email becomes the placeholder below. Consent becomes `redacted`. `erased_at` is set. Groups and non-personal fields stay |
| Orders | `email` becomes the same placeholder, and the shipping (and later billing) address's personal lines are removed. Country, company, lines, totals, payments and fulfilments stay. Tax and accounting need them, and "placed orders are never deleted" |
| Carts | Unplaced carts of the customer are deleted, and placed ones are cleared like orders |
| Email records | `ntf_…` rows lose `to` and their rendered body |
| Events | Nothing to do for payloads that carry IDs, not values (0016). If 0033 lets events carry a non-personal field's values (§2), those can still hold what a merchant typed, so erasure clears this customer's such payloads in `outbox_events` |

- Then it records **`customer.redacted {customer, orders: [...]}`**: Shopify's
  `customers/redact`. Apps with `customers:read` get it whatever they subscribed to, the
  way `app.uninstalled` is always delivered. The manifest docs say an app must delete
  what it holds about that customer.
  - 0029 AP10's "data an app already received isn't Hypha's to delete" stays true;
    this is how Hypha asks.

**The placeholder email (CD10).** Email stays mandatory (`NOT NULL`, unique), so
erasure writes `erased+<customer uuid>@redacted.invalid`.
- `.invalid` is reserved (RFC 2606): no such address can ever receive mail.
- The UUID keeps it unique and says nothing about the person.
- `Email` gets `is_placeholder()`. `hypha-mail` records a message to such an address as
  `skipped` ("customer erased") rather than handing it to the sender. Apps see the
  customer's `erased_at` and must not treat the placeholder as an address.

**After erasure:**
- An erased customer reads as `{id, email (the placeholder), erased_at, groups, orders}`
  and can't be edited (`customer_erased`, 409). Nothing can claim its email back.
- A new order with the person's real email makes a **new** customer, since nothing
  matches it any more.

**Not in v1:**
- Delaying erasure while an order is recent (Shopify holds `customers/redact` for six
  months after the last order). The cancel window is the only delay, and a merchant
  under a retention duty sets it longer. Q6 asks whether to add the hold.
- Automatic erasure of customers inactive for N years (a later `customers.retention`
  setting).

### 7.4 Export

- `GET /admin/customers/{id}/export` (`customers:read` plus every `customer_data:*`
  scope) answers one JSON document:
  - the customer with every field and custom field
  - its addresses, orders (with their addresses and lines) and payments (method,
    amounts, status; no provider secrets)
  - email records and consent history
- It records `customer.data_requested {customer}` so apps can answer too (Shopify's
  `customers/data_request`).
- A request to a merchant is answered by the merchant, so there's no self-service
  export until accounts.

### 7.5 What we don't do

- **Column encryption of personal fields.** Search and sort need the plain values, and
  Postgres-at-rest and TLS cover storage. `SecretBox` stays for secrets.
- **An access log of who read whom.** Shopify asks it of Level 2 apps, not of itself.
  The timeline (later) records changes, not reads.

## 8. Segments (later, reserved)

Dynamic lists, `tag = vip AND orders_count >= 3 AND email_marketing = subscribed`, are
what email apps and discounts want next. This note reserves:

- **Every list filter becomes a typed `CustomerFilter` field**: label fields, groups, consent,
  country, created dates, order count, amount spent. A segment is a saved
  `CustomerFilter` with a version, not a string language. Agents and the admin build
  it from the filter's JSON schema, and a new attribute is a new optional field, so a
  stored segment never needs a migration like Shopify's saved searches did.
- **Order statistics on the customer:** `orders_count`, `first_order_at`,
  `last_order_at`, and the amount spent per currency (markets mean several).
  - Computed on read from the `orders.customer_id` index in v1, for the customer page.
  - Denormalized by an event handler on `order.placed`/`order.cancelled` when segments
    need to filter on them.
- **Membership changes** become `customer_segment.joined`/`left` events, evaluated by a
  job, with a subscription that names its segments (not one per segment like Shopify's).

## 9. Accounts (their own note, reserved here)

0008 §9 already plans `Principal::Customer` with email codes first (Shopify's
passwordless accounts). This note keeps the customer record free of login state:

- There's no `state: enabled/invited/declined` on `Customer`. Shopify's became
  meaningless with passwordless accounts.
- An identity points at the customer, as with staff.
- **One customer per email (0005 C2) makes claiming guest orders automatic.** Once the
  shopper proves the address with a code, they're the customer who already owns those
  orders. That's Saleor's behaviour, without Medusa's order transfer.
- Custom fields' `customer_account` visibility and the shopper's own export and erasure
  request come with accounts.

## 10. B2B (sketch, later)

Groups are not companies. Following Shopify, a later composing crate `hypha-companies`
would hold:

- `Company` → `CompanyLocation`. The location carries what a B2B buyer is priced and
  billed by:
  - a catalog (`CatalogContext::CompanyLocation`)
  - payment terms
  - tax exemption
  - checkout-to-draft
- **Contacts:** a contact is a customer with a role per location (ordering only, or
  location admin).
- **Unlike Shopify, one customer may be a contact of several companies.** Shared
  inboxes and agencies are common, and with one customer per email the alternative is
  `+suffix` addresses.

Nothing in this note blocks it. Groups stay the B2C and wholesale tool, and a company
doesn't need to be faked as a group (Medusa's starter).

## 11. Decisions on the questions (2026-10-20)

- **Q1. Email optional? No.** Email stays mandatory, and erasure writes a placeholder
  that can never receive mail (§7.3, CD10).
  - Strictly, Shopify allows a customer with only a name or a phone. We choose not to:
    one customer per email (0005) stays the rule, with no partial unique index and no
    optional `email` in responses.
  - Phone-only customers (POS) would reopen this.
- **Q2. Staff roles obey `customer_data:*`? Deferred.** Roles are already scope sets;
  revisit with the staff-roles work (§7.1).
- **Q3. `customer_data:profile`? Yes.** It covers the note and personal custom fields.
  It's needed for GDPR minimisation, and it lets an MCP or agent key work with customers
  without seeing what's written about them.
- **Q4. A tag system? Deferred.** Labels become a list custom field. §2 lists what the
  custom fields note must give them.
- **Q5. Custom fields? Their own note, built first.** Generic over owner types; §4 is
  the customer side's input.
- **Q6. Hold erasure for recent orders? No.** The configurable cancel window is enough.
- **Q7. Email change resets email consent? Yes** (§5).

## 12. Build order

Custom fields come first, from their own note. Then, each step a commit series with its
tests, routes, OpenAPI, SDK and e2e steps:

1. **Orders by customer:** the `orders.customer_id` index, an orders list `customer`
   filter, order statistics on the customer response, and the admin customer page using
   them. It's small and independent of custom fields, so it can go first.
2. **The note and editing:**
   - `customer_data:profile` and the note's redaction.
   - Admin: edit forms for the customer (none exist yet).
3. **Delete:** `can_delete`, `customer.deleted`, the admin's delete button.
4. **Marketing consent:**
   - The consent fields and command (newest consent wins); the reset on email change;
     the event.
   - The cart's `marketing_opt_in` and checkout setting it.
   - The storefront's checkbox, and the admin's consent badge and editor.
5. **Customer groups:** the aggregate, membership, events, the `group` filter, admin
   pages.
6. **Custom fields on customers:**
   - Customers as an owner type; personal fields redacted under
     `customer_data:profile`.
   - A `custom.tags`-style list field for labels, the fields on the admin's customer
     page, and the list filter.
7. **Privacy:**
   - `hypha-privacy`, `customers:erase`, the erasure request, the job, the placeholder
     email and `customer.redacted`; export and `customer.data_requested`.
   - The apps docs' "what an app must do on `customer.redacted`".
8. **Then, in their own notes:** customer accounts; group prices, payment methods and
   tax exemption (after accounts); segments; merging customers; B2B companies; staff
   roles under `customer_data:*`.

A Mailchimp-style app is the end-to-end check for steps 4–7:
- It's installed with `customers:read`, `customer_data:name|email` and
  `customers:write`.
- It subscribes to `customer.created`, `.updated`, `.custom_fields_updated` and
  `.marketing_consent_updated`, and reads the customer on each.
- It writes consent back with its own `consented_at`, and stores its member ID in
  `app_mailchimp.member_id`.
- It deletes its member on `customer.redacted`.
