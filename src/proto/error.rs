//! Websocket protocol error type.
use std::{fmt, string::FromUtf8Error};

/// Error encountered on protocol violations by the other end of the connection.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug)]
#[non_exhaustive]
pub enum ProtocolError {
    /// A fragmented control frame was received.
    FragmentedControlFrame,
    /// An invalid close code has been received.
    InvalidCloseCode,
    /// An invalid opcode was received.
    InvalidOpcode,
    /// An invalid payload length was received.
    InvalidPayloadLength,
    /// An invalid RSV was received. This is used by extensions, which are
    /// currently unsupported.
    InvalidRsv,
    /// An invalid UTF-8 segment was received when valid UTF-8 was expected.
    InvalidUtf8,
    /// A message has an opcode that did not match the attempted interpretation
    /// of the data. Encountered for example when attempting to use
    /// [`Message::as_close`] on a text message.
    ///
    /// [`Message::as_close`]: `super::Message::as_close`
    MessageHasWrongOpcode,
    // A masked frame was unexpectedly received.
    UnexpectedMaskedFrame,
    /// An unmasked frame was unexpectedly received.
    UnexpectedUnmaskedFrame,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolError::FragmentedControlFrame => f.write_str("fragmented control frame"),
            ProtocolError::InvalidCloseCode => f.write_str("invalid close code"),
            ProtocolError::InvalidOpcode => f.write_str("invalid opcode"),
            ProtocolError::InvalidPayloadLength => f.write_str("invalid payload length"),
            ProtocolError::InvalidRsv => f.write_str("invalid extension"),
            ProtocolError::InvalidUtf8 => f.write_str("invalid utf-8"),
            ProtocolError::MessageHasWrongOpcode => {
                f.write_str("attempted to treat message data in invalid way")
            }
            ProtocolError::UnexpectedMaskedFrame => f.write_str("unexpected masked frame"),
            ProtocolError::UnexpectedUnmaskedFrame => f.write_str("unexpected unmasked frame"),
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
