//! Who reads and writes which fields (design note 0033, §5). One function each, used by
//! the commands here and by the HTTP layer (responses, redaction, filters, webhook
//! entries), so the rules live in one place.

use hypha_core::{Actor, AppKey, Ctx, Scope};

use crate::{
    definition::{AdminAccess, CustomFieldDefinition, StorefrontAccess},
    namespace::Namespace,
};

/// Who is calling, as namespaces see it. `Ctx`'s `Actor::App` holds the app's ID, not
/// its key, so the host says which app (or spore) it is after loading it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallerKind {
    /// Staff, the merchant's API keys, the system: on the admin surface, not as an app.
    Merchant,
    /// An app (`Actor::App`), by its key (`app_<name>`).
    App(AppKey),
    /// A spore's own code, through its `SporeEnv` (`spore_<name>`).
    Spore(AppKey),
    /// The storefront surface (a publishable key).
    Storefront,
}

/// A caller for custom fields: its `Ctx` (scopes, actor) and what kind of caller it is.
#[derive(Debug, Clone, Copy)]
pub struct FieldCaller<'a> {
    ctx: &'a Ctx,
    kind: &'a CallerKind,
}

impl<'a> FieldCaller<'a> {
    pub fn new(ctx: &'a Ctx, kind: &'a CallerKind) -> Self {
        Self { ctx, kind }
    }

    pub fn ctx(&self) -> &'a Ctx {
        self.ctx
    }

    /// The kind, with a guard: a `Merchant` whose actor is an app is an app the host
    /// didn't name, which owns no namespace and isn't the merchant.
    fn effective(&self) -> Effective<'a> {
        match self.kind {
            CallerKind::Merchant if matches!(self.ctx.actor(), Actor::App(_)) => {
                Effective::UnnamedApp
            }
            CallerKind::Merchant => Effective::Merchant,
            CallerKind::App(key) | CallerKind::Spore(key) => Effective::Owner(key),
            CallerKind::Storefront => Effective::Storefront,
        }
    }

    /// Whether the caller sees personal values (§5.2): staff sessions and the system
    /// always (AP10), others with `customer_data:profile`.
    pub fn sees_personal(&self) -> bool {
        self.ctx.is_staff()
            || self.ctx.actor() == Actor::System
            || self.ctx.has_scope(Scope::CustomerDataProfile)
    }

    /// Whether `namespace` is the caller's own (an app's or a spore's).
    fn owns(&self, namespace: &Namespace) -> bool {
        matches!(self.effective(), Effective::Owner(key) if namespace.managed_by() == Some(key))
    }
}

enum Effective<'a> {
    Merchant,
    Owner(&'a AppKey),
    UnnamedApp,
    Storefront,
}

/// What a caller sees of a field's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadAccess {
    Visible,
    /// Personal and the caller lacks `customer_data:profile`: absent from
    /// `custom_fields` and listed in `redacted` (`custom_fields.custom.birthday`).
    Redacted,
    /// Not for this caller at all (another app's private field, the storefront for a
    /// field it isn't given, no read scope on the owner).
    Hidden,
}

/// Why a caller may not write a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteDenied {
    /// The caller lacks the owner's write scope.
    Scope(Scope),
    /// Another app's or spore's field (staff only when `access.admin` is `read_write`),
    /// or the storefront.
    ReadOnly,
    /// A personal field, without `customer_data:profile`.
    Personal,
}

/// What `caller` sees of `definition`'s values (§5.1, §5.2).
pub fn read_access(definition: &CustomFieldDefinition, caller: &FieldCaller<'_>) -> ReadAccess {
    let read_scope = definition.owner_type().read_scope();
    if !caller.ctx.has_scope(read_scope) {
        return ReadAccess::Hidden;
    }
    let visible = match caller.effective() {
        Effective::Storefront => definition.access().storefront == StorefrontAccess::Read,
        Effective::Merchant => true,
        Effective::Owner(_) | Effective::UnnamedApp => {
            !definition.private() || caller.owns(definition.namespace())
        }
    };
    if !visible {
        return ReadAccess::Hidden;
    }
    if definition.personal() && !caller.sees_personal() {
        return ReadAccess::Redacted;
    }
    ReadAccess::Visible
}

