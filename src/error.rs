#[cfg(feature = "native-tls")]
use tokio_native_tls::native_tls;
#[cfg(feature = "__rustls")]
use tokio_rustls::rustls::client::InvalidDnsNameError;
#[cfg(feature = "rustls-native-roots")]
use tokio_rustls::webpki;

use std::io;

use crate::proto::ProtocolError;

#[derive(Debug)]
pub enum Error {
    AlreadyClosed,
    ConnectionClosed,
    Protocol(ProtocolError),
    Io(io::Error),
    #[cfg(feature = "native-tls")]
    NativeTls(native_tls::Error),
    #[cfg(feature = "__rustls")]
    InvalidDNSName(InvalidDnsNameError),
    #[cfg(feature = "rustls-native-roots")]
    Webpki(webpki::Error),
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
