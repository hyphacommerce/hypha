//! What each template gets (design note 0028, §3, §11, NT3): typed serde structs, the
//! contract of the templates' variables. Fields are added, never renamed or removed.
//! `null` where a value is absent; money and times arrive formatted (`format.rs`).
//!
//! Each kind of email has a `sample()`, a made-up order (or staff message) for
//! previews, test sends, the editor's variable list and checking a template on save.

use hypha_core::{Address, Currency, Email, Money, OrderId, Timestamp, Timezone};
use hypha_fulfillment::Fulfillment;
use hypha_notifications::{NotificationSettings, TemplateKey};
use hypha_orders::{CancelReason, Order};
use hypha_payments::Payment;
use hypha_stores::Store;
use serde::Serialize;

use crate::format::{MoneyValue, TimeValue};

/// `shop`: in every template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShopContext {
    pub name: String,
    /// The store's contact email.
    pub email: Option<String>,
    /// The storefront's address: the origin of the settings' order URL.
    pub url: Option<String>,
}

impl ShopContext {
    pub fn of(store: &Store, settings: &NotificationSettings) -> Self {
        Self {
            name: store.name().to_string(),
            email: store.contact_email().map(|e| e.as_str().to_string()),
            url: settings.order_url().map(|u| u.origin().to_string()),
        }
    }
}

/// `brand`: in every template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BrandContext {
    pub logo_url: Option<String>,
    /// Always set: the settings' accent, or its default.
    pub accent: String,
    pub footer: Option<String>,
}

impl BrandContext {
    /// `logo_url`: the logo asset's URL, which the host makes (`MailEnv::asset_url`).
    pub fn of(settings: &NotificationSettings, logo_url: Option<String>) -> Self {
        Self {
            logo_url,
            accent: settings.accent().as_str().to_string(),
            footer: settings.footer().map(str::to_string),
        }
    }
}

/// `order.lines[]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LineContext {
    /// The product's title.
    pub title: String,
    /// The variant's option values ("M / Blue"); `null` for a simple product.
    pub variant: Option<String>,
    pub sku: Option<String>,
    pub quantity: u32,
    pub unit_price: MoneyValue,
    pub total: MoneyValue,
}

/// `order.shipping_address`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AddressContext {
    /// First and last name.
    pub name: String,
    pub company: Option<String>,
    pub line1: String,
    pub line2: Option<String>,
    pub city: String,
    pub province: Option<String>,
    pub postal_code: Option<String>,
    /// The country's name ("Croatia").
    pub country: String,
    pub phone: Option<String>,
}

impl AddressContext {
    pub fn of(address: &Address) -> Self {
        let parts = address.parts();
        Self {
            name: full_name(address),
            company: parts.company.clone(),
            line1: parts.line1.clone(),
            line2: parts.line2.clone(),
            city: parts.city.clone(),
            province: parts.province.clone(),
            postal_code: parts.postal_code.clone(),
            country: address.country().name().to_string(),
            phone: parts.phone.clone(),
        }
    }
}

fn full_name(address: &Address) -> String {
    let parts = address.parts();
    match &parts.first_name {
        Some(first) => format!("{first} {}", parts.last_name),
        None => parts.last_name.clone(),
    }
}

/// `order.payment`: the order's newest payment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PaymentContext {
    /// The payment method's name as the buyer chose it ("Bank transfer").
    pub method: String,
    /// `pending`, `paid`, … (`PaymentStatus::as_str`).
    pub status: String,
    /// What the buyer does to pay (bank details), copied at checkout; may hold
    /// newlines.
    pub instructions: Option<String>,
}

impl PaymentContext {
    /// A payment recorded by hand has no method: it reads as "Manual payment".
    pub const BY_HAND: &str = "Manual payment";

    pub fn of(payment: &Payment) -> Self {
        Self {
            method: payment.method_name().unwrap_or(Self::BY_HAND).to_string(),
            status: payment.status().as_str().to_string(),
            instructions: payment.instructions().map(str::to_string),
        }
    }
}

