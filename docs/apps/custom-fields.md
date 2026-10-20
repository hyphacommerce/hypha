# Custom fields in the manifest

An app keeps its own data on Hypha's objects (a customer's ID in the app, an order's
number in an ERP) as custom fields (design note 0033). It declares them in its manifest's
`custom_fields`. They're always in the app's own namespace, so a field with the key
`member_id` in the app `app_mailchimp` is addressed as `app_mailchimp.member_id`. A
manifest can't name a namespace.

```json
"custom_fields": [
  { "owner_type": "customer", "key": "member_id", "type": "text", "name": "Mailchimp member",
    "description": "The contact's ID in Mailchimp",
    "validations": { "max_length": 64 },
    "access": { "admin": "read", "storefront": "none" }, "private": false,
    "capabilities": { "unique": true, "filterable": true } }
]
```

## Fields

| Field | |
| --- | --- |
| `owner_type` | `product`, `variant`, `category` (a product category), `customer` or `order` |
| `key` | `[a-z][a-z0-9_]{1,63}`: 2 to 64 lowercase letters, digits or `_`, starting with a letter. Unique per owner type |
| `type` | `text`, `multiline_text`, `rich_text`, `integer`, `decimal`, `boolean`, `date`, `date_time`, `url`, `money`, `measurement`, `choice`, `reference`, `json`, or `list.<t>` for `t` in `text`, `integer`, `decimal`, `date`, `date_time`, `url`, `money`, `measurement`, `choice`, `reference`. It can't change once installed |
| `name` | The label on forms, 1 to 255 characters |
| `description` | Optional help text, 1 to 1,000 characters |
| `validations` | An object with the type's validations (`min_length`, `max_length`, `pattern`, `min`, `max`, `choices`, `kind`, …; design note 0033, §3). Hypha checks them against the type when it creates the field |
| `access.admin` | `read` (the default): staff see the values but only the app writes them. `read_write`: staff can edit them too |
| `access.storefront` | `none` (the default) or `read`: storefronts see the values |
| `private` | `false` by default. When `true`, only the app and staff read the values; other apps don't, even with the owner's read scope |
| `personal` | `false` by default. Only for `customer` and `order` fields: the value is about a person. Callers without `customer_data:profile` don't see it, and webhooks leave it out. A manifest with a personal field must ask for `customer_data:profile`, with a reason |
| `capabilities.filterable` | Lists can filter on the field |
| `capabilities.unique` | No two owners hold the same value. Only for `text`, `integer` and `url` |

A manifest declares at most 128 fields per owner type. Every broken rule is reported at
once with its path, as `422 manifest_invalid`, e.g. `custom_fields[2].type`, or
`custom_fields[1].key` for a key declared twice on the same owner type.

## Lifecycle

- **Install:** the fields are created when staff approve the app. The consent page lists
  the owners the app adds fields to, and which fields are personal.
- **Refresh:** a refreshed manifest's fields are applied when it is (approved, when it
  asks for new scopes):
  - a new key creates a field
  - a changed name, description, validations, access or capabilities updates it
  - a key the manifest no longer declares is **deleted with its values**; the consent
    page lists these as "removes fields"
  - a changed `type` makes the manifest invalid (`custom_fields[i].type`): declare a new
    key instead
- **Definitions API:** an app's fields are read-only there (`409 custom_field_managed`).
  Their values are written by the app (and by staff when `access.admin` is
  `read_write`).
- **Uninstall:** the fields and values stay, readable by staff. Removing the app (its
  second `DELETE`) deletes them.
