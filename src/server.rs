//! Implementation of a websocket server.
//!
//! This can be used in two ways:
//!   - By letting the library perform a HTTP/1.1 Upgrade handshake on an
//!     established stream, via [`Builder::accept`]
//!   - By performing the handshake yourself and then using [`Builder::serve`]
//!     to let it take over a websocket stream
use std::{future::poll_fn, io, pin::Pin};

use futures_core::Stream;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio_util::codec::{Decoder, Framed};

use crate::{
    proto::{Config, Limits, Role},
    upgrade::client_request,
    Error, WebsocketStream,
};

/// HTTP/1.1 400 Bad Request response payload.
const BAD_REQUEST: &[u8] = b"HTTP/1.1 400 Bad Request\r\n\r\n";

/// Builder for websocket server connections.
pub struct Builder {
    /// Configuration for the websocket stream.
    config: Config,
    /// Limits to impose on the websocket stream.
    limits: Limits,
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
            config: Config::default(),
            limits: Limits::default(),
        }
    }

    /// Sets the configuration for the websocket stream.
    #[must_use]
    pub fn config(mut self, config: Config) -> Self {
        self.config = config;

        self
    }

    /// Sets the limits for the websocket stream.
    #[must_use]
    pub fn limits(mut self, limits: Limits) -> Self {
        self.limits = limits;

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
                    self.config,
                    self.limits,
                ))
            }
            Some(Err(e)) => {
                parts.io.write_all(BAD_REQUEST).await?;

                Err(e)
            }
            None => Err(Error::Io(io::ErrorKind::UnexpectedEof.into())),
        }
    }

    /// Takes over an already established stream and uses it to send and receive
    /// websocket messages.
    ///
    /// This does not perform a HTTP upgrade handshake.
    pub fn serve<S: AsyncRead + AsyncWrite + Unpin>(&self, stream: S) -> WebsocketStream<S> {
        WebsocketStream::from_raw_stream(stream, Role::Server, self.config, self.limits)
    }
}
