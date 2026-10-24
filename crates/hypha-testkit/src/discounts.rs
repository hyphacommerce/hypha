use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{Mutex, MutexGuard},
};

use hypha_core::{
    CustomerId, DiscountId, OrderId, Page, PageRequest, Recorded, RepoError, Timestamp, Versioned,
    stamp_insert, stamp_update,
};
use hypha_discounts::{
    CodeFilter, CodeSelection, Discount, DiscountCode, DiscountFilter, DiscountRepo, Method, Usage,
    UsageClaim, keys,
};

use crate::{
    InMemoryOutbox,
    table::{Table, missing, no_unique_keys, page_of},
};

/// What a database keeps beside the discounts: codes, counters and usages.
#[derive(Debug, Clone, Default)]
pub(crate) struct DiscountRows {
    /// By normalised code.
    pub(crate) codes: BTreeMap<String, DiscountCode>,
    /// Each discount's uses, from zero when it's created.
    pub(crate) counts: HashMap<DiscountId, u64>,
    /// With whether the use was once per customer.
    pub(crate) usages: Vec<(Usage, bool)>,
}

impl DiscountRows {
    /// A batch's claim: both counters move only under their limits, and a
    /// once-per-customer use is unique per customer.
    pub(crate) fn claim(&mut self, claim: &UsageClaim) -> Result<(), RepoError> {
        let limit_hit = RepoError::Duplicate(keys::DISCOUNT_USAGE_LIMIT);
        let used = *self.counts.get(&claim.discount).ok_or(limit_hit.clone())?;
        if claim.limit.is_some_and(|max| used >= u64::from(max)) {
            return Err(limit_hit);
        }
        if let Some((code, limit)) = &claim.code {
            let row = self.codes.get(code).ok_or(limit_hit.clone())?;
            if row.discount != claim.discount || limit.is_some_and(|max| row.used >= u64::from(max))
            {
                return Err(limit_hit);
            }
        }
        if claim.once_per_customer
            && self.usages.iter().any(|(u, once)| {
                *once && u.discount == claim.discount && u.customer == claim.customer
            })
        {
            return Err(RepoError::Duplicate(keys::DISCOUNT_PER_CUSTOMER));
        }
        *self.counts.entry(claim.discount).or_default() += 1;
        if let Some((code, _)) = &claim.code
            && let Some(row) = self.codes.get_mut(code)
        {
            row.used += 1;
        }
        self.usages.push((
            Usage {
                discount: claim.discount,
                order: claim.order,
                customer: claim.customer,
                code: claim.code.as_ref().map(|(c, _)| c.clone()),
                amount: claim.amount,
                at: claim.at,
            },
            claim.once_per_customer,
        ));
        Ok(())
    }

    /// Adds codes, all or none, refusing a taken one.
    pub(crate) fn add_codes(
        &mut self,
        codes: &[DiscountCode],
        now: Timestamp,
    ) -> Result<(), RepoError> {
        let mut seen = HashSet::new();
        for code in codes {
            if self.codes.contains_key(&code.normalized) || !seen.insert(&code.normalized) {
                return Err(RepoError::Duplicate(keys::DISCOUNT_CODE));
            }
        }
        for code in codes {
            let mut stored = code.clone();
            stored.used = 0;
            stored.created_at = Some(now);
            self.codes.insert(code.normalized.clone(), stored);
        }
        Ok(())
    }

    pub(crate) fn remove_codes(&mut self, discount: DiscountId, selection: &CodeSelection) {
        self.codes.retain(|normalized, code| {
            code.discount != discount
                || match selection {
                    CodeSelection::Codes(listed) => !listed.contains(normalized),
                    CodeSelection::Batch(batch) => code.batch.as_deref() != Some(batch.as_str()),
                }
        });
    }

    pub(crate) fn forget(&mut self, discount: DiscountId) {
        self.codes.retain(|_, c| c.discount != discount);
        self.counts.remove(&discount);
        self.usages.retain(|(u, _)| u.discount != discount);
    }
}

