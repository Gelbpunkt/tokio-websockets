//! Websocket protocol error type.
use std::fmt;

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
    /// A masked frame was unexpectedly received.
    UnexpectedMaskedFrame,
    /// An unmasked frame was unexpectedly received.
    UnexpectedUnmaskedFrame,
}

impl ProtocolError {
    /// Stringify this variant.
    pub(super) const fn as_str(&self) -> &'static str {
        match self {
            Self::FragmentedControlFrame => "fragmented control frame",
            Self::InvalidCloseCode => "invalid close code",
            Self::InvalidOpcode => "invalid opcode",
            Self::InvalidPayloadLength => "invalid payload length",
            Self::InvalidRsv => "invalid extension",
            Self::InvalidUtf8 => "invalid utf-8",
            Self::UnexpectedMaskedFrame => "unexpected masked frame",
            Self::UnexpectedUnmaskedFrame => "unexpected unmasked frame",
        }
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::error::Error for ProtocolError {}