/// `order`: in the order templates and `staff_new_order`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OrderContext {
    /// `ord_…`.
    pub id: String,
    /// "#1001".
    pub number: String,
    /// The order's page with its access token, from the settings' order URL; `null`
    /// without one.
    pub url: Option<String>,
    /// The buyer's email.
    pub email: String,
    pub placed_at: TimeValue,
    pub lines: Vec<LineContext>,
    pub subtotal: MoneyValue,
    pub shipping: MoneyValue,
    pub tax: MoneyValue,
    pub total: MoneyValue,
    /// Whether the prices include tax (then `tax` is part of the total, not on top).
    pub taxes_included: bool,
    pub shipping_address: Option<AddressContext>,
    /// The shipping lines' names, joined with ", ".
    pub shipping_method: Option<String>,
    pub payment: Option<PaymentContext>,
}

impl OrderContext {
    /// `order` with its newest `payment`; `url` is its page's link (`OrderUrl::link`).
    pub fn of(
        order: &Order,
        payment: Option<&Payment>,
        url: Option<String>,
        zone: Option<Timezone>,
    ) -> Self {
        let names: Vec<&str> = order
            .shipping_lines()
            .iter()
            .map(|l| l.name.as_str())
            .collect();
        Self {
            id: order.id().to_string(),
            number: order.number().to_string(),
            url,
            email: order.email().as_str().to_string(),
            placed_at: TimeValue::of(order.placed_at(), zone),
            lines: order
                .lines()
                .iter()
                .map(|line| LineContext {
                    title: line.product_title.clone(),
                    variant: line.variant_title.clone(),
                    sku: line.sku.clone(),
                    quantity: line.quantity.value(),
                    unit_price: MoneyValue::of(line.unit_price.money()),
                    total: MoneyValue::of(line.total),
                })
                .collect(),
            subtotal: MoneyValue::of(order.subtotal()),
            shipping: MoneyValue::of(order.shipping_total()),
            tax: MoneyValue::of(order.tax_total()),
            total: MoneyValue::of(order.total()),
            taxes_included: order.taxes_included(),
            shipping_address: order.shipping_address().map(AddressContext::of),
            shipping_method: (!names.is_empty()).then(|| names.join(", ")),
            payment: payment.map(PaymentContext::of),
        }
    }
}

/// `customer`: who the order is for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CustomerContext {
    /// From the shipping address; `null` without one.
    pub name: Option<String>,
}

impl CustomerContext {
    pub fn of(order: &Order) -> Self {
        Self {
            name: order.shipping_address().map(full_name),
        }
    }
}

/// `fulfillment.lines[]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShippedLineContext {
    pub title: String,
    pub variant: Option<String>,
    pub quantity: u32,
}

/// `fulfillment.tracking[]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrackingContext {
    pub carrier: Option<String>,
    pub number: String,
    pub url: Option<String>,
}

/// `fulfillment`: in `order_shipped` and `order_tracking_updated`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FulfillmentContext {
    pub lines: Vec<ShippedLineContext>,
    pub tracking: Vec<TrackingContext>,
}

impl FulfillmentContext {
    /// `fulfillment` of `order`: its lines named as the order names them.
    pub fn of(fulfillment: &Fulfillment, order: &Order) -> Self {
        Self {
            lines: fulfillment
                .lines()
                .iter()
                .filter_map(|shipped| {
                    let line = order.lines().iter().find(|l| l.id == shipped.line)?;
                    Some(ShippedLineContext {
                        title: line.product_title.clone(),
                        variant: line.variant_title.clone(),
                        quantity: shipped.quantity.value(),
                    })
                })
                .collect(),
            tracking: fulfillment
                .tracking()
                .iter()
                .map(|t| TrackingContext {
                    carrier: t.carrier().map(str::to_string),
                    number: t.number().to_string(),
                    url: t.url().map(str::to_string),
                })
                .collect(),
        }
    }
}

/// `cancellation`: in `order_cancelled`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CancellationContext {
    /// The reason as the buyer reads it ("Items unavailable"); `null` for `other`.
    pub reason: Option<String>,
}

impl CancellationContext {
    pub fn of(reason: Option<CancelReason>) -> Self {
        Self {
            reason: reason.and_then(reason_text).map(str::to_string),
        }
    }
}

/// What the buyer is told for each reason; nothing for `other`, which says nothing.
pub fn reason_text(reason: CancelReason) -> Option<&'static str> {
    match reason {
        CancelReason::Customer => Some("Requested by you"),
        CancelReason::Inventory => Some("Items unavailable"),
        CancelReason::Fraud => Some("Suspected fraud"),
        CancelReason::Declined => Some("Payment declined"),
        CancelReason::Staff => Some("Cancelled by the store"),
        CancelReason::Other => None,
    }
}

