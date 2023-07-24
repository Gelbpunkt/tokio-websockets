//! Types required for the websocket protocol implementation.
use bytes::{BufMut, Bytes, BytesMut};

use super::error::ProtocolError;
use crate::utf8;

/// The opcode of a websocket frame. It denotes the type of the frame or an
/// assembled message.
///
/// A fully assembled [`Message`] will never have a continuation opcode.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(super) enum OpCode {
    /// A continuation opcode. This will never be encountered in a full
    /// [`Message`].
    Continuation,
    /// A text opcode.
    Text,
    /// A binary opcode.
    Binary,
    /// A close opcode.
    Close,
    /// A ping opcode.
    Ping,
    /// A pong opcode.
    Pong,
}

impl OpCode {
    /// Whether this is a control opcode (i.e. close, ping or pong).
    pub(super) fn is_control(self) -> bool {
        matches!(self, Self::Close | Self::Ping | Self::Pong)
    }
}

impl TryFrom<u8> for OpCode {
    type Error = ProtocolError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Continuation),
            1 => Ok(Self::Text),
            2 => Ok(Self::Binary),
            8 => Ok(Self::Close),
            9 => Ok(Self::Ping),
            10 => Ok(Self::Pong),
            _ => Err(ProtocolError::InvalidOpcode),
        }
    }
}

impl From<OpCode> for u8 {
    fn from(value: OpCode) -> Self {
        match value {
            OpCode::Continuation => 0,
            OpCode::Text => 1,
            OpCode::Binary => 2,
            OpCode::Close => 8,
            OpCode::Ping => 9,
            OpCode::Pong => 10,
        }
    }
}

/// A close status code as defined by section [7.4 of the RFC](https://datatracker.ietf.org/doc/html/rfc6455#section-7.4).
///
/// Enum variants are provided for codes and ranges defined in the
/// RFC and commonly used ones. All others are invalid.
///
/// This is intended to be used via the `TryFrom<u16>` and `Into<u16>` trait
/// implementations.
///
/// Enum variant descriptions are taken from the RFC.
#[derive(Debug, Clone)]
pub enum CloseCode {
    /// 1000 indicates a normal closure, meaning that the purpose for which the
    /// connection was established has been fulfilled.
    NormalClosure,
    /// 1001 indicates that an endpoint is "going away", such as a server going
    /// down or a browser having navigated away from a page.
    GoingAway,
    /// 1002 indicates that an endpoint is terminating the connection due to a
    /// protocol error.
    ProtocolError,
    /// 1003 indicates that an endpoint is terminating the connection because it
    /// has received a type of data it cannot accept (e.g., an endpoint that
    /// understands only text data MAY send this if it receives a binary
    /// message).
    UnsupportedData,
    /// Reserved. The specific meaning might be defined in the future.
    Reserved,
    /// 1005 is a reserved value and MUST NOT be set as a status code in a Close
    /// control frame by an endpoint. It is designated for use in applications
    /// expecting a status code to indicate that no status code was actually
    /// present.
    NoStatusReceived,
    /// 1006 is a reserved value and MUST NOT be set as a status code in a Close
    /// control frame by an endpoint. It is designated for use in applications
    /// expecting a status code to indicate that the connection was closed
    /// abnormally, e.g., without sending or receiving a Close control frame.
    AbnormalClosure,
    /// 1007 indicates that an endpoint is terminating the connection because it
    /// has received data within a message that was not consistent with the type
    /// of the message (e.g., non-UTF-8 data within a text message).
    InvalidFramePayloadData,
    /// 1008 indicates that an endpoint is terminating the connection because it
    /// has received a message that violates its policy. This is a generic
    /// status code that can be returned when there is no other more suitable
    /// status code (e.g., 1003 or 1009) or if there is a need to hide specific
    /// details about the policy.
    PolicyViolation,
    /// 1009 indicates that an endpoint is terminating the connection because it
    /// has received a message that is too big for it to process.
    MessageTooBig,
    /// 1010 indicates that an endpoint (client) is terminating the connection
    /// because it has expected the server to negotiate one or more extension,
    /// but the server didn't return them in the response message of the
    /// WebSocket handshake. The list of extensions that are needed SHOULD
    /// appear in the /reason/ part of the Close frame. Note that this status
    /// code is not used by the server, because it can fail the WebSocket
    /// handshake instead.
    MandatoryExtension,
    /// 1011 indicates that a server is terminating the connection because it
    /// encountered an unexpected condition that prevented it from fulfilling
    /// the request.
    InternalServerError,
    /// 1015 is a reserved value and MUST NOT be set as a status code in a Close
    /// control frame by an endpoint. It is designated for use in applications
    /// expecting a status code to indicate that the connection was closed due
    /// to a failure to perform a TLS handshake (e.g., the server certificate
    /// can't be verified).
    TlsHandshake,
    /// Status codes in the range 1000-2999 are reserved for definition by this
    /// protocol, its future revisions, and extensions specified in a permanent
    /// and readily available public specification.
    ReservedForStandards(u16),
    /// Status codes in the range 3000-3999 are reserved for use by libraries,
    /// frameworks, and applications. These status codes are registered
    /// directly with IANA. The interpretation of these codes is undefined by
    /// this protocol.
    Libraries(u16),
    /// Status codes in the range 4000-4999 are reserved for private use and
    /// thus can't be registered. Such codes can be used by prior agreements
    /// between WebSocket applications. The interpretation of these codes is
    /// undefined by this protocol.
    Private(u16),
}

