use core::time::Duration;

use hypha_auth::{IdentityRepo, Session, SessionClient, SessionRepo, TokenKind};
use hypha_cart::{Cart, CartRepo};
use hypha_core::{
    Actor, CartId, ChannelId, CountryCode, CustomerId, Email, MarketId, NotifyError, Scope,
};
use hypha_customer_accounts::{
    CommandError, CustomerEmail, LoggedIn, authenticate_customer, login_with_code, logout,
    logout_all, request_login_code,
};
use hypha_customers::{Customer, CustomerRepo};
use hypha_testkit::{InMemoryPorts, TestEnv};
use pollster::block_on;
use uuid::Uuid;

const NINA: &str = "nina@example.com";

fn channel() -> ChannelId {
    ChannelId::from_uuid(Uuid::from_u128(0xC1))
}

fn other_channel() -> ChannelId {
    ChannelId::from_uuid(Uuid::from_u128(0xC2))
}

fn world() -> (InMemoryPorts, TestEnv) {
    (InMemoryPorts::default(), TestEnv::default())
}

fn request(ports: &InMemoryPorts, env: &TestEnv, email: &str) -> Result<(), CommandError> {
    block_on(request_login_code(ports, env, email))
}

fn login(
    ports: &InMemoryPorts,
    env: &TestEnv,
    email: &str,
    code: &str,
    cart: Option<CartId>,
) -> Result<LoggedIn, CommandError> {
    block_on(login_with_code(
        ports,
        env,
        channel(),
        email,
        code,
        &SessionClient::default(),
        cart,
    ))
}

/// Asks for a code and signs in with it.
fn sign_in(ports: &InMemoryPorts, env: &TestEnv, email: &str) -> LoggedIn {
    request(ports, env, email).unwrap();
    let code = env.last_code().unwrap();
    login(ports, env, email, &code, None).unwrap()
}

fn customer_with(ports: &InMemoryPorts, email: &str) -> Option<Customer> {
    block_on(ports.customers.get_by_email(&Email::new(email).unwrap())).unwrap()
}

#[test]
fn asking_for_a_code_creates_the_customer_and_emails_a_six_digit_code() {
    let (ports, env) = world();
    request(&ports, &env, "  Nina@Example.com ").unwrap();

    let nina = customer_with(&ports, NINA).expect("created at the first request (AC2)");
    assert_eq!(nina.email().as_str(), NINA);
    let sent = env.sent_to_customers();
    assert_eq!(sent.len(), 1);
    let CustomerEmail::LoginCode {
        to,
        code,
        expires_at,
    } = &sent[0]
    else {
        panic!("not a code: {:?}", sent[0]);
    };
    assert_eq!(to.as_str(), NINA);
    assert_eq!(code.len(), 6);
    assert!(code.bytes().all(|b| b.is_ascii_digit()), "{code}");
    assert_eq!(*expires_at, env.now().plus(Duration::from_secs(600)));
}

#[test]
fn asking_again_within_a_minute_sends_nothing_and_later_replaces_the_code() {
    let (ports, env) = world();
    request(&ports, &env, NINA).unwrap();
    let first = env.last_code().unwrap();
    env.advance(59);
    request(&ports, &env, NINA).unwrap();
    assert_eq!(env.sent_to_customers().len(), 1, "too soon: nothing sent");

    env.advance(1);
    request(&ports, &env, NINA).unwrap();
    assert_eq!(env.sent_to_customers().len(), 2);
    assert_eq!(
        login(&ports, &env, NINA, &first, None).err(),
        Some(CommandError::InvalidCode),
        "the first code is replaced"
    );
    let second = env.last_code().unwrap();
    let me = login(&ports, &env, NINA, &second, None).unwrap();
    assert_eq!(
        Some(me.customer),
        customer_with(&ports, NINA),
        "still one customer"
    );
}

#[test]
fn nothing_is_sent_for_an_invalid_or_placeholder_address() {
    let (ports, env) = world();
    assert_eq!(request(&ports, &env, "not an email"), Ok(()));
    let placeholder = Email::placeholder(Uuid::from_u128(9));
    assert_eq!(request(&ports, &env, placeholder.as_str()), Ok(()));
    assert!(env.sent_to_customers().is_empty());
    assert_eq!(customer_with(&ports, placeholder.as_str()), None);
}

#[test]
fn a_sending_failure_is_reported() {
    let (ports, env) = world();
    env.fail_sending(true);
    assert_eq!(
        request(&ports, &env, NINA),
        Err(CommandError::Notify(NotifyError(
            "the test made sending fail".to_string()
        )))
    );
}

