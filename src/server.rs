//! Implementation of a WebSocket server.
//!
//! This can be used in two ways:
//!   - By letting the library perform a HTTP/1.1 Upgrade handshake on an
//!     established stream, via [`Builder::accept`]
//!   - By performing the handshake yourself and then using [`Builder::serve`]
//!     to let it take over a WebSocket stream
use std::{future::poll_fn, io, pin::Pin};

use futures_core::Stream;
use http::{HeaderName, HeaderValue, header};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio_util::codec::FramedRead;

use crate::{
    Error, WebSocketStream,
    proto::{Config, Limits, Role},
    upgrade::{Handshake, client_request},
};

/// HTTP/1.1 400 Bad Request response payload.
const BAD_REQUEST: &[u8] = b"HTTP/1.1 400 Bad Request\r\n\r\n";

/// List of headers added by the server which will cause an error
/// if added by the user:
///
/// - `host`
/// - `upgrade`
/// - `connection`
/// - `sec-websocket-accept`
pub const DISALLOWED_HEADERS: &[HeaderName] = &[
    header::UPGRADE,
    header::CONNECTION,
    header::SEC_WEBSOCKET_ACCEPT,
];

/// Builder for WebSocket server connections.
pub struct Builder {
    /// Configuration for the WebSocket stream.
    config: Config,
    /// Limits to impose on the WebSocket stream.
    limits: Limits,
    /// Configuration for the HTTP upgrade handshake.
    handshake: Handshake,
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

impl Builder {
    /// Creates a [`Builder`] that can be used to create a [`WebSocketStream`]
    /// to receive messages at the server end.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: Config::default(),
            limits: Limits::default(),
            handshake: Handshake::default(),
        }
    }

    /// Sets the configuration for the WebSocket stream.
    #[must_use]
    pub fn config(mut self, config: Config) -> Self {
        self.config = config;

        self
    }

    /// Sets the limits for the WebSocket stream.
    #[must_use]
    pub fn limits(mut self, limits: Limits) -> Self {
        self.limits = limits;

        self
    }

    /// Adds an extra HTTP header to the switching protocols response.
    ///
    /// # Errors
    ///
    /// Returns [`Error::DisallowedHeader`] if the header is in
    /// the [`DISALLOWED_HEADERS`] list.
    pub fn add_header(mut self, name: HeaderName, value: HeaderValue) -> Result<Self, Error> {
        if DISALLOWED_HEADERS.contains(&name) {
            return Err(Error::DisallowedHeader);
        }
        self.handshake.headers.insert(name, value);

        Ok(self)
    }

    /// Sets the maximum number of HTTP headers to parse from the client's
    /// upgrade request. The default is 64.
    #[must_use]
    pub fn max_headers(mut self, max: usize) -> Self {
        self.handshake.max_headers = max;

        self
    }

    /// Perform a HTTP upgrade handshake on an already established stream and
    /// uses it to send and receive WebSocket messages.
    ///
    /// # Errors
    ///
    /// This method returns an [`Error`] if the handshake fails.
    pub async fn accept<S: AsyncRead + AsyncWrite + Unpin>(
        &self,
        stream: S,
    ) -> Result<(http::Request<()>, WebSocketStream<S>), Error> {
        let mut framed = FramedRead::new(
            stream,
            client_request::Codec {
                handshake: &self.handshake,
            },
        );
        let reply = poll_fn(|cx| Pin::new(&mut framed).poll_next(cx)).await;

        match reply {
            Some(Ok((request, response))) => {
                framed.get_mut().write_all(&response).await?;
                Ok((
                    request,
                    WebSocketStream::from_framed(framed, Role::Server, self.config, self.limits),
                ))
            }
            Some(Err(e)) => {
                framed.get_mut().write_all(BAD_REQUEST).await?;

                Err(e)
            }
            None => Err(Error::Io(io::ErrorKind::UnexpectedEof.into())),
        }
    }

    /// Takes over an already established stream and uses it to send and receive
    /// WebSocket messages.
    ///
    /// This does not perform a HTTP upgrade handshake.
    pub fn serve<S: AsyncRead + AsyncWrite + Unpin>(&self, stream: S) -> WebSocketStream<S> {
        WebSocketStream::from_raw_stream(stream, Role::Server, self.config, self.limits)
    }
}
