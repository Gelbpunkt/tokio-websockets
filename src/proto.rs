//! This module contains a correct and complete implementation of [RFC6455](https://datatracker.ietf.org/doc/html/rfc6455).
//!
//! Any extensions are currently not implemented.
use std::{
    collections::VecDeque,
    fmt,
    future::poll_fn,
    hint::unreachable_unchecked,
    mem::{replace, take},
    pin::Pin,
    string::FromUtf8Error,
    task::{ready, Context, Poll},
};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use futures_core::Stream;
use futures_sink::Sink;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{Decoder, Encoder, Framed};

use crate::{mask, utf8, Error};

/// Outgoing messages are split into frames of this size.
const FRAME_SIZE: usize = 4096;

/// The opcode of a websocket frame. It denotes the type of the frame or an
/// assembled message.
///
/// A fully assembled [`Message`] will never have a continuation opcode.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum OpCode {
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
    fn is_control(self) -> bool {
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

/// A frame of a websocket [`Message`].
#[derive(Debug)]
struct Frame {
    /// The [`OpCode`] of the frame.
    opcode: OpCode,
    /// Whether this is the last frame of a message.
    is_final: bool,
    /// The payload bytes of the frame.
    payload: Bytes,
}

/// Error encountered on protocol violations by the other end of the connection.
#[derive(Debug)]
pub enum ProtocolError {
    /// An invalid close code (smaller than 1000 or greater or equal than 5000)
    /// has been received.
    InvalidCloseCode,
    /// A close frame with payload length of one byte has been received.
    InvalidCloseSequence,
    /// An invalid opcode was received.
    InvalidOpcode,
    /// An invalid RSV (not equal to 0) was received. This is used for
    /// extensions, which are currently not supported.
    InvalidRsv,
    /// An invalid payload length byte was received, i.e. payload length is not
    /// in 8-, 16- or 64-bit range.
    InvalidPayloadLength,
    /// Invalid UTF-8 was received when valid UTF-8 was expected, for example in
    /// text messages.
    InvalidUtf8,
    /// A message was received with a continuation opcode. This error should
    /// never be encountered due to the nature of the library.
    DisallowedOpcode,
    /// A close message with reserved close code was received.
    DisallowedCloseCode,
    /// A message has an opcode that did not match the attempted interpretation
    /// of the data. Encountered for example when attempting to use
    /// [`Message::as_close`] on a text message.
    MessageHasWrongOpcode,
    /// The server on the other end masked the payload.
    ServerMaskedData,
    /// A control frame with a payload length greater than 255 bytes was
    /// received.
    InvalidControlFrameLength,
    /// A fragemented control frame was received.
    FragmentedControlFrame,
    /// A continuation frame was received when a message start frame with
    /// non-continuation opcode was expected.
    UnexpectedContinuation,
    /// A non-continuation and non-control frame was received when the previous
    /// message was not fully received yet.
    UnfinishedMessage,
    /// The client on the other end did not mask the payload.
    UnmaskedData,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolError::InvalidCloseCode => f.write_str("invalid close code"),
            ProtocolError::InvalidCloseSequence => f.write_str("invalid close sequence"),
            ProtocolError::InvalidOpcode => f.write_str("invalid opcode"),
            ProtocolError::InvalidRsv => f.write_str("invalid or unsupported RSV"),
            ProtocolError::InvalidPayloadLength => f.write_str("invalid payload length"),
            ProtocolError::InvalidUtf8 => f.write_str("invalid utf-8"),
            ProtocolError::DisallowedOpcode => f.write_str("disallowed opcode"),
            ProtocolError::DisallowedCloseCode => f.write_str("disallowed close code"),
            ProtocolError::MessageHasWrongOpcode => {
                f.write_str("attempted to treat message data in invalid way")
            }
            ProtocolError::ServerMaskedData => f.write_str("server masked frame"),
            ProtocolError::InvalidControlFrameLength => f.write_str("invalid control frame length"),
            ProtocolError::FragmentedControlFrame => f.write_str("fragmented control frame"),
            ProtocolError::UnexpectedContinuation => f.write_str("unexpected continuation"),
            ProtocolError::UnfinishedMessage => f.write_str("unfinished message"),
            ProtocolError::UnmaskedData => f.write_str("client did not mask frame"),
        }
    }
}

