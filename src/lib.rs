#![deny(
    clippy::pedantic,
    clippy::missing_docs_in_private_items,
    clippy::missing_errors_doc
)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![doc = include_str!("../README.md")]

#[cfg(feature = "client")]
pub mod client;
pub mod error;
#[cfg(all(feature = "http-integration", feature = "client"))]
pub mod http;
mod mask;
pub mod proto;
#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "client")]
mod sha;
pub mod tls;
#[cfg(any(feature = "client", feature = "server"))]
mod upgrade;
mod utf8;

#[cfg(feature = "client")]
pub use client::Builder as ClientBuilder;
pub use error::Error;
pub use proto::{CloseCode, Message, OpCode, WebsocketStream};
#[cfg(feature = "server")]
pub use server::Builder as ServerBuilder;
pub use tls::{Connector, MaybeTlsStream};

#[cfg(all(feature = "http-integration", feature = "client"))]
pub use self::http::upgrade_request;
