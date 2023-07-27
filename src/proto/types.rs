//! Types required for the websocket protocol implementation.
use std::{hint::unreachable_unchecked, mem::replace, num::NonZeroU16, slice::Chunks};

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

/// Close status code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CloseCode(NonZeroU16);

// rustfmt reorders these alphabetically
#[rustfmt::skip]
impl CloseCode {
    /// Normal closure, meaning that the purpose for which the connection was
    /// established has been fulfilled.
    pub const NORMAL_CLOSURE: Self = Self(unsafe { NonZeroU16::new_unchecked(1000) });
    /// Endpoint is "going away", such as a server going down or a browser
    /// having navigated away from a page.
    pub const GOING_AWAY: Self = Self(unsafe { NonZeroU16::new_unchecked(1001) });
    /// Endpoint is terminating the connection due to a protocol error.
    pub const PROTOCOL_ERROR: Self = Self(unsafe { NonZeroU16::new_unchecked(1002) });
    /// Endpoint is terminating the connection because it has received a type of
    /// data it cannot accept.
    pub const UNSUPPORTED_DATA: Self = Self(unsafe { NonZeroU16::new_unchecked(1003) });
    /// No status code was actually present.
    pub const NO_STATUS_RECEIVED: Self = Self(unsafe { NonZeroU16::new_unchecked(1005) });
    /// Endpoint is terminating the connection because it has received data
    /// within a message that was not consistent with the type of the message.
    pub const INVALID_FRAME_PAYLOAD_DATA: Self = Self(unsafe { NonZeroU16::new_unchecked(1007) });
    /// Endpoint is terminating the connection because it has received a message
    /// that violates its policy.
    pub const POLICY_VIOLATION: Self = Self(unsafe { NonZeroU16::new_unchecked(1008) });
    /// Endpoint is terminating the connection because it has received a message
    /// that is too big for it to process.
    pub const MESSAGE_TOO_BIG: Self = Self(unsafe { NonZeroU16::new_unchecked(1009) });
    /// Client is terminating the connection because it has expected the server
    /// to negotiate one or more extension, but the server didn't return them in
    /// the response message of the Websocket handshake.
    pub const MANDATORY_EXTENSION: Self = Self(unsafe { NonZeroU16::new_unchecked(1010) });
    /// Server is terminating the connection because it encountered an
    /// unexpected condition that prevented it from fulfilling the request.
    pub const INTERNAL_SERVER_ERROR: Self = Self(unsafe { NonZeroU16::new_unchecked(1011) });
    /// Service is restarted. A client may reconnect, and if it choses to do,
    /// should reconnect using a randomized delay of 5--30s.
    pub const SERVICE_RESTART: Self = Self(unsafe { NonZeroU16::new_unchecked(1012) });
    /// Service is experiencing overload. A client should only connect to a
    /// different IP (when there are multiple for the target) or reconnect to
    /// the same IP upon user action.
    pub const SERVICE_OVERLOAD: Self = Self(unsafe { NonZeroU16::new_unchecked(1013) });
    /// The server was acting as a gateway or proxy and received an invalid
    /// response from the upstream server. This is similar to the HTTP 502
    /// status code.
    pub const BAD_GATEWAY: Self = Self(unsafe { NonZeroU16::new_unchecked(1014) });
}

impl CloseCode {
    /// Whether the close code is allowed to be sent over the wire.
    pub(super) fn is_sendable(self) -> bool {
        match self.0.get() {
            1004 | 1005 | 1006 | 1015 => false,
            1000..=4999 => true,
            // SAFETY: `TryFrom` is the only way to accquire self and it errors for these values
            0..=999 | 5000..=u16::MAX => unsafe { unreachable_unchecked() },
        }
    }
}

impl From<CloseCode> for u16 {
    fn from(value: CloseCode) -> Self {
        value.0.get()
    }
}

impl TryFrom<u16> for CloseCode {
    type Error = ProtocolError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            // SAFETY: We just checked that the value is non-zero
            1000..=1015 | 3000..=4999 => Ok(Self(unsafe { NonZeroU16::new_unchecked(value) })),
            0..=999 | 1016..=2999 | 5000..=u16::MAX => Err(ProtocolError::InvalidCloseCode),
        }
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
        payload: Bytes::from_static(&CloseCode::NORMAL_CLOSURE.0.get().to_be_bytes()),
    };

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
                CloseCode::NO_STATUS_RECEIVED
            };

            let reason = if self.payload.len() > 2 {
                // SAFETY: self.data.len() is greater or equal to 2
                unsafe { std::str::from_utf8_unchecked(self.payload.get_unchecked(2..)) }
            } else {
                ""
            };

            Ok((close_code, reason))
        } else {
            Err(ProtocolError::MessageHasWrongOpcode)
        }
    }

    /// Returns an iterator over frames of `frame_size` length to split this
    /// message into.
    pub(super) fn as_frames(&self, frame_size: usize) -> MessageFrames<'_> {
        MessageFrames {
            inner: self.payload.chunks(frame_size),
            payload: &self.payload,
            opcode: self.opcode,
        }
    }
}

/// Iterator over frames of a chunked message.
pub(super) struct MessageFrames<'a> {
    /// Iterator over payload chunks.
    inner: Chunks<'a, u8>,
    /// The full message payload this iterates over.
    payload: &'a Bytes,
    /// Opcode for the next frame.
    opcode: OpCode,
}

impl Iterator for MessageFrames<'_> {
    type Item = Frame;

    fn next(&mut self) -> Option<Self::Item> {
        let chunk = if self.opcode == OpCode::Continuation {
            self.inner.next()?
        } else {
            self.inner.next().unwrap_or_default()
        };

        Some(Frame {
            opcode: replace(&mut self.opcode, OpCode::Continuation),
            // TODO: Use ExactSizeIterator::is_empty when stable
            is_final: self.inner.len() == 0,
            payload: self.payload.slice_ref(chunk),
        })
    }
}

impl From<&ProtocolError> for Message {
    fn from(val: &ProtocolError) -> Self {
        match val {
            ProtocolError::InvalidUtf8 => {
                Message::close(Some(CloseCode::INVALID_FRAME_PAYLOAD_DATA), "invalid utf8")
            }
            _ => Message::close(Some(CloseCode::PROTOCOL_ERROR), val.as_str()),
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
    pub(super) max_frame_size: Option<usize>,
    /// The maximum allowed message size. `None` equals no limit. The default is
    /// 64 MiB.
    pub(super) max_message_size: Option<usize>,
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

    /// Sets the maximum allowed frame size. `None` equals no limit. The default
    /// is 16 MiB.
    #[must_use]
    pub fn max_frame_size(mut self, size: Option<usize>) -> Self {
        self.max_frame_size = size;

        self
    }

    /// Sets the maximum allowed message size. `None` equals no limit. The
    /// default is 64 MiB.
    #[must_use]
    pub fn max_message_size(mut self, size: Option<usize>) -> Self {
        self.max_message_size = size;

        self
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

impl From<Message> for Frame {
    fn from(value: Message) -> Self {
        Self {
            opcode: value.opcode,
            is_final: true,
            payload: value.payload,
        }
    }
}
