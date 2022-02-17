use bytes::{Buf, BufMut, Bytes, BytesMut};
use futures_util::{Sink, SinkExt, StreamExt};
/// <https://datatracker.ietf.org/doc/html/rfc6455#section-5.2>
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{Decoder, Encoder, Framed};

use std::{
    mem::take,
    pin::Pin,
    string::FromUtf8Error,
    task::{Context, Poll},
};

use crate::{mask, utf8, Error};

const FRAME_SIZE: usize = 4096;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum OpCode {
    Continuation,
    Text,
    Binary,
    Close,
    Ping,
    Pong,
}

impl OpCode {
    fn is_control(self) -> bool {
        return matches!(self, Self::Close | Self::Ping | Self::Pong);
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

#[derive(Debug)]
pub struct Frame {
    opcode: OpCode,
    is_final: bool,
    payload: Bytes,
}

#[derive(Debug)]
pub enum ProtocolError {
    InvalidCloseCode,
    InvalidCloseSequence,
    InvalidOpcode,
    InvalidRsv,
    InvalidPayloadLength,
    InvalidUtf8,
    DisallowedOpcode,
    DisallowedCloseCode,
    MessageHasWrongOpcode,
    ServerMaskedData,
    InvalidControlFrameLength,
    FragmentedControlFrame,
    UnexpectedContinuation,
    UnfinishedMessage,
}

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

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Role {
    Client,
    Server,
}

#[derive(Debug, Clone)]
pub enum CloseCode {
    NormalClosure,
    GoingAway,
    ProtocolError,
    UnsupportedData,
    Reserved,
    NoStatusReceived,
    AbnormalClosure,
    InvalidFramePayloadData,
    PolicyViolation,
    MessageTooBig,
    MandatoryExtension,
    InternalServerError,
    TlsHandshake,
    ReservedForStandards(u16),
    Libraries(u16),
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

#[derive(Debug, Clone)]
pub struct Message {
    opcode: OpCode,
    data: Bytes,
}

impl Message {
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
                        utf8::parse_str(unsafe { data.get_unchecked(2..) })?;
                    }

                    Ok(Self { opcode, data })
                }
            }
        }
    }

    fn into_raw(self) -> (OpCode, Bytes) {
        (self.opcode, self.data)
    }

    #[must_use]
    pub fn text(data: String) -> Self {
        Self {
            opcode: OpCode::Text,
            data: data.into(),
        }
    }

    #[must_use]
    pub fn binary<D: Into<Bytes>>(data: D) -> Self {
        Self {
            opcode: OpCode::Binary,
            data: data.into(),
        }
    }

    #[must_use]
    pub fn close(code: Option<CloseCode>, reason: Option<&str>) -> Self {
        let mut data = BytesMut::new();

        if let Some(code) = code {
            data.put_u16(code.into());
        }

        if let Some(reason) = reason {
            data.extend_from_slice(reason.as_bytes());
        }

        Self {
            opcode: OpCode::Close,
            data: data.freeze(),
        }
    }

    #[must_use]
    pub fn ping<D: Into<Bytes>>(data: D) -> Self {
        Self {
            opcode: OpCode::Ping,
            data: data.into(),
        }
    }

    #[must_use]
    pub fn pong<D: Into<Bytes>>(data: D) -> Self {
        Self {
            opcode: OpCode::Pong,
            data: data.into(),
        }
    }

    #[must_use]
    pub fn is_text(&self) -> bool {
        self.opcode == OpCode::Text
    }

    #[must_use]
    pub fn is_binary(&self) -> bool {
        self.opcode == OpCode::Binary
    }

    #[must_use]
    pub fn is_close(&self) -> bool {
        self.opcode == OpCode::Close
    }

    #[must_use]
    pub fn is_ping(&self) -> bool {
        self.opcode == OpCode::Ping
    }

    #[must_use]
    pub fn is_pong(&self) -> bool {
        self.opcode == OpCode::Pong
    }

    #[must_use]
    pub fn into_data(self) -> Bytes {
        self.data
    }

    pub fn as_data(&self) -> &Bytes {
        &self.data
    }

    pub fn as_text(&self) -> Result<&str, ProtocolError> {
        match self.opcode {
            OpCode::Text => Ok(unsafe { std::str::from_utf8_unchecked(&self.data) }),
            OpCode::Binary => Ok(utf8::parse_str(&self.data)?),
            _ => Err(ProtocolError::MessageHasWrongOpcode),
        }
    }

    pub fn as_close(&self) -> Result<(Option<CloseCode>, Option<&str>), ProtocolError> {
        if self.opcode == OpCode::Close {
            let close_code = if self.data.len() >= 2 {
                let close_code_value = u16::from_be_bytes(unsafe {
                    self.data.get_unchecked(0..2).try_into().unwrap_unchecked()
                });
                Some(CloseCode::try_from(close_code_value)?)
            } else {
                None
            };

            let reason = if self.data.len() > 2 {
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

#[derive(Debug)]
enum StreamState {
    Active,
    ClosedByPeer,
    ClosedByUs,
    CloseAcknowledged,
    Terminated,
}

impl StreamState {
    fn can_read(&self) -> bool {
        return matches!(self, Self::Active | Self::ClosedByUs);
    }

    fn check_active(&self) -> Result<(), Error> {
        match self {
            Self::Terminated => Err(Error::AlreadyClosed),
            _ => Ok(()),
        }
    }
}

pub struct WebsocketStream<T> {
    inner: Framed<T, WebsocketProtocol>,

    state: StreamState,

    framing_payload: BytesMut,
    framing_opcode: OpCode,
    framing_final: bool,

    utf8_valid_up_to: usize,
}

impl<T> WebsocketStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    pub fn from_raw_stream(stream: T, role: Role, fail_fast_on_invalid_utf8: bool) -> Self {
        Self {
            inner: WebsocketProtocol {
                role,
                state: StreamState::Active,
                payload_in: 0,
                utf8_valid_up_to: if fail_fast_on_invalid_utf8 {
                    Some(0)
                } else {
                    None
                },
            }
            .framed(stream),
            state: StreamState::Active,
            framing_payload: BytesMut::new(),
            framing_opcode: OpCode::Continuation,
            framing_final: false,
            utf8_valid_up_to: 0,
        }
    }

    pub fn from_framed<U>(
        framed: Framed<T, U>,
        role: Role,
        fail_fast_on_invalid_utf8: bool,
    ) -> Self {
        Self {
            inner: framed.map_codec(|_| WebsocketProtocol {
                role,
                state: StreamState::Active,
                payload_in: 0,
                utf8_valid_up_to: if fail_fast_on_invalid_utf8 {
                    Some(0)
                } else {
                    None
                },
            }),
            state: StreamState::Active,
            framing_payload: BytesMut::new(),
            framing_opcode: OpCode::Continuation,
            framing_final: false,
            utf8_valid_up_to: 0,
        }
    }

    async fn read_full_message(&mut self) -> Option<Result<(OpCode, Bytes), Error>> {
        if let Err(e) = self.state.check_active() {
            return Some(Err(e));
        };

        while !self.framing_final {
            match self.inner.next().await? {
                Ok(frame) => {
                    // Control frames are allowed in between other frames
                    if frame.opcode.is_control() {
                        return Some(Ok((frame.opcode, frame.payload)));
                    }

                    if self.framing_opcode == OpCode::Continuation {
                        if frame.opcode == OpCode::Continuation {
                            return Some(Err(Error::Protocol(
                                ProtocolError::UnexpectedContinuation,
                            )));
                        }

                        self.framing_opcode = frame.opcode;
                    } else if frame.opcode != OpCode::Continuation {
                        return Some(Err(Error::Protocol(ProtocolError::UnfinishedMessage)));
                    }

                    self.framing_final = frame.is_final;
                    self.framing_payload.extend_from_slice(&frame.payload);

                    if self.framing_opcode == OpCode::Text {
                        let (should_fail, valid_up_to) = utf8::should_fail_fast(
                            unsafe { self.framing_payload.get_unchecked(self.utf8_valid_up_to..) },
                            self.framing_final,
                        );

                        if should_fail {
                            return Some(Err(Error::Protocol(ProtocolError::InvalidUtf8)));
                        }

                        self.utf8_valid_up_to += valid_up_to;
                    }
                }
                Err(e) => {
                    return Some(Err(e));
                }
            }
        }

        let opcode = self.framing_opcode;
        let payload = take(&mut self.framing_payload).freeze();

        self.framing_opcode = OpCode::Continuation;
        self.framing_final = false;
        self.utf8_valid_up_to = 0;

        Some(Ok((opcode, payload)))
    }

    pub async fn next(&mut self) -> Option<Result<Message, Error>> {
        if !self.state.can_read() {
            return None;
        }

        let (opcode, payload) = match self.read_full_message().await? {
            Ok((opcode, payload)) => (opcode, payload),
            Err(e) => {
                if let Error::Protocol(protocol) = &e {
                    let close_msg = protocol.into();

                    if let Err(e) = self.send(close_msg).await {
                        return Some(Err(e));
                    };
                }

                return Some(Err(e));
            }
        };

        let message = match Message::from_raw(opcode, payload) {
            Ok(msg) => msg,
            Err(e) => {
                let close_msg = Message::from(&e);

                if let Err(e) = self.send(close_msg).await {
                    return Some(Err(e));
                };

                return Some(Err(Error::Protocol(e)));
            }
        };

        match &message.opcode {
            OpCode::Close => match self.state {
                StreamState::Active => {
                    self.state = StreamState::ClosedByPeer;
                    if let Err(e) = self.send(message.clone()).await {
                        return Some(Err(e));
                    };
                }
                StreamState::ClosedByPeer | StreamState::CloseAcknowledged => return None,
                StreamState::ClosedByUs => {
                    self.state = StreamState::CloseAcknowledged;
                }
                StreamState::Terminated => unreachable!(),
            },
            OpCode::Ping => {
                let mut msg = message.clone();
                msg.opcode = OpCode::Pong;

                if let Err(e) = self.send(msg).await {
                    return Some(Err(e));
                };
            }
            _ => {}
        }

        Some(Ok(message))
    }

    pub async fn close(
        &mut self,
        close_code: Option<CloseCode>,
        reason: Option<&str>,
    ) -> Result<(), Error> {
        self.send(Message::close(close_code, reason)).await
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

struct WebsocketProtocol {
    role: Role,
    state: StreamState,
    payload_in: usize,
    utf8_valid_up_to: Option<usize>,
}

impl Encoder<Message> for WebsocketProtocol {
    type Error = Error;

    #[allow(clippy::cast_possible_truncation)]
    fn encode(&mut self, item: Message, dst: &mut BytesMut) -> Result<(), Self::Error> {
        self.state.check_active()?;

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
            let mask = if self.role == Role::Client {
                Some([
                    fastrand::u8(0..=255),
                    fastrand::u8(0..=255),
                    fastrand::u8(0..=255),
                    fastrand::u8(0..=255),
                ])
            } else {
                None
            };
            let mask_bit = 128 * u8::from(mask.is_some());
            let opcode_value: u8 = frame_opcode.into();

            let initial_byte = (u8::from(is_final) << 7) + opcode_value;

            dst.put_u8(initial_byte);

            if chunk_size > u16::MAX as usize {
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
                mask::frame(&mask, unsafe { dst.get_unchecked_mut(start_of_data..) });
            }

            next_chunk = chunks.next();
            chunk_number += 1;
        }

        if self.role == Role::Server && !self.state.can_read() {
            self.state = StreamState::Terminated;
            Err(Error::ConnectionClosed)
        } else {
            Ok(())
        }
    }
}

macro_rules! ensure_buffer_has_space {
    ($buf:expr, $space:expr) => {
        if $buf.len() < $space {
            $buf.reserve($space);

            return Ok(None);
        }
    };
}

impl Decoder for WebsocketProtocol {
    type Item = Frame;

    type Error = Error;

    #[allow(clippy::cast_possible_truncation, clippy::too_many_lines)]
    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // Opcode and payload length must be present
        ensure_buffer_has_space!(src, 2);

        let fin_and_rsv = unsafe { src.get_unchecked(0) };
        let payload_len_1 = unsafe { src.get_unchecked(1) };

        // Bit 0
        let fin = fin_and_rsv & 1 << 7 != 0;

        // Bits 1-3
        let rsv = fin_and_rsv & 0x70;

        if rsv != 0 {
            return Err(Error::Protocol(ProtocolError::InvalidRsv));
        }

        // Bits 4-7
        let opcode_value = fin_and_rsv & 31;
        let opcode = OpCode::try_from(opcode_value)?;

        if !fin && opcode.is_control() {
            return Err(Error::Protocol(ProtocolError::FragmentedControlFrame));
        }

        let mask = payload_len_1 >> 7 != 0;

        if mask && self.role == Role::Client {
            return Err(Error::Protocol(ProtocolError::ServerMaskedData));
        }

        // Bits 1-7
        let mut payload_length = (payload_len_1 & 127) as usize;

        let mut offset = 2;

        if payload_length > 125 {
            if opcode.is_control() {
                return Err(Error::Protocol(ProtocolError::InvalidControlFrameLength));
            }

            if payload_length == 126 {
                ensure_buffer_has_space!(src, 4);
                payload_length = u16::from_be_bytes(unsafe {
                    src.get_unchecked(2..4).try_into().unwrap_unchecked()
                }) as usize;
                offset = 4;
            } else if payload_length == 127 {
                ensure_buffer_has_space!(src, 10);
                payload_length = u64::from_be_bytes(unsafe {
                    src.get_unchecked(2..10).try_into().unwrap_unchecked()
                }) as usize;
                offset = 10;
            } else {
                return Err(Error::Protocol(ProtocolError::InvalidPayloadLength));
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
            let to_read = data_available - self.payload_in;
            let bytes_missing = payload_length - data_available;

            if bytes_missing > 0 {
                // If data is missing, we might have to fail fast on invalid UTF8
                if let Some(utf8_valid_up_to) = &mut self.utf8_valid_up_to {
                    if opcode == OpCode::Text {
                        // Data might be masked, so unmask it here
                        if mask {
                            let mut masking_key = [0; 4];
                            masking_key
                                .copy_from_slice(unsafe { src.get_unchecked(offset - 4..offset) });

                            masking_key.rotate_left(self.payload_in & 3);

                            let unmasked_until = offset + self.payload_in;

                            mask::frame(&masking_key, unsafe {
                                src.get_unchecked_mut(unmasked_until..unmasked_until + to_read)
                            });
                        }

                        self.payload_in = data_available;

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

            // Since we only unmasked if data was previously incomplete, unmask the entire rest
            if mask {
                let mut masking_key = [0; 4];
                masking_key.copy_from_slice(unsafe { src.get_unchecked(offset - 4..offset) });

                masking_key.rotate_left(self.payload_in & 3);

                let unmasked_until = offset + self.payload_in;

                mask::frame(&masking_key, unsafe {
                    src.get_unchecked_mut(unmasked_until..unmasked_until + to_read)
                });
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
