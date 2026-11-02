//! Validation apps (`checkout.validate`): what the store's apps say about a cart, asked as
//! the shopper goes (the address set, an option chosen, checkout) and kept with the cart,
//! so an app is asked again only when what it would be sent changes. Changing lines asks
//! nobody: the answers go stale, and checkout asks again.

use futures_util::future::join_all;
use hypha_cart::{CartRepo, HasCarts, PricedCart};
use hypha_core::{
    AppCheck, AppKey, CartId, Clock, Ctx, Message, MessagePath, RepoError, Scope, ValidationStep,
};

use crate::{CartChecks, CommandError};

/// A validation app checkout asks, with what it would be sent about a cart: the host's
/// body (redacted for that app) and its digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Validator<B> {
    pub app: AppKey,
    /// Whether checkout stops when it can't be asked (`validation_unavailable`).
    pub block_on_failure: bool,
    /// The digest of `body`, which a kept answer is matched by: the step isn't in it, so an
    /// answer holds for the same cart whatever the step.
    pub digest: String,
    pub body: B,
}

/// The store's validation apps, given by the host like `RateProvider`. No `Send` bound,
/// like every port.
#[allow(async_fn_in_trait)]
pub trait CheckoutValidators {
    /// What one app is sent: the host's wire body.
    type Body;

    /// The apps asked about `cart` now (answering `checkout.validate` and enabled in the
    /// store's settings), in a stable order, each with what it would be sent.
    async fn validators(&self, cart: &PricedCart) -> Result<Vec<Validator<Self::Body>>, RepoError>;

    /// Asks one app at `step`: its messages, each with the app as its source, or why it
    /// couldn't answer (no answer, an error, an answer the point refuses).
    async fn ask(
        &self,
        validator: &Validator<Self::Body>,
        step: ValidationStep,
    ) -> Result<Vec<Message>, String>;
}

impl<T: CheckoutValidators + ?Sized> CheckoutValidators for &T {
    type Body = T::Body;

    async fn validators(&self, cart: &PricedCart) -> Result<Vec<Validator<T::Body>>, RepoError> {
        (**self).validators(cart).await
    }

    async fn ask(
        &self,
        validator: &Validator<T::Body>,
        step: ValidationStep,
    ) -> Result<Vec<Message>, String> {
        (**self).ask(validator, step).await
    }
}

/// No validation apps: hosts' tests and stores that have none.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoValidators;

impl CheckoutValidators for NoValidators {
    type Body = ();

    async fn validators(&self, _cart: &PricedCart) -> Result<Vec<Validator<()>>, RepoError> {
        Ok(Vec::new())
    }

    async fn ask(
        &self,
        _validator: &Validator<()>,
        _step: ValidationStep,
    ) -> Result<Vec<Message>, String> {
        Ok(Vec::new())
    }
}

/// The answers `cart` should keep after `step`: each app whose kept answer isn't for what
/// it would be sent now (or was a failure) is asked, all at once; an app no longer asked
/// loses its answer. `None` when that changes nothing.
pub(crate) async fn refreshed_checks<V: CheckoutValidators>(
    validators: &V,
    clock: &impl Clock,
    cart: &PricedCart,
    step: ValidationStep,
) -> Result<Option<Vec<AppCheck>>, RepoError> {
    let kept = &cart.app_checks;
    let current = validators.validators(cart).await?;
    let holds =
        |v: &Validator<V::Body>| kept.iter().any(|k| k.app == v.app && k.answers(&v.digest));
    let stale: Vec<&Validator<V::Body>> = current.iter().filter(|v| !holds(v)).collect();
    let answers = join_all(stale.iter().map(|v| validators.ask(v, step))).await;
    let now = clock.now();
    let mut asked = stale.iter().zip(answers);
    let checks: Vec<AppCheck> = current
        .iter()
        .filter_map(|v| {
            if holds(v) {
                return kept.iter().find(|k| k.app == v.app).cloned();
            }
            let (_, answer) = asked.next()?;
            let (messages, failure) = match answer {
                Ok(messages) => (messages, None),
                Err(why) => (Vec::new(), Some(why)),
            };
            Some(AppCheck {
                app: v.app.clone(),
                digest: v.digest.clone(),
                messages,
                failure,
                checked_at: now,
            })
        })
        .collect();
    Ok((checks != *kept).then_some(checks))
}