/// `user`: in the staff templates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UserContext {
    pub email: String,
}

/// An order email: `order_confirmation`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OrderMail {
    pub shop: ShopContext,
    pub brand: BrandContext,
    pub order: OrderContext,
    pub customer: CustomerContext,
}

/// `order_shipped`, `order_tracking_updated`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShipmentMail {
    #[serde(flatten)]
    pub mail: OrderMail,
    pub fulfillment: FulfillmentContext,
}

/// `order_cancelled`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CancellationMail {
    #[serde(flatten)]
    pub mail: OrderMail,
    pub cancellation: CancellationContext,
}

/// `staff_new_order`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NewOrderMail {
    #[serde(flatten)]
    pub mail: OrderMail,
    /// The order in the admin; `null` without `server.admin_url`.
    pub admin_url: Option<String>,
}

/// `staff_invite`, `staff_password_reset`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StaffMail {
    pub shop: ShopContext,
    pub brand: BrandContext,
    pub user: UserContext,
    /// The admin's invite or reset link with the token (the bare token without
    /// `server.admin_url`).
    pub link: String,
    pub expires_at: TimeValue,
}

/// `customer_login_code` (design note 0034, §6): the code itself, how long it works.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CodeMail {
    pub shop: ShopContext,
    pub brand: BrandContext,
    /// Six digits.
    pub code: String,
    pub expires_at: TimeValue,
    /// How long the code works from now, rounded down, at least 1.
    pub expires_in_minutes: u64,
}

/// `customer_erasure_requested` (design note 0034, §3): when the erasure runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErasureMail {
    pub shop: ShopContext,
    pub brand: BrandContext,
    pub due_at: TimeValue,
}

/// One template's context, whichever it is: what rendering is given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum MailContext {
    Order(OrderMail),
    Shipment(ShipmentMail),
    Cancellation(CancellationMail),
    NewOrder(NewOrderMail),
    Staff(StaffMail),
    Code(CodeMail),
    Erasure(ErasureMail),
}

impl MailContext {
    /// The made-up context of `key`'s template (the layout's is an order
    /// confirmation's): two lines, a bank transfer's instructions, a tracking number,
    /// a few fields left `null`.
    pub fn sample(key: TemplateKey) -> Self {
        let mail = sample_order_mail();
        match key {
            TemplateKey::Layout | TemplateKey::OrderConfirmation => MailContext::Order(mail),
            TemplateKey::OrderShipped | TemplateKey::OrderTrackingUpdated => {
                MailContext::Shipment(ShipmentMail {
                    mail,
                    fulfillment: FulfillmentContext {
                        lines: vec![
                            ShippedLineContext {
                                title: "Linen shirt".to_string(),
                                variant: Some("M / Blue".to_string()),
                                quantity: 2,
                            },
                            ShippedLineContext {
                                title: "Ceramic mug".to_string(),
                                variant: None,
                                quantity: 1,
                            },
                        ],
                        tracking: vec![TrackingContext {
                            carrier: Some("DHL".to_string()),
                            number: "JD014600006281234567".to_string(),
                            url: Some(
                                "https://www.dhl.com/track?tracking-id=JD014600006281234567"
                                    .to_string(),
                            ),
                        }],
                    },
                })
            }
            TemplateKey::OrderCancelled => MailContext::Cancellation(CancellationMail {
                mail,
                cancellation: CancellationContext::of(Some(CancelReason::Inventory)),
            }),
            TemplateKey::StaffNewOrder => MailContext::NewOrder(NewOrderMail {
                admin_url: Some(format!(
                    "https://admin.example.com/orders/{}",
                    mail.order.id
                )),
                // Staff get no buyer's link (it carries the buyer's token).
                mail: OrderMail {
                    order: OrderContext {
                        url: None,
                        ..mail.order
                    },
                    ..mail
                },
            }),
            TemplateKey::StaffInvite => MailContext::Staff(sample_staff(
                "https://admin.example.com/invite?token=hypha_inv_sample",
            )),
            TemplateKey::StaffPasswordReset => MailContext::Staff(sample_staff(
                "https://admin.example.com/reset-password?token=hypha_pwr_sample",
            )),
            TemplateKey::CustomerErasureRequested => MailContext::Erasure(ErasureMail {
                shop: sample_shop(),
                brand: sample_brand(),
                // Ten days after the sample order.
                due_at: TimeValue::of(SAMPLE_PLACED_AT.plus_secs(10 * 24 * 3600), None),
            }),
            TemplateKey::CustomerLoginCode => MailContext::Code(CodeMail {
                shop: sample_shop(),
                brand: sample_brand(),
                code: "481 516".replace(' ', ""),
                expires_at: TimeValue::of(SAMPLE_PLACED_AT.plus_secs(10 * 60), None),
                expires_in_minutes: 10,
            }),
        }
    }

