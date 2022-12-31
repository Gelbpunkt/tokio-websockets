//! Helpers for a minimal HTTP/1.1 upgrade client and server implementation.
use std::hint::unreachable_unchecked;

use base64::{display::Base64Display, engine::general_purpose::STANDARD, Engine};
use bytes::{Buf, BytesMut};
use httparse::{self, Header, Request, Response};
use tokio_util::codec::{Decoder, Encoder};

use crate::{sha::digest, utf8::parse_str, Error};

/// HTTP status code for Switching Protocols.
const SWITCHING_PROTOCOLS: u16 = 101;
/// A static HTTP/1.1 101 Switching Protocols response up until the
/// `Sec-WebSocket-Accept` header value.
const SWITCHING_PROTOCOLS_BODY: &str = "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-Websocket-Accept: ";

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

/// Returns whether an ASCII byte slice is contained in another one, ignoring
/// captalization.
fn contains_ignore_ascii_case(mut haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }

    while haystack.len() >= needle.len() {
        if unsafe { haystack.get_unchecked(..needle.len()) }.eq_ignore_ascii_case(needle) {
            return true;
        }

        haystack = &haystack[1..];
    }

    false
}

/// A client's opening handshake.
struct ClientRequest {
    /// The SHA-1 digest of the `Sec-WebSocket-Key` header.
    ws_accept: [u8; 20],
}

impl ClientRequest {
    /// Parses the client's opening handshake.
    ///
    /// # Errors
    ///
    /// This method fails when a header required for the websocket protocol is
    /// missing in the handshake.
    pub fn parse<'a, F>(header: F) -> Result<Self, String>
    where
        F: Fn(&'static str) -> Option<&'a str> + 'a,
    {
        let find_header =
            |name| header(name).ok_or_else(|| format!("client didn't provide {name} header"));

        let check_header = |name, expected| {
            let actual = find_header(name)?;
            if actual.eq_ignore_ascii_case(expected) {
                Ok(())
            } else {
                Err(format!(
                    "client provided incorrect {name} header: expected {expected}, got {actual}"
                ))
            }
        };

        let check_header_contains = |name, expected: &str| {
            let actual = find_header(name)?;
            if contains_ignore_ascii_case(actual.as_bytes(), expected.as_bytes()) {
                Ok(())
            } else {
                Err(format!(
                    "client provided incorrect {name} header: expected string containing {expected}, got {actual}"
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

    /// Returns the value that the client expects to see in the server's
    /// `Sec-WebSocket-Accept` header.
    #[must_use]
    pub fn ws_accept(&self) -> String {
        STANDARD.encode(self.ws_accept)
    }
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

/// A codec that implements a [`Decoder`] for HTTP/1.1 upgrade requests and
/// yields a HTTP/1.1 response to reply with.
///
/// It does not implement an [`Encoder`].
pub struct ClientRequestCodec {}

impl Decoder for ClientRequestCodec {
    type Error = Error;
    type Item = String;

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

        let ws_accept = match ClientRequest::parse(|name| {
            let h = headers.iter().find(|h| h.name.eq_ignore_ascii_case(name))?;
            parse_str(h.value).ok()
        }) {
            Ok(req) => req.ws_accept(),
            Err(e) => {
                let mut error_msg = String::from("Invalid client upgrade request: ");
                error_msg.push_str(&e);
                return Err(Error::Upgrade(error_msg));
            }
        };

        src.advance(request_len);

        let mut resp = String::with_capacity(SWITCHING_PROTOCOLS_BODY.len() + ws_accept.len() + 4);

        resp.push_str(SWITCHING_PROTOCOLS_BODY);
        resp.push_str(&ws_accept);
        resp.push_str("\r\n\r\n");

        Ok(Some(resp))
    }
}