/// What the apps asked now say about `cart`, from what it keeps: each app's last messages
/// (stale ones too: checkout asks again before it decides), but those pointing at a line
/// or group the cart no longer has; for an app that failed, nothing, or
/// `validation_unavailable` when its failure blocks checkout. An app never asked yet says
/// nothing until it is.
pub(crate) fn app_messages<B>(current: &[Validator<B>], cart: &PricedCart) -> Vec<Message> {
    let in_cart = |m: &Message| match m.path() {
        MessagePath::Line(i) => usize::try_from(i).is_ok_and(|i| i < cart.lines.len()),
        MessagePath::ShippingGroup(i) => {
            usize::try_from(i).is_ok_and(|i| i < cart.shipping_groups.len())
        }
        _ => true,
    };
    let mut messages = Vec::new();
    for v in current {
        let Some(kept) = cart.app_checks.iter().find(|k| k.app == v.app) else {
            continue;
        };
        if kept.failure.is_some() {
            if v.block_on_failure {
                messages.push(Message::validation_unavailable(&v.app));
            }
            continue;
        }
        messages.extend(kept.messages.iter().filter(|m| in_cart(m)).cloned());
    }
    messages
}

/// Asks the store's validation apps about a cart after a change at `step` (its address
/// set, an option chosen) and keeps their answers with it; `cart` is the cart as just
/// priced, and gains them. An app that fails is kept as failed, to be asked again. A write
/// that loses a race keeps nothing: the answers are asked for again at checkout.
pub async fn validate_cart<S: HasCarts, K: CartChecks>(
    ctx: &Ctx,
    ports: &S,
    checks: &K,
    clock: &impl Clock,
    cart: &mut PricedCart,
    step: ValidationStep,
) -> Result<(), CommandError> {
    ctx.require(Scope::CartsWrite)?;
    if cart.order.is_some() {
        return Ok(());
    }
    let Some(fresh) = refreshed_checks(checks.validators(), clock, cart, step).await? else {
        return Ok(());
    };
    keep(ports, cart.id, fresh.clone()).await?;
    cart.app_checks = fresh;
    Ok(())
}

