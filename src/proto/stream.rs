//! Frame aggregating abstraction over the low-level [`super::codec`]
//! implementation that provides [`futures_sink::Sink`] and
//! [`futures_core::Stream`] implementations that take [`Message`] as a
//! parameter.
use std::{
    collections::VecDeque,
    io::{self, IoSlice},
    mem::{replace, take},
    pin::Pin,
    task::{ready, Context, Poll, Waker},
};

use bytes::{Buf, BytesMut};
use futures_core::Stream;
use futures_sink::Sink;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::{codec::FramedRead, io::poll_write_buf};

#[cfg(any(feature = "client", feature = "server"))]
use super::types::Role;
use super::{
    codec::WebSocketProtocol,
    types::{Frame, Message, OpCode, Payload, StreamState},
    Config, Limits,
};
use crate::{CloseCode, Error};

/// Helper struct for storing a frame header, the header size and payload.
#[derive(Debug)]
struct EncodedFrame {
    /// Encoded frame header and mask.
    header: [u8; 14],
    /// Potentially masked message payload, ready for writing to the I/O.
    payload: Payload,
}

impl EncodedFrame {
    /// Whether or not this frame is masked.
    #[inline]
    fn is_masked(&self) -> bool {
        self.header[1] >> 7 != 0
    }

    /// Returns the length of the combined header and mask in bytes.
    #[inline]
    fn header_len(&self) -> usize {
        let mask_bytes = if self.is_masked() { 4 } else { 0 };
        match self.header[1] & 127 {
            127 => 10 + mask_bytes,
            126 => 4 + mask_bytes,
            _ => 2 + mask_bytes,
        }
    }

    /// Total length of the frame.
    fn len(&self) -> usize {
        self.header_len() + self.payload.len()
    }
}

/// Queued up frames that are being sent.
#[derive(Debug)]
struct FrameQueue {
    /// Queue of outgoing frames to send. Some parts of the first item may have
    /// been sent already.
    queue: VecDeque<EncodedFrame>,
    /// Amount of partial bytes written of the first frame in the queue.
    bytes_written: usize,
    /// Total amount of bytes remaining to be sent in the frame queue.
    pending_bytes: usize,
}

impl FrameQueue {
    /// Creates a new, empty [`FrameQueue`].
    #[cfg(any(feature = "client", feature = "server"))]
    fn new() -> Self {
        Self {
            queue: VecDeque::with_capacity(1),
            bytes_written: 0,
            pending_bytes: 0,
        }
    }

    /// Queue a frame to be sent.
    fn push(&mut self, item: EncodedFrame) {
        self.pending_bytes += item.len();
        self.queue.push_back(item);
    }
}

impl Buf for FrameQueue {
    fn remaining(&self) -> usize {
        self.pending_bytes
    }

    fn chunk(&self) -> &[u8] {
        if let Some(frame) = self.queue.front() {
            if self.bytes_written >= frame.header_len() {
                unsafe {
                    frame
                        .payload
                        .get_unchecked(self.bytes_written - frame.header_len()..)
                }
            } else {
                &frame.header[self.bytes_written..frame.header_len()]
            }
        } else {
            &[]
        }
    }

    fn advance(&mut self, mut cnt: usize) {
        self.pending_bytes -= cnt;
        cnt += self.bytes_written;

        while cnt > 0 {
            let item = self
                .queue
                .front()
                .expect("advance called with too long count");
            let item_len = item.len();

            if cnt >= item_len {
                self.queue.pop_front();
                self.bytes_written = 0;
                cnt -= item_len;
            } else {
                self.bytes_written = cnt;
                return;
            }
        }
    }

