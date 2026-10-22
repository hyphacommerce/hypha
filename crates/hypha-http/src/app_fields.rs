//! An app's custom fields: the `custom_fields` its manifest declares become definitions in
//! its namespace (`app_<name>`) whenever a manifest is applied (an approval, a refresh that
//! needs none), staged in the very batch that applies it, so the app and its fields change
//! together or not at all.
//!
//! It lives here because it joins two crates that can't see each other: `hypha-oauth`
//! (a composing crate over `hypha-apps`, which owns manifests) and the
//! `hypha-custom-fields` leaf. `hypha-oauth` prepares its writes without committing them
//! (`Prepared`); this module stages the sync into the same batch and commits.

use hypha_apps::{AppCustomField, AppManifest, ManifestError, ManifestProblem};
use hypha_core::{AppKey, Ctx, HasBatch, IdGenerator, RepoError};
use hypha_custom_fields::{
    Access, AdminAccess, Capabilities, CommandError, CustomFieldDefinition, DefinitionRepo,
    DefinitionSpec, FieldType, HasCustomFieldDefinitions, HasCustomFieldValues, Namespace,
    OwnerType, RawValidations, StageCustomFieldDefinitions, StageCustomFieldValues,
    StorefrontAccess, SyncPlan, keys, plan_sync, stage_managed_definitions,
};
use hypha_oauth::Prepared;

use crate::error::ApiError;

/// The ports a sync needs.
pub trait FieldSyncPorts:
    HasCustomFieldDefinitions
    + HasCustomFieldValues
    + HasBatch<Batch: StageCustomFieldDefinitions + StageCustomFieldValues>
{
}

impl<T> FieldSyncPorts for T where
    T: HasCustomFieldDefinitions
        + HasCustomFieldValues
        + HasBatch<Batch: StageCustomFieldDefinitions + StageCustomFieldValues>
{
}

/// One manifest field as a definition's spec (the namespace comes from the app).
fn spec_of(field: &AppCustomField) -> Result<DefinitionSpec, ManifestProblem> {
    let problem = |part: &str, message: String| ManifestProblem {
        path: part.to_string(),
        message,
    };
    // The manifest's checks accept exactly the owner types and types the leaf names.
    let owner_type = OwnerType::ALL
        .into_iter()
        .find(|t| t.as_str() == field.owner_type.as_str())
        .ok_or_else(|| {
            problem(
                "owner_type",
                format!("{} is not an owner type", field.owner_type),
            )
        })?;
    let field_type = FieldType::parse(&field.field_type.to_string())
        .map_err(|e| problem("type", e.to_string()))?;
    let validations: RawValidations =
        serde_json::from_value(serde_json::Value::Object(field.validations.clone()))
            .map_err(|e| problem("validations", e.to_string()))?;
    Ok(DefinitionSpec {
        owner_type,
        key: field.key.clone(),
        field_type,
        name: field.name.clone(),
        description: field.description.clone(),
        validations,
        access: Access {
            admin: match field.admin {
                hypha_apps::AdminAccess::Read => AdminAccess::Read,
                hypha_apps::AdminAccess::ReadWrite => AdminAccess::ReadWrite,
            },
            storefront: match field.storefront {
                hypha_apps::StorefrontAccess::None => StorefrontAccess::None,
                hypha_apps::StorefrontAccess::Read => StorefrontAccess::Read,
            },
        },
        private: field.private,
        personal: field.personal,
        capabilities: Capabilities {
            filterable: field.filterable,
            unique: field.unique,
        },
    })
}

/// The manifest's fields as definitions in its app's namespace, each checked against
/// the definitions' own rules (a validation that doesn't fit the type, a filterable
/// rich text): every field that can't be one is a problem at `custom_fields[i]…`, as
/// the manifest's own checks report.
pub fn specs_of(manifest: &AppManifest) -> Result<Vec<DefinitionSpec>, ManifestError> {
    let namespace = Namespace::of_app(manifest.key().clone());
    let mut specs = Vec::new();
    let mut problems = Vec::new();
    for (n, field) in manifest.custom_fields().iter().enumerate() {
        let checked = spec_of(field).and_then(|spec| {
            CustomFieldDefinition::new(
                hypha_core::CustomFieldDefinitionId::from_uuid(uuid::Uuid::nil()),
                namespace.clone(),
                spec.clone(),
                0,
            )
            .map(|_| spec)
            .map_err(|e| ManifestProblem {
                path: String::new(),
                message: e.to_string(),
            })
        });
        match checked {
            Ok(spec) => specs.push(spec),
            Err(p) if p.path.is_empty() => problems.push(ManifestProblem {
                path: format!("custom_fields[{n}]"),
                message: p.message,
            }),
            Err(p) => problems.push(ManifestProblem {
                path: format!("custom_fields[{n}].{}", p.path),
                message: p.message,
            }),
        }
    }
    match problems.is_empty() {
        true => Ok(specs),
        false => Err(ManifestError { problems }),
    }
}

/// A manifest refused for its fields: 422 `manifest_invalid` with `details.errors`.
fn invalid(e: ManifestError) -> ApiError {
    ApiError::manifest(
        axum::http::StatusCode::UNPROCESSABLE_ENTITY,
        hypha_apps::FetchError::Invalid(e),
    )
}

