//! Frame aggregating abstraction over the low-level [`super::codec`]
//! implementation that provides [`futures_sink::Sink`] and
//! [`futures_core::Stream`] implementations that take [`Message`] as a
//! parameter.
use std::{
    hint::unreachable_unchecked,
    mem::{replace, take},
    pin::Pin,
    task::{ready, Context, Poll},
};

use bytes::BytesMut;
use futures_core::Stream;
use futures_sink::Sink;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

#[cfg(any(feature = "client", feature = "server"))]
use super::types::{Limits, Role};
use super::{
    codec::WebsocketProtocol,
    types::{Frame, Message, OpCode, StreamState},
    Config,
};
use crate::{CloseCode, Error};

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
#[allow(clippy::module_name_repetitions)]
#[derive(Debug)]
pub struct WebsocketStream<T> {
    /// The underlying stream using the [`WebsocketProtocol`] to read and write
    /// full frames.
    inner: Framed<T, WebsocketProtocol>,

    /// Configuration for the stream.
    config: Config,

    /// The [`StreamState`] of the current stream.
    state: StreamState,

    /// Payload of the full message that is being assembled.
    partial_payload: BytesMut,
    /// Opcode of the full message that is being assembled.
    partial_opcode: OpCode,

    /// Whether the sink needs to be flushed.
    needs_flush: bool,

    /// Pending frame to be encoded once the sink is ready.
    pending_frame: Option<Frame>,
}

impl<T> WebsocketStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    /// Create a new [`WebsocketStream`] from a raw stream.
    #[cfg(any(feature = "client", feature = "server"))]
    pub(crate) fn from_raw_stream(stream: T, role: Role, config: Config, limits: Limits) -> Self {
        use tokio_util::codec::Decoder;

        Self {
            inner: WebsocketProtocol::new(role, limits).framed(stream),
            config,
            state: StreamState::Active,
            partial_payload: BytesMut::new(),
            partial_opcode: OpCode::Continuation,
            needs_flush: false,
            pending_frame: None,
        }
    }

    /// Create a new [`WebsocketStream`] from an existing [`Framed`]. This
    /// allows for reusing the internal buffer of the [`Framed`] object.
    #[cfg(any(feature = "client", feature = "server"))]
    pub(crate) fn from_framed<U>(
        framed: Framed<T, U>,
        role: Role,
        config: Config,
        limits: Limits,
    ) -> Self {
        Self {
            inner: framed.map_codec(|_| WebsocketProtocol::new(role, limits)),
            config,
            state: StreamState::Active,
            partial_payload: BytesMut::new(),
            partial_opcode: OpCode::Continuation,
            needs_flush: false,
            pending_frame: None,
        }
    }

    /// Attempt to pull out the next frame from the [`Framed`] this stream and
    /// from that update the stream's internal state.
    ///
    /// # Errors
    ///
    /// This method returns an [`Error`] if reading from the stream fails or a
    /// protocol violation is encountered.
    fn poll_next_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame, Error>>> {
        if !matches!(self.state, StreamState::Active | StreamState::ClosedByUs) {
            return Poll::Ready(None);
        }

        if self.pending_frame.is_some() && Pin::new(&mut self.inner).poll_ready(cx)?.is_ready() {
            // SAFETY: We just ensured that the pending frame is some
            let frame = unsafe { self.pending_frame.take().unwrap_unchecked() };
            self.as_mut().start_send(Message {
                opcode: frame.opcode,
                payload: frame.payload,
            })?;
        }

        if self.needs_flush {
            _ = Pin::new(&mut self.inner).poll_flush(cx)?;
            self.needs_flush = false;
        }

        let frame = match ready!(Pin::new(&mut self.inner).poll_next(cx)) {
            Some(Ok(frame)) => frame,
            Some(Err(e)) => {
                if self.state == StreamState::ClosedByUs {
                    self.state = StreamState::CloseAcknowledged;
                } else {
                    self.state = StreamState::ClosedByPeer;
                    if !self.pending_frame.as_ref().map_or(false, Frame::is_close) {
                        self.pending_frame = match &e {
                            Error::Protocol(e) => Some(Frame::from(e)),
                            Error::PayloadTooLong { max_len, .. } => Some(
                                Message::close(
                                    Some(CloseCode::MESSAGE_TOO_BIG),
                                    &format!("max length: {max_len}"),
                                )
                                .into(),
                            ),
                            _ => None,
                        };
                    }
                }
                return Poll::Ready(Some(Err(e)));
            }
            None => return Poll::Ready(None),
        };

        match frame.opcode {
            OpCode::Close => match self.state {
                StreamState::Active => {
                    self.state = StreamState::ClosedByPeer;
                    if !self.pending_frame.as_ref().map_or(false, Frame::is_close) {
                        let mut frame = frame.clone();
                        frame.payload.truncate(2);

                        self.pending_frame = Some(frame);
                    }
                }
                // SAFETY: match statement at the start of the method ensures that this is not the
                // case
                StreamState::ClosedByPeer | StreamState::CloseAcknowledged => unsafe {
                    unreachable_unchecked()
                },
                StreamState::ClosedByUs => {
                    self.state = StreamState::CloseAcknowledged;
                }
            },
            OpCode::Ping
                if self.state == StreamState::Active
                    && !self.pending_frame.as_ref().map_or(false, Frame::is_close) =>
            {
                let mut frame = frame.clone();
                frame.opcode = OpCode::Pong;

                self.pending_frame = Some(frame);
            }
            _ => {}
        }

        Poll::Ready(Some(Ok(frame)))
    }
}

