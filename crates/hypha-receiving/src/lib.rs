//! Receiving: goods arriving against purchase orders. A workflow over purchasing and
//! inventory: each receipt writes the order's counts, the receipt and the stock it brought
//! (with its cost and ledger rows) in one batch, and taking one back undoes all three.

pub mod commands;

pub use commands::{
    CancelledReceipt, CommandError, RECEIVE, Receive, ReceiveLine, Received, ReceivingPorts,
    cancel_receipt, receive,
};
