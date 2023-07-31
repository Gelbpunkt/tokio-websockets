//! Implementation of a websocket client.
//!
//! This can be used in three ways:
//!   - By letting the library connect to a remote URI and performing a HTTP/1.1
//!     Upgrade handshake, via [`Builder::connect`]
//!   - By letting the library perform a HTTP/1.1 Upgrade handshake on an
//!     established stream, via [`Builder::connect_on`]
//!   - By performing the handshake yourself and then using
//!     [`Builder::take_over`] to let it take over a websocket stream
use std::{
    future::poll_fn,
    net::{SocketAddr, ToSocketAddrs},
    pin::Pin,
    str::FromStr,
};

use base64::{engine::general_purpose, Engine};
use futures_core::Stream;
use http::{header::HeaderName, HeaderMap, HeaderValue, Uri};
use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
};
use tokio_util::codec::Decoder;

use crate::{
    proto::{Config, Limits, Role},
    upgrade::{self, server_response},
    Connector, Error, MaybeTlsStream, WebsocketStream,
};

/// Generates a new, random 16-byte websocket key and encodes it as base64.
pub(crate) fn make_key() -> [u8; 24] {
    let mut key_base64 = [0; 24];
    let key_bytes = crate::rand::get_key();

    // SAFETY: We know that 16 bytes will be 24 bytes base64-encoded
    unsafe {
        general_purpose::STANDARD
            .encode_slice(key_bytes, &mut key_base64)
            .unwrap_unchecked()
    };

    key_base64
}

/// Guesses the port to connect on for a URI. If none is specified, port 443
/// will be used for TLS, 80 for plain HTTP.
fn default_port(uri: &Uri) -> Option<u16> {
    if let Some(port) = uri.port_u16() {
        return Some(port);
    }

    let scheme = uri.scheme_str();

    match scheme {
        Some("https" | "wss") => Some(443),
        Some("http" | "ws") => Some(80),
        _ => None,
    }
}

/// Builds a HTTP/1.1 Upgrade request for a URI with extra headers and a
/// websocket key.
fn build_request(uri: &Uri, key: &[u8], headers: &HeaderMap) -> Vec<u8> {
    let mut buf = Vec::new();

    buf.extend_from_slice(b"GET ");
    buf.extend_from_slice(uri.path().as_bytes());

    if let Some(query) = uri.query() {
        buf.extend_from_slice(b"?");
        buf.extend_from_slice(query.as_bytes());
    }

    buf.extend_from_slice(b" HTTP/1.1\r\n");

    if let Some(host) = uri.host() {
        buf.extend_from_slice(b"Host: ");
        buf.extend_from_slice(host.as_bytes());

        if let Some(port) = default_port(uri) {
            buf.extend_from_slice(b":");
            buf.extend_from_slice(port.to_string().as_bytes());
        }

        buf.extend_from_slice(b"\r\n");
    }

    buf.extend_from_slice(b"Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: ");
    buf.extend_from_slice(key);
    buf.extend_from_slice(b"\r\nSec-WebSocket-Version: 13\r\n");

    for (name, value) in headers {
        buf.extend_from_slice(name.as_str().as_bytes());
        buf.extend_from_slice(b": ");
        buf.extend_from_slice(value.as_bytes());
        buf.extend_from_slice(b"\r\n");
    }

    buf.extend_from_slice(b"\r\n");

    buf
}

/// Resolve a hostname and a port to an IP address.
///
/// This currently uses a blocking `getaddrinfo` syscall.
async fn resolve(host: String, port: u16) -> Result<SocketAddr, Error> {
    let task = tokio::task::spawn_blocking(move || {
        (host, port)
            .to_socket_addrs()?
            .next()
            .ok_or(Error::CannotResolveHost)
    });

    task.await.expect("Tokio threadpool failed")
}

/// Builder for websocket client connections.
pub struct Builder<'a> {
    /// URI to connect to, required unless connecting to an established
    /// websocket stream.
    uri: Option<Uri>,
    /// A TLS connector to use for the connection. If not set and required, a
    /// new one will be created.
    connector: Option<&'a Connector>,
    /// Configuration for the websocket stream.
    config: Config,
    /// Limits to impose on the websocket stream.
    limits: Limits,
    /// Headers to be sent with the upgrade request.
    headers: HeaderMap,
}

