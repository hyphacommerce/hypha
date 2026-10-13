# 0005: Customers

- **Status:** Accepted (2026-10-13). C2 and C3 were decided in conversation; the rest
  were proposed and not objected to.
- **Scope:** customer records, their address book, and the buyer on a cart. Customer
  login (accounts, auth), groups, marketing consent and tax exemption come later.
  Orders (note 0007) use what this note adds: they find or create the customer by email
  at checkout.
- **Related:** 0002 (closed aggregates, link by ID), 0003 and 0004 (the same leaf-crate
  shape).

## Decisions

| | Decision |
| --- | --- |
| C1 | A new leaf crate `hypha-customers` owns the `Customer` aggregate (`cus_…`): email, optional first name, last name and phone |
| C2 | **One customer per email** (Shopify), not Medusa's guest + registered pair. Email is a unique key; emails are stored lowercased, so the comparison is case-insensitive |
| C3 | **No accounts or login yet.** Customer auth needs the auth work on the roadmap; an account state is added then |
| C4 | Customers own an address book (`addr_…` entries) with an optional default shipping and default billing address. `Address` is a validated value type in `hypha-core`, because orders snapshot addresses too |
| C5 | A cart has an optional `email` and `customer`, like Shopify's `buyerIdentity`. Setting the customer also sets the cart's email to theirs |
| C6 | `find_or_create_customer(email)` is the command checkout uses, so a guest becomes a customer on their first order |
| C7 | `Email` is a validated value type in `hypha-core`: trimmed, lowercased, one `@`, a dot in the domain, no spaces, at most 254 characters |

## 1. What others do

**Shopify** ([`Customer`](https://shopify.dev/docs/api/admin-graphql/latest/objects/Customer), [`CustomerState`](https://shopify.dev/docs/api/admin-graphql/latest/enums/CustomerState), [`CartBuyerIdentityInput`](https://shopify.dev/docs/api/storefront/latest/input-objects/CartBuyerIdentityInput))
- One customer per email (or phone). Whether they have a login is the record's `state`:
  `DISABLED` (no account), `INVITED`, `ENABLED` or `DECLINED`.
- Addresses with a `defaultAddress`, tags, a note, tax exemptions, marketing consent,
  and order statistics (`numberOfOrders`, `amountSpent`).
- A customer with orders can't be deleted.
- A cart's `buyerIdentity` holds `email`, `phone`, `countryCode`, and the logged-in
  customer (through a `customerAccessToken`).

**Medusa v2** ([customer accounts](https://docs.medusajs.com/resources/commerce-modules/customer/customer-accounts), [Customer model](https://docs.medusajs.com/resources/references/customer/models/Customer))
- `Customer` has `email`, names, `phone`, `company_name`, `metadata` and
  `has_account`.
- A guest order creates a customer with `has_account: false`. Registering with the same
  email creates a **second** customer with `has_account: true`, so one email can have a
  guest and a registered record.
- `CustomerAddress` rows carry `is_default_shipping` and `is_default_billing`.
- Customer groups, used to target promotions and prices.
- Checkout runs `findOrCreateCustomerStep` by email.

**What we take:** one record per email and an account state added later (Shopify);
default shipping and billing addresses, and find-or-create at checkout (Medusa).

**What we don't take:** Medusa's two records per email. A guest's orders and their
later account would live on different customers and need merging.

## 2. Model

**`Email` (`hypha-core`)**
- Validated on construction (C7) and serialized as a string, like `CountryCode`.
- Lowercasing the whole address is technically wrong for the part before the `@`, but
  every mail provider treats it case-insensitively, and so do Shopify's lookups.

**`Address` (`hypha-core`)**
- Fields: `first_name`, `last_name`, `company`, `line1`, `line2`, `city`, `province`,
  `postal_code`, `country` (`CountryCode`), `phone`.
- Required: `last_name`, `line1`, `city`, `country`. Every text field is trimmed, an
  empty optional field becomes `None`, and each is at most 255 characters.
- Built from a plain `AddressParts` struct and readable as one, so storage and hosts can
  copy it field by field.
- Postal code formats differ per country; they aren't checked yet.

**`Customer` aggregate (`hypha-customers`)**
- `email`, `first_name`, `last_name`, `phone` (each at most 255 characters, empty means
  none).
- `addresses`: `{ id: AddressId, address: Address }`, owned by the customer like variants
  by a product.
- `default_shipping`, `default_billing`: `Option<AddressId>`, always pointing at one of
  its addresses. Removing an address clears a default that pointed at it.

**Commands:** `create_customer`, `get_customer`, `update_customer` (an empty string
clears an optional field), `add_address`, `update_address`, `remove_address`, and
`find_or_create_customer`. Duplicate emails are `customer_exists` (409), found by a
pre-check and by mapping the repo's unique key, as elsewhere.

## 3. Changes to existing crates

- **`hypha-core`:** `CustomerId` (`cus_`), `AddressId` (`addr_`); scopes
  `customers:read`, `customers:write`; `Email`, `Address`.
- **`hypha-cart`:** `Cart` gains `email` and `customer`; commands `set_cart_email` and
  `set_cart_customer`. `CartPorts` gains the customer repo, so `set_cart_customer` can
  check that the customer exists (a new edge `hypha-cart → hypha-customers`, allowed:
  composing → leaf).
- **`hypha-store-postgres`:** migration 0008 (`customers`, `customer_addresses`) and
  0009 (`carts.email`, `carts.customer_id`, both nullable, so no backfill is needed).
- **`hypha-host-axum`:** `POST /customers`, `GET`/`PATCH /customers/{id}`,
  `POST /customers/{id}/addresses`, `PUT`/`DELETE /customers/{id}/addresses/{address_id}`,
  `PUT /carts/{id}/email`, `PUT /carts/{id}/customer` (`null` clears it).

## 4. Build order

1. Core: IDs, scopes, `Email`, `Address`.
2. `hypha-customers`: aggregate, commands, in-memory repo and contract.
3. Postgres: migration 0008 and the repo.
4. Cart: buyer email and customer; migration 0009.
5. Host routes and e2e.

## 5. Later

- Customer accounts and login (an account state on the customer, Shopify-style).
- Customer groups (B2B prices), marketing consent, tags and notes, tax exemption.
- Listing and searching customers; deleting them (refused once they have orders).
