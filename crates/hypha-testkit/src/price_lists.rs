use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, MutexGuard},
};

use hypha_categories::Category;
use hypha_core::{
    CategoryId, CustomerGroupId, Keyset, Page, PageRequest, PriceListId, ProductId, Recorded,
    RepoError, Timestamp, VariantId, Versioned, stamp_insert, stamp_update,
};
use hypha_customers::CustomerGroup;
use hypha_markets::{
    FixedPrice, FixedPriceFilter, PriceList, PriceListFilter, PriceListRepo, PriceListSort,
    repo::keys,
};
use hypha_products::Product;

use crate::{
    InMemoryCatalogRepo, InMemoryCategoryRepo, InMemoryOutbox, InMemoryProductRepo,
    catalogs::PriceListRefs,
    categories::{Members, products_in_or_below},
    table::{Table, missing, page_of},
};

#[derive(Default)]
pub struct InMemoryPriceListRepo {
    price_lists: Table<PriceListId, PriceList>,
    /// Fixed prices, stored apart from the list like a separate table would be.
    fixed_prices: Mutex<HashMap<PriceListId, HashMap<VariantId, FixedPrice>>>,
    /// The schedule job's progress, a one-row table.
    schedule: Mutex<Option<Timestamp>>,
    pub(crate) outbox: InMemoryOutbox,
    /// What a database would join: the catalogs' price lists (which refuse a delete), the
    /// products and categories fixed prices are filtered by, and the categories' tree.
    /// Empty unless shared (`InMemoryPorts` shares its repos').
    catalog_refs: Arc<Mutex<PriceListRefs>>,
    products: Arc<Table<ProductId, Product>>,
    categories: Arc<Table<CategoryId, Category>>,
    members: Arc<Mutex<Members>>,
    /// The customer groups audiences name; `None` (not shared) checks nothing.
    groups: Option<Arc<Table<CustomerGroupId, CustomerGroup>>>,
}

impl InMemoryPriceListRepo {
    /// A repo writing its events to `outbox` (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }

    /// The same repo, reading the catalogs' price lists, the products and the categories
    /// from these repos, as a database joins them.
    pub fn joining(
        self,
        catalogs: &InMemoryCatalogRepo,
        products: &InMemoryProductRepo,
        categories: &InMemoryCategoryRepo,
    ) -> Self {
        Self {
            catalog_refs: catalogs.price_list_refs.clone(),
            products: products.products.clone(),
            categories: categories.categories.clone(),
            members: categories.members.clone(),
            ..self
        }
    }

    /// The same repo, refusing an audience that names a group `groups` doesn't have, as a
    /// database's reference does.
    pub fn joining_groups(self, groups: &crate::InMemoryCustomerGroupRepo) -> Self {
        Self {
            groups: Some(groups.groups.clone()),
            ..self
        }
    }

    /// Whether some list's audience names `group`: a group in use can't be deleted.
    pub(crate) fn names_group(&self, group: CustomerGroupId) -> bool {
        self.price_lists
            .find(|l| l.audience().groups().contains(&group))
            .is_some()
    }

    /// `Reference(PRICE_LIST_GROUP)` when `list`'s audience names a group that isn't
    /// stored (when groups are shared).
    fn check_groups(&self, list: &PriceList) -> Result<(), RepoError> {
        let Some(groups) = &self.groups else {
            return Ok(());
        };
        if list
            .audience()
            .groups()
            .iter()
            .any(|g| groups.get(g).is_none())
        {
            return Err(RepoError::Reference(keys::PRICE_LIST_GROUP));
        }
        Ok(())
    }

    /// The events this repo wrote (into its outbox, shared or not).
    pub fn outbox(&self) -> &InMemoryOutbox {
        &self.outbox
    }

