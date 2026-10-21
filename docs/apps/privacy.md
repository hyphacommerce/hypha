# Personal data and apps

What an app holds about a customer is the app's to protect. Hypha tells apps what it
knows and what it did, and asks them to follow (design note 0032, §7; design note 0029,
AP10).

## What you receive

- Webhook payloads and extension requests are redacted per your grant: without
  `customer_data:name`, `:email`, `:phone` or `:address` the matching fields are absent
  and listed in `redacted`; custom fields marked `personal` need `customer_data:profile`.
- `customer.marketing_consent_updated` carries a channel's consent state
  (`subscribed`, `unsubscribed`, `pending`, `redacted`), its opt-in level and nothing
  personal. Write consent back with `PUT /admin/customers/{id}/marketing/{channel}`: the
  newest `consented_at` wins, so send the time the customer actually said so, and a
  write older than what Hypha holds answers `applied: false`.
- Labels (`custom.tags`) and customer groups (`customer_group.*` events, `groups` on the
  customer) aren't personal data: they need only `customers:read`.

## `customer.redacted`

When a merchant erases a customer, Hypha removes the person's data from the customer,
their orders, carts and email records, and then records `customer.redacted`:

```json
{ "customer": "cus_…", "orders": ["ord_…", "ord_…"] }
```

Every installed app with `customers:read` receives it, whether or not it subscribed to
`customer.*`, the way `app.uninstalled` is always delivered. On receiving it an app
must:

1. Delete what it holds about that customer: names, emails, phones, addresses, notes,
   and anything derived from them (a mailing-list member, a CRM contact, support
   tickets' personal fields).
2. Do the same for the listed orders' personal data, if it stored any. Order numbers,
   totals, countries and line items stay in Hypha and may stay with you.
3. Keep nothing that would identify the person again: after erasure the customer
   reads as its ID, a placeholder email (`erased+<uuid>@redacted.invalid`, under a
   domain that can never receive mail), its groups and its orders' statistics, with
   `erased_at` set. Never treat the placeholder as an address.

Deliveries are retried like any webhook; handle a repeat as a no-op.

## `customer.data_requested`

Recorded when staff export a customer's data (`GET /admin/customers/{id}/export`): the
merchant is answering a person's request to see what is held about them. If you hold
data about the customer, make it available to the merchant on request; Hypha's export
covers only what Hypha holds.

## `customer.deleted`

A customer without orders was removed for good. Its custom field values go with it on
Hypha's side; remove your copies too.
