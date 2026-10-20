//! Personal data leaves only for callers allowed to see it (design note 0029, AP10,
//! §4.2). One place decides, per field, for every channel: admin responses (the DTO
//! families below), webhook payloads (`event_data`), extension requests (`for_app`:
//! `payments.start`'s `customer_email`, `shipping.rates`' destination) and, later, proxy
//! headers.
//!
//! The rules:
//! - A staff session (`Actor::User`) sees everything: staff work the store.
//! - Every other admin-surface caller (secret API keys, apps, agents) sees a personal
//!   field only with its `customer_data:*` scope: `name` (first and last names),
//!   `email`, `phone`, `address` (street lines, postal code, city, province; country
//!   and company stay). Without it the field is absent, and the object lists it in
//!   `"redacted": [...]`.
//! - The storefront surface isn't redacted: a publishable key serves shoppers their own
//!   cart and order (they typed that data, and checkout needs it back), so its routes
//!   never call this module.
//! - Staff records (users, invites, the store's own settings) aren't customer data:
//!   `users:read` governs them, not these scopes.
//! - Webhooks and extension requests: the receiving app's grant decides (a hand-made
//!   app without a grant sees no personal data).

use std::fmt;

use hypha_apps::App;
use hypha_core::{Actor, Ctx, Scope};
use hypha_custom_fields::{
    CallerKind, CustomFieldDefinition, FieldCaller, FieldKey, ReadAccess, read_access,
};
use hypha_extensions::PostalAddress;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{MapAccess, SeqAccess, Visitor},
    ser::{SerializeMap, SerializeSeq},
};
use serde_json::Value;

use crate::error::ApiError;

/// Which personal fields go: `true` removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Redaction {
    name: bool,
    email: bool,
    phone: bool,
    address: bool,
}

/// The personal fields, by the scope that unlocks them.
const NAME: &[&str] = &["first_name", "last_name"];
const EMAIL: &[&str] = &["email"];
const PHONE: &[&str] = &["phone"];
const ADDRESS: &[&str] = &["line1", "line2", "city", "province", "postal_code"];

impl Redaction {
    /// Nothing removed.
    pub const NONE: Redaction = Redaction {
        name: false,
        email: false,
        phone: false,
        address: false,
    };

    /// For an admin-surface caller (see the module's rules).
    pub fn of(ctx: &Ctx) -> Self {
        if ctx.is_staff() {
            return Self::NONE;
        }
        Self::by_scopes(|s| ctx.has_scope(s))
    }

    /// For a caller holding the scopes `has` says it holds (an app's grant, for its
    /// webhooks).
    pub fn by_scopes(has: impl Fn(Scope) -> bool) -> Self {
        Self {
            name: !has(Scope::CustomerDataName),
            email: !has(Scope::CustomerDataEmail),
            phone: !has(Scope::CustomerDataPhone),
            address: !has(Scope::CustomerDataAddress),
        }
    }

    /// For what Hypha sends `app` (webhooks, extension requests): its grant's scopes;
    /// an app without a grant (made by hand, or uninstalled) gets no personal data.
    pub fn for_app(app: &App) -> Self {
        Self::by_scopes(|scope| app.grant().is_some_and(|g| g.has(scope)))
    }

    pub fn is_none(&self) -> bool {
        *self == Self::NONE
    }

    /// Whether email addresses go.
    pub fn removes_email(&self) -> bool {
        self.email
    }

    /// A postal address an extension point sends (`shipping.rates`' destination), as
    /// `address` strips one in a response: names, phone, street, postal code, city and
    /// province go without their scopes, listed in its `redacted`; country and company
    /// stay.
    pub fn postal_address(&self, a: &mut PostalAddress) {
        let removed: Vec<&'static str> = self
            .removed()
            .filter(|f| [NAME, PHONE, ADDRESS].iter().any(|g| g.contains(f)))
            .collect();
        for field in &removed {
            match *field {
                "first_name" => a.first_name = None,
                "last_name" => a.last_name = None,
                "phone" => a.phone = None,
                "line1" => a.line1 = None,
                "line2" => a.line2 = None,
                "city" => a.city = None,
                "province" => a.province = None,
                "postal_code" => a.postal_code = None,
                _ => {}
            }
        }
        a.redacted = (!removed.is_empty()).then(|| removed.iter().map(|f| f.to_string()).collect());
    }

