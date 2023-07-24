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

use super::{
    codec::WebsocketProtocol,
    types::{Limits, Message, OpCode, Role, StreamState},
    ProtocolError,
};
use crate::{utf8, Error};

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

    /// Payload of the full message that is being assembled.
    partial_payload: BytesMut,
    /// Opcode of the full message that is being assembled.
    partial_opcode: OpCode,

    /// Index up to which the full message payload was validated to be valid
    /// UTF-8.
    utf8_valid_up_to: usize,

    /// Whether the sink needs to be flushed.
    needs_flush: bool,

    /// Pending message to be encoded once the sink is ready.
    pending_message: Option<Message>,
}

impl<T> WebsocketStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    /// Create a new [`WebsocketStream`] from a raw stream.
    #[cfg(any(feature = "client", feature = "server"))]
    pub(crate) fn from_raw_stream(stream: T, role: Role, limits: Limits) -> Self {
        use tokio_util::codec::Decoder;

        Self {
            inner: WebsocketProtocol::new(role, limits).framed(stream),
            partial_payload: BytesMut::new(),
            partial_opcode: OpCode::Continuation,
            utf8_valid_up_to: 0,
            needs_flush: false,
            pending_message: None,
        }
    }

    /// Create a new [`WebsocketStream`] from an existing [`Framed`]. This
    /// allows for reusing the internal buffer of the [`Framed`] object.
    #[cfg(any(feature = "client", feature = "server"))]
    pub(crate) fn from_framed<U>(framed: Framed<T, U>, role: Role, limits: Limits) -> Self {
        Self {
            inner: framed.map_codec(|_| WebsocketProtocol::new(role, limits)),
            partial_payload: BytesMut::new(),
            partial_opcode: OpCode::Continuation,
            utf8_valid_up_to: 0,
            needs_flush: false,
            pending_message: None,
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
    ) -> Poll<Option<Result<Message, Error>>> {
        loop {
            let (opcode, payload, fin) = match ready!(Pin::new(&mut self.inner).poll_next(cx)) {
                Some(Ok(frame)) => (frame.opcode, frame.payload, frame.is_final),
                Some(Err(e)) => return Poll::Ready(Some(Err(e))),
                None => return Poll::Ready(None),
            };

            // Control frames are allowed in between other frames
            if opcode.is_control() {
                return Poll::Ready(Some(Ok(Message { opcode, payload })));
            }

            if self.partial_opcode == OpCode::Continuation {
                if opcode == OpCode::Continuation {
                    return Poll::Ready(Some(Err(Error::Protocol(
                        ProtocolError::UnexpectedContinuation,
                    ))));
                } else if fin {
                    return Poll::Ready(Some(Ok(Message { opcode, payload })));
                }

                self.partial_opcode = opcode;
            } else if opcode != OpCode::Continuation {
                return Poll::Ready(Some(Err(Error::Protocol(ProtocolError::UnfinishedMessage))));
            }

            if let Some(max_message_size) = self.inner.codec().limits.max_message_size {
                let message_size = self.partial_payload.len() + payload.len();

                if message_size > max_message_size {
                    return Poll::Ready(Some(Err(Error::MessageTooLong {
                        size: message_size,
                        max_size: max_message_size,
                    })));
                }
            }

            self.partial_payload.extend_from_slice(&payload);

            if self.partial_opcode == OpCode::Text {
                // SAFETY: self.utf8_valid_up_to is an index in self.partial_payload and cannot
                // exceed its length
                self.utf8_valid_up_to += utf8::should_fail_fast(
                    unsafe { self.partial_payload.get_unchecked(self.utf8_valid_up_to..) },
                    fin,
                )?;
            }

            if fin {
                break;
            }
        }

        let opcode = replace(&mut self.partial_opcode, OpCode::Continuation);
        let payload = take(&mut self.partial_payload).freeze();

        self.utf8_valid_up_to = 0;

        Poll::Ready(Some(Ok(Message { opcode, payload })))
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
                ready!(Pin::new(&mut self.inner).poll_ready(cx))?;
                // SAFETY: arm is only taken until the pending close message is encoded
                let item = unsafe { self.pending_message.take().unwrap_unchecked() };
                Pin::new(&mut self.inner).start_send(item)?;
                if self.inner.codec().role == Role::Server {
                    ready!(Pin::new(&mut self.inner).poll_close(cx))?;
                } else {
                    ready!(Pin::new(&mut self.inner).poll_flush(cx))?;
                }
                return Poll::Ready(None);
            }
            StreamState::CloseAcknowledged => {
                if self.inner.codec().role == Role::Server {
                    ready!(Pin::new(&mut self.inner).poll_close(cx))?;
                } else {
                    ready!(Pin::new(&mut self.inner).poll_flush(cx))?;
                }
                return Poll::Ready(None);
            }
        }

        if self.pending_message.is_some() && Pin::new(&mut self.inner).poll_ready(cx).is_ready() {
            // SAFETY: We just ensured that the pending message is some
            let item = unsafe { self.pending_message.take().unwrap_unchecked() };
            self.as_mut().start_send(item)?;
        }

        if self.needs_flush {
            _ = self.as_mut().poll_flush(cx)?;
        }

        let message = match ready!(self.as_mut().poll_read_next_message(cx)) {
            Some(Ok(msg)) => msg,
            Some(Err(e)) => {
                if self.inner.codec().state == StreamState::Active {
                    if let Error::Protocol(protocol) = &e {
                        self.pending_message = Some(protocol.into());
                    }
                }

                return Poll::Ready(Some(Err(e)));
            }
            None => return Poll::Ready(None),
        };

        match &message.opcode {
            OpCode::Close => match self.inner.codec().state {
                StreamState::Active => {
                    self.inner.codec_mut().state = StreamState::ClosedByPeer;
                    let mut msg = message.clone();
                    msg.payload.truncate(2);

                    self.pending_message = Some(msg);
                }
                // SAFETY: match statement at the start of the method ensures that this is not the
                // case
                StreamState::ClosedByPeer | StreamState::CloseAcknowledged => unsafe {
                    unreachable_unchecked()
                },
                StreamState::ClosedByUs => {
                    self.inner.codec_mut().state = StreamState::CloseAcknowledged;
                }
            },
            OpCode::Ping
                if self.inner.codec().state == StreamState::Active
                    && !self
                        .pending_message
                        .as_ref()
                        .map_or(false, Message::is_close) =>
            {
                let mut msg = message.clone();
                msg.opcode = OpCode::Pong;

                self.pending_message = Some(msg);
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
        ready!(Pin::new(&mut self.inner).poll_ready(cx))?;
        if let Some(item) = self.pending_message.take() {
            self.as_mut().start_send(item)?;
            Pin::new(&mut self.inner).poll_ready(cx)
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn start_send(mut self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
        Pin::new(&mut self.inner).start_send(item)?;
        self.needs_flush = true;

        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let res = Pin::new(&mut self.inner).poll_flush(cx);
        if matches!(res, Poll::Ready(Ok(()))) {
            self.needs_flush = false;
        }

        res
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.inner.codec().state == StreamState::Active
            && !self
                .pending_message
                .as_ref()
                .map_or(false, Message::is_close)
        {
            self.pending_message = Some(Message::DEFAULT_CLOSE);
        }
        while ready!(self.as_mut().poll_next(cx)).is_some() {}
        Poll::Ready(Ok(()))
    }
}
