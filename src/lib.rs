#![deny(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

#[cfg(feature = "client")]
pub mod client;
pub mod proto;

#[cfg(feature = "client")]
pub use client::upgrade_request;

pub use proto::{CloseCode, Message, OpCode, Role, WebsocketStream};
