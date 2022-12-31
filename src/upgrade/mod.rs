//! HTTP upgrade request and response generation and validation helpers.
#[cfg(feature = "server")]
pub(crate) mod client_request;
#[cfg(feature = "client")]
pub(crate) mod server_response;

/// Errors that occur during the HTTP upgrade handshake between client and
/// server.
#[derive(Debug)]
pub enum Error {
    /// Header required in the request or response is not present.
    MissingHeader(&'static str),
    /// `Upgrade` header sent by the client does not match "websocket".
    UpgradeNotWebsocket,
    /// `Connection` header sent by the client does not contain "Upgrade".
    ConnectionNotUpgrade,
    /// `Sec-WebSocket-Version` header sent by the client is not supported by
    /// the server.
    UnsupportedWebsocketVersion,
    /// Failed to parse client request or server response.
    Parsing(httparse::Error),
    /// Server did not return a HTTP Switching Protocols response.
    DidNotSwitchProtocols(u16),
    /// Server returned a `Sec-WebSocket-Accept` that is not compatible with the
    /// `Sec-WebSocket-Key sent by the client.
    WrongWebsocketAccept,
}

impl From<httparse::Error> for Error {
    fn from(err: httparse::Error) -> Self {
        Self::Parsing(err)
    }
}
