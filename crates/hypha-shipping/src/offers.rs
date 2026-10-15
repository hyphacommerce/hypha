//! What a group of a cart is offered (design note 0015, section 6): every enabled option
//! of the destination's zone, priced. Built-in rates are computed here; each provider is
//! asked once for all its options, all providers at once. A provider that fails leaves
//! its options out with the reason, never failing the cart (SH7).

use std::collections::BTreeMap;

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
}

impl NotOffered {
    /// Stable codes, for the cart's per-option reasons (and UCP messages).
    pub fn code(&self) -> &'static str {
        match self {
            NotOffered::NoPriceInCurrency => "no_price_in_currency",
            NotOffered::WeightUnknown => "weight_unknown",
            NotOffered::ProviderFailed(_) => "provider_failed",
            NotOffered::ServiceNotQuoted => "service_not_quoted",
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
/// option order. Backup options appear only when the zone has provider options and none
/// of them could be offered.
pub async fn offers<P: RateProvider>(
    zone: &ShippingZone,
    providers: &P,
    request: &RateRequest,
) -> Vec<OfferOutcome> {
    let weight = total_weight(request);
    let enabled: Vec<&ShippingOption> = zone.options().iter().filter(|o| o.enabled()).collect();

    // One quote per provider, all at once.
    let mut by_provider: BTreeMap<&ProviderKey, ()> = BTreeMap::new();
    for option in &enabled {
        if let OptionRate::Provider { provider, .. } = option.rate() {
            by_provider.insert(provider, ());
        }
    }
    let keys: Vec<&ProviderKey> = by_provider.into_keys().collect();
    let quotes = join_all(keys.iter().map(|key| providers.quote(key, request))).await;
    let quotes: BTreeMap<&ProviderKey, Result<Vec<Rate>, ProviderError>> = keys
        .into_iter()
        .zip(quotes)
        .map(|(key, quote)| {
            (
                key,
                quote.and_then(|rates| same_currency(key, request, rates)),
            )
        })
        .collect();

    let outcome = |option: &ShippingOption| match option.rate() {
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
            // Every provider option's key was quoted above.
            None => not_offered(option, NotOffered::ServiceNotQuoted),
        },
    };

    let regular: Vec<(&ShippingOption, OfferOutcome)> = enabled
        .iter()
        .filter(|o| !o.is_backup())
        .map(|o| (*o, outcome(o)))
        .collect();
    let has_provider_options = regular
        .iter()
        .any(|(o, _)| matches!(o.rate(), OptionRate::Provider { .. }));
    let every_provider_option_failed = has_provider_options
        && regular.iter().all(|(o, result)| {
            !matches!(o.rate(), OptionRate::Provider { .. })
                || matches!(result, OfferOutcome::NotOffered { .. })
        });

    // Back in the zone's order, backups included only when needed.
    enabled
        .iter()
        .filter(|o| !o.is_backup() || every_provider_option_failed)
        .map(|o| {
            regular
                .iter()
                .find(|(r, _)| r.id() == o.id())
                .map(|(_, result)| result.clone())
                .unwrap_or_else(|| outcome(o))
        })
        .collect()
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

    /// Providers by key: their rates, or the error they fail with.
    #[derive(Default)]
    struct Fake(HashMap<String, Result<Vec<Rate>, ProviderError>>);

    impl RateProvider for Fake {
        async fn services(&self, _: &ProviderKey) -> Result<Vec<ServiceInfo>, ProviderError> {
            Ok(vec![])
        }
        async fn quote(
            &self,
            key: &ProviderKey,
            _: &RateRequest,
        ) -> Result<Vec<Rate>, ProviderError> {
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
        let providers = Fake(HashMap::from([(
            "app_dhl".to_string(),
            Ok(vec![rate("economy", eur(300)), rate("express", eur(1290))]),
        )]));
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
        let providers = Fake(HashMap::from([(
            "app_dhl".to_string(),
            Err(failing.clone()),
        )]));
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
        let providers = Fake(HashMap::from([
            (
                "app_dhl".to_string(),
                Err(ProviderError::Timeout(key("app_dhl"))),
            ),
            ("app_gls".to_string(), Ok(vec![rate("parcel", eur(450))])),
        ]));
        let outcomes = pollster::block_on(offers(&z, &providers, &request(None)));
        assert_eq!(offered_ids(&outcomes), vec![oid(2)]);
        assert_eq!(outcomes.len(), 2, "the backup isn't listed at all");
    }

    #[test]
    fn a_service_the_provider_didnt_quote_isnt_offered() {
        let z = zone(vec![option(1, by("app_dhl", "overnight"), false)]);
        let providers = Fake(HashMap::from([(
            "app_dhl".to_string(),
            Ok(vec![rate("express", eur(1290))]),
        )]));
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
        let providers = Fake(HashMap::from([(
            "app_dhl".to_string(),
            Ok(vec![rate("express", usd)]),
        )]));
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
