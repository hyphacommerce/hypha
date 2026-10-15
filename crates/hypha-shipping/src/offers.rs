//! What a group of a cart is offered (design note 0015, section 6): every enabled option
//! of the destination's zone, priced. Built-in rates are computed here; each provider is
//! asked once for all its options, all providers at once. A provider that fails leaves
//! its options out with the reason, never failing the cart (SH7).

use std::collections::{BTreeMap, BTreeSet};

use futures_util::future::join_all;
use hypha_core::{Money, ShippingOptionId, TaxClassId, TotalWeight};

use crate::{
    DeliveryEstimate, OptionRate, ProviderError, ProviderKey, Rate, RateProvider, RateRequest,
    ShippingOption, ShippingZone, Unpriced,
};

/// An option the buyer can pick, with its price now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub option: ShippingOptionId,
    pub code: String,
    pub name: String,
    pub amount: Money,
    pub estimate: Option<DeliveryEstimate>,
    pub tax_class: Option<TaxClassId>,
    /// For provider options: who quoted it, which service, and the quote's reference.
    pub quote: Option<QuoteSource>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteSource {
    pub provider: ProviderKey,
    pub service: String,
    pub reference: Option<String>,
}

/// Why an option isn't offered to this group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotOffered {
    /// A built-in rate with no table in the cart's currency.
    NoPriceInCurrency,
    /// Weight tiers, and a line's weight isn't known.
    WeightUnknown,
    /// Its provider failed or timed out.
    ProviderFailed(ProviderError),
    /// Its provider answered without this service.
    ServiceNotQuoted,
    /// A backup, and a regular option is offered: backups only stand in when none is.
    BackupNotNeeded,
}