    fn removed(&self) -> impl Iterator<Item = &'static str> {
        [
            (self.name, NAME),
            (self.email, EMAIL),
            (self.phone, PHONE),
            (self.address, ADDRESS),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .flat_map(|(_, fields)| fields.iter().copied())
    }

    /// Removes the personal fields among `only` from `object`, listing what it removed
    /// in its `redacted`. Fields that aren't there (or are `null`) are listed all the
    /// same, so the list doesn't tell which ones had a value.
    fn strip(&self, object: &mut Fields, only: &[&[&str]]) {
        let removed: Vec<&str> = self
            .removed()
            .filter(|field| only.iter().any(|group| group.contains(field)))
            .collect();
        if removed.is_empty() {
            return;
        }
        object.retain(|(key, _)| !removed.contains(&key.as_str()) && key != "redacted");
        object.push((
            "redacted".to_string(),
            Node::Scalar(Value::Array(
                removed.iter().map(|f| Value::String((*f).into())).collect(),
            )),
        ));
    }

    /// A postal address (an order's, a cart's, an address-book entry): names, phone and
    /// street, postal code, city, province.
    fn address(&self, node: &mut Node) {
        if let Node::Object(object) = node {
            self.strip(object, &[NAME, PHONE, ADDRESS]);
        }
    }

    /// A customer: names, email, phone, and the address book.
    fn customer(&self, node: &mut Node) {
        if let Node::Object(object) = node {
            if let Some(Node::Array(addresses)) = field(object, "addresses") {
                addresses.iter_mut().for_each(|a| self.address(a));
            }
            self.strip(object, &[NAME, EMAIL, PHONE]);
        }
    }

    /// An order or a cart: the buyer's email and the addresses.
    fn order(&self, node: &mut Node) {
        if let Node::Object(object) = node {
            for key in ["shipping_address", "billing_address"] {
                if let Some(address) = field(object, key) {
                    self.address(address);
                }
            }
            self.strip(object, &[EMAIL]);
        }
    }

    /// An email's record: who it went to.
    fn notification(&self, node: &mut Node) {
        if let Node::Object(object) = node
            && self.email
        {
            object.retain(|(key, _)| key != "to" && key != "redacted");
            object.push((
                "redacted".to_string(),
                Node::Scalar(Value::Array(vec!["to".into()])),
            ));
        }
    }

    /// Any value (a webhook payload): every personal field anywhere in it.
    fn deep(&self, value: &mut Value) {
        match value {
            Value::Object(object) => {
                object.values_mut().for_each(|v| self.deep(v));
                let fields: Vec<&str> = self.removed().collect();
                let before = object.len();
                object.retain(|k, _| !fields.contains(&k.as_str()));
                if object.len() != before {
                    object.insert(
                        "redacted".to_string(),
                        Value::Array(fields.iter().map(|f| Value::String((*f).into())).collect()),
                    );
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|v| self.deep(v)),
            _ => {}
        }
    }
}

/// An object's fields in the order they were serialized.
type Fields = Vec<(String, Node)>;

fn field<'a>(object: &'a mut Fields, key: &str) -> Option<&'a mut Node> {
    object.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// JSON that keeps object keys in the order the DTO wrote them (`serde_json::Value`
/// sorts them, without its `preserve_order` feature), so a redacted response reads like
/// any other.
#[derive(Debug, Clone, PartialEq)]
enum Node {
    Object(Fields),
    Array(Vec<Node>),
    Scalar(Value),
}

impl Node {
    fn of(dto: impl Serialize) -> Result<Self, ApiError> {
        let text = serde_json::to_string(&dto).map_err(ApiError::internal)?;
        serde_json::from_str(&text).map_err(ApiError::internal)
    }
}

