//! Suppliers and their catalogues.

use hypha_core::{
    Page, PageRequest, Recorded, RepoError, SupplierId, VariantId, Version, Versioned,
    stamp_insert, stamp_update,
};
use hypha_purchasing::{
    ContactDraft, PaymentDue, PaymentTermsDraft, Supplier, SupplierDraft, SupplierFilter,
    SupplierRepo, SupplierSnapshot, SupplierVariant, SupplierVariantDraft, SupplierVariantRepo,
};
use serde::{Deserialize, Serialize};
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::{
    columns::{
        ADDRESS_LEN, Param, address_columns, address_from_row, address_values, col,
        currency_from_db, stamp_columns, stamps_from_row, unit_cost_from_db, unit_cost_to_db,
        version_from_db, version_to_db, write_stamps,
    },
    error::{corrupt, repo_error},
    outbox::insert_events,
    store::{PgConnection, PgStore, Queries, in_transaction},
};

/// A contact as stored in the `contacts` list.
#[derive(Serialize, Deserialize)]
struct ContactJson {
    name: String,
    email: Option<String>,
    phone: Option<String>,
    primary: bool,
}

/// The columns a supplier's fields are written to, in `values`' order.
const FIELDS: [&str; 9] = [
    "name",
    "contacts",
    "currency",
    "payment_terms_due",
    "payment_terms_days",
    "payment_terms_note",
    "lead_time_days",
    "tax_number",
    "notes",
];

fn columns() -> String {
    format!(
        "id, version, name, contacts::text AS contacts, {}, currency, payment_terms_due, \
         payment_terms_days, payment_terms_note, lead_time_days, tax_number, notes, archived, {}",
        address_columns("address_"),
        stamp_columns("")
    )
}

fn days_to_db(days: Option<u32>) -> Result<Option<i32>, RepoError> {
    days.map(|d| i32::try_from(d).map_err(|_| corrupt(format!("{d} days"))))
        .transpose()
}

fn days_from_db(days: Option<i32>) -> Result<Option<u32>, RepoError> {
    days.map(|d| u32::try_from(d).map_err(|_| corrupt(format!("{d} days"))))
        .transpose()
}

fn supplier_from_row(row: &Row) -> Result<Supplier, RepoError> {
    let id = SupplierId::from_uuid(col(row, "id")?);
    let contacts: String = col(row, "contacts")?;
    let contacts: Vec<ContactJson> = serde_json::from_str(&contacts)
        .map_err(|e| corrupt(format!("supplier {id} contacts: {e}")))?;
    let due: Option<String> = col(row, "payment_terms_due")?;
    let days = days_from_db(col(row, "payment_terms_days")?)?;
    let payment_terms = match (due.as_deref(), days) {
        (None, _) => None,
        (Some("in_advance"), None) => Some(PaymentDue::InAdvance),
        (Some("on_receipt"), None) => Some(PaymentDue::OnReceipt),
        (Some("net"), Some(days)) => Some(PaymentDue::Net { days }),
        (Some(other), _) => {
            return Err(corrupt(format!("supplier {id} payment terms {other:?}")));
        }
    }
    .map(|due| -> Result<_, RepoError> {
        Ok(PaymentTermsDraft {
            due,
            note: col(row, "payment_terms_note")?,
        })
    })
    .transpose()?;
    let currency: String = col(row, "currency")?;
    Supplier::restore(SupplierSnapshot {
        id,
        version: version_from_db(col(row, "version")?)?,
        stamps: stamps_from_row(row)?,
        draft: SupplierDraft {
            name: col(row, "name")?,
            contacts: contacts
                .into_iter()
                .map(|c| ContactDraft {
                    name: c.name,
                    email: c.email,
                    phone: c.phone,
                    primary: c.primary,
                })
                .collect(),
            address: address_from_row(row, "address_")?,
            currency: currency_from_db(&currency)?,
            payment_terms,
            lead_time_days: days_from_db(col(row, "lead_time_days")?)?,
            tax_number: col(row, "tax_number")?,
            notes: col(row, "notes")?,
            archived: col(row, "archived")?,
        },
    })
    .map_err(|e| corrupt(format!("supplier {id}: {e}")))
}