impl TryFrom<u16> for CloseCode {
    type Error = ProtocolError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            1000 => Ok(Self::NormalClosure),
            1001 => Ok(Self::GoingAway),
            1002 => Ok(Self::ProtocolError),
            1003 => Ok(Self::UnsupportedData),
            1004 => Ok(Self::Reserved),
            1005 => Ok(Self::NoStatusReceived),
            1006 => Ok(Self::AbnormalClosure),
            1007 => Ok(Self::InvalidFramePayloadData),
            1008 => Ok(Self::PolicyViolation),
            1009 => Ok(Self::MessageTooBig),
            1010 => Ok(Self::MandatoryExtension),
            1011 => Ok(Self::InternalServerError),
            1015 => Ok(Self::TlsHandshake),
            1012..=1014 | 1016..=2999 => Ok(Self::ReservedForStandards(value)),
            3000..=3999 => Ok(Self::Libraries(value)),
            4000..=4999 => Ok(Self::Private(value)),
            _ => Err(ProtocolError::InvalidCloseCode),
        }
    }
}

impl From<CloseCode> for u16 {
    fn from(value: CloseCode) -> Self {
        match value {
            CloseCode::NormalClosure => 1000,
            CloseCode::GoingAway => 1001,
            CloseCode::ProtocolError => 1002,
            CloseCode::UnsupportedData => 1003,
            CloseCode::Reserved => 1004,
            CloseCode::NoStatusReceived => 1005,
            CloseCode::AbnormalClosure => 1006,
            CloseCode::InvalidFramePayloadData => 1007,
            CloseCode::PolicyViolation => 1008,
            CloseCode::MessageTooBig => 1009,
            CloseCode::MandatoryExtension => 1010,
            CloseCode::InternalServerError => 1011,
            CloseCode::TlsHandshake => 1015,
            CloseCode::ReservedForStandards(value)
            | CloseCode::Libraries(value)
            | CloseCode::Private(value) => value,
        }
    }
}

impl CloseCode {
    /// Whether the close code is allowed to be used, i.e. not in the reserved
    /// ranges specified [by the RFC](https://datatracker.ietf.org/doc/html/rfc6455#section-7.4.2).
    pub(super) fn is_allowed(&self) -> bool {
        !matches!(
            self,
            Self::Reserved
                | Self::NoStatusReceived
                | Self::AbnormalClosure
                | Self::TlsHandshake
                | Self::ReservedForStandards(_)
        )
    }
}

/// A websocket message. This is cheaply clonable and uses [`Bytes`] as the
/// payload storage underneath.
///
/// Received messages are always validated prior to dealing with them, so all
/// the type casting methods are either almost or fully zero cost.
#[derive(Debug, Clone)]
pub struct Message {
    /// The [`OpCode`] of the message.
    pub(super) opcode: OpCode,
    /// The payload of the message.
    pub(super) payload: Bytes,
}

impl Message {
    /// Default close message.
    #[allow(clippy::declare_interior_mutable_const)]
    pub(super) const DEFAULT_CLOSE: Self = Self {
        opcode: OpCode::Close,
        payload: Bytes::from_static(&1000_u16.to_be_bytes()),
    };

    /// Returns the raw [`OpCode`] and payload of the message and consumes it.
    pub(super) fn into_raw(self) -> (OpCode, Bytes) {
        (self.opcode, self.payload)
    }

    /// Create a new text message.
    #[must_use]
    pub fn text(payload: String) -> Self {
        Self {
            opcode: OpCode::Text,
            payload: payload.into(),
        }
    }

    /// Create a new binary message.
    #[must_use]
    pub fn binary<D: Into<Bytes>>(payload: D) -> Self {
        Self {
            opcode: OpCode::Binary,
            payload: payload.into(),
        }
    }

    /// Create a new close message. If an non-empty reason is specified, a
    /// [`CloseCode`] must be specified for it to be included.
    #[must_use]
    pub fn close(code: Option<CloseCode>, reason: &str) -> Self {
        let mut payload = BytesMut::with_capacity((2 + reason.len()) * usize::from(code.is_some()));

        if let Some(code) = code {
            payload.put_u16(code.into());

            payload.extend_from_slice(reason.as_bytes());
        }

        Self {
            opcode: OpCode::Close,
            payload: payload.freeze(),
        }
    }

    /// Create a new ping message.
    #[must_use]
    pub fn ping<D: Into<Bytes>>(payload: D) -> Self {
        Self {
            opcode: OpCode::Ping,
            payload: payload.into(),
        }
    }

    /// Create a new pong message.
    #[must_use]
    pub fn pong<D: Into<Bytes>>(payload: D) -> Self {
        Self {
            opcode: OpCode::Pong,
            payload: payload.into(),
        }
    }