impl NotOffered {
    /// Stable codes, for the cart's per-option reasons (and UCP messages).
    pub fn code(&self) -> &'static str {
        match self {
            NotOffered::NoPriceInCurrency => "no_price_in_currency",
            NotOffered::WeightUnknown => "weight_unknown",
            NotOffered::ProviderFailed(_) => "provider_failed",
            NotOffered::ServiceNotQuoted => "service_not_quoted",
            NotOffered::BackupNotNeeded => "backup_not_needed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OfferOutcome {
    Offered(Offer),
    NotOffered {
        option: ShippingOptionId,
        name: String,
        reason: NotOffered,
    },
}

/// The offers for one group of a cart going to a country `zone` covers, in the zone's
/// option order. Backup options are offered only when no regular option is (disabled,
/// failed or missing alike), and listed as `BackupNotNeeded` otherwise, so selecting one
/// says why. Each provider is asked once, all at once: a built-in regular option that's
/// offered means backups aren't needed, and then only regular options' providers are
/// asked; otherwise backups' providers are asked together with them, so a slow regular
/// provider's timeout isn't followed by a second wait for the backups'.
pub async fn offers<P: RateProvider>(
    zone: &ShippingZone,
    providers: &P,
    request: &RateRequest,
) -> Vec<OfferOutcome> {
    let weight = total_weight(request);
    let enabled: Vec<&ShippingOption> = zone.options().iter().filter(|o| o.enabled()).collect();
    let (backups, regular): (Vec<&ShippingOption>, Vec<&ShippingOption>) =
        enabled.iter().partition(|o| o.is_backup());

    // Built-in prices need no provider: computed first, they tell whether the backups'
    // providers can be skipped.
    let no_quotes = BTreeMap::new();
    let builtin_offered = regular.iter().any(|o| {
        matches!(o.rate(), OptionRate::Builtin(_))
            && matches!(
                outcome(o, request, weight, &no_quotes),
                OfferOutcome::Offered(_)
            )
    });
    let mut quotes = BTreeMap::new();
    let asked = if builtin_offered { &regular } else { &enabled };
    quote_all(asked, providers, request, &mut quotes).await;
    let regular: Vec<(&ShippingOption, OfferOutcome)> = regular
        .iter()
        .map(|o| (*o, outcome(o, request, weight, &quotes)))
        .collect();
    let needs_backups = !regular
        .iter()
        .any(|(_, result)| matches!(result, OfferOutcome::Offered(_)));

    // Needed means no regular option was offered, built-in ones included, so every
    // backup's provider was asked above.
    let backups: Vec<(&ShippingOption, OfferOutcome)> = if needs_backups {
        backups
            .iter()
            .map(|o| (*o, outcome(o, request, weight, &quotes)))
            .collect()
    } else {
        backups
            .iter()
            .map(|o| (*o, not_offered(o, NotOffered::BackupNotNeeded)))
            .collect()
    };

    // Back in the zone's order.
    enabled
        .iter()
        .filter_map(|o| {
            regular
                .iter()
                .chain(&backups)
                .find(|(r, _)| r.id() == o.id())
                .map(|(_, result)| result.clone())
        })
        .collect()
}

type Quotes<'a> = BTreeMap<&'a ProviderKey, Result<Vec<Rate>, ProviderError>>;

/// Asks every provider `options` use and `quotes` doesn't have yet, all at once.
async fn quote_all<'a, P: RateProvider>(
    options: &[&'a ShippingOption],
    providers: &P,
    request: &RateRequest,
    quotes: &mut Quotes<'a>,
) {
    let keys: Vec<&ProviderKey> = options
        .iter()
        .filter_map(|o| match o.rate() {
            OptionRate::Provider { provider, .. } => Some(provider),
            OptionRate::Builtin(_) => None,
        })
        .filter(|key| !quotes.contains_key(key))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let answers = join_all(keys.iter().map(|key| providers.quote(key, request))).await;
    for (key, answer) in keys.into_iter().zip(answers) {
        let answer = answer.and_then(|rates| same_currency(key, request, rates));
        quotes.insert(key, answer);
    }
}

/// `option` priced: computed, or from its provider's quote.
fn outcome(
    option: &ShippingOption,
    request: &RateRequest,
    weight: Option<TotalWeight>,
    quotes: &Quotes<'_>,
) -> OfferOutcome {
    match option.rate() {
        OptionRate::Builtin(rate) => match rate.price(request.subtotal, weight) {
            Ok(price) => offered(option, price.money(), option.estimate(), None),
            Err(Unpriced::NoPriceInCurrency) => not_offered(option, NotOffered::NoPriceInCurrency),
            Err(Unpriced::WeightUnknown) => not_offered(option, NotOffered::WeightUnknown),
        },
        OptionRate::Provider { provider, service } => match quotes.get(provider) {
            Some(Ok(rates)) => match rates.iter().find(|r| &r.service == service) {
                Some(rate) => offered(
                    option,
                    rate.amount.money(),
                    rate.estimate.or(option.estimate()),
                    Some(QuoteSource {
                        provider: provider.clone(),
                        service: service.clone(),
                        reference: rate.reference.clone(),
                    }),
                ),
                None => not_offered(option, NotOffered::ServiceNotQuoted),
            },
            Some(Err(e)) => not_offered(option, NotOffered::ProviderFailed(e.clone())),
            // `quote_all` asked every provider of the options it was given.
            None => not_offered(option, NotOffered::ServiceNotQuoted),
        },
    }
}

/// The group's weight, when every line's is known.
fn total_weight(request: &RateRequest) -> Option<TotalWeight> {
    request
        .items
        .iter()
        .try_fold(TotalWeight::default(), |total, item| {
            item.weight.map(|w| total.add(w, item.quantity.value()))
        })
}

/// A provider's rates, refused whole if one is in another currency than the request.
fn same_currency(
    provider: &ProviderKey,
    request: &RateRequest,
    rates: Vec<Rate>,
) -> Result<Vec<Rate>, ProviderError> {
    match rates
        .iter()
        .find(|r| r.amount.money().currency() != request.currency)
    {
        Some(r) => Err(ProviderError::WrongCurrency {
            provider: provider.clone(),
            expected: request.currency,
            got: r.amount.money().currency(),
        }),
        None => Ok(rates),
    }
}

fn offered(
    option: &ShippingOption,
    amount: Money,
    estimate: Option<DeliveryEstimate>,
    quote: Option<QuoteSource>,
) -> OfferOutcome {
    OfferOutcome::Offered(Offer {
        option: option.id(),
        code: option.code().as_str().to_string(),
        name: option.name().to_string(),
        amount,
        estimate,
        tax_class: option.tax_class(),
        quote,
    })
}

fn not_offered(option: &ShippingOption, reason: NotOffered) -> OfferOutcome {
    OfferOutcome::NotOffered {
        option: option.id(),
        name: option.name().to_string(),
        reason,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use hypha_core::{
        Address, AddressParts, CountryCode, Currency, Price, Quantity, ShippingZoneId, VariantId,
        Weight,
    };
    use uuid::Uuid;

    use super::*;
    use crate::{BuiltinRate, NewOption, RateBasis, RateItem, RateTable, ServiceInfo, Tier};

    /// Providers by key: their rates, or the error they fail with. Records who was asked.
    #[derive(Default)]
    struct Fake(
        HashMap<String, Result<Vec<Rate>, ProviderError>>,
        std::cell::RefCell<Vec<String>>,
    );

    fn fake<const N: usize>(answers: [(String, Result<Vec<Rate>, ProviderError>); N]) -> Fake {
        Fake(HashMap::from(answers), Default::default())
    }

    impl RateProvider for Fake {
        async fn services(&self, _: &ProviderKey) -> Result<Vec<ServiceInfo>, ProviderError> {
            Ok(vec![])
        }
        async fn quote(
            &self,
            key: &ProviderKey,
            _: &RateRequest,
        ) -> Result<Vec<Rate>, ProviderError> {
            self.1.borrow_mut().push(key.as_str().to_string());
            self.0
                .get(key.as_str())
                .cloned()
                .unwrap_or_else(|| Err(ProviderError::Unknown(key.clone())))
        }
    }

    fn eur(amount: i64) -> Price {
        Price::new(Money::new(amount, Currency::Eur).unwrap()).unwrap()
    }
    fn key(k: &str) -> ProviderKey {
        ProviderKey::new(k).unwrap()
    }
    fn oid(n: u128) -> ShippingOptionId {
        ShippingOptionId::from_uuid(Uuid::from_u128(n))
    }
    fn rate(service: &str, amount: Price) -> Rate {
        Rate {
            service: service.to_string(),
            name: service.to_string(),
            amount,
            estimate: None,
            reference: Some(format!("q-{service}")),
        }
    }

    fn option(n: u128, rate: OptionRate, is_backup: bool) -> NewOption {
        NewOption {
            id: oid(n),
            name: format!("Option {n}"),
            code: format!("o{n}"),
            rate,
            estimate: None,
            tax_class: None,
            is_backup,
            enabled: true,
        }
    }
    fn flat(amount: i64) -> OptionRate {
        OptionRate::Builtin(BuiltinRate::flat(vec![eur(amount)]).unwrap())
    }
    fn by(provider: &str, service: &str) -> OptionRate {
        OptionRate::Provider {
            provider: key(provider),
            service: service.to_string(),
        }
    }

    fn zone(options: Vec<NewOption>) -> ShippingZone {
        let mut z = ShippingZone::new(
            ShippingZoneId::from_uuid(Uuid::from_u128(1)),
            "HR",
            vec![CountryCode::new("HR").unwrap()],
        )
        .unwrap();
        for o in options {
            z.add_option(o).unwrap();
        }
        z
    }

    fn request(weight: Option<u32>) -> RateRequest {
        RateRequest {
            currency: Currency::Eur,
            origin: None,
            destination: Address::new(AddressParts {
                first_name: None,
                last_name: "Horvat".to_string(),
                company: None,
                line1: "Ilica 1".to_string(),
                line2: None,
                city: "Zagreb".to_string(),
                province: None,
                postal_code: None,
                country: "HR".to_string(),
                phone: None,
            })
            .unwrap(),
            items: vec![RateItem {
                variant: VariantId::from_uuid(Uuid::from_u128(9)),
                sku: None,
                quantity: Quantity::new(2).unwrap(),
                unit_price: eur(1000),
                weight: weight.map(Weight::from_grams),
            }],
            subtotal: Money::new(2000, Currency::Eur).unwrap(),
        }
    }

    fn offered_ids(outcomes: &[OfferOutcome]) -> Vec<ShippingOptionId> {
        outcomes
            .iter()
            .filter_map(|o| match o {
                OfferOutcome::Offered(offer) => Some(offer.option),
                OfferOutcome::NotOffered { .. } => None,
            })
            .collect()
    }

    fn quote(outcomes: &[OfferOutcome], option: ShippingOptionId) -> &OfferOutcome {
        outcomes
            .iter()
            .find(|o| match o {
                OfferOutcome::Offered(offer) => offer.option == option,
                OfferOutcome::NotOffered { option: id, .. } => *id == option,
            })
            .unwrap()
    }

    #[test]
    fn built_in_and_provider_options_are_priced_in_zone_order() {
        let z = zone(vec![
            option(1, by("app_dhl", "express"), false),
            option(2, flat(490), false),
        ]);
        let providers = fake([(
            "app_dhl".to_string(),
            Ok(vec![rate("economy", eur(300)), rate("express", eur(1290))]),
        )]);
        let outcomes = pollster::block_on(offers(&z, &providers, &request(None)));
        assert_eq!(offered_ids(&outcomes), vec![oid(1), oid(2)]);
        let OfferOutcome::Offered(express) = quote(&outcomes, oid(1)) else {
            panic!("express offered")
        };
        assert_eq!(express.amount, eur(1290).money());
        assert_eq!(
            express.quote,
            Some(QuoteSource {
                provider: key("app_dhl"),
                service: "express".to_string(),
                reference: Some("q-express".to_string()),
            })
        );
    }

    #[test]
    fn a_failing_provider_leaves_its_options_out_and_brings_in_backups() {
        let z = zone(vec![
            option(1, by("app_dhl", "express"), false),
            option(2, flat(990), true),
        ]);
        let failing = ProviderError::Timeout(key("app_dhl"));
        let providers = fake([("app_dhl".to_string(), Err(failing.clone()))]);
        let outcomes = pollster::block_on(offers(&z, &providers, &request(None)));
        assert_eq!(offered_ids(&outcomes), vec![oid(2)]);
        assert_eq!(
            quote(&outcomes, oid(1)),
            &OfferOutcome::NotOffered {
                option: oid(1),
                name: "Option 1".to_string(),
                reason: NotOffered::ProviderFailed(failing),
            }
        );
    }

    #[test]
    fn backups_stay_hidden_while_a_provider_option_is_offered() {
        let z = zone(vec![
            option(1, by("app_dhl", "express"), false),
            option(2, by("app_gls", "parcel"), false),
            option(3, flat(990), true),
        ]);
        let providers = fake([
            (
                "app_dhl".to_string(),
                Err(ProviderError::Timeout(key("app_dhl"))),
            ),
            ("app_gls".to_string(), Ok(vec![rate("parcel", eur(450))])),
        ]);
        let outcomes = pollster::block_on(offers(&z, &providers, &request(None)));
        assert_eq!(offered_ids(&outcomes), vec![oid(2)]);
        assert!(matches!(
            quote(&outcomes, oid(3)),
            OfferOutcome::NotOffered {
                reason: NotOffered::BackupNotNeeded,
                ..
            }
        ));
    }

    #[test]
    fn backups_stay_hidden_while_a_built_in_option_is_offered() {
        let z = zone(vec![
            option(1, by("app_dhl", "express"), false),
            option(2, flat(490), false),
            option(3, flat(990), true),
        ]);
        let providers = fake([(
            "app_dhl".to_string(),
            Err(ProviderError::Timeout(key("app_dhl"))),
        )]);
        let outcomes = pollster::block_on(offers(&z, &providers, &request(None)));
        assert_eq!(offered_ids(&outcomes), vec![oid(2)]);
    }

    #[test]
    fn backups_are_offered_when_every_regular_option_is_disabled() {
        let mut disabled = option(1, by("app_dhl", "express"), false);
        disabled.enabled = false;
        let z = zone(vec![disabled, option(2, flat(990), true)]);
        let outcomes = pollster::block_on(offers(&z, &fake([]), &request(None)));
        assert_eq!(offered_ids(&outcomes), vec![oid(2)]);
    }

    #[test]
    fn a_backups_provider_is_asked_only_when_backups_are_needed() {
        let z = zone(vec![
            option(1, flat(490), false),
            option(2, by("app_post", "parcel"), true),
        ]);
        let providers = fake([("app_post".to_string(), Ok(vec![rate("parcel", eur(300))]))]);
        let outcomes = pollster::block_on(offers(&z, &providers, &request(None)));
        assert_eq!(offered_ids(&outcomes), vec![oid(1)]);
        assert!(providers.1.borrow().is_empty(), "app_post wasn't asked");
    }

    /// Like `Fake`, but each answer waits one poll, and it records how many calls were
    /// waiting at once.
    #[derive(Default)]
    struct Slow {
        waiting: std::cell::Cell<usize>,
        most_waiting: std::cell::Cell<usize>,
    }

    impl RateProvider for Slow {
        async fn services(&self, _: &ProviderKey) -> Result<Vec<ServiceInfo>, ProviderError> {
            Ok(vec![])
        }
        async fn quote(
            &self,
            key: &ProviderKey,
            _: &RateRequest,
        ) -> Result<Vec<Rate>, ProviderError> {
            self.waiting.set(self.waiting.get() + 1);
            self.most_waiting
                .set(self.most_waiting.get().max(self.waiting.get()));
            let mut yielded = false;
            std::future::poll_fn(|cx| {
                if yielded {
                    std::task::Poll::Ready(())
                } else {
                    yielded = true;
                    cx.waker().wake_by_ref();
                    std::task::Poll::Pending
                }
            })
            .await;
            self.waiting.set(self.waiting.get() - 1);
            match key.as_str() {
                "app_post" => Ok(vec![rate("parcel", eur(300))]),
                _ => Err(ProviderError::Timeout(key.clone())),
            }
        }
    }

    #[test]
    fn without_a_built_in_offer_backups_providers_are_asked_with_the_regular_ones() {
        let z = zone(vec![
            option(1, by("app_dhl", "express"), false),
            option(2, by("app_post", "parcel"), true),
        ]);
        let providers = Slow::default();
        let outcomes = pollster::block_on(offers(&z, &providers, &request(None)));
        assert_eq!(offered_ids(&outcomes), vec![oid(2)]);
        // One round: DHL's timeout and the post's answer were awaited together.
        assert_eq!(providers.most_waiting.get(), 2);
    }

    #[test]
    fn a_service_the_provider_didnt_quote_isnt_offered() {
        let z = zone(vec![option(1, by("app_dhl", "overnight"), false)]);
        let providers = fake([("app_dhl".to_string(), Ok(vec![rate("express", eur(1290))]))]);
        let outcomes = pollster::block_on(offers(&z, &providers, &request(None)));
        assert!(matches!(
            quote(&outcomes, oid(1)),
            OfferOutcome::NotOffered {
                reason: NotOffered::ServiceNotQuoted,
                ..
            }
        ));
    }

    #[test]
    fn rates_in_another_currency_fail_the_provider() {
        let z = zone(vec![option(1, by("app_dhl", "express"), false)]);
        let usd = Price::new(Money::new(1000, Currency::Usd).unwrap()).unwrap();
        let providers = fake([("app_dhl".to_string(), Ok(vec![rate("express", usd)]))]);
        let outcomes = pollster::block_on(offers(&z, &providers, &request(None)));
        assert!(matches!(
            quote(&outcomes, oid(1)),
            OfferOutcome::NotOffered {
                reason: NotOffered::ProviderFailed(ProviderError::WrongCurrency { .. }),
                ..
            }
        ));
    }

    #[test]
    fn weight_tiers_use_the_groups_weight_times_quantities() {
        let table = RateTable::new(
            Currency::Eur,
            vec![
                Tier {
                    up_to: Some(1000),
                    price: eur(400),
                },
                Tier {
                    up_to: None,
                    price: eur(900),
                },
            ],
        )
        .unwrap();
        let weighted =
            OptionRate::Builtin(BuiltinRate::new(RateBasis::Weight, vec![table]).unwrap());
        let z = zone(vec![option(1, weighted, false)]);
        // 2 × 600 g = 1200 g: the second tier.
        let outcomes = pollster::block_on(offers(&z, &Fake::default(), &request(Some(600))));
        let OfferOutcome::Offered(offer) = quote(&outcomes, oid(1)) else {
            panic!("offered")
        };
        assert_eq!(offer.amount, eur(900).money());

        let unknown = pollster::block_on(offers(&z, &Fake::default(), &request(None)));
        assert!(matches!(
            quote(&unknown, oid(1)),
            OfferOutcome::NotOffered {
                reason: NotOffered::WeightUnknown,
                ..
            }
        ));
    }

    #[test]
    fn disabled_options_are_left_out_entirely() {
        let mut off = option(1, flat(100), false);
        off.enabled = false;
        let z = zone(vec![off, option(2, flat(200), false)]);
        let outcomes = pollster::block_on(offers(&z, &Fake::default(), &request(None)));
        assert_eq!(offered_ids(&outcomes), vec![oid(2)]);
        assert_eq!(outcomes.len(), 1);
    }
}