/// Discounts, their codes and their usage, in memory.
#[derive(Default)]
pub struct InMemoryDiscounts {
    pub(crate) discounts: Table<DiscountId, Discount>,
    rows: Mutex<DiscountRows>,
    outbox: InMemoryOutbox,
    /// How many of the next `usage_counts` reads see no uses (`hide_next_usage_counts`).
    hidden_counts: std::sync::atomic::AtomicUsize,
}

impl InMemoryDiscounts {
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }

    /// Makes the next `n` reads of usage counts see none, as if another checkout used
    /// the discount right after: to lose the race for the last use, which the batch's
    /// claim must still catch.
    pub fn hide_next_usage_counts(&self, n: usize) {
        self.hidden_counts
            .store(n, std::sync::atomic::Ordering::SeqCst);
    }

    pub(crate) fn lock_rows(&self) -> MutexGuard<'_, DiscountRows> {
        self.rows.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn matches(d: &Discount, filter: &DiscountFilter, rows: &DiscountRows) -> bool {
    let codes = || rows.codes.values().filter(|c| c.discount == d.id());
    filter.phase.is_none_or(|p| d.phase(filter.at) == p)
        && filter.method.is_none_or(|m| d.method() == m)
        && filter.class.is_none_or(|c| d.classes().contains(c))
        && filter
            .code
            .as_ref()
            .is_none_or(|code| codes().any(|c| &c.normalized == code))
        && filter.q.as_ref().is_none_or(|q| {
            d.title().to_lowercase().contains(&q.to_lowercase())
                || codes().any(|c| c.normalized.contains(&q.to_uppercase()))
        })
}

fn code_matches(code: &DiscountCode, filter: &CodeFilter) -> bool {
    filter
        .q
        .as_ref()
        .is_none_or(|q| code.normalized.contains(q.as_str()))
        && filter
            .batch
            .as_ref()
            .is_none_or(|b| code.batch.as_deref() == Some(b.as_str()))
}

impl DiscountRepo for InMemoryDiscounts {
    async fn get(&self, id: DiscountId) -> Result<Option<Discount>, RepoError> {
        Ok(self.discounts.get(&id))
    }

    async fn get_many(&self, ids: &[DiscountId]) -> Result<Vec<Discount>, RepoError> {
        let mut found = self.discounts.get_many(ids);
        found.sort_by_key(Discount::id);
        found.dedup_by_key(|d| d.id());
        Ok(found)
    }

    async fn list(
        &self,
        filter: &DiscountFilter,
        page: PageRequest<DiscountId>,
    ) -> Result<Page<Discount>, RepoError> {
        let rows = self.lock_rows().clone();
        Ok(self
            .discounts
            .page(|d| matches(d, filter, &rows), page, Clone::clone))
    }

    async fn live_automatic(&self, at: Timestamp) -> Result<Vec<Discount>, RepoError> {
        Ok(self
            .discounts
            .all_oldest_first()
            .into_iter()
            .filter(|d| d.method() == Method::Automatic && d.is_live(at))
            .collect())
    }

    async fn count_live_automatic(&self, at: Timestamp) -> Result<u64, RepoError> {
        Ok(self
            .discounts
            .all()
            .iter()
            .filter(|d| d.counts_as_live_automatic(at))
            .count() as u64)
    }

    async fn insert(&self, discount: &mut Discount) -> Result<(), RepoError> {
        stamp_insert(discount, self.outbox.now());
        let events = discount.take_events();
        self.discounts
            .insert(discount.id(), discount, keys::DISCOUNT_ID, &no_unique_keys)?;
        self.lock_rows().counts.insert(discount.id(), 0);
        self.outbox.write(events, Some(discount.version()));
        Ok(())
    }

    async fn update(&self, discount: &mut Discount) -> Result<(), RepoError> {
        stamp_update(discount, self.outbox.now());
        let events = discount.take_events();
        self.discounts
            .update(discount.id(), discount, &no_unique_keys)?;
        self.outbox.write(events, Some(discount.version()));
        Ok(())
    }