    /// `sample(key)` with every value that may be absent absent: no contact email or
    /// shop URL, no logo or footer, no order link, one line without variant or SKU, no
    /// shipping address, method or payment, no customer name, no tracking, no
    /// cancellation reason, no admin URL. A template saved must render against it too,
    /// or a real order without them would break it.
    pub fn sparse(key: TemplateKey) -> Self {
        let mut context = Self::sample(key);
        let thin = |mail: &mut OrderMail| {
            thin_branding(&mut mail.shop, &mut mail.brand);
            let order = &mut mail.order;
            order.url = None;
            order.lines.truncate(1);
            for line in &mut order.lines {
                line.variant = None;
                line.sku = None;
            }
            order.taxes_included = false;
            order.shipping_address = None;
            order.shipping_method = None;
            order.payment = None;
            mail.customer.name = None;
        };
        match &mut context {
            MailContext::Order(m) => thin(m),
            MailContext::Shipment(m) => {
                thin(&mut m.mail);
                m.fulfillment.lines.truncate(1);
                for line in &mut m.fulfillment.lines {
                    line.variant = None;
                }
                m.fulfillment.tracking.clear();
            }
            MailContext::Cancellation(m) => {
                thin(&mut m.mail);
                m.cancellation.reason = None;
            }
            MailContext::NewOrder(m) => {
                thin(&mut m.mail);
                m.admin_url = None;
            }
            MailContext::Staff(m) => thin_branding(&mut m.shop, &mut m.brand),
            MailContext::Code(m) => thin_branding(&mut m.shop, &mut m.brand),
            MailContext::Erasure(m) => thin_branding(&mut m.shop, &mut m.brand),
        }
        context
    }

    /// `sample(key)` with its objects present but their own optional fields absent: an
    /// address with only its required lines, a payment without instructions, tracking
    /// without carrier or URL. With `sparse`, what a saved template is checked against.
    pub fn sparse_fields(key: TemplateKey) -> Self {
        let mut context = Self::sample(key);
        let thin = |mail: &mut OrderMail| {
            if let Some(address) = &mut mail.order.shipping_address {
                address.company = None;
                address.line2 = None;
                address.province = None;
                address.postal_code = None;
                address.phone = None;
            }
            if let Some(payment) = &mut mail.order.payment {
                payment.instructions = None;
            }
        };
        match &mut context {
            MailContext::Order(m) => thin(m),
            MailContext::Shipment(m) => {
                thin(&mut m.mail);
                for tracking in &mut m.fulfillment.tracking {
                    tracking.carrier = None;
                    tracking.url = None;
                }
            }
            MailContext::Cancellation(m) => thin(&mut m.mail),
            MailContext::NewOrder(m) => thin(&mut m.mail),
            MailContext::Staff(_) | MailContext::Code(_) | MailContext::Erasure(_) => {}
        }
        context
    }

    /// What a template of `key` is checked against before it's saved: the sample, and
    /// the sample with what may be absent absent (`sparse`, `sparse_fields`).
    pub fn checks(key: TemplateKey) -> [MailContext; 3] {
        [
            Self::sample(key),
            Self::sparse(key),
            Self::sparse_fields(key),
        ]
    }

    /// Puts the store's own `shop` and `brand` in, so a preview of the sample looks
    /// like the store's emails.
    pub fn with_branding(mut self, shop: ShopContext, brand: BrandContext) -> Self {
        let (s, b) = match &mut self {
            MailContext::Order(m) => (&mut m.shop, &mut m.brand),
            MailContext::Shipment(m) => (&mut m.mail.shop, &mut m.mail.brand),
            MailContext::Cancellation(m) => (&mut m.mail.shop, &mut m.mail.brand),
            MailContext::NewOrder(m) => (&mut m.mail.shop, &mut m.mail.brand),
            MailContext::Staff(m) => (&mut m.shop, &mut m.brand),
            MailContext::Code(m) => (&mut m.shop, &mut m.brand),
            MailContext::Erasure(m) => (&mut m.shop, &mut m.brand),
        };
        *s = shop;
        *b = brand;
        self
    }
}