    fn chunks_vectored<'a>(&'a self, dst: &mut [io::IoSlice<'a>]) -> usize {
        let mut n = 0;
        for (idx, frame) in self.queue.iter().enumerate() {
            if idx == 0 {
                if frame.header_len() > self.bytes_written {
                    dst[n] = IoSlice::new(&frame.header[self.bytes_written..frame.header_len()]);
                    n += 1;
                }

                if !frame.payload.is_empty() {
                    dst[n] = IoSlice::new(unsafe {
                        frame
                            .payload
                            .get_unchecked(self.bytes_written.saturating_sub(frame.header_len())..)
                    });
                    n += 1;
                }
            } else {
                dst[n] = IoSlice::new(&frame.header[..frame.header_len()]);
                n += 1;
                if !frame.payload.is_empty() {
                    dst[n] = IoSlice::new(&frame.payload);
                    n += 1;
                }
            }
        }

        n
    }
}

/// A WebSocket stream that full messages can be read from and written to.
///
/// The stream implements [`futures_sink::Sink`] and [`futures_core::Stream`].
///
/// You must use a [`ClientBuilder`] or [`ServerBuilder`] to
/// obtain a WebSocket stream.
///
/// For usage examples, see the top level crate documentation, which showcases a
/// simple echo server and client.
///
/// [`ClientBuilder`]: crate::ClientBuilder
/// [`ServerBuilder`]: crate::ServerBuilder
#[allow(clippy::module_name_repetitions)]
#[derive(Debug)]
pub struct WebSocketStream<T> {
    /// The underlying stream using the [`WebSocketProtocol`] to read and write
    /// full frames.
    inner: FramedRead<T, WebSocketProtocol>,

    /// Configuration for the stream.
    config: Config,

    /// The [`StreamState`] of the current stream.
    state: StreamState,

    /// Payload of the full message that is being assembled.
    partial_payload: BytesMut,
    /// Opcode of the full message that is being assembled.
    partial_opcode: OpCode,

    /// Buffer that outgoing frame headers are formatted into.
    header_buf: [u8; 14],

    /// Queue of outgoing frames to send.
    frame_queue: FrameQueue,

    /// Waker used for currently actively polling
    /// [`WebSocketStream::poll_flush`] until completion.
    flushing_waker: Option<Waker>,
}