impl Serialize for Node {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Node::Object(fields) => {
                let mut map = serializer.serialize_map(Some(fields.len()))?;
                for (key, value) in fields {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
            Node::Array(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Node::Scalar(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(NodeVisitor)
    }
}

struct NodeVisitor;

impl<'de> Visitor<'de> for NodeVisitor {
    type Value = Node;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("JSON")
    }
    fn visit_bool<E>(self, v: bool) -> Result<Node, E> {
        Ok(Node::Scalar(Value::Bool(v)))
    }
    fn visit_i64<E>(self, v: i64) -> Result<Node, E> {
        Ok(Node::Scalar(Value::from(v)))
    }
    fn visit_u64<E>(self, v: u64) -> Result<Node, E> {
        Ok(Node::Scalar(Value::from(v)))
    }
    fn visit_f64<E>(self, v: f64) -> Result<Node, E> {
        Ok(Node::Scalar(Value::from(v)))
    }
    fn visit_str<E>(self, v: &str) -> Result<Node, E> {
        Ok(Node::Scalar(Value::String(v.to_string())))
    }
    fn visit_string<E>(self, v: String) -> Result<Node, E> {
        Ok(Node::Scalar(Value::String(v)))
    }
    fn visit_unit<E>(self) -> Result<Node, E> {
        Ok(Node::Scalar(Value::Null))
    }
    fn visit_none<E>(self) -> Result<Node, E> {
        Ok(Node::Scalar(Value::Null))
    }
    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Node, D::Error> {
        Node::deserialize(d)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Node, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(Node::Array(items))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Node, A::Error> {
        let mut fields = Vec::new();
        while let Some((key, value)) = map.next_entry::<String, Node>()? {
            fields.push((key, value));
        }
        Ok(Node::Object(fields))
    }
}

/// The DTO families that carry personal data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Customer,
    /// Orders and carts.
    Order,
    Notification,
}

impl Family {
    fn apply(self, r: &Redaction, node: &mut Node) {
        match self {
            Family::Customer => r.customer(node),
            Family::Order => r.order(node),
            Family::Notification => r.notification(node),
        }
    }
}

/// A response with the caller's redaction applied: what handlers of those families
/// return (`Json<Redacted>`), documented as the DTO it was made from.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct Redacted(Node);

/// `dto` of `family` as `ctx` may see it.
pub fn one(ctx: &Ctx, family: Family, dto: impl Serialize) -> Result<Redacted, ApiError> {
    one_with_fields(ctx, family, dto, &[])
}

/// A list page (`{"items": [...], …}`) of `family` as `ctx` may see it.
pub fn page(ctx: &Ctx, family: Family, page: impl Serialize) -> Result<Redacted, ApiError> {
    page_with_fields(ctx, family, page, &[])
}

/// `one` for a DTO carrying `custom_fields` (design note 0033, §5.2): `hidden` are the
/// personal fields the caller may not read (`hypha_custom_fields::FieldsRead::redacted`,
/// per request, since definitions are data). They're absent from its `custom_fields`
/// and listed in its `redacted` as `custom_fields.<key>`, held or not, like every other
/// personal field.
pub fn one_with_fields(
    ctx: &Ctx,
    family: Family,
    dto: impl Serialize,
    hidden: &[FieldKey],
) -> Result<Redacted, ApiError> {
    let mut node = Node::of(dto)?;
    let r = Redaction::of(ctx);
    if !r.is_none() {
        family.apply(&r, &mut node);
    }
    hide_fields(&mut node, hidden);
    Ok(Redacted(node))
}

/// `page` with `one_with_fields`' custom fields, for each item that carries them.
pub fn page_with_fields(
    ctx: &Ctx,
    family: Family,
    page: impl Serialize,
    hidden: &[FieldKey],
) -> Result<Redacted, ApiError> {
    let mut node = Node::of(page)?;
    let r = Redaction::of(ctx);
    if (!r.is_none() || !hidden.is_empty())
        && let Node::Object(fields) = &mut node
        && let Some(Node::Array(items)) = field(fields, "items")
    {
        for item in items.iter_mut() {
            if !r.is_none() {
                family.apply(&r, item);
            }
            hide_fields(item, hidden);
        }
    }
    Ok(Redacted(node))
}

/// Removes `hidden` from an object's `custom_fields` and lists them in its `redacted`;
/// an object without `custom_fields` is left alone.
fn hide_fields(node: &mut Node, hidden: &[FieldKey]) {
    let Node::Object(object) = node else {
        return;
    };
    if hidden.is_empty() {
        return;
    }
    let keys: Vec<String> = hidden.iter().map(|k| k.to_string()).collect();
    match field(object, "custom_fields") {
        Some(Node::Object(values)) => values.retain(|(key, _)| !keys.contains(key)),
        _ => return,
    }
    let names = keys
        .iter()
        .map(|k| Value::String(format!("custom_fields.{k}")));
    match field(object, "redacted") {
        Some(Node::Scalar(Value::Array(listed))) => listed.extend(names),
        _ => {
            object.retain(|(key, _)| key != "redacted");
            object.push((
                "redacted".to_string(),
                Node::Scalar(Value::Array(names.collect())),
            ));
        }
    }
}

/// Fails unless `ctx` may see every personal field `scopes` unlock: for searching or
/// sorting by them, which would tell what they hold. Staff always may.
pub fn require_fields(ctx: &Ctx, scopes: &[Scope]) -> Result<(), ApiError> {
    if ctx.is_staff() {
        return Ok(());
    }
    for scope in scopes {
        ctx.require(*scope)
            .map_err(|e| ApiError::from(hypha_auth::CommandError::Auth(e)))?;
    }
    Ok(())
}

/// An event's `data` as an app holding `has` may receive it.
pub fn event_data(has: impl Fn(Scope) -> bool, data: &Value) -> Value {
    let r = Redaction::by_scopes(has);
    let mut data = data.clone();
    if !r.is_none() {
        r.deep(&mut data);
    }
    data
}

/// Whether `event_type` is a `<owner>.custom_fields_updated` event, whose entries
/// `custom_field_changes` filters per receiving app.
pub fn is_custom_fields_event(event_type: &str) -> bool {
    event_type.ends_with(".custom_fields_updated")
}

/// A `<owner>.custom_fields_updated` event's `data` as `app` may receive it (design note
/// 0033, §6.4): change entries for fields it may not read are dropped (another app's
/// `private` field; a personal one without `customer_data:profile`; any field without
/// the owner's read scope), judged by `definitions` as they are now, with the rules
/// `hypha_custom_fields::read_access` applies to the app's own API calls. An entry
/// whose definition is gone since stays only in the merchant's or the app's own
/// namespace. `None` when no entry is left: the delivery isn't sent.
///
/// An app without a grant (made by hand) reads the records it subscribed to but no
/// personal data, as its other webhooks.
pub fn custom_field_changes(
    app: &App,
    definitions: &[CustomFieldDefinition],
    data: &Value,
) -> Option<Value> {
    let key = app.key().clone();
    let kind = match key.as_str().starts_with("spore_") {
        true => CallerKind::Spore(key),
        false => CallerKind::App(key),
    };
    let scopes = match app.grant() {
        Some(grant) => grant.scopes().to_vec(),
        None => vec![Scope::ProductsRead, Scope::CustomersRead, Scope::OrdersRead],
    };
    let ctx = Ctx::new(Actor::App(app.id()), scopes);
    changes_visible_to(&FieldCaller::new(&ctx, &kind), definitions, data)
}

/// `custom_field_changes` for any caller (`GET /admin/events/{id}` for an app or a key,
/// which sees an event as an app would receive it).
pub fn changes_visible_to(
    caller: &FieldCaller<'_>,
    definitions: &[CustomFieldDefinition],
    data: &Value,
) -> Option<Value> {
    let Some(changes) = data.get("changes").and_then(Value::as_array) else {
        return Some(data.clone());
    };
    let own = caller.namespace_owner().map(|k| k.as_str().to_string());
    let kept: Vec<Value> = changes
        .iter()
        .filter(|change| {
            let Some(name) = change.get("key").and_then(Value::as_str) else {
                return false;
            };
            match definitions
                .iter()
                .find(|d| d.field_key().to_string() == name)
            {
                Some(definition) => read_access(definition, caller) == ReadAccess::Visible,
                None => {
                    let namespace = name.split_once('.').map_or(name, |(ns, _)| ns);
                    namespace == "custom" || own.as_deref() == Some(namespace)
                }
            }
        })
        .cloned()
        .collect();
    if kept.is_empty() {
        return None;
    }
    let mut data = data.clone();
    data["changes"] = Value::Array(kept);
    Some(data)
}

#[cfg(test)]
mod tests {
    use hypha_core::{Actor, ApiKeyId, AppId, UserId};
    use serde_json::json;
    use uuid::Uuid;

