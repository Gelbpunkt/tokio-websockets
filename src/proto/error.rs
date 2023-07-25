//! Websocket protocol error type.
use std::{fmt, string::FromUtf8Error};

/// Error encountered on protocol violations by the other end of the connection.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug)]
#[non_exhaustive]
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
    ///
    /// [`Message::as_close`]: `super::Message::as_close`
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