/// A sync's error: a field whose stored definition has another type is the manifest's
/// problem at that field's `type`, as `check_retypes` reports one; the rest are the leaf's.
fn sync_error(manifest: &AppManifest, e: CommandError) -> ApiError {
    if let CommandError::TypeChanged(key) = &e {
        let at = manifest
            .custom_fields()
            .iter()
            .position(|f| key.ends_with(&format!(".{}", f.key)));
        if let Some(n) = at {
            return invalid(ManifestError {
                problems: vec![ManifestProblem {
                    path: format!("custom_fields[{n}].type"),
                    message: format!(
                        "{key} is stored with another type: a field's type can't change (declare a new key)"
                    ),
                }],
            });
        }
    }
    ApiError::from(e)
}

/// Stages making the app's namespace hold exactly `manifest`'s fields into `batch` (as
/// the system: whoever applies a manifest was allowed to, and the app's own refresh
/// needs no `apps:write`).
async fn stage<P: FieldSyncPorts>(
    ports: &P,
    ids: &impl IdGenerator,
    batch: &mut P::Batch,
    manifest: &AppManifest,
) -> Result<(Namespace, SyncPlan), ApiError> {
    let namespace = Namespace::of_app(manifest.key().clone());
    let specs = specs_of(manifest).map_err(invalid)?;
    let plan = stage_managed_definitions(&Ctx::system(), ports, ids, &namespace, specs, batch)
        .await
        .map_err(|e| sync_error(manifest, e))?;
    Ok((namespace, plan))
}

/// Commits what `hypha-oauth` prepared, with the app's fields synced to the manifest it
/// applies (if it applies one) in the same batch.
pub async fn commit_applying<P: FieldSyncPorts, T>(
    ports: &P,
    ids: &impl IdGenerator,
    prepared: Prepared<P::Batch, T>,
) -> Result<T, ApiError> {
    let Prepared {
        mut batch,
        answer,
        applies,
    } = prepared;
    let synced = match &applies {
        Some(manifest) => Some(stage(ports, ids, &mut batch, manifest).await?),
        None => None,
    };
    match ports.commit(batch).await {
        Ok(()) => Ok(answer),
        Err(e @ RepoError::Duplicate(keys::CUSTOM_FIELD_KEY)) => Err(match &synced {
            Some((namespace, plan)) => {
                hypha_custom_fields::sync_commit_error(ports, namespace, plan, e)
                    .await
                    .into()
            }
            None => e.into(),
        }),
        Err(e) => Err(e.into()),
    }
}

/// The definitions in `key`'s namespace now (none for a key that isn't one).
pub async fn stored_fields<P: HasCustomFieldDefinitions>(
    ports: &P,
    key: &str,
) -> Result<Vec<CustomFieldDefinition>, ApiError> {
    let Ok(key) = AppKey::new(key) else {
        return Ok(Vec::new());
    };
    Ok(ports
        .custom_field_definitions()
        .list_namespace(&Namespace::of_app(key))
        .await?)
}

/// Whether applying `manifest` over the namespace's `stored` definitions deletes any
/// (and their values): such a refresh waits for staff's consent. A manifest whose
/// fields can't be synced at all says no here, and is refused when it's applied.
pub fn removes_fields(stored: &[CustomFieldDefinition], manifest: &AppManifest) -> bool {
    let namespace = Namespace::of_app(manifest.key().clone());
    specs_of(manifest)
        .ok()
        .and_then(|specs| plan_sync(&namespace, stored, &specs).ok())
        .is_some_and(|plan| !plan.delete.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn manifest(fields: serde_json::Value) -> AppManifest {
        AppManifest::parse(
            &serde_json::to_vec(&json!({
                "key": "app_erp",
                "name": "ERP",
                "version": "1.0.0",
                "app_url": "https://erp.example.com",
                "redirect_uris": ["/callback"],
                "scopes": ["customers:read"],
                "privacy_url": "/privacy",
                "custom_fields": fields
            }))
            .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn a_manifest_field_becomes_a_spec_with_its_access_and_capabilities() {
        let m = manifest(json!([{
            "owner_type": "customer", "key": "member_id", "type": "text", "name": "Member",
            "validations": {"max_length": 40},
            "access": {"admin": "read_write", "storefront": "read"},
            "private": true, "capabilities": {"unique": true, "filterable": true}
        }]));
        let specs = specs_of(&m).unwrap();
        assert_eq!(specs.len(), 1);
        let spec = &specs[0];
        assert_eq!(spec.owner_type, OwnerType::Customer);
        assert_eq!(spec.field_type, FieldType::parse("text").unwrap());
        assert_eq!(spec.validations.max_length, Some(40));
        assert_eq!(spec.access.admin, AdminAccess::ReadWrite);
        assert_eq!(spec.access.storefront, StorefrontAccess::Read);
        assert!(spec.private && spec.capabilities.unique && spec.capabilities.filterable);
    }

    #[test]
    fn validations_that_do_not_fit_the_type_are_problems_at_the_field() {
        let m = manifest(json!([
            {"owner_type": "product", "key": "ok", "type": "text", "name": "Ok"},
            {"owner_type": "product", "key": "age", "type": "integer", "name": "Age",
             "validations": {"max_length": 3}},
            {"owner_type": "product", "key": "bad", "type": "text", "name": "Bad",
             "validations": {"nonsense": 1}}
        ]));
        let paths: Vec<String> = specs_of(&m)
            .unwrap_err()
            .problems
            .into_iter()
            .map(|p| p.path)
            .collect();
        assert_eq!(paths, ["custom_fields[1]", "custom_fields[2].validations"]);
    }
}