impl std::error::Error for ProtocolError {}

impl From<&ProtocolError> for Message {
    fn from(val: &ProtocolError) -> Self {
        match val {
            ProtocolError::InvalidUtf8 => Message::close(
                Some(CloseCode::InvalidFramePayloadData),
                Some("invalid utf8"),
            ),
            _ => Message::close(Some(CloseCode::ProtocolError), Some("protocol violation")),
        }
    }
}

impl From<FromUtf8Error> for ProtocolError {
    fn from(_: FromUtf8Error) -> Self {
        Self::InvalidUtf8
    }
}

impl From<std::str::Utf8Error> for ProtocolError {
    fn from(_: std::str::Utf8Error) -> Self {
        Self::InvalidUtf8
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
    fn is_allowed(&self) -> bool {
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
    opcode: OpCode,
    /// The payload of the message.
    data: Bytes,
}

impl Message {
    /// Assembles and verifies a message from raw message payload and
    /// [`OpCode`].
    ///
    /// # Errors
    ///
    /// This method returns an [`Error`] if the message has a continuation
    /// opcode or a disallowed close code.
    fn from_raw(opcode: OpCode, data: Bytes) -> Result<Self, ProtocolError> {
        match opcode {
            OpCode::Continuation => Err(ProtocolError::DisallowedOpcode),
            OpCode::Text | OpCode::Binary | OpCode::Ping | OpCode::Pong => {
                Ok(Self { opcode, data })
            }
            OpCode::Close => {
                if data.is_empty() {
                    Ok(Self { opcode, data })
                } else {
                    // SAFETY: The Decoder ensures that close frames consist of at least two bytes
                    // A conversion from two u8s to a u16 cannot fail.
                    let close_code_value = u16::from_be_bytes(unsafe {
                        data.get_unchecked(0..2).try_into().unwrap_unchecked()
                    });
                    let close_code = CloseCode::try_from(close_code_value)?;

                    // Verify that the close code is allowed
                    if !close_code.is_allowed() {
                        return Err(ProtocolError::DisallowedCloseCode);
                    }

                    // Verify that the reason is allowed
                    if data.len() > 2 {
                        // SAFETY: The Decoder ensures that close frames consist of at least two
                        // bytes
                        utf8::parse_str(unsafe { data.get_unchecked(2..) })?;
                    }

                    Ok(Self { opcode, data })
                }
            }
        }
    }

    /// Returns the raw [`OpCode`] and payload of the message and consumes it.
    fn into_raw(self) -> (OpCode, Bytes) {
        (self.opcode, self.data)
    }

    /// Create a new text message.
    #[must_use]
    pub fn text(data: String) -> Self {
        Self {
            opcode: OpCode::Text,
            data: data.into(),
        }
    }

    /// Create a new binary message.
    #[must_use]
    pub fn binary<D: Into<Bytes>>(data: D) -> Self {
        Self {
            opcode: OpCode::Binary,
            data: data.into(),
        }
    }

    /// Create a new close message. If a reason is specified, a [`CloseCode`]
    /// must be specified for it to be included in the payload.
    #[must_use]
    pub fn close(code: Option<CloseCode>, reason: Option<&str>) -> Self {
        let mut data = BytesMut::new();

        if let Some(code) = code {
            data.put_u16(code.into());

            if let Some(reason) = reason {
                data.extend_from_slice(reason.as_bytes());
            }
        }

        Self {
            opcode: OpCode::Close,
            data: data.freeze(),
        }
    }

    /// Create a new ping message.
    #[must_use]
    pub fn ping<D: Into<Bytes>>(data: D) -> Self {
        Self {
            opcode: OpCode::Ping,
            data: data.into(),
        }
    }

    /// Create a new pong message.
    #[must_use]
    pub fn pong<D: Into<Bytes>>(data: D) -> Self {
        Self {
            opcode: OpCode::Pong,
            data: data.into(),
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
    pub fn into_data(self) -> Bytes {
        self.data
    }

    /// Returns a reference to the message payload, regardless of message type.
    pub fn as_data(&self) -> &Bytes {
        &self.data
    }

    /// Returns a reference to the message payload as a string if it is a text
    /// message or a binary message and valid UTF-8.
    ///
    /// For text messages, this is just a free transmutation because UTF-8 is
    /// already validated previously.
    ///
    /// # Errors
    ///
    /// This method returns an [`Error`] if the message is neither text or
    /// binary or binary and invalid UTF-8.
    pub fn as_text(&self) -> Result<&str, ProtocolError> {
        match self.opcode {
            // SAFETY: UTF-8 is validated by the Decoder and/or when the message is assembled from
            // frames in the case of text messages.
            OpCode::Text => Ok(unsafe { std::str::from_utf8_unchecked(&self.data) }),
            OpCode::Binary => Ok(utf8::parse_str(&self.data)?),
            _ => Err(ProtocolError::MessageHasWrongOpcode),
        }
    }

    /// Returns the [`CloseCode`] and close reason if the message is a close
    /// message.
    ///
    /// # Errors
    ///
    /// This method returns an [`Error`] if the message is not a close message.
    pub fn as_close(&self) -> Result<(Option<CloseCode>, Option<&str>), ProtocolError> {
        if self.opcode == OpCode::Close {
            let close_code = if self.data.len() >= 2 {
                // SAFETY: self.data.len() is greater or equal to 2
                let close_code_value = u16::from_be_bytes(unsafe {
                    self.data.get_unchecked(0..2).try_into().unwrap_unchecked()
                });
                Some(CloseCode::try_from(close_code_value)?)
            } else {
                None
            };

            let reason = if self.data.len() > 2 {
                // SAFETY: self.data.len() is greater or equal to 2
                Some(unsafe { std::str::from_utf8_unchecked(self.data.get_unchecked(2..)) })
            } else {
                None
            };

            Ok((close_code, reason))
        } else {
            Err(ProtocolError::MessageHasWrongOpcode)
        }
    }
}

/// The connection state of the stream.
#[derive(Debug)]
#[allow(unused)]
enum StreamState {
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

/// Configuration for limitations on reading of [`Message`]s from a
/// [`WebsocketStream`] to prevent high memory usage caused by malicious actors.
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

/// A websocket stream that full messages can be read from and written to.
///
/// The stream implements [`futures_sink::Sink`] and [`futures_core::Stream`].
///
/// You must use a [`ClientBuilder`] or [`ServerBuilder`] to
/// obtain a websocket stream.
///
/// For usage examples, see the top level crate documentation, which showcases a
/// simple echo server and client.
///
/// [`ClientBuilder`]: crate::ClientBuilder
/// [`ServerBuilder`]: crate::ServerBuilder
#[derive(Debug)]
pub struct WebsocketStream<T> {
    /// The underlying stream using the [`WebsocketProtocol`] to read and write
    /// full frames.
    inner: Framed<T, WebsocketProtocol>,

    /// Payload of the full message that is being assembled.
    partial_payload: BytesMut,
    /// Opcode of the full message that is being assembled.
    partial_opcode: OpCode,

    /// Index up to which the full message payload was validated to be valid
    /// UTF-8.
    utf8_valid_up_to: usize,

    /// Whether the sink needs to be flushed.
    needs_flush: bool,

    /// Pending messages to be encoded once the sink is ready.
    pending_messages: VecDeque<Message>,
}

impl<T> WebsocketStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    /// Create a new [`WebsocketStream`] from a raw stream.
    #[cfg(any(feature = "client", feature = "server"))]
    pub(crate) fn from_raw_stream(
        stream: T,
        role: Role,
        limits: Limits,
        fail_fast_on_invalid_utf8: bool,
    ) -> Self {
        Self {
            inner: WebsocketProtocol {
                role,
                limits,
                state: StreamState::Active,
                payload_in: 0,
                utf8_valid_up_to: if fail_fast_on_invalid_utf8 {
                    Some(0)
                } else {
                    None
                },
            }
            .framed(stream),
            partial_payload: BytesMut::new(),
            partial_opcode: OpCode::Continuation,
            utf8_valid_up_to: 0,
            needs_flush: false,
            pending_messages: VecDeque::new(),
        }
    }

    /// Create a new [`WebsocketStream`] from an existing [`Framed`]. This
    /// allows for reusing the internal buffer of the [`Framed`] object.
    #[cfg(any(feature = "client", feature = "server"))]
    pub(crate) fn from_framed<U>(
        framed: Framed<T, U>,
        role: Role,
        limits: Limits,
        fail_fast_on_invalid_utf8: bool,
    ) -> Self {
        Self {
            inner: framed.map_codec(|_| WebsocketProtocol {
                role,
                limits,
                state: StreamState::Active,
                payload_in: 0,
                utf8_valid_up_to: if fail_fast_on_invalid_utf8 {
                    Some(0)
                } else {
                    None
                },
            }),
            partial_payload: BytesMut::new(),
            partial_opcode: OpCode::Continuation,
            utf8_valid_up_to: 0,
            needs_flush: false,
            pending_messages: VecDeque::new(),
        }
    }

    /// Assemble the next raw [`Message`] parts from the websocket stream from
    /// intermediate frames from the codec.
    ///
    /// # Errors
    ///
    /// This method returns an [`Error`] if reading from the stream fails or a
    /// protocol violation is encountered.
    fn poll_read_next_message(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<(OpCode, Bytes), Error>>> {
        loop {
            let frame = match ready!(Pin::new(&mut self.inner).poll_next(cx)) {
                Some(Ok(frame)) => frame,
                Some(Err(e)) => return Poll::Ready(Some(Err(e))),
                None => return Poll::Ready(None),
            };

            // Control frames are allowed in between other frames
            if frame.opcode.is_control() {
                return Poll::Ready(Some(Ok((frame.opcode, frame.payload))));
            }

            if self.partial_opcode == OpCode::Continuation {
                if frame.opcode == OpCode::Continuation {
                    return Poll::Ready(Some(Err(Error::Protocol(
                        ProtocolError::UnexpectedContinuation,
                    ))));
                }

                self.partial_opcode = frame.opcode;
            } else if frame.opcode != OpCode::Continuation {
                return Poll::Ready(Some(Err(Error::Protocol(ProtocolError::UnfinishedMessage))));
            }

            if let Some(max_message_size) = self.inner.codec().limits.max_message_size {
                let message_size = self.partial_payload.len() + frame.payload.len();

                if message_size > max_message_size {
                    return Poll::Ready(Some(Err(Error::MessageTooLong {
                        size: message_size,
                        max_size: max_message_size,
                    })));
                }
            }

            self.partial_payload.extend_from_slice(&frame.payload);

            if self.partial_opcode == OpCode::Text {
                // SAFETY: self.utf8_valid_up_to is an index in self.partial_payload and cannot
                // exceed its length
                let (should_fail, valid_up_to) = utf8::should_fail_fast(
                    unsafe { self.partial_payload.get_unchecked(self.utf8_valid_up_to..) },
                    frame.is_final,
                );

                if should_fail {
                    return Poll::Ready(Some(Err(Error::Protocol(ProtocolError::InvalidUtf8))));
                }

                self.utf8_valid_up_to += valid_up_to;
            }

            if frame.is_final {
                break;
            }
        }

        let opcode = replace(&mut self.partial_opcode, OpCode::Continuation);
        let payload = take(&mut self.partial_payload).freeze();

        self.utf8_valid_up_to = 0;

        Poll::Ready(Some(Ok((opcode, payload))))
    }

    /// Attempts to write a message to the sink immediately. If if can't be done
    /// immediately, it is queued for sending later.
    fn try_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        message: Message,
    ) -> Result<(), Error> {
        // Make sure that the sink can be written to
        if self.as_mut().poll_ready(cx).is_pending() {
            // Postpone it
            self.pending_messages.push_back(message);
            return Ok(());
        }

        // Encode it into the buffer
        self.as_mut().start_send(message)?;

        // Attempt to write other pending messages
        self.as_mut().try_write_pending(cx)?;

        // Attempt to flush, and postpone it if pending
        if self.as_mut().poll_flush(cx).is_pending() {
            self.needs_flush = true;
        }

        Ok(())
    }

    /// Attempts to write all pending messages to the sink.
    fn try_write_pending(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Result<(), Error> {
        while !self.pending_messages.is_empty() {
            // Make sure that the sink can be written to
            if self.as_mut().poll_ready(cx).is_pending() {
                break;
            };

            // SAFETY: We just ensured that the pending_messages are not empty
            let item = unsafe { self.pending_messages.pop_front().unwrap_unchecked() };

            // Encode it into the buffer
            self.as_mut().start_send(item)?;

            self.needs_flush = true;
        }

        // If an item is buffered, but not written yet, flush the transport
        if self.needs_flush {
            // We will try flush again on the next invocation if it is pending
            match self.as_mut().poll_flush(cx) {
                Poll::Ready(Ok(_)) => self.needs_flush = false,
                Poll::Ready(Err(e)) => return Err(e),
                Poll::Pending => {}
            }
        }

        Ok(())
    }

    /// Send a close [`Message`] with an optional [`CloseCode`] and reason for
    /// closure.
    ///
    /// The reason will only be included in the sent payload if a close code was
    /// specified.
    ///
    /// # Errors
    ///
    /// This method returns an [`Error`] if sending the close frame fails.
    pub async fn close(
        &mut self,
        close_code: Option<CloseCode>,
        reason: Option<&str>,
    ) -> Result<(), Error> {
        let mut item = Some(Message::close(close_code, reason));
        let mut this = Pin::new(self);
        poll_fn(|cx| {
            if item.is_some() {
                ready!(this.as_mut().poll_ready(cx)?);
            }
            if let Some(item) = item.take() {
                this.as_mut().start_send(item)?;
            }
            this.as_mut().poll_flush(cx)
        })
        .await
    }
}

impl<T> Stream for WebsocketStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    type Item = Result<Message, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.inner.codec().state {
            StreamState::Active | StreamState::ClosedByUs => {}
            StreamState::ClosedByPeer => {
                ready!(self.as_mut().poll_ready(cx))?;
                self.as_mut().try_write_pending(cx)?;
                ready!(self.as_mut().poll_flush(cx))?;
                return Poll::Ready(None);
            }
            StreamState::CloseAcknowledged => {
                ready!(self.as_mut().poll_flush(cx))?;
                return Poll::Ready(None);
            }
        }

        self.as_mut().try_write_pending(cx)?;

        let (opcode, payload) = match ready!(self.as_mut().poll_read_next_message(cx)) {
            Some(Ok((opcode, payload))) => (opcode, payload),
            Some(Err(e)) => {
                if let Error::Protocol(protocol) = &e {
                    let close_msg = protocol.into();

                    if let Err(e) = self.try_write(cx, close_msg) {
                        return Poll::Ready(Some(Err(e)));
                    };
                }

                return Poll::Ready(Some(Err(e)));
            }
            None => return Poll::Ready(None),
        };

        let message = match Message::from_raw(opcode, payload) {
            Ok(msg) => msg,
            Err(e) => {
                let close_msg = Message::from(&e);

                if let Err(e) = self.try_write(cx, close_msg) {
                    return Poll::Ready(Some(Err(e)));
                };

                return Poll::Ready(Some(Err(Error::Protocol(e))));
            }
        };

        match &message.opcode {
            OpCode::Close => match self.inner.codec().state {
                StreamState::Active => {
                    self.inner.codec_mut().state = StreamState::ClosedByPeer;
                    if let Err(e) = self.try_write(cx, message.clone()) {
                        return Poll::Ready(Some(Err(e)));
                    };
                }
                StreamState::ClosedByPeer | StreamState::CloseAcknowledged => {
                    return Poll::Ready(None)
                }
                StreamState::ClosedByUs => {
                    self.inner.codec_mut().state = StreamState::CloseAcknowledged;
                }
            },
            OpCode::Ping => {
                let mut msg = message.clone();
                msg.opcode = OpCode::Pong;

                if let Err(e) = self.try_write(cx, msg) {
                    return Poll::Ready(Some(Err(e)));
                };
            }
            _ => {}
        }

        Poll::Ready(Some(Ok(message)))
    }
}

impl<T> Sink<Message> for WebsocketStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    type Error = Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.inner).poll_ready(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
        Pin::new(&mut self.inner).start_send(item)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.inner).poll_close(cx)
    }
}