fn thin_branding(shop: &mut ShopContext, brand: &mut BrandContext) {
    shop.email = None;
    shop.url = None;
    brand.logo_url = None;
    brand.footer = None;
}

fn sample_shop() -> ShopContext {
    ShopContext {
        name: "Fern & Field".to_string(),
        email: Some("hello@fernandfield.example".to_string()),
        url: Some("https://shop.fernandfield.example".to_string()),
    }
}

fn sample_brand() -> BrandContext {
    BrandContext {
        logo_url: None,
        accent: NotificationSettings::DEFAULT_ACCENT.to_string(),
        footer: Some("Fern & Field d.o.o.\nIlica 1, 10000 Zagreb, Croatia".to_string()),
    }
}

fn eur(amount: i64) -> MoneyValue {
    // Small constants, well inside `Money::MAX_AMOUNT`.
    MoneyValue::of(Money::new(amount, Currency::EUR).unwrap_or(Money::zero(Currency::EUR)))
}

/// 18 Oct 2026, 12:05 UTC.
const SAMPLE_PLACED_AT: Timestamp = Timestamp::from_unix_micros(1_792_325_100_000_000);

fn sample_order_mail() -> OrderMail {
    let id = OrderId::from_uuid(uuid::Uuid::from_u128(
        0x0199_f3a2_8c00_7000_8000_0000_0000_1001,
    ));
    OrderMail {
        shop: sample_shop(),
        brand: sample_brand(),
        order: OrderContext {
            id: id.to_string(),
            number: "#1001".to_string(),
            url: Some(format!(
                "https://shop.fernandfield.example/order/{id}?token=hypha_ord_sample"
            )),
            email: "ana.horvat@example.com".to_string(),
            placed_at: TimeValue::of(SAMPLE_PLACED_AT, None),
            lines: vec![
                LineContext {
                    title: "Linen shirt".to_string(),
                    variant: Some("M / Blue".to_string()),
                    sku: Some("LS-M-BLU".to_string()),
                    quantity: 2,
                    unit_price: eur(4500),
                    total: eur(9000),
                },
                LineContext {
                    title: "Ceramic mug".to_string(),
                    variant: None,
                    sku: None,
                    quantity: 1,
                    unit_price: eur(1800),
                    total: eur(1800),
                },
            ],
            subtotal: eur(10800),
            shipping: eur(490),
            tax: eur(2258),
            total: eur(11290),
            taxes_included: true,
            shipping_address: Some(AddressContext {
                name: "Ana Horvat".to_string(),
                company: None,
                line1: "Ilica 42".to_string(),
                line2: Some("Stan 3".to_string()),
                city: "Zagreb".to_string(),
                province: None,
                postal_code: Some("10000".to_string()),
                country: "Croatia".to_string(),
                phone: Some("+385 91 234 5678".to_string()),
            }),
            shipping_method: Some("Standard delivery".to_string()),
            payment: Some(PaymentContext {
                method: "Bank transfer".to_string(),
                status: "pending".to_string(),
                instructions: Some(
                    "Pay to: Fern & Field d.o.o.\nIBAN: HR12 1001 0051 8630 0016 0\nReference: 1001"
                        .to_string(),
                ),
            }),
        },
        customer: CustomerContext {
            name: Some("Ana Horvat".to_string()),
        },
    }
}

fn sample_staff(link: &str) -> StaffMail {
    StaffMail {
        shop: sample_shop(),
        brand: sample_brand(),
        user: UserContext {
            email: "marko@fernandfield.example".to_string(),
        },
        link: link.to_string(),
        // Three days after the sample order.
        expires_at: TimeValue::of(SAMPLE_PLACED_AT.plus_secs(3 * 24 * 3600), None),
    }
}

/// The staff member a staff email is for, as templates see them.
pub fn user_context(email: &Email) -> UserContext {
    UserContext {
        email: email.as_str().to_string(),
    }
}
