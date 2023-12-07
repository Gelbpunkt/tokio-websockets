//! General error type used in the crate.
use std::{fmt, io};

#[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
use rustls_pki_types::InvalidDnsNameError;
#[cfg(feature = "native-tls")]
use tokio_native_tls::native_tls;

use crate::proto::ProtocolError;

/// Generic error when using WebSockets with this crate.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// Attempted to read from or write to a closed stream.
    AlreadyClosed,
    /// DNS lookup failed.
    CannotResolveHost,
    /// Attempted to connect a client to a remote without configured URI.
    #[cfg(feature = "client")]
    NoUriConfigured,
    /// WebSocket protocol violation.
    Protocol(ProtocolError),
    /// Payload length limit was exceeded.
    PayloadTooLong { len: usize, max_len: usize },
    /// I/O error.
    Io(io::Error),
    /// TLS error originating in [`native_tls`].
    #[cfg(feature = "native-tls")]
    NativeTls(native_tls::Error),
    /// Attempted to connect to an invalid DNS name.
    #[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
    InvalidDNSName(InvalidDnsNameError),
    /// A general rustls error.
    #[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
    Rustls(tokio_rustls::rustls::Error),
    /// The HTTP/1.1 Upgrade failed.
    #[cfg(any(feature = "client", feature = "server"))]
    Upgrade(crate::upgrade::Error),
    /// Rustls was enabled via crate features, but no crypto provider was
    /// configured prior to connecting.
    #[cfg(all(
        any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"),
        not(feature = "ring")
    ))]
    NoTlsConnectorConfigured,
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

#[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
impl From<InvalidDnsNameError> for Error {
    fn from(err: InvalidDnsNameError) -> Self {
        Self::InvalidDNSName(err)
    }
}

#[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
impl From<tokio_rustls::rustls::Error> for Error {
    fn from(err: tokio_rustls::rustls::Error) -> Self {
        Self::Rustls(err)
    }
}

#[cfg(any(feature = "client", feature = "server"))]
impl From<crate::upgrade::Error> for Error {
    fn from(err: crate::upgrade::Error) -> Self {
        Self::Upgrade(err)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::AlreadyClosed => {
                f.write_str("attempted to send message after closing connection")
            }
            Error::CannotResolveHost => f.write_str("client DNS lookup failed"),
            #[cfg(feature = "client")]
            Error::NoUriConfigured => f.write_str("client has no URI configured"),
            Error::Protocol(e) => e.fmt(f),
            Error::PayloadTooLong { len, max_len } => {
                f.write_str("payload length of ")?;
                len.fmt(f)?;
                f.write_str(" exceeds the limit of ")?;
                max_len.fmt(f)
            }
            Error::Io(e) => e.fmt(f),
            #[cfg(feature = "native-tls")]
            Error::NativeTls(e) => e.fmt(f),
            #[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
            Error::InvalidDNSName(_) => f.write_str("invalid DNS name"),
            #[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
            Error::Rustls(e) => e.fmt(f),
            #[cfg(any(feature = "client", feature = "server"))]
            Error::Upgrade(e) => e.fmt(f),
            #[cfg(all(
                any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"),
                not(feature = "ring")
            ))]
            Error::NoTlsConnectorConfigured => {
                f.write_str("wss uri set but no tls connector was configured")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::AlreadyClosed | Error::CannotResolveHost | Error::PayloadTooLong { .. } => None,
            #[cfg(feature = "client")]
            Error::NoUriConfigured => None,
            #[cfg(all(
                any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"),
                not(feature = "ring")
            ))]
            Error::NoTlsConnectorConfigured => None,
            Error::Protocol(e) => Some(e),
            Error::Io(e) => Some(e),
            #[cfg(feature = "native-tls")]
            Error::NativeTls(e) => Some(e),
            #[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
            Error::InvalidDNSName(e) => Some(e),
            #[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
            Error::Rustls(e) => Some(e),
            #[cfg(any(feature = "client", feature = "server"))]
            Error::Upgrade(e) => Some(e),
        }
    }
}