/// The actual implementation of the websocket byte-level protocol.
/// It provides an [`Encoder`] for entire [`Message`]s and a [`Decoder`] for
/// single frames that must be assembled by a client such as the
/// [`WebsocketStream`] later.
#[derive(Debug)]
struct WebsocketProtocol {
    /// The [`Role`] this implementation should assume for the stream.
    role: Role,
    /// The [`Limits`] imposed on this stream.
    limits: Limits,
    /// The [`StreamState`] of the current stream.
    state: StreamState,
    /// Length of the processed (unmasked) part of the payload.
    payload_in: usize,
    /// Index up to which the payload was validated to be valid UTF-8.
    /// `None` if the UTF-8 validation on partial frames is disabled.
    utf8_valid_up_to: Option<usize>,
}

impl Encoder<Message> for WebsocketProtocol {
    type Error = Error;

    #[allow(clippy::cast_possible_truncation)]
    fn encode(&mut self, item: Message, dst: &mut BytesMut) -> Result<(), Self::Error> {
        if !matches!(self.state, StreamState::Active)
            && !matches!(self.state, StreamState::ClosedByPeer if item.is_close())
        {
            return Err(Error::AlreadyClosed);
        }

        if item.is_close() {
            if matches!(self.state, StreamState::ClosedByPeer) {
                self.state = StreamState::CloseAcknowledged;
            } else {
                self.state = StreamState::ClosedByUs;
            }
        }

        let (opcode, data) = item.into_raw();
        let mut chunks = data.chunks(FRAME_SIZE).peekable();
        let mut next_chunk = Some(chunks.next().unwrap_or_default());
        let mut chunk_number = 0;

        while let Some(chunk) = next_chunk {
            let frame_opcode = if chunk_number == 0 {
                opcode
            } else {
                OpCode::Continuation
            };

            let is_final = chunks.peek().is_none();
            let chunk_size = chunk.len();
            let mask: Option<[u8; 4]> = if self.role == Role::Client {
                #[cfg(feature = "client")]
                {
                    Some(crate::rand::get_mask())
                }
                #[cfg(not(feature = "client"))]
                {
                    // SAFETY: This allows for making the dependency on random generators
                    // only required for clients, servers can avoid it entirely.
                    // Since it is not possible to create a stream with client role
                    // without the client builder (and that is locked behind the client feature),
                    // this branch is impossible to reach.
                    unsafe { std::hint::unreachable_unchecked() }
                }
            } else {
                None
            };
            let mask_bit = 128 * u8::from(mask.is_some());
            let opcode_value: u8 = frame_opcode.into();

            let initial_byte = (u8::from(is_final) << 7) + opcode_value;

            dst.put_u8(initial_byte);

            if u16::try_from(chunk_size).is_err() {
                dst.put_u8(127 + mask_bit);
                dst.put_u64(chunk_size as u64);
            } else if chunk_size > 125 {
                dst.put_u8(126 + mask_bit);
                dst.put_u16(chunk_size as u16);
            } else {
                dst.put_u8(chunk_size as u8 + mask_bit);
            }

            if let Some(mask) = &mask {
                dst.extend_from_slice(mask);
            }

            dst.extend_from_slice(chunk);

            if let Some(mask) = mask {
                let start_of_data = dst.len() - chunk.len();
                // SAFETY: We called dst.extend_from_slice(chunk), so start_of_data is an index
                // in dst, to be exact, the lenth of dst before the extend_from_slice call
                mask::frame(&mask, unsafe { dst.get_unchecked_mut(start_of_data..) }, 0);
            }

            next_chunk = chunks.next();
            chunk_number += 1;
        }

        Ok(())
    }
}

