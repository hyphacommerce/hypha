//! Extension points (design note 0029, AP3): the synchronous, versioned contracts Hypha
//! calls apps through mid-flow (`shipping.rates`, `payments.start`, `search.query`),
//! the call that signs, sends, caps, parses and records, and the call log
//! (`extension_calls`).
//!
//! A leaf domain: it depends on core only. The host sends the calls (`ExtensionCaller`)
//! and knows which app implements a point; callers apply the failure policy
//! (`apply_policy`).

pub mod call;
pub mod error;
pub mod point;
pub mod points;
pub mod record;

pub use call::{
    Answer, EXTENSION_HEADER, EXTENSION_VERSION_HEADER, Endpoint, ExtensionCaller,
    IDEMPOTENCY_KEY_HEADER, MAX_ANSWER_BYTES, OutgoingCall, STORE_HEADER, TransportError, call,
};
pub use error::{CommandError, ExtensionError};
pub use point::{Answered, ExtensionPoint, OnFailure, apply_policy};
pub use points::*;
pub use record::{
    CallOutcome, ExtensionCall, ExtensionCallRepo, ExtensionSettings, HasExtensionCalls, keys,
    list_extension_calls, prune_calls,
};