#[test]
fn an_existing_customer_gets_an_identity_and_a_changed_email_moves_it() {
    let (ports, env) = world();
    let mut nina = Customer::new(
        CustomerId::from_uuid(Uuid::from_u128(77)),
        Email::new(NINA).unwrap(),
    );
    block_on(ports.customers.insert(&mut nina)).unwrap();

    let first = sign_in(&ports, &env, NINA);
    assert_eq!(first.customer.id(), nina.id(), "no second customer");

    // Staff change her email; she signs in with the new one.
    let mut stored = block_on(ports.customers.get(nina.id())).unwrap().unwrap();
    stored
        .set_email(Email::new("nina@new.example").unwrap())
        .unwrap();
    block_on(ports.customers.update(&mut stored)).unwrap();
    let again = sign_in(&ports, &env, "nina@new.example");
    assert_eq!(again.customer.id(), nina.id());
    assert_eq!(customer_with(&ports, NINA), None, "her old address is free");
    let identity = block_on(
        ports
            .identities
            .get_by_principal(hypha_auth::Principal::Customer(nina.id())),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        identity.code_email().map(Email::as_str),
        Some("nina@new.example")
    );
    request(&ports, &env, NINA).unwrap();
    let newcomer = customer_with(&ports, NINA).expect("the old address is a new customer now");
    assert_ne!(newcomer.id(), nina.id());
}

#[test]
fn signing_in_uses_the_code_up_and_opens_a_thirty_day_session() {
    let (ports, env) = world();
    let logged_in = sign_in(&ports, &env, NINA);
    let token = logged_in.token.expose().to_string();
    assert_eq!(TokenKind::of(&token), Some(TokenKind::CustomerSession));
    assert!(token.starts_with("hypha_cs_"));
    assert_eq!(
        logged_in.session.expires_at(),
        env.now().plus(Duration::from_secs(30 * 24 * 3600))
    );
    assert!(logged_in.session.is_active(env.now()));

    let code = env.last_code().unwrap();
    assert_eq!(
        login(&ports, &env, NINA, &code, None).err(),
        Some(CommandError::InvalidCode),
        "a code works once"
    );

    let me = block_on(authenticate_customer(&ports, &env, channel(), &token)).unwrap();
    assert_eq!(me.customer.id(), logged_in.customer.id());
    assert_eq!(me.ctx.actor(), Actor::Customer(logged_in.customer.id()));
    assert_eq!(me.ctx.customer(), Some(logged_in.customer.id()));
    assert_eq!(me.ctx.channel(), Some(channel()));
    assert!(!me.ctx.is_staff());
    assert!(me.ctx.has_scope(Scope::CartsWrite));
    assert!(!me.ctx.has_scope(Scope::CustomersRead));
}

#[test]
fn wrong_expired_and_unknown_codes_answer_the_same() {
    let (ports, env) = world();
    assert_eq!(
        login(&ports, &env, NINA, "123456", None).err(),
        Some(CommandError::InvalidCode),
        "nobody asked"
    );
    assert_eq!(
        login(&ports, &env, "not an email", "123456", None).err(),
        Some(CommandError::InvalidCode)
    );
    request(&ports, &env, NINA).unwrap();
    let code = env.last_code().unwrap();
    let wrong = if code == "000000" { "000001" } else { "000000" };
    assert_eq!(
        login(&ports, &env, NINA, wrong, None).err(),
        Some(CommandError::InvalidCode)
    );
    env.advance(600);
    assert_eq!(
        login(&ports, &env, NINA, &code, None).err(),
        Some(CommandError::InvalidCode),
        "expired"
    );
}

#[test]
fn the_fifth_wrong_guess_voids_the_code() {
    let (ports, env) = world();
    request(&ports, &env, NINA).unwrap();
    let code = env.last_code().unwrap();
    let wrong = if code == "000000" { "000001" } else { "000000" };
    for _ in 0..4 {
        assert!(login(&ports, &env, NINA, wrong, None).is_err());
    }
    // Four wrong: the right one would still work. Make it five.
    assert!(login(&ports, &env, NINA, wrong, None).is_err());
    assert_eq!(
        login(&ports, &env, NINA, &code, None).err(),
        Some(CommandError::InvalidCode)
    );
    // A new code works again.
    env.advance(60);
    request(&ports, &env, NINA).unwrap();
    let fresh = env.last_code().unwrap();
    assert!(login(&ports, &env, NINA, &fresh, None).is_ok());
}

fn open_cart(ports: &InMemoryPorts, n: u128, channel: ChannelId) -> CartId {
    let id = CartId::from_uuid(Uuid::from_u128(n));
    let mut cart = Cart::new(
        id,
        MarketId::from_uuid(Uuid::from_u128(0xA1)),
        CountryCode::new("HR").unwrap(),
        channel,
    );
    block_on(ports.carts.insert(&mut cart)).unwrap();
    id
}

#[test]
fn signing_in_links_the_request_cart_but_not_another_channels() {
    let (ports, env) = world();
    let mine = open_cart(&ports, 1, channel());
    let elsewhere = open_cart(&ports, 2, other_channel());

    request(&ports, &env, NINA).unwrap();
    let code = env.last_code().unwrap();
    let me = login(&ports, &env, NINA, &code, Some(mine)).unwrap();
    let cart = block_on(ports.carts.get(mine)).unwrap().unwrap();
    assert_eq!(cart.customer(), Some(me.customer.id()));
    assert_eq!(cart.email().map(Email::as_str), Some(NINA));

    env.advance(60);
    request(&ports, &env, NINA).unwrap();
    let code = env.last_code().unwrap();
    login(&ports, &env, NINA, &code, Some(elsewhere)).unwrap();
    let cart = block_on(ports.carts.get(elsewhere)).unwrap().unwrap();
    assert_eq!(cart.customer(), None, "not this channel's cart");

    env.advance(60);
    request(&ports, &env, NINA).unwrap();
    let code = env.last_code().unwrap();
    let missing = CartId::from_uuid(Uuid::from_u128(99));
    assert!(
        login(&ports, &env, NINA, &code, Some(missing)).is_ok(),
        "an unknown cart is ignored"
    );
}