/// Macro that returns with `Ok(None)` early and reserves missing space if not
/// enough bytes are in a specified buffer.
macro_rules! ensure_buffer_has_space {
    ($buf:expr, $space:expr) => {
        if $buf.len() < $space {
            $buf.reserve($space);

            return Ok(None);
        }
    };
}

impl Decoder for WebsocketProtocol {
    type Error = Error;
    type Item = Frame;

    #[allow(clippy::cast_possible_truncation, clippy::too_many_lines)]
    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // Opcode and payload length must be present
        ensure_buffer_has_space!(src, 2);

        // SAFETY: The ensure_buffer_has_space call has validated this
        let fin_and_rsv = unsafe { src.get_unchecked(0) };
        let payload_len_1 = unsafe { src.get_unchecked(1) };

        // Bit 0
        let fin = fin_and_rsv >> 7 != 0;

        // Bits 1-3
        let rsv = fin_and_rsv & 0x70;

        if rsv != 0 {
            return Err(Error::Protocol(ProtocolError::InvalidRsv));
        }

        // Bits 4-7
        let opcode_value = fin_and_rsv & 0xF;
        let opcode = OpCode::try_from(opcode_value)?;

        if !fin && opcode.is_control() {
            return Err(Error::Protocol(ProtocolError::FragmentedControlFrame));
        }

