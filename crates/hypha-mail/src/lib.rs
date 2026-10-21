//! Mail (design note 0028, NT2): the workflow that turns orders, fulfilments, payments
//! and the store into emails. It builds each template's context (`context.rs`, the
//! templates' variables, a contract), queues emails on order events with the `Mailer`
//! handler, sends what's queued (the send job's steps, `send.rs`),
//! and holds the commands that need other domains: saving the settings with the logo's
//! asset usage, saving templates checked against their samples, previews, test sends,
//! resends and the staff flows' direct emails.
//!
//! What it needs besides repositories comes in through `MailEnv` (the sender, links,
//! the store), which hosts implement once.

pub mod commands;
pub mod context;
pub mod env;
pub mod error;
pub mod format;
pub mod handler;
pub mod send;

pub use commands::{
    StaffMailer, TemplateInput, UpdateSettings, preview, resend, save_template, send_customer,
    send_staff, send_test, update_settings,
};
pub use context::MailContext;
pub use env::MailEnv;
pub use error::CommandError;
pub use format::{MoneyValue, TimeValue, format_money};
pub use handler::{MailPorts, Mailer, order_context, queue_event};
pub use send::{
    Attempt, Branding, Message, Rendering, SendRun, Sources, branding, has_time, lease_due,
    record_attempt, render_for_sending, send_due, send_leased, send_now,
};