#[test]
fn a_session_is_refreshed_on_use_about_daily_and_dies_unused() {
    let (ports, env) = world();
    let token = sign_in(&ports, &env, NINA).token.expose().to_string();
    let day = 24 * 3600;
    let start = env.now();

    env.advance(day - 1);
    let me = block_on(authenticate_customer(&ports, &env, channel(), &token)).unwrap();
    assert_eq!(
        me.session.expires_at(),
        start.plus(Duration::from_secs(30 * day as u64)),
        "within the first day: only the use is recorded"
    );
    assert_eq!(me.session.last_used_at(), env.now());

    env.advance(2);
    let me = block_on(authenticate_customer(&ports, &env, channel(), &token)).unwrap();
    assert_eq!(
        me.session.expires_at(),
        env.now().plus(Duration::from_secs(30 * day as u64)),
        "after a day: extended"
    );
    let stored: Session = block_on(ports.sessions.get(me.session.id()))
        .unwrap()
        .unwrap();
    assert_eq!(stored.expires_at(), me.session.expires_at());

    env.advance(30 * day);
    assert_eq!(
        block_on(authenticate_customer(&ports, &env, channel(), &token)).err(),
        Some(CommandError::InvalidToken),
        "thirty days unused"
    );
}

#[test]
fn a_staff_or_malformed_token_is_invalid_and_an_erased_customer_is_gone() {
    let (ports, env) = world();
    for bad in ["", "hypha_sess_0000", "hypha_cs_abc", "hypha_cs_"] {
        assert_eq!(
            block_on(authenticate_customer(&ports, &env, channel(), bad)).err(),
            Some(CommandError::InvalidToken),
            "{bad}"
        );
    }
    let me = sign_in(&ports, &env, NINA);
    let token = me.token.expose().to_string();
    let mut customer = block_on(ports.customers.get(me.customer.id()))
        .unwrap()
        .unwrap();
    customer.erase(env.now());
    block_on(ports.customers.update(&mut customer)).unwrap();
    assert_eq!(
        block_on(authenticate_customer(&ports, &env, channel(), &token)).err(),
        Some(CommandError::CustomerNotFound(me.customer.id()))
    );
}

#[test]
fn logout_ends_one_session_and_logout_all_every_one() {
    let (ports, env) = world();
    let a = sign_in(&ports, &env, NINA);
    env.advance(60);
    let b = sign_in(&ports, &env, NINA);
    env.advance(60);
    let c = sign_in(&ports, &env, NINA);
    let (ta, tb, tc) = (
        a.token.expose().to_string(),
        b.token.expose().to_string(),
        c.token.expose().to_string(),
    );
    let auth = |t: &str| block_on(authenticate_customer(&ports, &env, channel(), t));

    block_on(logout(&ports, &env, a.session)).unwrap();
    assert_eq!(auth(&ta).err(), Some(CommandError::InvalidToken));
    assert!(auth(&tb).is_ok());

    block_on(logout_all(&ports, &env, &c.session)).unwrap();
    assert_eq!(auth(&tb).err(), Some(CommandError::InvalidToken));
    assert_eq!(auth(&tc).err(), Some(CommandError::InvalidToken));

    // Someone else's sessions aren't touched.
    let other = sign_in(&ports, &env, "ivo@example.com");
    let d = sign_in(&ports, &env, NINA);
    block_on(logout_all(&ports, &env, &d.session)).unwrap();
    assert!(auth(other.token.expose()).is_ok());
}

#[test]
fn own_gives_the_customer_their_own_scopes_in_their_channel_and_nobody_else_anything() {
    use hypha_core::Ctx;
    use hypha_customer_accounts::own;
    let me = CustomerId::from_uuid(Uuid::from_u128(5));
    let ctx = Ctx::in_channel(Actor::Customer(me), Scope::STOREFRONT.to_vec(), channel());
    let owned = own(&ctx).unwrap();
    assert_eq!(owned.customer, me);
    assert_eq!(owned.ctx.actor(), Actor::Customer(me));
    assert_eq!(owned.ctx.channel(), Some(channel()));
    for scope in [
        Scope::CustomersRead,
        Scope::CustomersWrite,
        Scope::CustomersErase,
        Scope::OrdersRead,
        Scope::CustomerDataProfile,
        Scope::CartsWrite,
    ] {
        assert!(owned.ctx.has_scope(scope), "{scope}");
    }
    assert!(!owned.ctx.has_scope(Scope::ProductsWrite));
    assert_eq!(
        own(&Ctx::new(Actor::System, Scope::ALL.to_vec())).err(),
        Some(CommandError::AccountRequired)
    );
}
