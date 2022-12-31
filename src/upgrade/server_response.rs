//! A [`Codec`] to perform a HTTP Upgrade handshake with a server and validate
//! the response.
use std::hint::unreachable_unchecked;

use base64::{engine::general_purpose::STANDARD, Engine};
use bytes::{Buf, BytesMut};
use httparse::{Header, Response};
use tokio_util::codec::{Decoder, Encoder};

use crate::{sha::digest, upgrade::Error};

/// HTTP status code for Switching Protocols.
const SWITCHING_PROTOCOLS: u16 = 101;

/// Find a header in an array of headers by name, ignoring ASCII case.
fn header<'a, 'header: 'a>(
    headers: &'a [Header<'header>],
    name: &'static str,
) -> Result<&'header [u8], Error> {
    let header = headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
        .ok_or(Error::MissingHeader(name))?;

    Ok(header.value)
}

/// [`Decoder`] for parsing the server's response to the client's HTTP
/// `Connection: Upgrade` request.
pub struct Codec {
    /// The SHA-1 digest of the `Sec-WebSocket-Key` header.
    ws_accept: [u8; 20],
}

impl Codec {
    /// Returns a new [`Codec`].
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

impl Decoder for Codec {
    type Error = crate::Error;
    type Item = ();

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let mut headers = [httparse::EMPTY_HEADER; 25];
        let mut response = Response::new(&mut headers);
        let status = response.parse(src).map_err(Error::Parsing)?;

        if !status.is_complete() {
            return Ok(None);
        }

        let response_len = status.unwrap();
        let code = response.code.unwrap();

        if code != SWITCHING_PROTOCOLS {
            return Err(crate::Error::Upgrade(Error::DidNotSwitchProtocols(code)));
        }

        let ws_accept_header = header(response.headers, "Sec-WebSocket-Accept")?;
        let mut ws_accept = [0; 20];
        // base64 0.21 calculates wrong size estimates of 21 bytes here, even though it
        // should be 20. Make the check pass by pretending that the output slice is
        // bigger than it is. This will not be present in a release build.
        // See https://github.com/marshallpierce/rust-base64/pull/207#issuecomment-1368294711
        let ws_accept_fake_length = unsafe {
            let ptr = ws_accept.as_mut_ptr();
            std::slice::from_raw_parts_mut(ptr, 21)
        };
        STANDARD
            .decode_slice(ws_accept_header, ws_accept_fake_length)
            .map_err(|_| Error::WrongWebsocketAccept)?;

        if self.ws_accept != ws_accept {
            return Err(crate::Error::Upgrade(Error::WrongWebsocketAccept));
        }

        src.advance(response_len);

        Ok(Some(()))
    }
}

impl Encoder<()> for Codec {
    type Error = crate::Error;

    fn encode(&mut self, _item: (), _dst: &mut BytesMut) -> Result<(), Self::Error> {
        unsafe { unreachable_unchecked() }
    }
}
