//! Implementation of a tokio-util [`Decoder`] for websocket
//! frames. The [`Encoder`] is a placeholder and unreachable, since tokio-util's
//! internal buffer used in the encoder comes with a hefty performance penalty
//! for large payloads due to the required memmove. Instead, we implement our
//! own, zero-copy implementation in the sink implementation of the
//! [`WebsocketStream`].
//!
//! [`WebsocketStream`]: super::WebsocketStream
use std::hint::unreachable_unchecked;

use bytes::{Buf, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use super::types::{Frame, Limits, OpCode, Role};
use crate::{
    mask,
    proto::ProtocolError,
    utf8::{self, Validator},
    CloseCode, Error,
};

/// The actual implementation of the websocket byte-level protocol.
/// It provides a [`Decoder`] for single frames that must be assembled by a
/// client such as the [`WebsocketStream`] later.
///
/// [`WebsocketStream`]: super::WebsocketStream
#[derive(Debug)]
pub(super) struct WebsocketProtocol {
    /// The [`Role`] this implementation should assume for the stream.
    pub(super) role: Role,
    /// The [`Limits`] imposed on this stream.
    pub(super) limits: Limits,
    /// Opcode of the full message.
    fragmented_message_opcode: OpCode,
    /// Index up to which the payload was processed (unmasked and validated).
    payload_processed: usize,
    /// UTF-8 validator.
    validator: Validator,
}

impl WebsocketProtocol {
    /// Creates a new websocket codec.
    #[cfg(any(feature = "client", feature = "server"))]
    pub(super) fn new(role: Role, limits: Limits) -> Self {
        Self {
            role,
            limits,
            fragmented_message_opcode: OpCode::Continuation,
            payload_processed: 0,
            validator: Validator::new(),
        }
    }
}

impl Encoder<Frame> for WebsocketProtocol {
    type Error = Error;

    fn encode(&mut self, _item: Frame, _dst: &mut BytesMut) -> Result<(), Self::Error> {
        unsafe { unreachable_unchecked() }
    }
}

/// Macro that returns `Ok(None)` early and reserves missing capacity if buf is
/// not large enough.
macro_rules! ensure_buffer_has_space {
    ($buf:expr, $space:expr) => {
        if $buf.len() < $space {
            $buf.reserve(($space as usize).saturating_sub($buf.capacity()));

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
        let opcode = OpCode::try_from(fin_and_rsv & 0xF)?;

        if opcode.is_control() {
            if !fin {
                return Err(Error::Protocol(ProtocolError::FragmentedControlFrame));
            }
        } else if self.fragmented_message_opcode == OpCode::Continuation {
            if opcode == OpCode::Continuation {
                return Err(Error::Protocol(ProtocolError::InvalidOpcode));
            }
        } else if opcode != OpCode::Continuation {
            return Err(Error::Protocol(ProtocolError::InvalidOpcode));
        }

        // Bit 0
        let mask = payload_len_1 >> 7 != 0;

        if mask && self.role == Role::Client {
            return Err(Error::Protocol(ProtocolError::UnexpectedMaskedFrame));
        } else if !mask && self.role == Role::Server {
            return Err(Error::Protocol(ProtocolError::UnexpectedUnmaskedFrame));
        }

        // Bits 1-7
        let mut payload_length = (payload_len_1 & 127) as usize;

        let mut offset = 2;

        // Close frames must be at least 2 bytes in length
        if opcode == OpCode::Close && payload_length == 1 {
            return Err(Error::Protocol(ProtocolError::InvalidPayloadLength));
        } else if payload_length > 125 {
            if opcode.is_control() {
                return Err(Error::Protocol(ProtocolError::InvalidPayloadLength));
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

        if payload_length > self.limits.max_payload_len.unwrap_or(usize::MAX) {
            return Err(Error::PayloadTooLong {
                len: payload_length,
                max_len: self.limits.max_payload_len.unwrap_or(usize::MAX),
            });
        }

        // There could be a mask here, but we only load it later,
        // so just increase the offset to calculate the available data
        if mask {
            ensure_buffer_has_space!(src, offset + 4);
            offset += 4;
        }

        if payload_length != 0 {
            let payload_available = (src.len() - offset).min(payload_length);

            if payload_length != payload_available {
                // Validate partial frame payload data
                if opcode == OpCode::Text
                    || (opcode == OpCode::Continuation
                        && self.fragmented_message_opcode == OpCode::Text)
                {
                    if mask {
                        // SAFETY: The masking key and the payload do not overlap in src
                        // TODO: Replace with split_at_mut_unchecked when stable
                        let (masking_key, payload_masked) = unsafe {
                            let masking_key_ptr =
                                src.get_unchecked(offset - 4..offset) as *const [u8];
                            let payload_masked_ptr = src.get_unchecked_mut(
                                offset + self.payload_processed..offset + payload_available,
                            ) as *mut [u8];

                            (&*masking_key_ptr, &mut *payload_masked_ptr)
                        };

                        mask::frame(masking_key, payload_masked, self.payload_processed & 3);
                    }

                    // SAFETY: self.payload_data_validated <= payload_available
                    self.validator.feed(
                        unsafe {
                            src.get_unchecked(
                                offset + self.payload_processed..offset + payload_available,
                            )
                        },
                        false,
                    )?;

                    self.payload_processed = payload_available;
                }

                src.reserve((payload_length - payload_available).saturating_sub(src.capacity()));

                return Ok(None);
            }

            if mask {
                // SAFETY: The masking key and the payload do not overlap in src
                // TODO: Replace with split_at_mut_unchecked when stable
                let (masking_key, payload_masked) = unsafe {
                    let masking_key_ptr = src.get_unchecked(offset - 4..offset) as *const [u8];
                    let payload_masked_ptr = src
                        .get_unchecked_mut(offset + self.payload_processed..offset + payload_length)
                        as *mut [u8];

                    (&*masking_key_ptr, &mut *payload_masked_ptr)
                };

                mask::frame(masking_key, payload_masked, self.payload_processed & 3);
            }

            if opcode == OpCode::Text
                || (opcode == OpCode::Continuation
                    && self.fragmented_message_opcode == OpCode::Text)
            {
                // SAFETY: self.payload_data_validated <= payload_length
                self.validator.feed(
                    unsafe {
                        src.get_unchecked(offset + self.payload_processed..offset + payload_length)
                    },
                    fin,
                )?;
            } else if opcode == OpCode::Close {
                // SAFETY: Close frames with a non-zero payload length are validated to not have
                // a length of 1
                // A conversion from two u8s to a u16 cannot fail
                let code = CloseCode::try_from(u16::from_be_bytes(unsafe {
                    src.get_unchecked(offset..offset + 2)
                        .try_into()
                        .unwrap_unchecked()
                }))?;
                if !code.is_sendable() {
                    return Err(Error::Protocol(ProtocolError::InvalidCloseCode));
                }

                // SAFETY: payload_length <= src.len()
                let _reason = utf8::parse_str(unsafe {
                    src.get_unchecked(offset + 2..offset + payload_length)
                })?;
            }
        }

        // Advance the offset into the payload body
        src.advance(offset);
        // Take the payload
        let payload = src.split_to(payload_length).into();

        if fin && !opcode.is_control() {
            self.fragmented_message_opcode = OpCode::Continuation;
        } else if opcode != OpCode::Continuation {
            self.fragmented_message_opcode = opcode;
        }

        self.payload_processed = 0;

        Ok(Some(Frame {
            opcode,
            payload,
            is_final: fin,
        }))
    }
}