impl<T> Stream for WebsocketStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    type Item = Result<Message, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let max_len = self.inner.codec().limits.max_payload_len;

        loop {
            let (opcode, payload, fin) = match ready!(self.as_mut().poll_next_frame(cx)) {
                Some(Ok(frame)) => (frame.opcode, frame.payload, frame.is_final),
                Some(Err(e)) => return Poll::Ready(Some(Err(e))),
                None => return Poll::Ready(None),
            };

            if opcode != OpCode::Continuation {
                if fin {
                    return Poll::Ready(Some(Ok(Message { opcode, payload })));
                }
                self.partial_opcode = opcode;
            } else if self.partial_payload.len() + payload.len() > max_len.unwrap_or(usize::MAX) {
                return Poll::Ready(Some(Err(Error::PayloadTooLong {
                    len: self.partial_payload.len() + payload.len(),
                    max_len: max_len.unwrap_or(usize::MAX),
                })));
            }

            self.partial_payload.extend_from_slice(&payload);

            if fin {
                break;
            }
        }

        let opcode = replace(&mut self.partial_opcode, OpCode::Continuation);
        let payload = take(&mut self.partial_payload).freeze();

        Poll::Ready(Some(Ok(Message { opcode, payload })))
    }
}

impl<T> Sink<Message> for WebsocketStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    type Error = Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(Pin::new(&mut self.inner).poll_ready(cx))?;
        if let Some(frame) = self.pending_frame.take() {
            self.as_mut().start_send(Message {
                opcode: frame.opcode,
                payload: frame.payload,
            })?;
            Pin::new(&mut self.inner).poll_ready(cx)
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn start_send(mut self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
        if !(self.state == StreamState::Active
            || matches!(self.state, StreamState::ClosedByPeer if item.is_close()))
        {
            return Err(Error::AlreadyClosed);
        }

        if item.is_close() {
            if self.state == StreamState::ClosedByPeer {
                self.state = StreamState::CloseAcknowledged;
            } else {
                self.state = StreamState::ClosedByUs;
            }
        }

        if item.opcode.is_control() {
            Pin::new(&mut self.inner).start_send(item.into())?;
        } else {
            // Chunk the message into frames
            for frame in item.as_frames(self.config.frame_size) {
                Pin::new(&mut self.inner).start_send(frame)?;
            }
        }

        self.needs_flush = true;

        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.pending_frame.is_some() {
            ready!(Pin::new(&mut self.inner).poll_ready(cx))?;
            // SAFETY: We just ensured that the pending frame is some
            let frame = unsafe { self.pending_frame.take().unwrap_unchecked() };
            self.as_mut().start_send(Message {
                opcode: frame.opcode,
                payload: frame.payload,
            })?;
        }

        ready!(Pin::new(&mut self.inner).poll_flush(cx))?;
        self.needs_flush = false;

        Poll::Ready(Ok(()))
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.state == StreamState::Active
            && !self.pending_frame.as_ref().map_or(false, Frame::is_close)
        {
            self.pending_frame = Some(Frame::DEFAULT_CLOSE);
        }
        while ready!(self.as_mut().poll_next(cx)).is_some() {}

        ready!(self.as_mut().poll_flush(cx))?;
        Pin::new(self.inner.get_mut())
            .poll_shutdown(cx)
            .map_err(Error::Io)
    }
}