        // Bit 0
        let mask = payload_len_1 >> 7 != 0;

        if mask && self.role == Role::Client {
            return Err(Error::Protocol(ProtocolError::ServerMaskedData));
        }

        if !mask && self.role == Role::Server {
            return Err(Error::Protocol(ProtocolError::UnmaskedData));
        }

        // Bits 1-7
        let mut payload_length = (payload_len_1 & 127) as usize;

        let mut offset = 2;

        if payload_length > 125 {
            if opcode.is_control() {
                return Err(Error::Protocol(ProtocolError::InvalidControlFrameLength));
            }

            if payload_length == 126 {
                ensure_buffer_has_space!(src, offset + 2);
                // SAFETY: The ensure_buffer_has_space call has validated this
                // A conversion from two u8s to a u16 cannot fail
                payload_length = u16::from_be_bytes(unsafe {
                    src.get_unchecked(2..4).try_into().unwrap_unchecked()
                }) as usize;
                if payload_length <= 125 {
                    return Err(Error::Protocol(ProtocolError::InvalidPayloadLength));
                }
                offset = 4;
            } else if payload_length == 127 {
                ensure_buffer_has_space!(src, offset + 8);
                // SAFETY: The ensure_buffer_has_space call has validated this
                // A conversion from 8 u8s to a u64 cannot fail
                payload_length = u64::from_be_bytes(unsafe {
                    src.get_unchecked(2..10).try_into().unwrap_unchecked()
                }) as usize;
                if u16::try_from(payload_length).is_ok() {
                    return Err(Error::Protocol(ProtocolError::InvalidPayloadLength));
                }
                offset = 10;
            } else {
                // SAFETY: Constructed from 7 bits so the max value is 127
                unsafe { unreachable_unchecked() }
            }
        }

