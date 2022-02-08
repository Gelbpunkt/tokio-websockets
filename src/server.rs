use httparse::Request;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

use crate::{upgrade::ClientRequest, utf8::parse_str, Error, Role, WebsocketStream};

const MAX_REQUEST_SIZE: usize = 4096;
const BAD_REQUEST: &[u8] = b"HTTP/1.1 400 Bad Request\r\n\r\n";

pub async fn accept<T>(stream: T) -> Result<WebsocketStream<T>, Error>
where
    T: AsyncWrite + AsyncRead + Unpin,
{
    let mut buf_reader = BufReader::with_capacity(MAX_REQUEST_SIZE, stream);

    // Minimum length for a valid HTTP request with websocket upgrade request
    let mut buf = Vec::with_capacity(159);
    loop {
        let last_read = buf_reader.read_until(b'\n', &mut buf).await?;
        if (last_read == 2 && &buf[buf.len() - 2..] == b"\r\n") || buf.len() >= MAX_REQUEST_SIZE {
            break;
        }
    }

    let mut headers = [httparse::EMPTY_HEADER; 25];
    let mut request = Request::new(&mut headers);
    let status = request
        .parse(&buf)
        .map_err(|e| Error::Upgrade(e.to_string()))?;

    if !status.is_complete() {
        return Err(Error::Upgrade(String::from("Invalid request body")));
    }

    let ws_accept = if let Ok(req) = ClientRequest::parse(|name| {
        let h = headers.iter().find(|h| h.name == name)?;
        parse_str(h.value).ok()
    }) {
        req.ws_accept()
    } else {
        buf_reader.write_all(BAD_REQUEST).await?;
        return Err(Error::Upgrade(String::from(
            "Invalid client upgrade request",
        )));
    };

    let body = format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-Websocket-Accept: {}\r\n\r\n", ws_accept);
    buf_reader.write_all(body.as_bytes()).await?;

    let ws_stream = WebsocketStream::from_raw_stream(buf_reader.into_inner(), Role::Server);

    Ok(ws_stream)
}
