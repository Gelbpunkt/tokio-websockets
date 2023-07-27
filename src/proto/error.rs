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

impl ProtocolError {
    /// Stringify this variant.
    pub(super) const fn as_str(&self) -> &'static str {
        match self {
            ProtocolError::FragmentedControlFrame => "fragmented control frame",
            ProtocolError::InvalidCloseCode => "invalid close code",
            ProtocolError::InvalidOpcode => "invalid opcode",
            ProtocolError::InvalidPayloadLength => "invalid payload length",
            ProtocolError::InvalidRsv => "invalid extension",
            ProtocolError::InvalidUtf8 => "invalid utf-8",
            ProtocolError::MessageHasWrongOpcode => {
                "attempted to treat message data in invalid way"
            }
            ProtocolError::UnexpectedMaskedFrame => "unexpected masked frame",
            ProtocolError::UnexpectedUnmaskedFrame => "unexpected unmasked frame",
        }
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
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