    /// Fixed prices, locked, for a batch (`InMemoryPorts::commit`).
    pub(crate) fn lock_fixed_prices(&self) -> MutexGuard<'_, FixedPrices> {
        self.fixed_prices.lock().unwrap()
    }

    /// The schedule job's progress, locked, for a batch.
    pub(crate) fn lock_schedule(&self) -> MutexGuard<'_, Option<Timestamp>> {
        self.schedule.lock().unwrap()
    }

    /// The next `n` lookups by a unique key (`get_by_name`, which name checks read) find
    /// nothing, as if another request inserted the row right after: for testing a command
    /// that loses the race between its pre-check and its write.
    pub fn hide_next_lookups(&self, n: usize) {
        self.price_lists.hide_next_lookups(n);
    }

    pub(crate) fn exists(&self, id: PriceListId) -> bool {
        self.price_lists.get(&id).is_some()
    }

    /// Whether the fixed price of `variant` passes `filter`, judged by the product owning
    /// the variant, as a database's join would.
    fn passes(&self, filter: &FixedPriceFilter, variant: VariantId) -> bool {
        if filter.is_empty() {
            return true;
        }
        let Some(product) = self.products.find(|p| p.variant(variant).is_some()) else {
            return false;
        };
        let in_category = filter.category().is_some_and(|c| {
            products_in_or_below(&self.members, &self.categories, c).contains(&product.id())
        });
        let sku = product.variant(variant).and_then(|v| v.sku());
        let texts = [product.title(), product.handle().as_str()]
            .into_iter()
            .chain(sku);
        filter.matches(product.id(), in_category, texts)
    }
}

pub(crate) type FixedPrices = HashMap<PriceListId, HashMap<VariantId, FixedPrice>>;

/// A name belongs to one list, ignoring case.
fn unique(new: &PriceList, other: &PriceList) -> Option<&'static str> {
    (new.name().to_lowercase() == other.name().to_lowercase()).then_some(keys::PRICE_LIST_NAME)
}

impl PriceListRepo for InMemoryPriceListRepo {
    async fn get(&self, id: PriceListId) -> Result<Option<PriceList>, RepoError> {
        Ok(self.price_lists.get(&id))
    }

    async fn get_many(&self, ids: &[PriceListId]) -> Result<Vec<PriceList>, RepoError> {
        let mut found = self.price_lists.get_many(ids);
        found.sort_by_key(|l| l.id());
        found.dedup_by_key(|l| l.id());
        Ok(found)
    }

    async fn list_all(&self) -> Result<Vec<PriceList>, RepoError> {
        Ok(self.price_lists.all_oldest_first())
    }

    async fn get_by_name(&self, name: &str) -> Result<Option<PriceList>, RepoError> {
        let name = name.to_lowercase();
        Ok(self.price_lists.find(|l| l.name().to_lowercase() == name))
    }

    async fn list(
        &self,
        filter: &PriceListFilter,
        sort: PriceListSort,
        page: PageRequest<Keyset<PriceListId>>,
    ) -> Result<Page<PriceList>, RepoError> {
        if let Some(after) = &page.after
            && !sort.fits(&after.key)
        {
            return Err(RepoError::Backend(format!(
                "a {:?} position for a {sort:?} list",
                after.key
            )));
        }
        Ok(self.price_lists.page_sorted(
            |l| filter.matches(l),
            &page,
            sort.direction(),
            PriceList::clone,
            |l| sort.key(l),
        ))
    }

    async fn fixed_price_page(
        &self,
        list: PriceListId,
        filter: &FixedPriceFilter,
        page: PageRequest<VariantId>,
    ) -> Result<Page<(VariantId, FixedPrice)>, RepoError> {
        // Copied first, so no other lock is held while the products' is.
        let prices: Vec<(VariantId, FixedPrice)> = {
            let all = self.fixed_prices.lock().unwrap();
            all.get(&list)
                .into_iter()
                .flatten()
                .map(|(v, p)| (*v, p.clone()))
                .collect()
        };
        let kept = prices.into_iter().filter(|(v, _)| self.passes(filter, *v));
        Ok(page_of(kept, page))
    }

    async fn fixed_price_counts(
        &self,
        lists: &[PriceListId],
    ) -> Result<HashMap<PriceListId, u64>, RepoError> {
        let all = self.fixed_prices.lock().unwrap();
        Ok(lists
            .iter()
            .filter_map(|l| {
                all.get(l)
                    .filter(|prices| !prices.is_empty())
                    .map(|prices| (*l, prices.len() as u64))
            })
            .collect())
    }

