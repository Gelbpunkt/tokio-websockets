//! Wrapper types for TLS functionality, abstracting over [`rustls`] and
//! [`native-tls`] connector and stream types.
//!
//! [`native-tls`]: tokio_native_tls::native_tls
//! [`rustls`]: tokio_rustls::rustls

#[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
use std::sync::Arc;
use std::{
    fmt::{Debug, Formatter, Result as FmtResult},
    io,
    pin::Pin,
    task::{Context, Poll},
};

#[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
use rustls_pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
#[cfg(all(
    any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"),
    feature = "ring"
))]
use tokio_rustls::rustls::crypto::ring;
#[cfg(any(feature = "rustls-native-roots", feature = "rustls-webpki-roots"))]
use tokio_rustls::rustls::crypto::CryptoProvider;
#[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
use tokio_rustls::rustls::{ClientConfig, RootCertStore};

use crate::Error;

/// A reusable TLS connector for wrapping streams.
pub enum Connector {
    /// Plain (non-TLS) connector.
    Plain,
    /// [`native-tls`] TLS connector.
    ///
    /// [`native-tls`]: tokio_native_tls::native_tls
    #[cfg(feature = "native-tls")]
    NativeTls(tokio_native_tls::TlsConnector),
    /// [`rustls`] TLS connector.
    ///
    /// [`rustls`]: tokio_rustls::rustls
    #[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
    Rustls(tokio_rustls::TlsConnector),
}

impl Debug for Connector {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::Plain => f.write_str("Connector::Plain"),
            #[cfg(feature = "native-tls")]
            Self::NativeTls(connector) => connector.fmt(f),
            #[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
            Self::Rustls(_) => f.write_str("Connector::Rustls"),
        }
    }
}

/// A stream that might be protected with TLS.
#[allow(clippy::large_enum_variant)] // Only one or two of these will be used
#[derive(Debug)]
pub enum MaybeTlsStream<S> {
    /// Unencrypted socket stream.
    Plain(S),
    /// Encrypted socket stream using [`native-tls`].
    ///
    /// [`native-tls`]: tokio_native_tls::native_tls
    #[cfg(feature = "native-tls")]
    NativeTls(tokio_native_tls::TlsStream<S>),
    /// Encrypted socket stream using [`rustls`].
    ///
    /// [`rustls`]: tokio_rustls::rustls
    #[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
    Rustls(tokio_rustls::client::TlsStream<S>),
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for MaybeTlsStream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(ref mut s) => Pin::new(s).poll_read(cx, buf),
            #[cfg(feature = "native-tls")]
            Self::NativeTls(s) => Pin::new(s).poll_read(cx, buf),
            #[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
            Self::Rustls(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for MaybeTlsStream<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        match self.get_mut() {
            Self::Plain(ref mut s) => Pin::new(s).poll_write(cx, buf),
            #[cfg(feature = "native-tls")]
            Self::NativeTls(s) => Pin::new(s).poll_write(cx, buf),
            #[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
            Self::Rustls(s) => Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        match self.get_mut() {
            Self::Plain(ref mut s) => Pin::new(s).poll_flush(cx),
            #[cfg(feature = "native-tls")]
            Self::NativeTls(s) => Pin::new(s).poll_flush(cx),
            #[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
            Self::Rustls(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        match self.get_mut() {
            Self::Plain(ref mut s) => Pin::new(s).poll_shutdown(cx),
            #[cfg(feature = "native-tls")]
            Self::NativeTls(s) => Pin::new(s).poll_shutdown(cx),
            #[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
            Self::Rustls(s) => Pin::new(s).poll_shutdown(cx),
        }
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<Result<usize, io::Error>> {
        match self.get_mut() {
            Self::Plain(ref mut s) => Pin::new(s).poll_write_vectored(cx, bufs),
            #[cfg(feature = "native-tls")]
            Self::NativeTls(s) => Pin::new(s).poll_write_vectored(cx, bufs),
            #[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
            Self::Rustls(s) => Pin::new(s).poll_write_vectored(cx, bufs),
        }
    }

    fn is_write_vectored(&self) -> bool {
        match self {
            Self::Plain(s) => s.is_write_vectored(),
            #[cfg(feature = "native-tls")]
            Self::NativeTls(s) => s.is_write_vectored(),
            #[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
            Self::Rustls(s) => s.is_write_vectored(),
        }
    }
}

impl Connector {
    /// Creates a new `Connector` with the underlying TLS library specified in
    /// the feature flags.
    ///
    /// # Errors
    ///
    /// This method returns an [`Error`] when creating the underlying TLS
    /// connector fails.
    #[cfg(any(
        not(any(
            feature = "native-tls",
            feature = "rustls-webpki-roots",
            feature = "rustls-native-roots"
        )),
        feature = "native-tls",
        all(
            feature = "ring",
            any(feature = "rustls-webpki-roots", feature = "rustls-native-roots")
        )
    ))] // Method is available at any time *except* when a rustls flag is enabled
        // without ring
    pub fn new() -> Result<Self, Error> {
        #[cfg(not(any(
            feature = "native-tls",
            feature = "rustls-webpki-roots",
            feature = "rustls-native-roots"
        )))]
        {
            Ok(Self::Plain)
        }
        #[cfg(all(
            feature = "native-tls",
            not(all(
                any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"),
                feature = "ring"
            ))
        ))]
        {
            Ok(Self::NativeTls(
                tokio_native_tls::native_tls::TlsConnector::new()?.into(),
            ))
        }
        #[cfg(all(
            any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"),
            feature = "ring"
        ))]
        {
            Self::new_rustls_with_crypto_provider(ring::default_provider().into())
        }
    }

    /// Creates a new `Connector` using `rustls` with a custom crypto provider
    /// as the TLS library and root certificates specified in the feature
    /// flags.
    ///
    /// # Errors
    ///
    /// This method returns an [`Error`] when creating the underlying TLS
    /// connector fails.
    #[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
    pub fn new_rustls_with_crypto_provider(provider: Arc<CryptoProvider>) -> Result<Self, Error> {
        let mut roots = RootCertStore::empty();

        #[cfg(feature = "rustls-native-roots")]
        {
            let certs = rustls_native_certs::load_native_certs()?;

            for cert in certs {
                roots.add(cert)?;
            }
        }

        #[cfg(feature = "rustls-webpki-roots")]
        {
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        };

        let config = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()?
            .with_root_certificates(roots)
            .with_no_client_auth();

        let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
        Ok(Self::Rustls(connector))
    }

    /// Wraps a given stream with a layer of TLS.
    ///
    /// # Errors
    ///
    /// This method returns an [`Error`] if the TLS handshake fails.
    #[cfg_attr(
        not(any(
            feature = "native-tls",
            feature = "rustls-webpki-roots",
            feature = "rustls-native-roots"
        )),
        allow(unused_variables, clippy::unused_async)
    )]
    pub async fn wrap<S: AsyncRead + AsyncWrite + Unpin>(
        &self,
        domain: &str,
        stream: S,
    ) -> Result<MaybeTlsStream<S>, Error> {
        match self {
            Self::Plain => Ok(MaybeTlsStream::Plain(stream)),
            #[cfg(feature = "native-tls")]
            Self::NativeTls(connector) => Ok(MaybeTlsStream::NativeTls(
                connector.connect(domain, stream).await?,
            )),
            #[cfg(any(feature = "rustls-webpki-roots", feature = "rustls-native-roots"))]
            Self::Rustls(connector) => Ok(MaybeTlsStream::Rustls(
                connector
                    .connect(ServerName::try_from(domain)?.to_owned(), stream)
                    .await?,
            )),
        }
    }
}
