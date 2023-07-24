//! This module contains a correct and complete implementation of [RFC6455](https://datatracker.ietf.org/doc/html/rfc6455).
//!
//! Any extensions are currently not implemented.
pub(crate) use self::types::Role;
pub use self::{
    error::ProtocolError,
    stream::WebsocketStream,
    types::{CloseCode, Limits, Message},
};

mod codec;
mod error;
mod stream;
mod types;