/// A supplier's field values, owned so the parameters can borrow them.
struct Values<'a> {
    draft: &'a SupplierDraft,
    currency: &'static str,
    contacts: String,
    address: [Option<&'a str>; ADDRESS_LEN],
    due: Option<&'static str>,
    days: Option<i32>,
    note: Option<&'a str>,
    lead_time: Option<i32>,
}

impl<'a> Values<'a> {
    fn of(draft: &'a SupplierDraft) -> Result<Self, RepoError> {
        let contacts: Vec<ContactJson> = draft
            .contacts
            .iter()
            .map(|c| ContactJson {
                name: c.name.clone(),
                email: c.email.clone(),
                phone: c.phone.clone(),
                primary: c.primary,
            })
            .collect();
        let terms = draft.payment_terms.as_ref();
        Ok(Self {
            draft,
            currency: draft.currency.as_str(),
            contacts: serde_json::to_string(&contacts)
                .map_err(|e| corrupt(format!("supplier contacts: {e}")))?,
            address: address_values(draft.address.as_ref()),
            due: terms.map(|t| t.due.as_str()),
            days: days_to_db(terms.and_then(|t| match t.due {
                PaymentDue::Net { days } => Some(days),
                PaymentDue::InAdvance | PaymentDue::OnReceipt => None,
            }))?,
            note: terms.and_then(|t| t.note.as_deref()),
            lead_time: days_to_db(draft.lead_time_days)?,
        })
    }

    /// The fields in `FIELDS` order, then the address, then `archived`.
    fn params(&'a self) -> Vec<Param<'a>> {
        let mut params: Vec<Param<'a>> = vec![
            (&self.draft.name, Type::TEXT),
            (&self.contacts, Type::TEXT),
            (&self.currency, Type::TEXT),
            (&self.due, Type::TEXT),
            (&self.days, Type::INT4),
            (&self.note, Type::TEXT),
            (&self.lead_time, Type::INT4),
            (&self.draft.tax_number, Type::TEXT),
            (&self.draft.notes, Type::TEXT),
        ];
        params.extend(
            self.address
                .iter()
                .map(|v| (v as &(dyn tokio_postgres::types::ToSql + Sync), Type::TEXT)),
        );
        params.push((&self.draft.archived, Type::BOOL));
        params
    }
}