        if let Some(max_frame_size) = self.limits.max_frame_size {
            if payload_length > max_frame_size {
                return Err(Error::FrameTooLong {
                    size: payload_length,
                    max_size: max_frame_size,
                });
            }
        }

        // There could be a mask here, but we only load it later,
        // so just increase the offset to calculate the available data
        if mask {
            ensure_buffer_has_space!(src, offset + 4);
            offset += 4;
        }

        if payload_length > 0 {
            // Get the actual payload, if any
            let data_available = (src.len() - offset).min(payload_length);
            let bytes_missing = payload_length - data_available;

            if bytes_missing > 0 {
                // If data is missing, we might have to fail fast on invalid UTF8
                if let Some(utf8_valid_up_to) = &mut self.utf8_valid_up_to {
                    if opcode == OpCode::Text {
                        let to_read = data_available - self.payload_in;

                        // Data might be masked, so unmask it here
                        if mask {
                            let unmasked_until = offset + self.payload_in;

                            // SAFETY: The masking key and the payload do not overlap in src
                            // TODO: Replace with split_at_mut_unchecked when stable
                            let (masking_key, to_unmask) = unsafe {
                                let masking_key_ptr =
                                    src.get_unchecked(offset - 4..offset) as *const [u8];
                                let to_unmask_ptr = src
                                    .get_unchecked_mut(unmasked_until..unmasked_until + to_read)
                                    as *mut [u8];

                                (&*masking_key_ptr, &mut *to_unmask_ptr)
                            };

                            mask::frame(masking_key, to_unmask, self.payload_in & 3);
                        }

                        self.payload_in = data_available;

                        // SAFETY: offset + utf8_valid_up_to is the index until which utf8 was
                        // validated for this frame and therefore guaranteed to be in bounds.
                        // self.payload_in is data_available, which is at most src.len()
                        let (should_fail, valid_up_to) = utf8::should_fail_fast(
                            unsafe {
                                src.get_unchecked(
                                    offset + *utf8_valid_up_to..offset + self.payload_in,
                                )
                            },
                            false,
                        );

                        if should_fail {
                            return Err(Error::Protocol(ProtocolError::InvalidUtf8));
                        }

                        *utf8_valid_up_to += valid_up_to;
                    }
                }

                src.reserve(bytes_missing);

                return Ok(None);
            }

            // Close frames must be at least 2 bytes in length
            if opcode == OpCode::Close && payload_length == 1 {
                return Err(Error::Protocol(ProtocolError::InvalidCloseSequence));
            }

            // Since we only unmasked if data was previously incomplete, unmask the entire
            // rest
            if mask {
                let unmasked_until = offset + self.payload_in;

                // SAFETY: The masking key and the payload do not overlap in src
                // TODO: Replace with split_at_mut_unchecked when stable
                let (masking_key, to_unmask) = unsafe {
                    let masking_key_ptr = src.get_unchecked(offset - 4..offset) as *const [u8];
                    let to_unmask_ptr = src
                        .get_unchecked_mut(unmasked_until..offset + payload_length)
                        as *mut [u8];

                    (&*masking_key_ptr, &mut *to_unmask_ptr)
                };

                mask::frame(masking_key, to_unmask, self.payload_in & 3);
            }
        }

        // Advance the offset into the payload body
        src.advance(offset);
        // Take the payload
        let payload = src.split_to(payload_length).freeze();

        self.payload_in = 0;

        if let Some(valid_up_to) = &mut self.utf8_valid_up_to {
            *valid_up_to = 0;
        };

        let frame = Frame {
            opcode,
            payload,
            is_final: fin,
        };

        Ok(Some(frame))
    }
}