impl<T> WebSocketStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    /// Create a new [`WebSocketStream`] from a raw stream.
    #[cfg(any(feature = "client", feature = "server"))]
    pub(crate) fn from_raw_stream(stream: T, role: Role, config: Config, limits: Limits) -> Self {
        Self {
            inner: FramedRead::new(stream, WebSocketProtocol::new(role, limits)),
            config,
            state: StreamState::Active,
            partial_payload: BytesMut::new(),
            partial_opcode: OpCode::Continuation,
            header_buf: [0; 14],
            frame_queue: FrameQueue::new(),
            flushing_waker: None,
        }
    }

    /// Create a new [`WebSocketStream`] from an existing [`FramedRead`]. This
    /// allows for reusing the internal buffer of the [`FramedRead`] object.
    #[cfg(any(feature = "client", feature = "server"))]
    pub(crate) fn from_framed<U>(
        framed: FramedRead<T, U>,
        role: Role,
        config: Config,
        limits: Limits,
    ) -> Self {
        Self {
            inner: framed.map_decoder(|_| WebSocketProtocol::new(role, limits)),
            config,
            state: StreamState::Active,
            partial_payload: BytesMut::new(),
            partial_opcode: OpCode::Continuation,
            header_buf: [0; 14],
            frame_queue: FrameQueue::new(),
            flushing_waker: None,
        }
    }

    /// Returns a reference to the underlying I/O stream wrapped by this stream.
    ///
    /// Care should be taken not to tamper with the stream of data to avoid
    /// corrupting the stream of frames.
    pub fn get_ref(&self) -> &T {
        self.inner.get_ref()
    }

    /// Returns a mutable reference to the underlying I/O stream wrapped by this
    /// stream.
    ///
    /// Care should be taken not to tamper with the stream of data to avoid
    /// corrupting the stream of frames.
    pub fn get_mut(&mut self) -> &mut T {
        self.inner.get_mut()
    }

    /// Returns a reference to the inner websocket limits.
    pub fn limits(&self) -> &Limits {
        &self.inner.decoder().limits
    }

    /// Returns a mutable reference to the inner websocket limits.
    pub fn limits_mut(&mut self) -> &mut Limits {
        &mut self.inner.decoder_mut().limits
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
        // In the case of Active or ClosedByUs, we want to receive more messages from
        // the remote. In the case of ClosedByPeer, we have to flush to make sure our
        // close acknowledge goes through.
        if self.state == StreamState::CloseAcknowledged {
            return Poll::Ready(None);
        } else if self.state == StreamState::ClosedByPeer {
            ready!(self.as_mut().poll_flush(cx))?;
            self.state = StreamState::CloseAcknowledged;
            return Poll::Ready(None);
        }

        // If there are pending items, try to flush the sink.
        // Futures only store a single waker. If we use poll_flush(cx) here, the stored
        // waker (i.e. usually that of the write task) is replaced with our waker (i.e.
        // that of the read task) and our write task may never get woken up again. We
        // circumvent this by not calling poll_flush at all if poll_flush is polled by
        // another task at the moment.
        if self.frame_queue.has_remaining() {
            let waker = self.flushing_waker.clone();
            _ = self.as_mut().poll_flush(&mut Context::from_waker(
                waker.as_ref().unwrap_or(cx.waker()),
            ))?;
        }

        let frame = match ready!(Pin::new(&mut self.inner).poll_next(cx)) {
            Some(Ok(frame)) => frame,
            Some(Err(e)) => {
                if matches!(e, Error::Io(_)) || self.state == StreamState::ClosedByUs {
                    self.state = StreamState::CloseAcknowledged;
                } else {
                    self.state = StreamState::ClosedByPeer;

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
                return Poll::Ready(Some(Err(e)));
            }
            None => return Poll::Ready(None),
        };

        match frame.opcode {
            OpCode::Close => match self.state {
                StreamState::Active => {
                    self.state = StreamState::ClosedByPeer;

                    let mut frame = frame.clone();
                    frame.payload.truncate(2);

                    self.queue_frame(frame);
                }
                StreamState::ClosedByPeer | StreamState::CloseAcknowledged => {
                    debug_assert!(false, "unexpected StreamState");
                }
                StreamState::ClosedByUs => {
                    self.state = StreamState::CloseAcknowledged;
                }
            },
            OpCode::Ping if self.state == StreamState::Active => {
                let mut frame = frame.clone();
                frame.opcode = OpCode::Pong;

                self.queue_frame(frame);
            }
            _ => {}
        }

        Poll::Ready(Some(Ok(frame)))
    }

    /// Masks and queues a frame for sending when [`poll_flush`] gets called.
    fn queue_frame(
        &mut self,
        #[cfg_attr(not(feature = "client"), allow(unused_mut))] mut frame: Frame,
    ) {
        if frame.opcode == OpCode::Close && self.state != StreamState::ClosedByPeer {
            self.state = StreamState::ClosedByUs;
        }

        #[cfg_attr(not(feature = "client"), allow(unused_variables))]
        let mask = frame.encode(&mut self.header_buf);

        #[cfg(feature = "client")]
        {
            if self.inner.decoder().role == Role::Client {
                let mut payload = BytesMut::from(frame.payload);
                crate::rand::get_mask(mask);
                // mask::frame will mutate the mask in-place, but we want to send the original
                // mask. This is essentially a u32, so copying it is cheap and easier than
                // special-casing this in the masking implementation.
                // &mut *mask won't work, the compiler will optimize the deref/copy away
                let mut mask_copy = *mask;
                crate::mask::frame(&mut mask_copy, &mut payload);
                frame.payload = Payload::from(payload);
                self.header_buf[1] |= 1 << 7;
            }
        }

        let item = EncodedFrame {
            header: self.header_buf,
            payload: frame.payload,
        };
        self.frame_queue.push(item);
    }

    /// Sets the waker that is currently flushing to a new one and does nothing
    /// if the waker is the same.
    fn set_flushing_waker(&mut self, waker: &Waker) {
        if !self
            .flushing_waker
            .as_ref()
            .is_some_and(|w| w.will_wake(waker))
        {
            self.flushing_waker = Some(waker.clone());
        }
    }
}

