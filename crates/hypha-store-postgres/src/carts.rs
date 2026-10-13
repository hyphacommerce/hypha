use hypha_cart::{Cart, CartLineSnapshot, CartRepo, CartSnapshot};
use hypha_core::{
    CartId, CountryCode, CustomerId, Email, MarketId, ProductId, RepoError, VariantId, Version,
    Versioned,
};
use tokio_postgres::{GenericClient, types::Type};
use uuid::Uuid;

use crate::{
    columns::{version_from_db, version_to_db},
    error::{corrupt, repo_error},
    store::{PgConnection, PgStore, consistent_read},
};

impl<C: PgConnection> CartRepo for PgStore<C> {
    async fn get(&self, id: CartId) -> Result<Option<Cart>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn.client(), |tx| load_cart(tx, id).await)
    }

    async fn insert(&self, cart: &mut Cart) -> Result<(), RepoError> {
        let s = cart.snapshot();
        let mut conn = self.conn().await;
        let tx = conn.client().transaction().await.map_err(repo_error)?;
        tx.query_typed(
            "INSERT INTO carts (id, version, market_id, country, email, customer_id)
             VALUES ($1, 1, $2, $3, $4, $5)",
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&s.market.as_uuid(), Type::UUID),
                (&s.country.as_str(), Type::TEXT),
                (&s.email.as_ref().map(Email::as_str), Type::TEXT),
                (&s.customer.map(|c| c.as_uuid()), Type::UUID),
            ],
        )
        .await
        .map_err(repo_error)?;
        write_lines(&tx, s.id, &s.lines).await?;
        tx.commit().await.map_err(repo_error)?;
        cart.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, cart: &mut Cart) -> Result<(), RepoError> {
        let s = cart.snapshot();
        let mut conn = self.conn().await;
        let tx = conn.client().transaction().await.map_err(repo_error)?;
        // The version check comes first; the lines are only replaced if it passes.
        let bumped = tx
            .query_typed_opt(
                "UPDATE carts SET version = version + 1, market_id = $3, country = $4, email = $5,
                     customer_id = $6
                 WHERE id = $1 AND version = $2 RETURNING version",
                &[
                    (&s.id.as_uuid(), Type::UUID),
                    (&version_to_db(s.version)?, Type::INT8),
                    (&s.market.as_uuid(), Type::UUID),
                    (&s.country.as_str(), Type::TEXT),
                    (&s.email.as_ref().map(Email::as_str), Type::TEXT),
                    (&s.customer.map(|c| c.as_uuid()), Type::UUID),
                ],
            )
            .await
            .map_err(repo_error)?;
        let Some(row) = bumped else {
            tx.rollback().await.map_err(repo_error)?;
            return Err(RepoError::StaleVersion);
        };
        let new_version = version_from_db(row.get(0))?;
        tx.query_typed(
            "DELETE FROM cart_lines WHERE cart_id = $1",
            &[(&s.id.as_uuid(), Type::UUID)],
        )
        .await
        .map_err(repo_error)?;
        write_lines(&tx, s.id, &s.lines).await?;
        tx.commit().await.map_err(repo_error)?;
        cart.set_version(new_version);
        Ok(())
    }
}

/// All lines in one statement; `WITH ORDINALITY` keeps their order.
async fn write_lines(
    tx: &impl GenericClient,
    cart: CartId,
    lines: &[CartLineSnapshot],
) -> Result<(), RepoError> {
    let products: Vec<Uuid> = lines.iter().map(|l| l.product.as_uuid()).collect();
    let variants: Vec<Uuid> = lines.iter().map(|l| l.variant.as_uuid()).collect();
    let quantities: Vec<i64> = lines.iter().map(|l| i64::from(l.quantity)).collect();
    tx.query_typed(
        "INSERT INTO cart_lines (cart_id, position, product_id, variant_id, quantity)
         SELECT $1, (n - 1)::integer, p, v, q
         FROM unnest($2::uuid[], $3::uuid[], $4::bigint[]) WITH ORDINALITY AS t (p, v, q, n)",
        &[
            (&cart.as_uuid(), Type::UUID),
            (&products, Type::UUID_ARRAY),
            (&variants, Type::UUID_ARRAY),
            (&quantities, Type::INT8_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// A cart and its lines, in 2 queries (run inside `consistent_read`).
async fn load_cart(client: &impl GenericClient, id: CartId) -> Result<Option<Cart>, RepoError> {
    let Some(row) = client
        .query_typed_opt(
            "SELECT version, market_id, country, email, customer_id FROM carts WHERE id = $1",
            &[(&id.as_uuid(), Type::UUID)],
        )
        .await
        .map_err(repo_error)?
    else {
        return Ok(None);
    };
    let lines = client
        .query_typed(
            "SELECT product_id, variant_id, quantity FROM cart_lines
             WHERE cart_id = $1 ORDER BY position",
            &[(&id.as_uuid(), Type::UUID)],
        )
        .await
        .map_err(repo_error)?
        .iter()
        .map(|line| {
            let quantity: i64 = line.get(2);
            Ok(CartLineSnapshot {
                product: ProductId::from_uuid(line.get(0)),
                variant: VariantId::from_uuid(line.get(1)),
                quantity: u32::try_from(quantity)
                    .map_err(|_| corrupt(format!("cart quantity {quantity}")))?,
            })
        })
        .collect::<Result<Vec<_>, RepoError>>()?;
    let country: String = row.get(2);
    let email: Option<String> = row.get(3);
    let snapshot = CartSnapshot {
        id,
        version: version_from_db(row.get(0))?,
        market: MarketId::from_uuid(row.get(1)),
        country: CountryCode::new(&country).map_err(corrupt)?,
        lines,
        email: email
            .as_deref()
            .map(Email::new)
            .transpose()
            .map_err(corrupt)?,
        customer: row.get::<_, Option<Uuid>>(4).map(CustomerId::from_uuid),
    };
    Cart::restore(snapshot)
        .map(Some)
        .map_err(|e| corrupt(format!("cart {id}: {e}")))
}