/// `col = $n, …` for every written column, numbered from `first`.
fn assignments(first: usize) -> String {
    let address = address_columns("address_");
    FIELDS
        .iter()
        .copied()
        .chain(address.split(", "))
        .chain(["archived"])
        .zip(first..)
        .map(|(column, n)| match column {
            "contacts" => format!("contacts = ${n}::text::jsonb"),
            _ => format!("{column} = ${n}"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

impl<C: PgConnection> SupplierRepo for PgStore<C> {
    async fn get(&self, id: SupplierId) -> Result<Option<Supplier>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                &format!("SELECT {} FROM suppliers WHERE id = $1", columns()),
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(supplier_from_row).transpose()
    }

    async fn list(
        &self,
        filter: &SupplierFilter,
        page: PageRequest<SupplierId>,
    ) -> Result<Page<Supplier>, RepoError> {
        let q = filter.q.as_deref().map(str::trim);
        let after = page.after.map(|id| id.as_uuid());
        let fetch = i64::from(page.limit.get()) + 1;
        let mut conn = self.conn().await;
        let found = conn
            .db()
            .query_typed(
                &format!(
                    "SELECT {} FROM suppliers
                     WHERE ($1::boolean IS NULL OR archived = $1)
                       AND ($2::text IS NULL OR strpos(lower(name), lower($2)) > 0)
                       AND ($3::uuid IS NULL OR id < $3)
                     ORDER BY id DESC LIMIT $4",
                    columns()
                ),
                &[
                    (&filter.archived, Type::BOOL),
                    (&q, Type::TEXT),
                    (&after, Type::UUID),
                    (&fetch, Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(supplier_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page::from_overfetch(found, page.limit))
    }

    async fn insert(&self, supplier: &mut Supplier) -> Result<(), RepoError> {
        stamp_insert(supplier, self.now());
        let s = supplier.snapshot();
        let events = supplier.take_events();
        let stamp = self.event_stamp();
        let version = Version::NEW.next();
        let values = Values::of(&s.draft)?;
        let id = s.id.as_uuid();
        in_transaction!(self, |tx| {
            let mut params: Vec<Param<'_>> = vec![(&id, Type::UUID)];
            params.extend(values.params());
            let written: Vec<String> = FIELDS
                .iter()
                .map(|f| f.to_string())
                .chain(address_columns("address_").split(", ").map(str::to_string))
                .chain(["archived".to_string()])
                .collect();
            let placeholders: Vec<String> = (2..2 + written.len())
                .map(|n| match n {
                    3 => format!("${n}::text::jsonb"),
                    _ => format!("${n}"),
                })
                .collect();
            tx.query_typed(
                &format!(
                    "INSERT INTO suppliers (id, version, {}) VALUES ($1, 1, {})",
                    written.join(", "),
                    placeholders.join(", ")
                ),
                &params,
            )
            .await
            .map_err(repo_error)?;
            write_stamps(tx, "suppliers", "id", &id, Type::UUID, s.stamps).await?;
            insert_events(tx, &events, Some(version), stamp).await
        })?;
        supplier.set_version(version);
        Ok(())
    }

    async fn update(&self, supplier: &mut Supplier) -> Result<(), RepoError> {
        stamp_update(supplier, self.now());
        let s = supplier.snapshot();
        let events = supplier.take_events();
        let stamp = self.event_stamp();
        let values = Values::of(&s.draft)?;
        let id = s.id.as_uuid();
        let old = version_to_db(s.version)?;
        let version = in_transaction!(self, |tx| {
            let mut params: Vec<Param<'_>> = vec![(&id, Type::UUID), (&old, Type::INT8)];
            params.extend(values.params());
            let row = tx
                .query_typed_opt(
                    &format!(
                        "UPDATE suppliers SET version = version + 1, {}
                         WHERE id = $1 AND version = $2 RETURNING version",
                        assignments(3)
                    ),
                    &params,
                )
                .await
                .map_err(repo_error)?
                .ok_or(RepoError::StaleVersion)?;
            let version = version_from_db(col(&row, "version")?)?;
            write_stamps(tx, "suppliers", "id", &id, Type::UUID, s.stamps).await?;
            insert_events(tx, &events, Some(version), stamp).await?;
            Ok(version)
        })?;
        supplier.set_version(version);
        Ok(())
    }

    async fn delete(&self, supplier: &mut Supplier) -> Result<(), RepoError> {
        let id = supplier.id().as_uuid();
        let version = version_to_db(supplier.version())?;
        let events = supplier.take_events();
        let stamp = self.event_stamp();
        in_transaction!(self, |tx| {
            // Its catalogue goes with it; a purchase order's key into it refuses.
            let deleted = tx
                .query_typed_opt(
                    "DELETE FROM suppliers WHERE id = $1 AND version = $2 RETURNING id",
                    &[(&id, Type::UUID), (&version, Type::INT8)],
                )
                .await
                .map_err(repo_error)?;
            if deleted.is_none() {
                return Err(RepoError::StaleVersion);
            }
            insert_events(tx, &events, None, stamp).await
        })
    }
}

const ROW_COLUMNS: &str = "supplier_id, variant_id, supplier_sku, unit_cost, unit_cost_currency, \
                           min_order_quantity, lead_time_days";

fn row_from_db(row: &Row) -> Result<SupplierVariant, RepoError> {
    let supplier = SupplierId::from_uuid(col(row, "supplier_id")?);
    let variant = VariantId::from_uuid(col(row, "variant_id")?);
    let currency: Option<String> = col(row, "unit_cost_currency")?;
    SupplierVariant::restore(
        supplier,
        variant,
        SupplierVariantDraft {
            supplier_sku: col(row, "supplier_sku")?,
            unit_cost: unit_cost_from_db(col(row, "unit_cost")?, currency.as_deref())?,
            min_order_quantity: days_from_db(col(row, "min_order_quantity")?)?,
            lead_time_days: days_from_db(col(row, "lead_time_days")?)?,
        },
    )
    .map_err(|e| corrupt(format!("supplier {supplier} variant {variant}: {e}")))
}

impl<C: PgConnection> SupplierVariantRepo for PgStore<C> {
    async fn put(&self, row: &SupplierVariant) -> Result<(), RepoError> {
        let (cost, currency) = unit_cost_to_db(row.unit_cost());
        let moq = days_to_db(row.min_order_quantity())?;
        let lead = days_to_db(row.lead_time_days())?;
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                &format!(
                    "INSERT INTO supplier_variants ({ROW_COLUMNS})
                     VALUES ($1, $2, $3, $4, $5, $6, $7)
                     ON CONFLICT (supplier_id, variant_id) DO UPDATE SET
                         supplier_sku = $3, unit_cost = $4, unit_cost_currency = $5,
                         min_order_quantity = $6, lead_time_days = $7"
                ),
                &[
                    (&row.supplier().as_uuid(), Type::UUID),
                    (&row.variant().as_uuid(), Type::UUID),
                    (&row.supplier_sku(), Type::TEXT),
                    (&cost, Type::INT8),
                    (&currency, Type::TEXT),
                    (&moq, Type::INT4),
                    (&lead, Type::INT4),
                ],
            )
            .await
            .map_err(repo_error)?;
        Ok(())
    }

    async fn get(
        &self,
        supplier: SupplierId,
        variant: VariantId,
    ) -> Result<Option<SupplierVariant>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed_opt(
                &format!(
                    "SELECT {ROW_COLUMNS} FROM supplier_variants
                     WHERE supplier_id = $1 AND variant_id = $2"
                ),
                &[
                    (&supplier.as_uuid(), Type::UUID),
                    (&variant.as_uuid(), Type::UUID),
                ],
            )
            .await
            .map_err(repo_error)?
            .as_ref()
            .map(row_from_db)
            .transpose()
    }

    async fn remove(&self, supplier: SupplierId, variant: VariantId) -> Result<bool, RepoError> {
        let mut conn = self.conn().await;
        let removed = conn
            .db()
            .execute_typed(
                "DELETE FROM supplier_variants WHERE supplier_id = $1 AND variant_id = $2",
                &[
                    (&supplier.as_uuid(), Type::UUID),
                    (&variant.as_uuid(), Type::UUID),
                ],
            )
            .await
            .map_err(repo_error)?;
        Ok(removed > 0)
    }

    async fn list(
        &self,
        supplier: SupplierId,
        page: PageRequest<VariantId>,
    ) -> Result<Page<SupplierVariant>, RepoError> {
        let after = page.after.map(|id| id.as_uuid());
        let fetch = i64::from(page.limit.get()) + 1;
        let mut conn = self.conn().await;
        let found = conn
            .db()
            .query_typed(
                &format!(
                    "SELECT {ROW_COLUMNS} FROM supplier_variants
                     WHERE supplier_id = $1 AND ($2::uuid IS NULL OR variant_id < $2)
                     ORDER BY variant_id DESC LIMIT $3"
                ),
                &[
                    (&supplier.as_uuid(), Type::UUID),
                    (&after, Type::UUID),
                    (&fetch, Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(row_from_db)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page::from_overfetch(found, page.limit))
    }

    async fn for_variant(&self, variant: VariantId) -> Result<Vec<SupplierVariant>, RepoError> {
        let mut conn = self.conn().await;
        conn.db()
            .query_typed(
                &format!(
                    "SELECT {ROW_COLUMNS} FROM supplier_variants
                     WHERE variant_id = $1 ORDER BY supplier_id"
                ),
                &[(&variant.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(row_from_db)
            .collect()
    }

    async fn has_costs(&self, supplier: SupplierId) -> Result<bool, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_one(
                "SELECT EXISTS (SELECT 1 FROM supplier_variants
                                WHERE supplier_id = $1 AND unit_cost IS NOT NULL) AS costs",
                &[(&supplier.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        col(&row, "costs")
    }

    async fn remove_variants(&self, variants: &[VariantId]) -> Result<(), RepoError> {
        if variants.is_empty() {
            return Ok(());
        }
        let ids: Vec<Uuid> = variants.iter().map(|v| v.as_uuid()).collect();
        let mut conn = self.conn().await;
        conn.db()
            .execute_typed(
                "DELETE FROM supplier_variants WHERE variant_id = ANY($1)",
                &[(&ids, Type::UUID_ARRAY)],
            )
            .await
            .map_err(repo_error)?;
        Ok(())
    }
}