/// Whether `caller` may write `definition`'s values (§5.1).
pub fn write_access(
    definition: &CustomFieldDefinition,
    caller: &FieldCaller<'_>,
) -> Result<(), WriteDenied> {
    let effective = caller.effective();
    if matches!(effective, Effective::Storefront) {
        return Err(WriteDenied::ReadOnly);
    }
    let write_scope = definition.owner_type().write_scope();
    if !caller.ctx.has_scope(write_scope) {
        return Err(WriteDenied::Scope(write_scope));
    }
    let allowed = match definition.namespace() {
        // The merchant's fields: anyone with the owner's write scope, as on Shopify.
        Namespace::Custom => true,
        Namespace::App(_) | Namespace::Spore(_) => match effective {
            Effective::Merchant => definition.access().admin == AdminAccess::ReadWrite,
            Effective::Owner(_) => caller.owns(definition.namespace()),
            Effective::UnnamedApp | Effective::Storefront => false,
        },
    };
    if !allowed {
        return Err(WriteDenied::ReadOnly);
    }
    if definition.personal() && !caller.sees_personal() {
        return Err(WriteDenied::Personal);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use hypha_core::{AppId, CustomFieldDefinitionId, UserId};
    use uuid::Uuid;

    use super::*;
    use crate::{
        definition::{Access, Capabilities, DefinitionSpec},
        owner::OwnerType,
        types::{FieldType, RawValidations},
    };

    fn definition(namespace: &str, owner_type: OwnerType) -> CustomFieldDefinition {
        CustomFieldDefinition::new(
            CustomFieldDefinitionId::from_uuid(Uuid::from_u128(1)),
            Namespace::parse(namespace).unwrap(),
            DefinitionSpec {
                owner_type,
                key: "field".to_string(),
                field_type: FieldType::parse("text").unwrap(),
                name: "Field".to_string(),
                description: None,
                validations: RawValidations::default(),
                access: Access::default(),
                private: false,
                personal: false,
                capabilities: Capabilities::default(),
            },
            0,
        )
        .unwrap()
    }

    fn private(mut d: CustomFieldDefinition) -> CustomFieldDefinition {
        d.change(crate::DefinitionChange {
            private: Some(true),
            ..Default::default()
        })
        .unwrap();
        d
    }

    fn staff(scopes: Vec<Scope>) -> Ctx {
        Ctx::new(Actor::User(UserId::from_uuid(Uuid::from_u128(1))), scopes)
    }

    fn app_ctx(scopes: Vec<Scope>) -> Ctx {
        Ctx::new(Actor::App(AppId::from_uuid(Uuid::from_u128(2))), scopes)
    }

    fn app(name: &str) -> CallerKind {
        CallerKind::App(AppKey::new(name).unwrap())
    }

    #[test]
    fn reading_needs_the_owners_read_scope() {
        let d = definition("custom", OwnerType::Order);
        let ctx = staff(vec![Scope::ProductsRead]);
        assert_eq!(
            read_access(&d, &FieldCaller::new(&ctx, &CallerKind::Merchant)),
            ReadAccess::Hidden
        );
        let ctx = staff(vec![Scope::OrdersRead]);
        assert_eq!(
            read_access(&d, &FieldCaller::new(&ctx, &CallerKind::Merchant)),
            ReadAccess::Visible
        );
    }

    #[test]
    fn a_private_field_is_its_app_and_staffs_only() {
        let d = private(definition("app_erp", OwnerType::Product));
        let ctx = app_ctx(vec![Scope::ProductsRead]);
        let erp = app("app_erp");
        let other = app("app_other");
        assert_eq!(
            read_access(&d, &FieldCaller::new(&ctx, &erp)),
            ReadAccess::Visible
        );
        assert_eq!(
            read_access(&d, &FieldCaller::new(&ctx, &other)),
            ReadAccess::Hidden
        );
        let staff = staff(vec![Scope::ProductsRead]);
        assert_eq!(
            read_access(&d, &FieldCaller::new(&staff, &CallerKind::Merchant)),
            ReadAccess::Visible
        );
    }

    #[test]
    fn an_app_the_host_did_not_name_is_not_the_merchant() {
        let d = private(definition("app_erp", OwnerType::Product));
        let ctx = app_ctx(vec![Scope::ProductsWrite]);
        let caller = FieldCaller::new(&ctx, &CallerKind::Merchant);
        assert_eq!(read_access(&d, &caller), ReadAccess::Hidden);
        assert_eq!(write_access(&d, &caller), Err(WriteDenied::ReadOnly));
    }

    #[test]
    fn the_storefront_sees_only_what_it_is_given() {
        let mut d = definition("custom", OwnerType::Product);
        let ctx = Ctx::new(Actor::System, vec![Scope::ProductsRead]);
        let caller = FieldCaller::new(&ctx, &CallerKind::Storefront);
        assert_eq!(read_access(&d, &caller), ReadAccess::Hidden);
        d.change(crate::DefinitionChange {
            access: Some(Access {
                admin: AdminAccess::ReadWrite,
                storefront: StorefrontAccess::Read,
            }),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(read_access(&d, &caller), ReadAccess::Visible);
        assert_eq!(write_access(&d, &caller), Err(WriteDenied::ReadOnly));
    }

    #[test]
    fn personal_values_need_the_profile_scope_unless_staff() {
        let mut d = definition("custom", OwnerType::Customer);
        d.change(crate::DefinitionChange {
            personal: Some(true),
            ..Default::default()
        })
        .unwrap();
        let key = Ctx::new(
            Actor::ApiKey(hypha_core::ApiKeyId::from_uuid(Uuid::from_u128(3))),
            vec![Scope::CustomersWrite],
        );
        let caller = FieldCaller::new(&key, &CallerKind::Merchant);
        assert_eq!(read_access(&d, &caller), ReadAccess::Redacted);
        assert_eq!(write_access(&d, &caller), Err(WriteDenied::Personal));
        let with_profile = Ctx::new(
            Actor::ApiKey(hypha_core::ApiKeyId::from_uuid(Uuid::from_u128(3))),
            vec![Scope::CustomersWrite, Scope::CustomerDataProfile],
        );
        assert_eq!(
            read_access(&d, &FieldCaller::new(&with_profile, &CallerKind::Merchant)),
            ReadAccess::Visible
        );
        let session = staff(vec![Scope::CustomersRead]);
        assert_eq!(
            read_access(&d, &FieldCaller::new(&session, &CallerKind::Merchant)),
            ReadAccess::Visible
        );
    }

    #[test]
    fn writing_needs_the_owners_write_scope() {
        let d = definition("custom", OwnerType::Product);
        let ctx = staff(vec![Scope::ProductsRead]);
        assert_eq!(
            write_access(&d, &FieldCaller::new(&ctx, &CallerKind::Merchant)),
            Err(WriteDenied::Scope(Scope::ProductsWrite))
        );
    }

    #[test]
    fn any_writer_writes_merchant_fields() {
        let d = definition("custom", OwnerType::Product);
        let ctx = app_ctx(vec![Scope::ProductsWrite]);
        assert_eq!(
            write_access(&d, &FieldCaller::new(&ctx, &app("app_erp"))),
            Ok(())
        );
    }

    #[test]
    fn only_its_app_writes_an_app_field_and_staff_when_allowed() {
        let mut d = definition("app_erp", OwnerType::Product);
        let ctx = app_ctx(vec![Scope::ProductsWrite]);
        assert_eq!(
            write_access(&d, &FieldCaller::new(&ctx, &app("app_erp"))),
            Ok(())
        );
        assert_eq!(
            write_access(&d, &FieldCaller::new(&ctx, &app("app_other"))),
            Err(WriteDenied::ReadOnly)
        );
        let staff = staff(vec![Scope::ProductsWrite]);
        assert_eq!(
            write_access(&d, &FieldCaller::new(&staff, &CallerKind::Merchant)),
            Ok(())
        );
        d.change(crate::DefinitionChange {
            access: Some(Access {
                admin: AdminAccess::Read,
                storefront: StorefrontAccess::None,
            }),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            write_access(&d, &FieldCaller::new(&staff, &CallerKind::Merchant)),
            Err(WriteDenied::ReadOnly)
        );
    }

    #[test]
    fn a_spore_writes_its_own_fields() {
        let d = definition("spore_stripe", OwnerType::Order);
        let ctx = Ctx::system();
        let spore = CallerKind::Spore(AppKey::new("spore_stripe").unwrap());
        assert_eq!(write_access(&d, &FieldCaller::new(&ctx, &spore)), Ok(()));
        let other = CallerKind::Spore(AppKey::new("spore_other").unwrap());
        assert_eq!(
            write_access(&d, &FieldCaller::new(&ctx, &other)),
            Err(WriteDenied::ReadOnly)
        );
    }
}