    async fn insert(&self, price_list: &mut PriceList) -> Result<(), RepoError> {
        self.check_groups(price_list)?;
        stamp_insert(price_list, self.outbox.now());
        let events = price_list.take_events();
        self.price_lists
            .insert(price_list.id(), price_list, keys::PRICE_LIST_ID, &unique)?;
        self.outbox.write(events, Some(price_list.version()));
        Ok(())
    }

    async fn update(&self, price_list: &mut PriceList) -> Result<(), RepoError> {
        self.check_groups(price_list)?;
        stamp_update(price_list, self.outbox.now());
        let events = price_list.take_events();
        self.price_lists
            .update(price_list.id(), price_list, &unique)?;
        self.outbox.write(events, Some(price_list.version()));
        Ok(())
    }

    async fn delete(&self, price_list: &mut PriceList) -> Result<(), RepoError> {
        if self
            .price_lists
            .get(&price_list.id())
            .is_none_or(|stored| stored.version() != price_list.version())
        {
            return Err(RepoError::StaleVersion);
        }
        let held = self
            .catalog_refs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .any(|lists| lists.contains(&price_list.id()));
        if held {
            return Err(RepoError::Reference(keys::CATALOG_PRICE_LIST));
        }
        let events = price_list.take_events();
        self.price_lists.remove(&price_list.id());
        self.fixed_prices.lock().unwrap().remove(&price_list.id());
        self.outbox.write(events, None);
        Ok(())
    }

    async fn set_fixed_prices(
        &self,
        list: PriceListId,
        prices: &[(VariantId, FixedPrice)],
    ) -> Result<(), RepoError> {
        if self.price_lists.get(&list).is_none() {
            return Err(missing("price list", list));
        }
        let mut seen = std::collections::HashSet::new();
        if let Some((variant, _)) = prices.iter().find(|(v, _)| !seen.insert(*v)) {
            // Like a database statement that would update the same row twice.
            return Err(RepoError::Backend(format!(
                "variant {variant} is listed twice; callers check_fixed_prices first"
            )));
        }
        let mut all = self.fixed_prices.lock().unwrap();
        all.entry(list).or_default().extend(prices.iter().cloned());
        Ok(())
    }

    async fn remove_fixed_prices(
        &self,
        list: PriceListId,
        variants: &[VariantId],
    ) -> Result<(), RepoError> {
        let mut all = self.fixed_prices.lock().unwrap();
        if let Some(prices) = all.get_mut(&list) {
            for variant in variants {
                prices.remove(variant);
            }
        }
        Ok(())
    }

    async fn lists_pricing(&self, variant: VariantId) -> Result<Vec<PriceListId>, RepoError> {
        let all = self.fixed_prices.lock().unwrap();
        let mut lists: Vec<PriceListId> = all
            .iter()
            .filter(|(_, prices)| prices.contains_key(&variant))
            .map(|(list, _)| *list)
            .collect();
        lists.sort();
        Ok(lists)
    }

    async fn priced_variants(
        &self,
        list: PriceListId,
        limit: u32,
    ) -> Result<Vec<VariantId>, RepoError> {
        let all = self.fixed_prices.lock().unwrap();
        let mut variants: Vec<VariantId> = all
            .get(&list)
            .into_iter()
            .flatten()
            .map(|(v, _)| *v)
            .collect();
        variants.sort();
        variants.truncate(limit as usize);
        Ok(variants)
    }

    async fn fixed_prices(
        &self,
        list: PriceListId,
        variants: &[VariantId],
    ) -> Result<HashMap<VariantId, FixedPrice>, RepoError> {
        let all = self.fixed_prices.lock().unwrap();
        let Some(prices) = all.get(&list) else {
            return Ok(HashMap::new());
        };
        Ok(variants
            .iter()
            .filter_map(|v| prices.get(v).map(|p| (*v, p.clone())))
            .collect())
    }

