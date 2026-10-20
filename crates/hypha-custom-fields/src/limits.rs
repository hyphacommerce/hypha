//! The published limits (design note 0033, §10, CF10). Kept small on purpose: raising
//! one later breaks nobody, lowering one breaks apps.

/// Definitions per owner type, per namespace.
pub const MAX_DEFINITIONS_PER_NAMESPACE: usize = 128;
/// Filterable definitions per owner type, across namespaces.
pub const MAX_FILTERABLE: usize = 50;
/// Keys in one `set_custom_fields` call.
pub const MAX_KEYS_PER_CALL: usize = 50;
/// Owners in one bulk call (`change_list_items`, `set_field_values`).
pub const MAX_OWNERS_PER_CALL: usize = 250;
/// A `text` value, in characters.
pub const MAX_TEXT_CHARS: usize = 255;
/// `multiline_text`, `rich_text` and `json`, in bytes (serialised, for `json`).
pub const MAX_LONG_BYTES: usize = 16 * 1024;
/// Any value, serialised.
pub const MAX_VALUE_BYTES: usize = 64 * 1024;
/// Items in a list.
pub const MAX_LIST_ITEMS: usize = 128;
/// Choices of a `choice` field.
pub const MAX_CHOICES: usize = 128;
/// A `url` value, in characters.
pub const MAX_URL_CHARS: usize = 2048;
/// A definition's name, in characters.
pub const MAX_NAME_CHARS: usize = 255;
/// A definition's description, in characters.
pub const MAX_DESCRIPTION_CHARS: usize = 1000;
/// A `pattern`, in characters.
pub const MAX_PATTERN_CHARS: usize = 255;
/// Owners named in `custom_field_values_invalid`.
pub const MAX_INVALID_OWNERS_LISTED: usize = 20;
/// Keys a list may ask for with `?custom_fields=`.
pub const MAX_LISTED_KEYS: usize = 10;
/// Event entries carry a value only up to this size, serialised (§6.4); bigger ones
/// carry only the key.
pub const MAX_EVENT_VALUE_BYTES: usize = 1024;
/// Significant digits of a `decimal` or a measurement's value.
pub const MAX_DECIMAL_DIGITS: u32 = 20;
/// Digits after the point of a `decimal` or a measurement's value.
pub const MAX_DECIMAL_SCALE: u32 = 10;
