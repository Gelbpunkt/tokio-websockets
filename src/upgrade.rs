use base64::display::Base64Display;
use bytes::{Buf, BytesMut};
use httparse::{self, Header, Response};
use tokio_util::codec::{Decoder, Encoder};

use crate::{sha::digest, Error};

const SWITCHING_PROTOCOLS: u16 = 101;

fn header<'a, 'header: 'a>(
    headers: &'a [Header<'header>],
    name: &'a str,
) -> Result<&'header [u8], Error> {
    let header = headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| {
            Error::Upgrade(format!(
                "server didn't respond with {name} header",
                name = name
            ))
        })?;

    Ok(header.value)
}

fn validate_server_response(
    expected_ws_accept: &[u8],
    data: &[u8],
) -> Result<Option<usize>, Error> {
    let mut headers = [httparse::EMPTY_HEADER; 25];
    let mut response = Response::new(&mut headers);
    let status = response
        .parse(data)
        .map_err(|e| Error::Upgrade(e.to_string()))?;

    if !status.is_complete() {
        return Ok(None);
    }

    let response_len = status.unwrap();
    let code = response.code.unwrap();

    if code != SWITCHING_PROTOCOLS {
        return Err(Error::Upgrade(format!(
            "server responded with HTTP error {code}",
            code = code
        )));
    }

    let ws_accept_header = header(response.headers, "Sec-WebSocket-Accept")?;
    let mut ws_accept = [0; 20];
    base64::decode_config_slice(&ws_accept_header, base64::STANDARD, &mut ws_accept)
        .map_err(|e| Error::Upgrade(e.to_string()))?;

    if expected_ws_accept != ws_accept {
        return Err(Error::Upgrade(format!(
            "server responded with incorrect Sec-WebSocket-Accept header: expected {expected}, got {actual}",
            expected = Base64Display::with_config(expected_ws_accept, base64::STANDARD),
            actual = Base64Display::with_config(&ws_accept, base64::STANDARD),
        )));
    }

    Ok(Some(response_len))
}

fn contains_ignore_ascii_case(mut haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }

    while haystack.len() >= needle.len() {
        if haystack[..needle.len()].eq_ignore_ascii_case(needle) {
            return true;
        }

        haystack = &haystack[1..];
    }

    false
}

/// A client's opening handshake.
pub struct ClientRequest {
    ws_accept: [u8; 20],
}

impl ClientRequest {
    /// Parses the client's opening handshake.
    ///
    /// # Errors
    ///
    /// This method fails when a header required for the websocket protocol is missing in the handshake.
    pub fn parse<'a, F>(header: F) -> Result<Self, String>
    where
        F: Fn(&'static str) -> Option<&'a str> + 'a,
    {
        let find_header = |name| {
            header(name).ok_or_else(|| format!("client didn't provide {name} header", name = name))
        };

        let check_header = |name, expected| {
            let actual = find_header(name)?;
            if actual.eq_ignore_ascii_case(expected) {
                Ok(())
            } else {
                Err(format!(
                    "client provided incorrect {name} header: expected {expected}, got {actual}",
                    name = name,
                    expected = expected,
                    actual = actual
                ))
            }
        };

        let check_header_contains = |name, expected: &str| {
            let actual = find_header(name)?;
            if contains_ignore_ascii_case(actual.as_bytes(), expected.as_bytes()) {
                Ok(())
            } else {
                Err(format!(
                    "client provided incorrect {name} header: expected string containing {expected}, got {actual}",
                    name = name,
                    expected = expected,
                    actual = actual
                ))
            }
        };

        check_header("Upgrade", "websocket")?;
        check_header_contains("Connection", "Upgrade")?;
        check_header("Sec-WebSocket-Version", "13")?;

        let key = find_header("Sec-WebSocket-Key")?;
        let ws_accept = digest(key.as_bytes());
        Ok(Self { ws_accept })
    }

    /// Copies the value that the client expects to see in the server's `Sec-WebSocket-Accept` header into a `String`.
    pub fn ws_accept_buf(&self, s: &mut String) {
        base64::encode_config_buf(&self.ws_accept, base64::STANDARD, s);
    }

    /// Returns the value that the client expects to see in the server's `Sec-WebSocket-Accept` header.
    #[must_use]
    pub fn ws_accept(&self) -> String {
        base64::encode_config(&self.ws_accept, base64::STANDARD)
    }
}

/// Decoder for parsing the server's response to the client's HTTP `Connection: Upgrade` request.
pub struct Codec {
    ws_accept: [u8; 20],
}

impl Codec {
    /// Returns a new [`Codec`].
    ///
    /// The `key` parameter provides the string passed to the server via the HTTP `Sec-WebSocket-Key` header.
    #[must_use]
    pub fn new(key: &[u8]) -> Self {
        Codec {
            ws_accept: digest(key),
        }
    }
}

impl Decoder for Codec {
    type Item = ();
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        (validate_server_response(&self.ws_accept, src)?).map_or(Ok(None), |response_len| {
            src.advance(response_len);
            Ok(Some(()))
        })
    }
}

impl Encoder<()> for Codec {
    type Error = Error;

    fn encode(&mut self, _item: (), _dst: &mut BytesMut) -> Result<(), Self::Error> {
        unimplemented!()
    }
}