async fn keep<S: HasCarts>(
    ports: &S,
    id: CartId,
    checks: Vec<AppCheck>,
) -> Result<(), CommandError> {
    let Some(mut stored) = ports.carts().get(id).await? else {
        return Ok(());
    };
    if stored.set_app_checks(checks).is_err() {
        // Checked out meanwhile: its answers no longer matter.
        return Ok(());
    }
    match ports.carts().update(&mut stored).await {
        Ok(()) | Err(RepoError::StaleVersion) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use hypha_core::{
        ChannelId, CountryCode, Currency, MarketId, MessageCode, MessageSource, Money, Severity,
        Timestamp,
    };
    use uuid::Uuid;

    use super::*;

    struct Fixed;
    impl Clock for Fixed {
        fn now(&self) -> Timestamp {
            Timestamp::from_unix_micros(7)
        }
    }

    fn app(name: &str) -> AppKey {
        AppKey::new(&format!("app_{name}")).unwrap()
    }

    fn message(app: &AppKey, path: MessagePath) -> Message {
        Message::new(
            Severity::Error,
            MessageCode::new("alcohol_restricted").unwrap(),
            path,
            "We can't ship alcohol here.",
            MessageSource::App { app: app.clone() },
        )
        .unwrap()
    }

    fn cart(checks: Vec<AppCheck>) -> PricedCart {
        let eur = Money::new(0, Currency::EUR).unwrap();
        PricedCart {
            id: CartId::from_uuid(Uuid::from_u128(1)),
            market: MarketId::from_uuid(Uuid::from_u128(2)),
            currency: Currency::EUR,
            country: CountryCode::new("HR").unwrap(),
            channel: ChannelId::from_uuid(Uuid::from_u128(3)),
            email: None,
            customer: None,
            order: None,
            lines: Vec::new(),
            taxes_included: true,
            subtotal: eur,
            shipping_address: None,
            address_check: None,
            app_checks: checks,
            shipping_groups: Vec::new(),
            shipping_total: eur,
            discounts: Vec::new(),
            discount_total: eur,
            discount_codes: Vec::new(),
            tax_total: eur,
            total: eur,
            payment_method: None,
            marketing_opt_in: false,
        }
    }

    fn kept(app: &AppKey, digest: &str, messages: Vec<Message>, failure: Option<&str>) -> AppCheck {
        AppCheck {
            app: app.clone(),
            digest: digest.to_string(),
            messages,
            failure: failure.map(str::to_string),
            checked_at: Timestamp::from_unix_micros(1),
        }
    }

    fn validator(app: &AppKey, digest: &str, blocks: bool) -> Validator<()> {
        Validator {
            app: app.clone(),
            block_on_failure: blocks,
            digest: digest.to_string(),
            body: (),
        }
    }

    /// Answers each app with what `answers` says, recording whom it asked.
    struct Apps {
        current: Vec<Validator<()>>,
        answers: Vec<(AppKey, Result<Vec<Message>, String>)>,
        asked: RefCell<Vec<AppKey>>,
    }

    impl CheckoutValidators for Apps {
        type Body = ();

        async fn validators(&self, _cart: &PricedCart) -> Result<Vec<Validator<()>>, RepoError> {
            Ok(self.current.clone())
        }

        async fn ask(
            &self,
            v: &Validator<()>,
            _step: ValidationStep,
        ) -> Result<Vec<Message>, String> {
            self.asked.borrow_mut().push(v.app.clone());
            self.answers
                .iter()
                .find(|(a, _)| a == &v.app)
                .map(|(_, r)| r.clone())
                .unwrap_or(Ok(Vec::new()))
        }
    }

    fn refresh(apps: &Apps, cart: &PricedCart) -> Option<Vec<AppCheck>> {
        pollster::block_on(refreshed_checks(
            apps,
            &Fixed,
            cart,
            ValidationStep::Address,
        ))
        .unwrap()
    }

    #[test]
    fn an_app_never_asked_is_asked() {
        let a = app("a");
        let apps = Apps {
            current: vec![validator(&a, "d1", false)],
            answers: vec![(a.clone(), Ok(vec![message(&a, MessagePath::Cart)]))],
            asked: RefCell::default(),
        };
        assert_eq!(
            refresh(&apps, &cart(Vec::new())),
            Some(vec![AppCheck {
                checked_at: Timestamp::from_unix_micros(7),
                ..kept(&a, "d1", vec![message(&a, MessagePath::Cart)], None)
            }])
        );
    }

    #[test]
    fn an_answer_for_the_same_request_isnt_asked_again() {
        let a = app("a");
        let apps = Apps {
            current: vec![validator(&a, "d1", false)],
            answers: Vec::new(),
            asked: RefCell::default(),
        };
        assert_eq!(
            refresh(&apps, &cart(vec![kept(&a, "d1", Vec::new(), None)])),
            None
        );
        assert!(apps.asked.borrow().is_empty());
    }

    #[test]
    fn a_stale_answer_is_asked_again() {
        let a = app("a");
        let apps = Apps {
            current: vec![validator(&a, "d2", false)],
            answers: Vec::new(),
            asked: RefCell::default(),
        };
        let fresh = refresh(&apps, &cart(vec![kept(&a, "d1", Vec::new(), None)])).unwrap();
        assert_eq!(fresh[0].digest, "d2");
        assert_eq!(*apps.asked.borrow(), vec![a]);
    }

    #[test]
    fn a_failure_is_asked_again() {
        let a = app("a");
        let apps = Apps {
            current: vec![validator(&a, "d1", false)],
            answers: Vec::new(),
            asked: RefCell::default(),
        };
        let fresh = refresh(
            &apps,
            &cart(vec![kept(&a, "d1", Vec::new(), Some("timeout"))]),
        );
        assert_eq!(fresh.unwrap()[0].failure, None);
    }

    #[test]
    fn a_failure_is_kept_with_why() {
        let a = app("a");
        let apps = Apps {
            current: vec![validator(&a, "d1", false)],
            answers: vec![(a.clone(), Err("timeout".to_string()))],
            asked: RefCell::default(),
        };
        let fresh = refresh(&apps, &cart(Vec::new())).unwrap();
        assert_eq!(fresh[0].failure.as_deref(), Some("timeout"));
        assert!(fresh[0].messages.is_empty());
    }

    #[test]
    fn only_the_stale_app_is_asked() {
        let (a, b) = (app("a"), app("b"));
        let apps = Apps {
            current: vec![validator(&a, "d1", false), validator(&b, "d2", false)],
            answers: Vec::new(),
            asked: RefCell::default(),
        };
        let fresh = refresh(&apps, &cart(vec![kept(&a, "d1", Vec::new(), None)])).unwrap();
        assert_eq!(*apps.asked.borrow(), vec![b.clone()]);
        assert_eq!(
            fresh.iter().map(|c| c.app.clone()).collect::<Vec<_>>(),
            vec![a, b]
        );
    }

    #[test]
    fn an_app_no_longer_asked_loses_its_answer() {
        let a = app("a");
        let apps = Apps {
            current: Vec::new(),
            answers: Vec::new(),
            asked: RefCell::default(),
        };
        assert_eq!(
            refresh(&apps, &cart(vec![kept(&a, "d1", Vec::new(), None)])),
            Some(Vec::new())
        );
    }

    #[test]
    fn an_apps_messages_are_the_carts() {
        let a = app("a");
        let m = message(&a, MessagePath::Email);
        let c = cart(vec![kept(&a, "d1", vec![m.clone()], None)]);
        assert_eq!(app_messages(&[validator(&a, "d1", false)], &c), vec![m]);
    }

    #[test]
    fn a_stale_answer_still_shows() {
        let a = app("a");
        let m = message(&a, MessagePath::Email);
        let c = cart(vec![kept(&a, "d1", vec![m.clone()], None)]);
        assert_eq!(app_messages(&[validator(&a, "d2", false)], &c), vec![m]);
    }

    #[test]
    fn a_message_on_a_line_the_cart_lost_is_dropped() {
        let a = app("a");
        let c = cart(vec![kept(
            &a,
            "d1",
            vec![message(&a, MessagePath::Line(0))],
            None,
        )]);
        assert!(app_messages(&[validator(&a, "d1", false)], &c).is_empty());
    }

    #[test]
    fn an_app_not_asked_now_says_nothing() {
        let a = app("a");
        let c = cart(vec![kept(
            &a,
            "d1",
            vec![message(&a, MessagePath::Cart)],
            None,
        )]);
        assert!(app_messages::<()>(&[], &c).is_empty());
    }

    #[test]
    fn a_failure_that_doesnt_block_says_nothing() {
        let a = app("a");
        let c = cart(vec![kept(&a, "d1", Vec::new(), Some("timeout"))]);
        assert!(app_messages(&[validator(&a, "d1", false)], &c).is_empty());
    }

    #[test]
    fn a_failure_that_blocks_says_validation_unavailable() {
        let a = app("a");
        let c = cart(vec![kept(&a, "d1", Vec::new(), Some("timeout"))]);
        assert_eq!(
            app_messages(&[validator(&a, "d1", true)], &c),
            vec![Message::validation_unavailable(&a)]
        );
    }
}