impl<'a> Builder<'a> {
    /// Creates a [`Builder`] with all defaults that is not configured to
    /// connect to any server.
    #[must_use]
    pub fn new() -> Self {
        Self {
            uri: None,
            connector: None,
            config: Config::default(),
            limits: Limits::default(),
            headers: HeaderMap::new(),
        }
    }

    /// Sets the [`Uri`] to connect to.
    ///
    /// # Errors
    ///
    /// This method returns a [`http::uri::InvalidUri`] error if URI parsing
    /// fails.
    pub fn uri(mut self, uri: &str) -> Result<Self, http::uri::InvalidUri> {
        self.uri = Some(Uri::from_str(uri)?);

        Ok(self)
    }

    /// Creates a [`Builder`] that connects to a given URI.
    ///
    /// This method never fails as the URI has already been parsed.
    #[must_use]
    pub fn from_uri(uri: Uri) -> Self {
        Self {
            uri: Some(uri),
            connector: None,
            config: Config::default(),
            limits: Limits::default(),
            headers: HeaderMap::new(),
        }
    }

    /// Sets the TLS connector for the client.
    ///
    /// By default, the client will create a new one for each connection instead
    /// of reusing one.
    #[must_use]
    pub fn connector(mut self, connector: &'a Connector) -> Self {
        self.connector = Some(connector);

        self
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

    /// Adds an extra HTTP header to the handshake request.
    #[must_use]
    pub fn add_header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.headers.insert(name, value);

        self
    }

    /// Establishes a connection to the websocket server. This requires a URI to
    /// be configured via [`Builder::uri`].
    ///
    /// # Errors
    ///
    /// This method returns an [`Error`] if connecting to the server fails or no
    /// URI has been configured.
    pub async fn connect(
        &self,
    ) -> Result<
        (
            WebsocketStream<MaybeTlsStream<TcpStream>>,
            upgrade::Response,
        ),
        Error,
    > {
        let uri = self.uri.as_ref().ok_or(Error::NoUriConfigured)?;
        let host = uri.host().ok_or(Error::CannotResolveHost)?;
        let port = default_port(uri).unwrap_or(80);
        let addr = resolve(host.to_string(), port).await?;

        let stream = TcpStream::connect(&addr).await?;

        let stream = if let Some(connector) = self.connector {
            connector.wrap(host, stream).await?
        } else if uri.scheme_str() == Some("wss") {
            let connector = Connector::new()?;

            connector.wrap(host, stream).await?
        } else {
            Connector::Plain.wrap(host, stream).await?
        };

        self.connect_on(stream).await
    }

    /// Takes over an already established stream and uses it to send and receive
    /// websocket messages. This requires a URI to be configured via
    /// [`Builder::uri`].
    ///
    /// This method assumes that the TLS connection has already been
    /// established, if needed. It sends an HTTP upgrade request and waits
    /// for an HTTP Switching Protocols response before proceeding.
    ///
    /// # Errors
    ///
    /// This method returns an [`Error`] if writing or reading from the stream
    /// fails or no URI has been configured.
    pub async fn connect_on<S: AsyncRead + AsyncWrite + Unpin>(
        &self,
        mut stream: S,
    ) -> Result<(WebsocketStream<S>, upgrade::Response), Error> {
        let uri = self.uri.as_ref().ok_or(Error::NoUriConfigured)?;

        let key_base64 = make_key();

        let upgrade_codec = server_response::Codec::new(&key_base64);
        let request = build_request(uri, &key_base64, &self.headers);
        stream.write_all(&request).await?;

        let mut framed = upgrade_codec.framed(stream);
        let res = poll_fn(|cx| Pin::new(&mut framed).poll_next(cx))
            .await
            .ok_or(Error::NoUpgradeResponse)??;

        Ok((
            WebsocketStream::from_framed(framed, Role::Client, self.config, self.limits),
            res,
        ))
    }

    /// Takes over an already established stream that has already performed a
    /// HTTP upgrade handshake and uses it to send and receive websocket
    /// messages.
    ///
    /// This method will not perform a TLS handshake or a HTTP upgrade
    /// handshake, it assumes the stream is ready to use for writing and
    /// reading the websocket protocol.
    pub fn take_over<S: AsyncRead + AsyncWrite + Unpin>(&self, stream: S) -> WebsocketStream<S> {
        WebsocketStream::from_raw_stream(stream, Role::Client, self.config, self.limits)
    }
}

impl<'a> Default for Builder<'a> {
    fn default() -> Self {
        Self::new()
    }
}
