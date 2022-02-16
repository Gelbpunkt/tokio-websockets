use futures_util::StreamExt;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio_util::codec::{Decoder, Framed};

use crate::{upgrade::ClientRequestCodec, Error, Role, WebsocketStream};

const BAD_REQUEST: &[u8] = b"HTTP/1.1 400 Bad Request\r\n\r\n";

pub async fn accept<T>(stream: T) -> Result<WebsocketStream<T>, Error>
where
    T: AsyncWrite + AsyncRead + Unpin,
{
    let (reply, framed) = ClientRequestCodec {}.framed(stream).into_future().await;
    let mut parts = framed.into_parts();

    match reply {
        Some(Ok(response)) => {
            parts.io.write_all(response.as_bytes()).await?;
            let framed = Framed::from_parts(parts);
            Ok(WebsocketStream::from_framed(framed, Role::Server))
        }
        Some(Err(e)) => {
            parts.io.write_all(BAD_REQUEST).await?;

            Err(e)
        }
        None => Err(Error::NoUpgradeResponse),
    }
}
