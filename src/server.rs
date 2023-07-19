//! Implementation of a websocket server.
//!
//! This can be used in two ways:
//!   - By letting the library perform a HTTP/1.1 Upgrade handshake on an
//!     established stream, via [`Builder::accept`]
//!   - By performing the handshake yourself and then using [`Builder::serve`]
//!     to let it take over a websocket stream
use std::{future::poll_fn, pin::Pin};

use futures_core::Stream;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio_util::codec::{Decoder, Framed};

use crate::{
    proto::{Limits, Role},
    upgrade::client_request,
    Error, WebsocketStream,
};

/// HTTP/1.1 400 Bad Request response payload.
const BAD_REQUEST: &[u8] = b"HTTP/1.1 400 Bad Request\r\n\r\n";

/// Builder for websocket server connections.
pub struct Builder {
    /// Limits to impose on the websocket stream.
    limits: Limits,
    /// Whether to perform UTF-8 checks on incomplete frame parts.
    /// This is not technically required by the websocket specification and adds
    /// a decent amount of overhead, especially when messages are sent in
    /// small chops.
    ///
    /// Because this is SHOULD behaviour, it is enabled by default.
    fail_fast_on_invalid_utf8: bool,
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

impl Builder {
    /// Creates a [`Builder`] that can be used to create a [`WebsocketStream`]
    /// to receive messages at the server end.
    #[must_use]
    pub fn new() -> Self {
        Self {
            limits: Limits::default(),
            fail_fast_on_invalid_utf8: true,
        }
    }

    /// Sets the limits for the websocket stream.
    #[must_use]
    pub fn limits(mut self, limits: Limits) -> Self {
        self.limits = limits;

        self
    }

    /// Toggle whether to perform UTF-8 checks on incomplete frame parts.
    /// This is not technically required by the websocket specification and adds
    /// a decent amount of overhead, especially when messages are sent in
    /// small chops.
    ///
    /// Because this is SHOULD behaviour, it is enabled by default.
    #[must_use]
    pub fn fail_fast_on_invalid_utf8(mut self, value: bool) -> Self {
        self.fail_fast_on_invalid_utf8 = value;

        self
    }

    /// Perform a HTTP upgrade handshake on an already established stream and
    /// uses it to send and receive websocket messages.
    ///
    /// # Errors
    ///
    /// This method returns an [`Error`] if the handshake fails.
    pub async fn accept<S: AsyncRead + AsyncWrite + Unpin>(
        &self,
        stream: S,
    ) -> Result<WebsocketStream<S>, Error> {
        let mut framed = client_request::Codec {}.framed(stream);
        let reply = poll_fn(|cx| Pin::new(&mut framed).poll_next(cx)).await;
        let mut parts = framed.into_parts();

        match reply {
            Some(Ok(response)) => {
                parts.io.write_all(response.as_bytes()).await?;
                let framed = Framed::from_parts(parts);
                Ok(WebsocketStream::from_framed(
                    framed,
                    Role::Server,
                    self.limits,
                    self.fail_fast_on_invalid_utf8,
                ))
            }
            Some(Err(e)) => {
                parts.io.write_all(BAD_REQUEST).await?;

                Err(e)
            }
            None => Err(Error::NoUpgradeResponse),
        }
    }

    /// Takes over an already established stream and uses it to send and receive
    /// websocket messages.
    ///
    /// This does not perform a HTTP upgrade handshake.
    pub fn serve<S: AsyncRead + AsyncWrite + Unpin>(&self, stream: S) -> WebsocketStream<S> {
        WebsocketStream::from_raw_stream(
            stream,
            Role::Server,
            self.limits,
            self.fail_fast_on_invalid_utf8,
        )
    }
}
