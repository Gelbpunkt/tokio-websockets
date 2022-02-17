use futures_util::StreamExt;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio_util::codec::{Decoder, Framed};

use crate::{upgrade::ClientRequestCodec, Error, Role, WebsocketStream};

const BAD_REQUEST: &[u8] = b"HTTP/1.1 400 Bad Request\r\n\r\n";

/// Builder for websocket server connections.
pub struct Builder {
    fail_fast_on_invalid_utf8: bool,
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

impl Builder {
    /// Creates a [`Builder`] that will perform a HTTP upgrade handshake
    /// and then receive messages as the server end.
    #[must_use]
    pub fn new() -> Self {
        Self {
            fail_fast_on_invalid_utf8: true,
        }
    }

    /// Toggle whether to perform UTF8 checks on incomplete frame parts.
    /// This is not technically required by the websocket specification and adds a decent
    /// amount of overhead, especially when messages are sent in small chops.
    ///
    /// Because this is SHOULD behaviour, it is enabled by default.
    #[must_use]
    pub fn fail_fast_on_invalid_utf8(mut self, value: bool) -> Self {
        self.fail_fast_on_invalid_utf8 = value;

        self
    }

    /// Perform a HTTP upgrade handshake on an already established stream and uses it to send
    /// and receive websocket messages.
    ///
    /// # Errors
    ///
    /// This method returns an [`Error`] if the handshake fails.
    pub async fn accept<S: AsyncRead + AsyncWrite + Unpin>(
        &self,
        stream: S,
    ) -> Result<WebsocketStream<S>, Error> {
        let (reply, framed) = ClientRequestCodec {}.framed(stream).into_future().await;
        let mut parts = framed.into_parts();

        match reply {
            Some(Ok(response)) => {
                parts.io.write_all(response.as_bytes()).await?;
                let framed = Framed::from_parts(parts);
                Ok(WebsocketStream::from_framed(
                    framed,
                    Role::Server,
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

    /// Takes over an already established stream and uses it to send and receive websocket messages.
    ///
    /// This does not perform a HTTP upgrade handshake.
    ///
    /// # Errors
    ///
    /// This method returns an [`Error`] if writing or reading from the stream fails.
    pub fn serve<S: AsyncRead + AsyncWrite + Unpin>(&self, stream: S) -> WebsocketStream<S> {
        WebsocketStream::from_raw_stream(stream, Role::Server, self.fail_fast_on_invalid_utf8)
    }
}