    /// Whether the message is a text message.
    #[must_use]
    pub fn is_text(&self) -> bool {
        self.opcode == OpCode::Text
    }

    /// Whether the message is a binary message.
    #[must_use]
    pub fn is_binary(&self) -> bool {
        self.opcode == OpCode::Binary
    }

    /// Whether the message is a close message.
    #[must_use]
    pub fn is_close(&self) -> bool {
        self.opcode == OpCode::Close
    }

    /// Whether the message is a ping message.
    #[must_use]
    pub fn is_ping(&self) -> bool {
        self.opcode == OpCode::Ping
    }

    /// Whether the message is a pong message.
    #[must_use]
    pub fn is_pong(&self) -> bool {
        self.opcode == OpCode::Pong
    }

    /// Returns the message payload and consumes the message, regardless of
    /// type.
    #[must_use]
    pub fn into_payload(self) -> Bytes {
        self.payload
    }

    /// Returns a reference to the message payload, regardless of message type.
    pub fn as_payload(&self) -> &Bytes {
        &self.payload
    }

    /// Returns a reference to the message payload as a string if it is a text
    /// message or a binary message and valid UTF-8.
    ///
    /// For text messages, this is just a free transmutation because UTF-8 is
    /// already validated previously.
    ///
    /// # Errors
    ///
    /// This method returns a [`ProtocolError`] if the message is neither text
    /// or binary or binary and invalid UTF-8.
    pub fn as_text(&self) -> Result<&str, ProtocolError> {
        match self.opcode {
            // SAFETY: UTF-8 is validated by the Decoder and/or when the message is assembled from
            // frames in the case of text messages.
            OpCode::Text => Ok(unsafe { std::str::from_utf8_unchecked(&self.payload) }),
            OpCode::Binary => Ok(utf8::parse_str(&self.payload)?),
            _ => Err(ProtocolError::MessageHasWrongOpcode),
        }
    }

    /// Returns the [`CloseCode`] and close reason if the message is a close
    /// message.
    ///
    /// # Errors
    ///
    /// This method returns a [`ProtocolError`] if the message is not a close
    /// message.
    pub fn as_close(&self) -> Result<(CloseCode, &str), ProtocolError> {
        if self.opcode == OpCode::Close {
            let close_code = if self.payload.len() >= 2 {
                // SAFETY: self.data.len() is greater or equal to 2
                let close_code_value = u16::from_be_bytes(unsafe {
                    self.payload
                        .get_unchecked(0..2)
                        .try_into()
                        .unwrap_unchecked()
                });
                CloseCode::try_from(close_code_value)?
            } else {
                CloseCode::NoStatusReceived
            };

            let reason = if self.payload.len() > 2 {
                // SAFETY: self.data.len() is greater or equal to 2
                unsafe { std::str::from_utf8_unchecked(&self.payload.get_unchecked(2..)) }
            } else {
                ""
            };

            Ok((close_code, reason))
        } else {
            Err(ProtocolError::MessageHasWrongOpcode)
        }
    }
}

impl From<&ProtocolError> for Message {
    fn from(val: &ProtocolError) -> Self {
        match val {
            ProtocolError::InvalidUtf8 => {
                Message::close(Some(CloseCode::InvalidFramePayloadData), "invalid utf8")
            }
            _ => Message::close(Some(CloseCode::ProtocolError), "protocol violation"),
        }
    }
}

/// Configuration for limitations on reading of [`Message`]s from a
/// [`WebsocketStream`] to prevent high memory usage caused by malicious actors.
///
/// [`WebsocketStream`]: super::WebsocketStream
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// The maximum allowed frame size. `None` equals no limit. The default is
    /// 16 MiB.
    pub max_frame_size: Option<usize>,
    /// The maximum allowed message size. `None` equals no limit. The default is
    /// 64 MiB.
    pub max_message_size: Option<usize>,
}

impl Limits {
    /// A limit configuration without any limits.
    #[must_use]
    pub fn unlimited() -> Self {
        Self {
            max_frame_size: None,
            max_message_size: None,
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_frame_size: Some(16 * 1024 * 1024),
            max_message_size: Some(64 * 1024 * 1024),
        }
    }
}

/// Role assumed by the [`WebsocketStream`] in a connection.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum Role {
    /// The client end.
    Client,
    /// The server end.
    Server,
}

/// The connection state of the stream.
#[derive(Debug, PartialEq)]
pub(super) enum StreamState {
    /// The connection is fully active and no close has been initiated.
    Active,
    /// The connection has been closed by the peer, but not yet acknowledged by
    /// us.
    ClosedByPeer,
    /// The connection has been closed by us, but not yet acknowledged.
    ClosedByUs,
    /// The close has been acknowledged by the end that did not initiate the
    /// close.
    CloseAcknowledged,
}

/// A frame of a websocket [`Message`].
#[derive(Debug)]
pub(super) struct Frame {
    /// The [`OpCode`] of the frame.
    pub opcode: OpCode,
    /// Whether this is the last frame of a message.
    pub is_final: bool,
    /// The payload bytes of the frame.
    pub payload: Bytes,
}
