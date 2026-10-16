//! Search in memory (design note 0019): an engine that runs the reference semantics,
//! the queue of marks, and the settings record.

use std::{collections::BTreeMap, sync::Mutex};

use hypha_core::{
    ChannelId, MarketId, ProductId, RepoError, Timestamp, stamp_insert, stamp_update,
};
use hypha_search_index::{
    IndexError, Mark, SearchDocument, SearchIndex, SearchQuery, SearchQueue, SearchResults,
    SearchSettings, SearchSettingsRepo, Target, keys, reference,
};

use crate::{
    InMemoryOutbox,
    table::{Table, no_unique_keys},
};

/// An engine over a list of documents, answering with `hypha_search_index::reference`.
#[derive(Default)]
pub struct InMemorySearchIndex {
    documents: Mutex<Vec<SearchDocument>>,
}

impl InMemorySearchIndex {
    /// Every document, in no particular order.
    pub fn documents(&self) -> Vec<SearchDocument> {
        self.lock().clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<SearchDocument>> {
        self.documents.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl SearchIndex for InMemorySearchIndex {
    async fn replace(
        &self,
        market: MarketId,
        products: &[ProductId],
        documents: &[SearchDocument],
    ) -> Result<(), IndexError> {
        let mut all = self.lock();
        all.retain(|d| !(d.market == market && products.contains(&d.product)));
        all.extend_from_slice(documents);
        Ok(())
    }

    async fn search(&self, query: &SearchQuery) -> Result<SearchResults, IndexError> {
        Ok(reference::search(&self.lock(), query))
    }

    async fn option_names(
        &self,
        market: MarketId,
        channel: Option<ChannelId>,
    ) -> Result<Vec<String>, IndexError> {
        Ok(reference::option_names(&self.lock(), market, channel))
    }

    async fn count(&self) -> Result<u64, IndexError> {
        Ok(self.lock().len() as u64)
    }
}

struct Queued {
    /// When it was first marked, in mark order: leases take the oldest first.
    seq: u64,
    generation: i64,
    leased_until: Option<Timestamp>,
}

#[derive(Default)]
struct QueueState {
    marks: BTreeMap<Target, Queued>,
    next_seq: u64,
}

#[derive(Default)]
pub struct InMemorySearchQueue {
    state: Mutex<QueueState>,
}

impl InMemorySearchQueue {
    /// The queued targets, oldest first.
    pub fn targets(&self) -> Vec<Target> {
        let state = self.lock();
        let mut marks: Vec<(&Target, &Queued)> = state.marks.iter().collect();
        marks.sort_by_key(|(_, q)| q.seq);
        marks.into_iter().map(|(t, _)| *t).collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, QueueState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl SearchQueue for InMemorySearchQueue {
    async fn mark(&self, targets: &[Target]) -> Result<(), RepoError> {
        let mut state = self.lock();
        for target in targets {
            let seq = state.next_seq;
            match state.marks.get_mut(target) {
                Some(queued) => queued.generation += 1,
                None => {
                    state.marks.insert(
                        *target,
                        Queued {
                            seq,
                            generation: 1,
                            leased_until: None,
                        },
                    );
                    state.next_seq += 1;
                }
            }
        }
        Ok(())
    }

    async fn lease(
        &self,
        now: Timestamp,
        until: Timestamp,
        limit: u32,
    ) -> Result<Vec<Mark>, RepoError> {
        let mut state = self.lock();
        let mut free: Vec<(Target, u64)> = state
            .marks
            .iter()
            .filter(|(_, q)| q.leased_until.is_none_or(|l| l <= now))
            .map(|(t, q)| (*t, q.seq))
            .collect();
        free.sort_by_key(|(_, seq)| *seq);
        free.truncate(limit as usize);
        Ok(free
            .into_iter()
            .filter_map(|(target, _)| {
                let queued = state.marks.get_mut(&target)?;
                queued.leased_until = Some(until);
                Some(Mark {
                    target,
                    generation: queued.generation,
                })
            })
            .collect())
    }

    async fn finish(&self, marks: &[Mark]) -> Result<Vec<Target>, RepoError> {
        let mut state = self.lock();
        let mut gone = Vec::new();
        for mark in marks {
            match state.marks.get_mut(&mark.target) {
                Some(queued) if queued.generation == mark.generation => {
                    state.marks.remove(&mark.target);
                }
                Some(queued) => queued.leased_until = None,
                None => gone.push(mark.target),
            }
        }
        Ok(gone)
    }

    async fn queued(&self) -> Result<u64, RepoError> {
        Ok(self.lock().marks.len() as u64)
    }
}

#[derive(Default)]
pub struct InMemorySearchSettingsRepo {
    settings: Table<(), SearchSettings>,
    pub(crate) outbox: InMemoryOutbox,
}

impl InMemorySearchSettingsRepo {
    /// A repo sharing `outbox`'s clock and events (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }
}

impl SearchSettingsRepo for InMemorySearchSettingsRepo {
    async fn get(&self) -> Result<Option<SearchSettings>, RepoError> {
        Ok(self.settings.get(&()))
    }

    async fn insert(&self, settings: &mut SearchSettings) -> Result<(), RepoError> {
        stamp_insert(settings, self.outbox.now());
        let events = hypha_core::Recorded::take_events(settings);
        self.settings
            .insert((), settings, keys::SETTINGS, &no_unique_keys)?;
        self.outbox
            .write(events, Some(hypha_core::Versioned::version(settings)));
        Ok(())
    }

    async fn update(&self, settings: &mut SearchSettings) -> Result<(), RepoError> {
        stamp_update(settings, self.outbox.now());
        let events = hypha_core::Recorded::take_events(settings);
        self.settings.update((), settings, &no_unique_keys)?;
        self.outbox
            .write(events, Some(hypha_core::Versioned::version(settings)));
        Ok(())
    }
}

/// Documents for engine tests (design note 0019): two markets; shirts with options
/// split by colour, a product without options, ties on price and creation time, a
/// title beyond ASCII and unavailable variants. Channel 1 sells products 1–3, channel 2
/// products 1 and 4 (design note 0020).
pub fn search_fixture() -> Vec<SearchDocument> {
    use hypha_core::{ChannelId, Currency, Money, VariantId};
    use hypha_search_index::PriceRange;
    use uuid::Uuid;

    let market = |n: u128| MarketId::from_uuid(Uuid::from_u128(n));
    let product = |n: u128| ProductId::from_uuid(Uuid::from_u128(n));
    let eur = |amount: i64| Money::new(amount, Currency::EUR).unwrap_or_else(|e| panic!("{e}"));
    struct V {
        n: u128,
        options: &'static [(&'static str, &'static str)],
        price: i64,
        available: bool,
        sku: Option<&'static str>,
    }
    struct P {
        n: u128,
        market: u128,
        title: &'static str,
        description: &'static str,
        vendor: Option<&'static str>,
        product_type: Option<&'static str>,
        tags: &'static [&'static str],
        created: i64,
        split: bool,
        variants: Vec<V>,
    }
    let v = |n, options, price, available, sku| V {
        n,
        options,
        price,
        available,
        sku,
    };
    let products = vec![
        P {
            n: 1,
            market: 1,
            title: "Linen Shirt",
            description: "Breathable linen.",
            vendor: Some("Acme"),
            product_type: Some("Shirt"),
            tags: &["Summer", "Linen"],
            created: 100,
            split: true,
            variants: vec![
                v(
                    11,
                    &[("Color", "Red"), ("Size", "S")],
                    3000,
                    true,
                    Some("LS-RED-S"),
                ),
                v(
                    12,
                    &[("Color", "Red"), ("Size", "M")],
                    3200,
                    false,
                    Some("LS-RED-M"),
                ),
                v(
                    13,
                    &[("Color", "Blue"), ("Size", "S")],
                    2800,
                    true,
                    Some("LS-BLU-S"),
                ),
            ],
        },
        P {
            n: 2,
            market: 1,
            title: "Cotton Shirt",
            description: "Soft cotton, not linen.",
            vendor: Some("Basics"),
            product_type: Some("Shirt"),
            tags: &["Summer"],
            created: 200,
            split: false,
            variants: vec![
                v(21, &[("color", "Red"), ("size", "L")], 1500, true, None),
                v(22, &[("color", "Green"), ("size", "L")], 1500, true, None),
            ],
        },
        P {
            n: 3,
            market: 1,
            title: "Čaša Mug",
            description: "Holds tea.",
            vendor: Some("Acme"),
            product_type: Some("Mug"),
            tags: &["Kitchen"],
            created: 300,
            split: false,
            variants: vec![v(31, &[], 900, true, Some("MUG-01"))],
        },
        P {
            n: 4,
            market: 1,
            title: "Linen Towel",
            // Words a database's own parser might split or drop: Unicode digits and
            // marks, and a run longer than a term may be (`MAX_TERM_CHARS`).
            description: "Plush, naïve x² cotton. zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
            vendor: None,
            product_type: None,
            tags: &[],
            created: 300,
            split: false,
            variants: vec![v(41, &[], 1200, false, None)],
        },
        P {
            n: 3,
            market: 2,
            title: "Čaša Mug",
            description: "Holds tea.",
            vendor: Some("Acme"),
            product_type: Some("Mug"),
            tags: &["Kitchen"],
            created: 300,
            split: false,
            variants: vec![v(31, &[], 1100, true, Some("MUG-01"))],
        },
    ];

    let mut documents = Vec::new();
    for p in products {
        let id = product(p.n);
        let channel = |n: u128| ChannelId::from_uuid(Uuid::from_u128(n));
        let channels = match p.n {
            1 => vec![channel(1), channel(2)],
            4 => vec![channel(2)],
            _ => vec![channel(1)],
        };
        let prices: Vec<i64> = p.variants.iter().map(|v| v.price).collect();
        let range = |prices: &[i64]| PriceRange {
            min: eur(*prices.iter().min().unwrap_or(&0)),
            max: eur(*prices.iter().max().unwrap_or(&0)),
        };
        let listing = |v: &V| {
            let color = v
                .options
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case("color"));
            match color {
                Some((_, value)) if p.split => format!("{id}|color={value}"),
                _ => id.to_string(),
            }
        };
        for (position, variant) in p.variants.iter().enumerate() {
            let same_listing: Vec<i64> = p
                .variants
                .iter()
                .filter(|other| listing(other) == listing(variant))
                .map(|other| other.price)
                .collect();
            let options: Vec<(String, String)> = variant
                .options
                .iter()
                .map(|(n, v)| (n.to_string(), v.to_string()))
                .collect();
            documents.push(SearchDocument {
                market: market(p.market),
                product: id,
                variant: VariantId::from_uuid(Uuid::from_u128(variant.n)),
                listing: listing(variant),
                channels: channels.clone(),
                handle: p.title.to_lowercase().replace(' ', "-"),
                title: p.title.to_string(),
                variant_title: (!options.is_empty()).then(|| {
                    options
                        .iter()
                        .map(|(_, v)| v.as_str())
                        .collect::<Vec<_>>()
                        .join(" / ")
                }),
                description: p.description.to_string(),
                vendor: p.vendor.map(str::to_string),
                product_type: p.product_type.map(str::to_string),
                tags: p.tags.iter().map(|t| t.to_string()).collect(),
                sku: variant.sku.map(str::to_string),
                options,
                price: eur(variant.price),
                product_price: range(&prices),
                listing_price: range(&same_listing),
                available: variant.available,
                image: None,
                created_at: Timestamp::from_unix_micros(p.created),
                position: position as u32,
            });
        }
    }
    documents
}
