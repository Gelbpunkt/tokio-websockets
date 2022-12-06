//! General error type used in the crate.
use std::io;

#[cfg(feature = "native-tls")]
use tokio_native_tls::native_tls;
#[cfg(feature = "__rustls")]
use tokio_rustls::rustls::client::InvalidDnsNameError;
#[cfg(feature = "rustls-native-roots")]
use tokio_rustls::webpki;

use crate::proto::ProtocolError;

/// Generic error when using websockets with this crate.
#[derive(Debug)]
pub enum Error {
    /// Attempted to read from or write to a closed stream.
    AlreadyClosed,
    /// DNS lookup failed.
    CannotResolveHost,
    /// Attempted to send a message on a closed stream.
    ConnectionClosed,
    /// Attempted to connect a client to a remote without configured URI.
    #[cfg(feature = "client")]
    NoUriConfigured,
    /// Websocket protocol violation.
    Protocol(ProtocolError),
    /// I/O error.
    Io(io::Error),
    /// TLS error originating in [`native_tls`].
    #[cfg(feature = "native-tls")]
    NativeTls(native_tls::Error),
    /// No response to the HTTP/1.1 Upgrade was received.
    NoUpgradeResponse,
    /// Attempted to connect to an invalid DNS name.
    #[cfg(feature = "__rustls")]
    InvalidDNSName(InvalidDnsNameError),
    /// Adding a native certificate to the storage for the TLS connector failed.
    #[cfg(feature = "rustls-native-roots")]
    Webpki(webpki::Error),
    /// The HTTP/1.1 Upgrade failed.
    #[cfg(feature = "client")]
    Upgrade(String),
}

#[cfg(feature = "native-tls")]
impl From<native_tls::Error> for Error {
    fn from(err: native_tls::Error) -> Self {
        Self::NativeTls(err)
    }
}

impl From<ProtocolError> for Error {
    fn from(err: ProtocolError) -> Self {
        Self::Protocol(err)
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

#[cfg(feature = "__rustls")]
impl From<InvalidDnsNameError> for Error {
    fn from(err: InvalidDnsNameError) -> Self {
        Self::InvalidDNSName(err)
    }
}

#[cfg(feature = "rustls-native-roots")]
impl From<webpki::Error> for Error {
    fn from(err: webpki::Error) -> Self {
        Self::Webpki(err)
    }
}
