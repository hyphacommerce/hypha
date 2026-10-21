//! Customer accounts (design note 0034): shoppers signing in to the storefront with a
//! code emailed to them, their sessions, and what they may do while signed in.
//!
//! A composing crate: `hypha-auth` holds the identity (`Principal::Customer`, a
//! `Credential::EmailCode`) and the session (`TokenKind::CustomerSession`),
//! `hypha-customers` the customer, `hypha-cart` the cart linked at login.

pub mod commands;
pub mod settings;

pub use commands::{
    AccountEnv, AccountPorts, CommandError, CustomerEmail, CustomerSession, LoggedIn,
    authenticate_customer, login_with_code, logout, logout_all, request_login_code,
};
pub use settings::{AccountSettings, HasAccountSettings};