    use super::*;

    fn order() -> Value {
        json!({
            "id": "ord_1",
            "email": "ana@example.com",
            "shipping_address": {
                "first_name": "Ana", "last_name": "Horvat", "company": "Acme",
                "line1": "Ilica 1", "line2": null, "city": "Zagreb", "province": null,
                "postal_code": "10000", "country": "HR", "phone": "+385 1 234"
            }
        })
    }

    fn app(scopes: Vec<Scope>) -> Ctx {
        Ctx::new(Actor::App(AppId::from_uuid(Uuid::from_u128(1))), scopes)
    }

    #[test]
    fn fields_keep_the_order_the_dto_wrote_them() {
        #[derive(Serialize)]
        struct Dto {
            id: &'static str,
            email: &'static str,
            amount: i64,
        }
        let dto = Dto {
            id: "ord_1",
            email: "a@b.c",
            amount: 9_007_199_254_740_991,
        };
        let text = serde_json::to_string(&one(&app(vec![]), Family::Order, dto).unwrap()).unwrap();
        assert_eq!(
            text,
            r#"{"id":"ord_1","amount":9007199254740991,"redacted":["email"]}"#
        );
    }

    #[test]
    fn staff_see_everything() {
        let staff = Ctx::new(Actor::User(UserId::from_uuid(Uuid::from_u128(1))), vec![]);
        let seen = serde_json::to_value(one(&staff, Family::Order, order()).unwrap()).unwrap();
        assert_eq!(seen, order());
    }

