#![deny(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

#[cfg(feature = "client")]
pub mod client;
pub mod error;
pub mod proto;
#[cfg(feature = "__tls")]
pub mod tls;

#[cfg(feature = "http-integration")]
pub use client::upgrade_request;
pub use error::Error;
pub use proto::{CloseCode, Message, OpCode, Role, WebsocketStream};
#[cfg(feature = "__tls")]
pub use tls::{Connector, MaybeTlsStream};
