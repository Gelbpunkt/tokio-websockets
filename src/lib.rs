#![deny(
    clippy::pedantic,
    clippy::missing_docs_in_private_items,
    clippy::missing_errors_doc,
    rustdoc::broken_intra_doc_links,
    warnings
)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![cfg_attr(feature = "nightly", feature(stdsimd))] // Required for AVX512 until stable
#![doc = include_str!("../README.md")]

// If the client or server implementation is enabled, at least one SHA1 backend
// is required.
#[cfg(all(
    any(feature = "client", feature = "server"),
    not(any(feature = "ring", feature = "openssl", feature = "sha1_smol"))
))]
compile_error!("client and server implementation require at least one SHA1 backend");

#[cfg(feature = "client")]
pub mod client;
pub mod error;
#[cfg(all(feature = "http-integration", feature = "client"))]
pub mod http;
mod mask;
pub mod proto;
#[cfg(feature = "client")]
mod rand;
#[cfg(feature = "server")]
pub mod server;
#[cfg(any(feature = "client", feature = "server"))]
mod sha;
pub mod tls;
#[cfg(any(feature = "client", feature = "server"))]
pub mod upgrade;
mod utf8;

#[cfg(feature = "client")]
pub use client::Builder as ClientBuilder;
pub use error::Error;
pub use proto::{CloseCode, Config, Limits, Message, Payload, WebSocketStream};
#[cfg(feature = "server")]
pub use server::Builder as ServerBuilder;
pub use tls::{Connector, MaybeTlsStream};

#[cfg(all(feature = "http-integration", feature = "client"))]
pub use self::http::upgrade_request;
