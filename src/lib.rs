#![deny(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

#[cfg(feature = "client")]
pub mod client;
pub mod proto;