    async fn fixed_prices_in(
        &self,
        lists: &[PriceListId],
        variants: &[VariantId],
    ) -> Result<HashMap<PriceListId, HashMap<VariantId, FixedPrice>>, RepoError> {
        let mut found = HashMap::new();
        for list in lists {
            let prices = self.fixed_prices(*list, variants).await?;
            if !prices.is_empty() {
                found.insert(*list, prices);
            }
        }
        Ok(found)
    }

    async fn schedule_ran_until(&self) -> Result<Option<Timestamp>, RepoError> {
        Ok(*self.schedule.lock().unwrap())
    }

    async fn product_categories(
        &self,
        products: &[ProductId],
    ) -> Result<HashMap<ProductId, HashSet<CategoryId>>, RepoError> {
        let direct: Vec<(CategoryId, ProductId)> = {
            let members = self.members.lock().unwrap_or_else(|e| e.into_inner());
            members
                .iter()
                .flat_map(|(c, ps)| {
                    products
                        .iter()
                        .filter(|p| ps.contains(p))
                        .map(move |p| (*c, *p))
                })
                .collect()
        };
        let mut found: HashMap<ProductId, HashSet<CategoryId>> = HashMap::new();
        for (category, product) in direct {
            let set = found.entry(product).or_default();
            let mut next = Some(category);
            // Up the parent pointers; a cycle (which the domain refuses) stops it.
            while let Some(c) = next.filter(|c| set.insert(*c)) {
                next = self.categories.get(&c).and_then(|cat| cat.parent());
            }
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::{Currency, Money, Price};
    use hypha_markets::PriceListDraft;
    use pollster::block_on;
    use uuid::Uuid;

    use super::*;

    fn list_id() -> PriceListId {
        PriceListId::from_uuid(Uuid::from_u128(1))
    }

    fn vid(n: u128) -> VariantId {
        VariantId::from_uuid(Uuid::from_u128(n))
    }

    fn eur(amount: i64) -> FixedPrice {
        FixedPrice::plain(Price::new(Money::new(amount, Currency::EUR).unwrap()).unwrap())
    }

    fn new_list() -> PriceList {
        PriceList::new(list_id(), PriceListDraft::new("Euro", Currency::EUR)).unwrap()
    }

    /// A repo holding the price list `list_id()`.
    fn with_list() -> InMemoryPriceListRepo {
        let repo = InMemoryPriceListRepo::default();
        let mut list = new_list();
        block_on(repo.insert(&mut list)).unwrap();
        repo
    }

    #[test]
    fn saves_and_loads_price_lists() {
        let repo = InMemoryPriceListRepo::default();
        let mut list = new_list();
        block_on(repo.insert(&mut list)).unwrap();
        assert_eq!(block_on(repo.get(list.id())), Ok(Some(list)));
    }

    #[test]
    fn get_unknown_is_none() {
        let repo = InMemoryPriceListRepo::default();
        let id = PriceListId::from_uuid(Uuid::from_u128(99));
        assert_eq!(block_on(repo.get(id)), Ok(None));
    }

    #[test]
    fn fixed_prices_returns_only_requested_variants_that_have_one() {
        let repo = with_list();
        block_on(repo.set_fixed_prices(list_id(), &[(vid(1), eur(100)), (vid(2), eur(200))]))
            .unwrap();

        let found = block_on(repo.fixed_prices(list_id(), &[vid(2), vid(9)])).unwrap();

        assert_eq!(found, HashMap::from([(vid(2), eur(200))]));
    }

    #[test]
    fn set_replaces_and_remove_deletes() {
        let repo = with_list();
        block_on(repo.set_fixed_prices(list_id(), &[(vid(1), eur(100)), (vid(2), eur(200))]))
            .unwrap();
        block_on(repo.set_fixed_prices(list_id(), &[(vid(1), eur(150))])).unwrap();
        block_on(repo.remove_fixed_prices(list_id(), &[vid(2), vid(9)])).unwrap();

        let found = block_on(repo.fixed_prices(list_id(), &[vid(1), vid(2)])).unwrap();

        assert_eq!(found, HashMap::from([(vid(1), eur(150))]));
    }
}
