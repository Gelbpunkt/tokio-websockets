//! Frame aggregating abstraction over the low-level [`super::codec`]
//! implementation that provides [`futures_sink::Sink`] and
//! [`futures_core::Stream`] implementations that take [`Message`] as a
//! parameter.
use std::{
    collections::VecDeque,
    hint::unreachable_unchecked,
    io,
    mem::{replace, take},
    pin::Pin,
    task::{ready, Context, Poll},
};

use bytes::{Buf, BytesMut};
use futures_core::Stream;
use futures_sink::Sink;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::{codec::Framed, io::poll_write_buf};

#[cfg(any(feature = "client", feature = "server"))]
use super::types::Limits;
use super::{
    codec::WebsocketProtocol,
    types::{Frame, Message, OpCode, Payload, Role, StreamState},
    Config,
};
use crate::{CloseCode, Error};

/// Helper struct for storing a frame header, the header size and payload.
#[derive(Debug)]
struct EncodedFrame {
    /// Encoded frame header.
    header: [u8; 10],
    /// Length of the header.
    header_len: u8,
    /// Mask that the payload was masked with.
    mask: Option<[u8; 4]>,
    /// Potentially masked message payload, ready for writing to the I/O.
    payload: Payload,
}

impl EncodedFrame {
    /// Whether this frame is a close frame.
    fn is_close(&self) -> bool {
        self.header[0] & 0xF == 8
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

    /// Buffer that outgoing frame headers are formatted into.
    header_buf: [u8; 10],

    /// Queue of outgoing frames to send.
    frame_queue: VecDeque<EncodedFrame>,
    /// Amount of partial bytes written of the first frame in the queue.
    bytes_written: usize,
}

// SAFETY: The only !Sync field in `WebsocketStream` is `frame_queue`.
// `frame_queue` must be used with exclusive, mutable access, which is
// currently the case. It is only used in methods that take `&mut self`
// and not borrowed in the methods.
unsafe impl<T> Sync for WebsocketStream<T> {}

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
            header_buf: [0; 10],
            frame_queue: VecDeque::with_capacity(1),
            bytes_written: 0,
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
            header_buf: [0; 10],
            frame_queue: VecDeque::with_capacity(1),
            bytes_written: 0,
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

        // If there are pending items, try to flush the sink
        if !self.frame_queue.is_empty() && self.as_mut().poll_ready(cx)?.is_ready() {
            _ = self.as_mut().poll_flush(cx)?;
        }