    async fn delete(&self, discount: &mut Discount) -> Result<(), RepoError> {
        let mut rows = self.discounts.lock();
        match rows.get(&discount.id()) {
            Some(stored) if stored.version() == discount.version() => {}
            _ => return Err(RepoError::StaleVersion),
        }
        rows.remove(&discount.id());
        self.lock_rows().forget(discount.id());
        self.outbox.write(discount.take_events(), None);
        Ok(())
    }

    async fn find_codes(&self, normalized: &[String]) -> Result<Vec<DiscountCode>, RepoError> {
        let rows = self.lock_rows();
        Ok(normalized
            .iter()
            .collect::<HashSet<_>>()
            .into_iter()
            .filter_map(|c| rows.codes.get(c).cloned())
            .collect())
    }

    async fn code_page(
        &self,
        discount: DiscountId,
        filter: &CodeFilter,
        page: PageRequest<String>,
    ) -> Result<Page<DiscountCode>, RepoError> {
        let rows = self.lock_rows();
        let found: Vec<DiscountCode> = rows
            .codes
            .values()
            .filter(|c| c.discount == discount && code_matches(c, filter))
            .filter(|c| {
                page.after
                    .as_ref()
                    .is_none_or(|after| c.normalized > *after)
            })
            .take(page.limit.get() as usize + 1)
            .cloned()
            .collect();
        Ok(Page::from_overfetch(found, page.limit))
    }

    async fn count_codes(
        &self,
        discount: DiscountId,
        filter: &CodeFilter,
    ) -> Result<u64, RepoError> {
        Ok(self
            .lock_rows()
            .codes
            .values()
            .filter(|c| c.discount == discount && code_matches(c, filter))
            .count() as u64)
    }

    async fn code_counts(
        &self,
        discounts: &[DiscountId],
    ) -> Result<HashMap<DiscountId, u64>, RepoError> {
        let mut counts = HashMap::new();
        for code in self.lock_rows().codes.values() {
            if discounts.contains(&code.discount) {
                *counts.entry(code.discount).or_default() += 1;
            }
        }
        Ok(counts)
    }

    async fn add_codes(&self, codes: &[DiscountCode]) -> Result<(), RepoError> {
        if let Some(orphan) = codes
            .iter()
            .find(|c| self.discounts.get(&c.discount).is_none())
        {
            return Err(missing("discount", orphan.discount));
        }
        self.lock_rows().add_codes(codes, self.outbox.now())
    }

    async fn usage_counts(
        &self,
        discounts: &[DiscountId],
    ) -> Result<HashMap<DiscountId, u64>, RepoError> {
        use std::sync::atomic::Ordering;
        if self
            .hidden_counts
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            return Ok(HashMap::new());
        }
        Ok(self
            .lock_rows()
            .counts
            .iter()
            .filter(|(d, used)| discounts.contains(d) && **used > 0)
            .map(|(d, used)| (*d, *used))
            .collect())
    }

    async fn used_by(
        &self,
        discounts: &[DiscountId],
        customer: CustomerId,
    ) -> Result<HashSet<DiscountId>, RepoError> {
        Ok(self
            .lock_rows()
            .usages
            .iter()
            .filter(|(u, _)| u.customer == customer && discounts.contains(&u.discount))
            .map(|(u, _)| u.discount)
            .collect())
    }

    async fn usage_page(
        &self,
        discount: DiscountId,
        page: PageRequest<OrderId>,
    ) -> Result<Page<Usage>, RepoError> {
        let rows = self.lock_rows();
        let found = page_of(
            rows.usages
                .iter()
                .filter(|(u, _)| u.discount == discount)
                .map(|(u, _)| (u.order, u.clone())),
            page,
        );
        Ok(Page {
            items: found.items.into_iter().map(|(_, u)| u).collect(),
            has_more: found.has_more,
        })
    }
}
