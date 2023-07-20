//! General error type used in the crate.
use std::{fmt, io};

#[cfg(feature = "native-tls")]
use tokio_native_tls::native_tls;
#[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
use tokio_rustls::rustls::client::InvalidDnsNameError;

use crate::proto::ProtocolError;

/// Generic error when using websockets with this crate.
#[derive(Debug)]
pub enum Error {
    /// Attempted to read from or write to a closed stream.
    AlreadyClosed,
    /// DNS lookup failed.
    CannotResolveHost,
    /// Attempted to connect a client to a remote without configured URI.
    #[cfg(feature = "client")]
    NoUriConfigured,
    /// Websocket protocol violation.
    Protocol(ProtocolError),
    /// Frame size limit was exceeded.
    FrameTooLong { size: usize, max_size: usize },
    /// Message size limit was exceeded.
    MessageTooLong { size: usize, max_size: usize },
    /// I/O error.
    Io(io::Error),
    /// TLS error originating in [`native_tls`].
    #[cfg(feature = "native-tls")]
    NativeTls(native_tls::Error),
    /// No response to the HTTP/1.1 Upgrade was received.
    NoUpgradeResponse,
    /// Attempted to connect to an invalid DNS name.
    #[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
    InvalidDNSName(InvalidDnsNameError),
    /// Adding a native certificate to the storage for the TLS connector failed.
    #[cfg(feature = "rustls-native-roots")]
    Rustls(tokio_rustls::rustls::Error),
    /// The HTTP/1.1 Upgrade failed.
    #[cfg(any(feature = "client", feature = "server"))]
    Upgrade(crate::upgrade::Error),
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

#[cfg(feature = "rustls-native-roots")]
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
            Error::FrameTooLong { size, max_size } => {
                f.write_str("frame size of ")?;
                size.fmt(f)?;
                f.write_str(" exceeds frame size limit of ")?;
                max_size.fmt(f)
            }
            Error::MessageTooLong { size, max_size } => {
                f.write_str("message size of ")?;
                size.fmt(f)?;
                f.write_str(" exceeds message size limit of ")?;
                max_size.fmt(f)
            }
            Error::Io(e) => e.fmt(f),
            #[cfg(feature = "native-tls")]
            Error::NativeTls(e) => e.fmt(f),
            Error::NoUpgradeResponse => f.write_str("upgrade handshake aborted by other end"),
            #[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
            Error::InvalidDNSName(_) => f.write_str("invalid DNS name"),
            #[cfg(feature = "rustls-native-roots")]
            Error::Rustls(e) => e.fmt(f),
            #[cfg(any(feature = "client", feature = "server"))]
            Error::Upgrade(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::AlreadyClosed
            | Error::CannotResolveHost
            | Error::NoUpgradeResponse
            | Error::FrameTooLong { .. }
            | Error::MessageTooLong { .. } => None,
            #[cfg(feature = "client")]
            Error::NoUriConfigured => None,
            Error::Protocol(e) => Some(e),
            Error::Io(e) => Some(e),
            #[cfg(feature = "native-tls")]
            Error::NativeTls(e) => Some(e),
            #[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
            Error::InvalidDNSName(e) => Some(e),
            #[cfg(feature = "rustls-native-roots")]
            Error::Rustls(e) => Some(e),
            #[cfg(any(feature = "client", feature = "server"))]
            Error::Upgrade(e) => Some(e),
        }
    }
}