impl<T> Stream for WebSocketStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    type Item = Result<Message, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let max_len = self.inner.decoder().limits.max_payload_len;

        loop {
            let (opcode, payload, fin) = match ready!(self.as_mut().poll_next_frame(cx)?) {
                Some(frame) => (frame.opcode, frame.payload, frame.is_final),
                None => return Poll::Ready(None),
            };
            let len = self.partial_payload.len() + payload.len();

            if opcode != OpCode::Continuation {
                if fin {
                    return Poll::Ready(Some(Ok(Message { opcode, payload })));
                }
                self.partial_opcode = opcode;
                self.partial_payload = BytesMut::from(payload);
            } else if len > max_len {
                return Poll::Ready(Some(Err(Error::PayloadTooLong { len, max_len })));
            } else {
                self.partial_payload.extend_from_slice(&payload);
            }

            if fin {
                break;
            }
        }

        let opcode = replace(&mut self.partial_opcode, OpCode::Continuation);
        let mut payload = Payload::from(take(&mut self.partial_payload));
        payload.set_utf8_validated(opcode == OpCode::Text);

        Poll::Ready(Some(Ok(Message { opcode, payload })))
    }
}

// The tokio-util implementation of a sink uses a buffer which start_send
// appends to and poll_flush tries to write from. This makes sense, but comes
// with a hefty performance penalty when sending large payloads, since this adds
// a memmove from the payload to the buffer. We completely avoid that overhead
// by storing messages in a deque.
impl<T> Sink<Message> for WebSocketStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    type Error = Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // tokio-util calls poll_flush when more than 8096 bytes are pending, otherwise
        // it returns Ready. We will just replicate that behavior
        if self.frame_queue.remaining() >= self.config.flush_threshold {
            self.as_mut().poll_flush(cx)
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn start_send(mut self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
        if self.state != StreamState::Active {
            return Err(Error::AlreadyClosed);
        }

        if item.opcode.is_control() || item.payload.len() <= self.config.frame_size {
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
        let flushing_waker = &mut this.flushing_waker;

        while frame_queue.has_remaining() {
            let n = match poll_write_buf(Pin::new(io), cx, frame_queue) {
                Poll::Ready(Ok(n)) => n,
                Poll::Ready(Err(e)) => {
                    *flushing_waker = None;
                    this.state = StreamState::CloseAcknowledged;
                    return Poll::Ready(Err(Error::Io(e)));
                }
                Poll::Pending => {
                    this.set_flushing_waker(cx.waker());
                    return Poll::Pending;
                }
            };

            if n == 0 {
                *flushing_waker = None;
                this.state = StreamState::CloseAcknowledged;
                return Poll::Ready(Err(Error::Io(io::ErrorKind::WriteZero.into())));
            }
        }

        match Pin::new(io).poll_flush(cx) {
            Poll::Ready(Ok(())) => {
                *flushing_waker = None;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(e)) => {
                *flushing_waker = None;
                this.state = StreamState::CloseAcknowledged;
                Poll::Ready(Err(Error::Io(e)))
            }
            Poll::Pending => {
                this.set_flushing_waker(cx.waker());
                Poll::Pending
            }
        }
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.state == StreamState::Active {
            self.queue_frame(Frame::DEFAULT_CLOSE);
        }
        while ready!(self.as_mut().poll_next(cx)).is_some() {}

        ready!(self.as_mut().poll_flush(cx))?;
        Pin::new(self.inner.get_mut())
            .poll_shutdown(cx)
            .map_err(Error::Io)
    }
}
