//! The language an order is placed in and what it copies in it: a cart's names as the
//! buyer read them, beside the store's own that staff read.

use hypha_cart::{ContentLanguages, PricedCart, PricedLine, promotions::Application};
use hypha_core::{Language, RepoError, ShippingOptionId};
use hypha_orders::{LocalizedDiscount, LocalizedLine, LocalizedShipping};
use hypha_payments::{LocalizedPayment, PaymentMethod};
use hypha_translations::{HasTranslations, Overlay, ResourceKind, ResourceRef, fields};

/// What checkout copies in the buyer's language: the translations of a cart's products,
/// shipping options, discounts and payment method, read in one go. Copies are made only
/// where they read differently from the store's own text.
pub(crate) struct Localizer {
    language: Language,
    /// `None` in the store's default language: nothing to copy.
    overlay: Option<Overlay>,
}

impl Localizer {
    /// Reads what `cart` and its payment `method` need in its language (none in the
    /// store's default).
    pub(crate) async fn load<S: HasTranslations>(
        ports: &S,
        languages: &ContentLanguages,
        cart: &PricedCart,
        method: Option<&PaymentMethod>,
    ) -> Result<Self, RepoError> {
        let language = languages.of(cart);
        if language == languages.source {
            return Ok(Self {
                language,
                overlay: None,
            });
        }
        let mut resources: Vec<ResourceRef> = Vec::new();
        let mut add = |r: ResourceRef| {
            if !resources.contains(&r) {
                resources.push(r);
            }
        };
        for line in &cart.lines {
            add(product(line));
        }
        for group in &cart.shipping_groups {
            if let Some(selected) = &group.selected {
                add(shipping_option(selected.option));
            }
        }
        for applied in &cart.discounts {
            add(discount(applied));
        }
        if let Some(method) = method {
            add(payment_method(method));
        }
        let overlay = Overlay::load(ports, &language, &languages.servable, &resources).await?;
        Ok(Self {
            language,
            overlay: Some(overlay),
        })
    }

    /// The language the order is placed in.
    pub(crate) fn language(&self) -> &Language {
        &self.language
    }

    /// A line's names in the buyer's language: the product's title and the variant's,
    /// joined from its option values' labels.
    pub(crate) fn line(&self, line: &PricedLine) -> Option<LocalizedLine> {
        let overlay = self.overlay.as_ref()?;
        let details = line.details.as_ref()?;
        let product = product(line);
        if !overlay.translates(product) {
            return None;
        }
        let localized = LocalizedLine {
            product_title: overlay
                .text_or(product, fields::product::TITLE, &details.product_title)
                .to_string(),
            variant_title: details.variant_title_with(|value| {
                overlay.text(product, &fields::product::option_value_label(value))
            }),
        };
        (localized.product_title != details.product_title
            || localized.variant_title != details.variant_title)
            .then_some(localized)
    }

    /// The chosen option's name in the buyer's language.
    pub(crate) fn shipping(
        &self,
        option: ShippingOptionId,
        name: &str,
    ) -> Option<LocalizedShipping> {
        let overlay = self.overlay.as_ref()?;
        let translated =
            overlay.text_or(shipping_option(option), fields::shipping_option::NAME, name);
        (translated != name).then(|| LocalizedShipping {
            name: translated.to_string(),
        })
    }

    /// A discount's title in the buyer's language.
    pub(crate) fn discount(&self, applied: &Application) -> Option<LocalizedDiscount> {
        let overlay = self.overlay.as_ref()?;
        let translated =
            overlay.text_or(discount(applied), fields::discount::TITLE, &applied.title);
        (translated != applied.title).then(|| LocalizedDiscount {
            title: translated.to_string(),
        })
    }

    /// A payment method's name and instructions in the buyer's language: only what the
    /// method has (no instructions to translate where it has none).
    pub(crate) fn payment(&self, method: &PaymentMethod) -> Option<LocalizedPayment> {
        let overlay = self.overlay.as_ref()?;
        let resource = payment_method(method);
        let name = overlay
            .text_or(resource, fields::payment_method::NAME, method.name())
            .to_string();
        let instructions = overlay.optional(
            resource,
            fields::payment_method::INSTRUCTIONS,
            method.instructions(),
        );
        let localized = LocalizedPayment {
            method_name: (name != method.name()).then_some(name),
            instructions: instructions
                .filter(|text| Some(*text) != method.instructions())
                .map(str::to_string),
        };
        (localized.method_name.is_some() || localized.instructions.is_some()).then_some(localized)
    }
}

fn product(line: &PricedLine) -> ResourceRef {
    ResourceRef::new(ResourceKind::Product, line.product.as_uuid())
}

fn shipping_option(option: ShippingOptionId) -> ResourceRef {
    ResourceRef::new(ResourceKind::ShippingOption, option.as_uuid())
}

fn discount(applied: &Application) -> ResourceRef {
    ResourceRef::new(ResourceKind::Discount, applied.discount.as_uuid())
}

fn payment_method(method: &PaymentMethod) -> ResourceRef {
    ResourceRef::new(ResourceKind::PaymentMethod, method.id().as_uuid())
}