        let frame = match ready!(Pin::new(&mut self.inner).poll_next(cx)) {
            Some(Ok(frame)) => frame,
            Some(Err(e)) => {
                if self.state == StreamState::ClosedByUs {
                    self.state = StreamState::CloseAcknowledged;
                } else {
                    self.state = StreamState::ClosedByPeer;

                    if !self.frame_queue.iter().any(EncodedFrame::is_close) {
                        match &e {
                            Error::Protocol(e) => self.queue_frame(Frame::from(e)),
                            Error::PayloadTooLong { max_len, .. } => self.queue_frame(
                                Message::close(
                                    Some(CloseCode::MESSAGE_TOO_BIG),
                                    &format!("max length: {max_len}"),
                                )
                                .into(),
                            ),
                            _ => {}
                        }
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

                    if !self.frame_queue.iter().any(EncodedFrame::is_close) {
                        let mut frame = frame.clone();
                        frame.payload.truncate(2);

                        self.queue_frame(frame);
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
                    && !self.frame_queue.iter().any(EncodedFrame::is_close) =>
            {
                let mut frame = frame.clone();
                frame.opcode = OpCode::Pong;

                self.queue_frame(frame);
            }
            _ => {}
        }

        Poll::Ready(Some(Ok(frame)))
    }

    /// Masks and queues a frame for sending when [`poll_flush`] gets called.
    fn queue_frame(&mut self, frame: Frame) {
        let (frame, mask): (Frame, Option<[u8; 4]>) = if self.inner.codec().role == Role::Client {
            #[cfg(feature = "client")]
            {
                let mut frame = frame;
                let mut payload = match BytesMut::try_from(frame.payload) {
                    Ok(b) => b,
                    Err(b) => BytesMut::from(&*b),
                };
                let mask = crate::rand::get_mask();
                crate::mask::frame(&mask, &mut payload, 0);
                frame.payload = Payload::from(payload);

                (frame, Some(mask))
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
            (frame, None)
        };

        let header_len = frame.encode(&mut self.header_buf);
        if mask.is_some() {
            self.header_buf[1] |= 1 << 7;
        }
        self.frame_queue.push_back(EncodedFrame {
            header: self.header_buf,
            header_len,
            mask,
            payload: frame.payload,
        });
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
            } else if self.partial_payload.len() + payload.len() > max_len {
                return Poll::Ready(Some(Err(Error::PayloadTooLong {
                    len: self.partial_payload.len() + payload.len(),
                    max_len,
                })));
            }

            self.partial_payload.extend_from_slice(&payload);

            if fin {
                break;
            }
        }

        let opcode = replace(&mut self.partial_opcode, OpCode::Continuation);
        let payload = take(&mut self.partial_payload).into();

        Poll::Ready(Some(Ok(Message { opcode, payload })))
    }
}

// The tokio-util implementation of a sink uses a buffer which start_send
// appends to and poll_flush tries to write from. This makes sense, but comes
// with a hefty performance penalty when sending large payloads, since this adds
// a memmove from the payload to the buffer. We completely avoid that overhead
// by storing messages in a deque.
impl<T> Sink<Message> for WebsocketStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    type Error = Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // tokio-util calls poll_flush when more than 8096 bytes are pending, otherwise
        // it returns Ready. We will just replicate that behavior
        let pending_bytes = self
            .frame_queue
            .iter()
            .map(|f| {
                f.header_len as usize + (u8::from(f.mask.is_some()) * 4) as usize + f.payload.len()
            })
            .sum::<usize>();

        if pending_bytes >= 8096 {
            self.as_mut().poll_flush(cx)
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
            let frame: Frame = item.into();
            self.queue_frame(frame);
        } else {
            // Chunk the message into frames
            for frame in item.into_frames(self.config.frame_size) {
                self.queue_frame(frame);
            }
        }

        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // Borrow checker hacks... It needs this to understand that we can separately
        // borrow the fields of the struct mutably
        let this = self.get_mut();
        let frame_queue = &mut this.frame_queue;
        let io = this.inner.get_mut();
        let bytes_written = &mut this.bytes_written;

        while !frame_queue.is_empty() {
            let frame = { unsafe { frame_queue.front().unwrap_unchecked() } };
            let frame_header = unsafe { frame.header.get_unchecked(..frame.header_len as usize) };
            let mut buf = frame_header
                .chain(
                    frame
                        .mask
                        .as_ref()
                        .map(<[u8; 4]>::as_slice)
                        .unwrap_or_default(),
                )
                .chain(&*frame.payload);
            buf.advance(*bytes_written);

            while buf.has_remaining() {
                let n = ready!(poll_write_buf(Pin::new(io), cx, &mut buf))?;

                if n == 0 {
                    return Poll::Ready(Err(Error::Io(io::ErrorKind::WriteZero.into())));
                }

                *bytes_written += n;
            }

            frame_queue.pop_front();
            *bytes_written = 0;
        }

        ready!(Pin::new(io).poll_flush(cx))?;

        Poll::Ready(Ok(()))
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.state == StreamState::Active && !self.frame_queue.iter().any(EncodedFrame::is_close)
        {
            self.queue_frame(Frame::DEFAULT_CLOSE);
        }
        while ready!(self.as_mut().poll_next(cx)).is_some() {}

        ready!(self.as_mut().poll_flush(cx))?;
        Pin::new(self.inner.get_mut())
            .poll_shutdown(cx)
            .map_err(Error::Io)
    }
}