    #[test]
    fn without_scopes_personal_fields_are_absent_and_listed() {
        let seen = serde_json::to_value(
            one(&app(vec![Scope::OrdersRead]), Family::Order, order()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            seen,
            json!({
                "id": "ord_1",
                "redacted": ["email"],
                "shipping_address": {
                    "company": "Acme", "country": "HR",
                    "redacted": ["first_name", "last_name", "phone", "line1", "line2", "city",
                                 "province", "postal_code"]
                }
            })
        );
    }

    #[test]
    fn each_scope_unlocks_its_fields_only() {
        let key = Ctx::new(
            Actor::ApiKey(ApiKeyId::from_uuid(Uuid::from_u128(2))),
            vec![Scope::CustomerDataEmail, Scope::CustomerDataAddress],
        );
        let seen = serde_json::to_value(one(&key, Family::Order, order()).unwrap()).unwrap();
        assert_eq!(seen["email"], "ana@example.com");
        assert!(seen.get("redacted").is_none());
        assert_eq!(seen["shipping_address"]["city"], "Zagreb");
        assert_eq!(
            seen["shipping_address"]["redacted"],
            json!(["first_name", "last_name", "phone"])
        );
    }

    #[test]
    fn a_customers_address_book_is_redacted_too() {
        let customer = json!({
            "id": "cus_1", "email": "a@b.c", "first_name": "A", "last_name": null,
            "phone": null, "addresses": [{"id": "addr_1", "last_name": "B", "line1": "x",
            "city": "y", "country": "HR"}]
        });
        let seen = serde_json::to_value(
            page(
                &app(vec![Scope::CustomerDataName]),
                Family::Customer,
                json!({"items": [customer]}),
            )
            .unwrap(),
        )
        .unwrap();
        let item = &seen["items"][0];
        assert_eq!(item["first_name"], "A");
        assert!(item.get("email").is_none());
        assert_eq!(item["redacted"], json!(["email", "phone"]));
        assert_eq!(item["addresses"][0]["last_name"], "B");
        assert!(item["addresses"][0].get("line1").is_none());
    }

    #[test]
    fn webhook_payloads_lose_personal_fields_anywhere() {
        let data = json!({"order": "ord_1", "buyer": {"email": "a@b.c", "country": "HR"},
                          "fields": ["email"]});
        let seen = event_data(|s| s == Scope::CustomerDataPhone, &data);
        assert_eq!(seen["buyer"].get("email"), None);
        assert_eq!(
            seen["fields"],
            json!(["email"]),
            "names of fields are no data"
        );
        assert_eq!(event_data(|_| true, &data), data);
    }

    fn destination() -> PostalAddress {
        PostalAddress {
            first_name: Some("Ana".into()),
            last_name: Some("Horvat".into()),
            company: Some("Shop d.o.o.".into()),
            line1: Some("Ilica 1".into()),
            line2: None,
            city: Some("Zagreb".into()),
            province: None,
            postal_code: Some("10000".into()),
            country: hypha_core::CountryCode::new("HR").unwrap(),
            phone: Some("+385 1".into()),
            redacted: None,
        }
    }

    #[test]
    fn a_rate_apps_destination_keeps_only_the_country_and_company_without_scopes() {
        let mut a = destination();
        Redaction::by_scopes(|_| false).postal_address(&mut a);
        assert_eq!(
            serde_json::to_value(&a).unwrap(),
            json!({"first_name": null, "company": "Shop d.o.o.", "line2": null,
                   "province": null, "postal_code": null, "country": "HR", "phone": null,
                   "redacted": ["first_name", "last_name", "phone", "line1", "line2",
                                "city", "province", "postal_code"]})
        );
    }

    #[test]
    fn customer_data_address_unlocks_the_street_and_postal_code_only() {
        let mut a = destination();
        Redaction::by_scopes(|s| s == Scope::CustomerDataAddress).postal_address(&mut a);
        assert_eq!(a.postal_code.as_deref(), Some("10000"));
        assert_eq!(a.line1.as_deref(), Some("Ilica 1"));
        assert_eq!(a.last_name, None);
        assert_eq!(a.phone, None);
        assert_eq!(
            a.redacted,
            Some(vec![
                "first_name".into(),
                "last_name".into(),
                "phone".into()
            ])
        );
        let mut all = destination();
        Redaction::NONE.postal_address(&mut all);
        assert_eq!(all, destination());
    }

    // ---------- custom fields (design note 0033, §5.2, §6.4) ----------

    fn hidden(keys: &[&str]) -> Vec<FieldKey> {
        keys.iter().map(|k| FieldKey::parse(k).unwrap()).collect()
    }

    #[test]
    fn hidden_fields_leave_custom_fields_and_join_redacted() {
        let customer = json!({
            "id": "cus_1",
            "custom_fields": {"custom.tier": "gold", "custom.birthday": "1990-05-04"}
        });
        let ctx = app(vec![
            Scope::CustomersRead,
            Scope::CustomerDataName,
            Scope::CustomerDataEmail,
            Scope::CustomerDataPhone,
        ]);
        let seen = serde_json::to_value(
            one_with_fields(
                &ctx,
                Family::Customer,
                customer,
                &hidden(&["custom.birthday", "custom.note"]),
            )
            .unwrap(),
        )
        .unwrap();
        // A key the owner doesn't hold is listed too: the list tells nothing.
        assert_eq!(
            seen,
            json!({
                "id": "cus_1",
                "custom_fields": {"custom.tier": "gold"},
                "redacted": ["custom_fields.custom.birthday", "custom_fields.custom.note"]
            })
        );
    }

    #[test]
    fn hidden_fields_are_appended_to_the_redacted_fields() {
        let customer = json!({
            "id": "cus_1",
            "email": "ana@example.com",
            "custom_fields": {"custom.birthday": "1990-05-04"}
        });
        let seen = serde_json::to_value(
            one_with_fields(
                &app(vec![Scope::CustomersRead]),
                Family::Customer,
                customer,
                &hidden(&["custom.birthday"]),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(seen["custom_fields"], json!({}));
        assert_eq!(
            seen["redacted"],
            json!([
                "first_name",
                "last_name",
                "email",
                "phone",
                "custom_fields.custom.birthday"
            ])
        );
    }

    #[test]
    fn an_object_without_custom_fields_is_left_alone() {
        let seen = serde_json::to_value(
            one_with_fields(
                &app(vec![Scope::OrdersRead, Scope::CustomerDataEmail]),
                Family::Order,
                json!({"id": "ord_1", "email": "ana@example.com"}),
                &hidden(&["custom.note"]),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(seen, json!({"id": "ord_1", "email": "ana@example.com"}));
    }

    #[test]
    fn a_pages_items_each_hide_their_fields() {
        let page = json!({"items": [
            {"id": "cus_1", "custom_fields": {"custom.birthday": "1990-05-04"}},
            {"id": "cus_2", "custom_fields": {}}
        ], "next_cursor": null});
        let ctx = app(vec![
            Scope::CustomersRead,
            Scope::CustomerDataName,
            Scope::CustomerDataEmail,
            Scope::CustomerDataPhone,
        ]);
        let seen = serde_json::to_value(
            page_with_fields(&ctx, Family::Customer, page, &hidden(&["custom.birthday"])).unwrap(),
        )
        .unwrap();
        for item in seen["items"].as_array().unwrap() {
            assert_eq!(item["custom_fields"], json!({}), "{item}");
            assert_eq!(
                item["redacted"],
                json!(["custom_fields.custom.birthday"]),
                "{item}"
            );
        }
    }

    fn definition(namespace: &str, key: &str, personal: bool) -> CustomFieldDefinition {
        use hypha_core::CustomFieldDefinitionId;
        use hypha_custom_fields::{
            Access, Capabilities, DefinitionSpec, FieldType, Namespace, OwnerType, RawValidations,
        };
        CustomFieldDefinition::new(
            CustomFieldDefinitionId::from_uuid(Uuid::from_u128(1)),
            Namespace::parse(namespace).unwrap(),
            DefinitionSpec {
                owner_type: OwnerType::Customer,
                key: key.to_string(),
                field_type: FieldType::parse("text").unwrap(),
                name: key.to_string(),
                description: None,
                validations: RawValidations::default(),
                access: Access::default(),
                private: false,
                personal,
                capabilities: Capabilities::default(),
            },
            0,
        )
        .unwrap()
    }

    fn changes(keys: &[&str]) -> Value {
        let changes: Vec<Value> = keys.iter().map(|k| json!({"key": k})).collect();
        json!({"customer": "cus_1", "changes": changes})
    }

    fn keys_of(data: &Value) -> Vec<&str> {
        data["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["key"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn an_event_loses_the_changes_its_receiver_may_not_read() {
        use hypha_core::AppKey;
        let definitions = [
            definition("custom", "tier", false),
            definition("custom", "birthday", true),
        ];
        let ctx = app(vec![Scope::CustomersRead]);
        let kind = CallerKind::App(AppKey::new("app_crm").unwrap());
        let caller = FieldCaller::new(&ctx, &kind);
        let data = changes(&["custom.tier", "custom.birthday"]);
        let seen = changes_visible_to(&caller, &definitions, &data).unwrap();
        assert_eq!(keys_of(&seen), ["custom.tier"]);
        assert_eq!(seen["customer"], "cus_1");
    }

    #[test]
    fn a_change_to_a_deleted_field_stays_only_in_its_own_or_the_merchants_namespace() {
        use hypha_core::AppKey;
        let ctx = app(vec![Scope::CustomersRead]);
        let kind = CallerKind::App(AppKey::new("app_crm").unwrap());
        let caller = FieldCaller::new(&ctx, &kind);
        let data = changes(&["custom.gone", "app_crm.gone", "app_other.gone"]);
        let seen = changes_visible_to(&caller, &[], &data).unwrap();
        assert_eq!(keys_of(&seen), ["custom.gone", "app_crm.gone"]);
    }

    #[test]
    fn an_event_with_no_change_left_is_not_sent() {
        let definitions = [definition("custom", "birthday", true)];
        let ctx = app(vec![Scope::CustomersRead]);
        let kind = CallerKind::Merchant;
        let caller = FieldCaller::new(&ctx, &kind);
        assert_eq!(
            changes_visible_to(&caller, &definitions, &changes(&["custom.birthday"])),
            None
        );
    }

    #[test]
    fn custom_fields_events_are_named_by_their_suffix() {
        assert!(is_custom_fields_event("customer.custom_fields_updated"));
        assert!(!is_custom_fields_event("customer.updated"));
    }
}
