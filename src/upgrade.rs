use std::hint::unreachable_unchecked;

use base64::display::Base64Display;
use bytes::{Buf, BytesMut};
use httparse::{self, Header, Request, Response};
use tokio_util::codec::{Decoder, Encoder};

use crate::{sha::digest, utf8::parse_str, Error};

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

    /// Returns the value that the client expects to see in the server's `Sec-WebSocket-Accept` header.
    #[must_use]
    pub fn ws_accept(&self) -> String {
        base64::encode_config(&self.ws_accept, base64::STANDARD)
    }
}

/// Decoder for parsing the server's response to the client's HTTP `Connection: Upgrade` request.
pub struct ServerResponseCodec {
    ws_accept: [u8; 20],
}

impl ServerResponseCodec {
    /// Returns a new [`Codec`].
    ///
    /// The `key` parameter provides the string passed to the server via the HTTP `Sec-WebSocket-Key` header.
    #[must_use]
    pub fn new(key: &[u8]) -> Self {
        Self {
            ws_accept: digest(key),
        }
    }
}

impl Decoder for ServerResponseCodec {
    type Item = ();
    type Error = Error;

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
                code = code
            )));
        }

        let ws_accept_header = header(response.headers, "Sec-WebSocket-Accept")?;
        let mut ws_accept = [0; 20];
        base64::decode_config_slice(&ws_accept_header, base64::STANDARD, &mut ws_accept)
            .map_err(|e| Error::Upgrade(e.to_string()))?;

        if self.ws_accept != ws_accept {
            return Err(Error::Upgrade(format!(
                "server responded with incorrect Sec-WebSocket-Accept header: expected {expected}, got {actual}",
                expected = Base64Display::with_config(&self.ws_accept, base64::STANDARD),
                actual = Base64Display::with_config(&ws_accept, base64::STANDARD),
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

pub struct ClientRequestCodec {}

impl Decoder for ClientRequestCodec {
    type Item = String;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let mut headers = [httparse::EMPTY_HEADER; 25];
        let mut request = Request::new(&mut headers);
        let status = request
            .parse(src)
            .map_err(|e| Error::Upgrade(e.to_string()))?;

        if !status.is_complete() {
            return Ok(None);
        }

        let request_len = status.unwrap();

        let ws_accept = if let Ok(req) = ClientRequest::parse(|name| {
            let h = headers.iter().find(|h| h.name == name)?;
            parse_str(h.value).ok()
        }) {
            req.ws_accept()
        } else {
            return Err(Error::Upgrade(String::from(
                "Invalid client upgrade request",
            )));
        };

        src.advance(request_len);

        Ok(Some(format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-Websocket-Accept: {}\r\n\r\n", ws_accept)))
    }
}
