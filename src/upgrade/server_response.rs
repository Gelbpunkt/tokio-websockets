use std::hint::unreachable_unchecked;

use base64::{display::Base64Display, engine::general_purpose::STANDARD, Engine};
use bytes::{Buf, BytesMut};
use httparse::{Header, Response};
use tokio_util::codec::{Decoder, Encoder};

use crate::{sha::digest, Error};

/// HTTP status code for Switching Protocols.
const SWITCHING_PROTOCOLS: u16 = 101;

/// Find a header in an array of headers by name, ignoring ASCII case.
fn header<'a, 'header: 'a>(
    headers: &'a [Header<'header>],
    name: &'a str,
) -> Result<&'header [u8], Error> {
    let header = headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| Error::Upgrade(format!("server didn't respond with {name} header",)))?;

    Ok(header.value)
}

/// [`Decoder`] for parsing the server's response to the client's HTTP
/// `Connection: Upgrade` request.
pub struct ServerResponseCodec {
    /// The SHA-1 digest of the `Sec-WebSocket-Key` header.
    ws_accept: [u8; 20],
}

impl ServerResponseCodec {
    /// Returns a new [`ServerResponseCodec`].
    ///
    /// The `key` parameter provides the string passed to the server via the
    /// HTTP `Sec-WebSocket-Key` header.
    #[must_use]
    pub fn new(key: &[u8]) -> Self {
        Self {
            ws_accept: digest(key),
        }
    }
}

impl Decoder for ServerResponseCodec {
    type Error = Error;
    type Item = ();

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let mut headers = [httparse::EMPTY_HEADER; 25];
        let mut response = Response::new(&mut headers);
        let status = response
            .parse(src)
            .map_err(|e| Error::Upgrade(e.to_string()))?;

        if !status.is_complete() {
            return Ok(None);
        }

        let response_len = status.unwrap();
        let code = response.code.unwrap();

        if code != SWITCHING_PROTOCOLS {
            return Err(Error::Upgrade(format!(
                "server responded with HTTP error {code}",
            )));
        }

        let ws_accept_header = header(response.headers, "Sec-WebSocket-Accept")?;
        let mut ws_accept = [0; 20];
        STANDARD
            .decode_slice(ws_accept_header, &mut ws_accept)
            .map_err(|e| Error::Upgrade(e.to_string()))?;

        if self.ws_accept != ws_accept {
            return Err(Error::Upgrade(format!(
                "server responded with incorrect Sec-WebSocket-Accept header: expected {expected}, got {actual}",
                expected = Base64Display::new(&self.ws_accept, &STANDARD),
                actual = Base64Display::new(&ws_accept, &STANDARD),
            )));
        }

        src.advance(response_len);

        Ok(Some(()))
    }
}

impl Encoder<()> for ServerResponseCodec {
    type Error = Error;

    fn encode(&mut self, _item: (), _dst: &mut BytesMut) -> Result<(), Self::Error> {
        unsafe { unreachable_unchecked() }
    }
}
