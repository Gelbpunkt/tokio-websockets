#![no_main]

extern crate tokio_websockets;

use std::{
    convert::TryFrom,
    future::Future,
    io,
    num::NonZeroUsize,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

use arbitrary::Arbitrary;
use futures::{pin_mut, stream::StreamExt, FutureExt, SinkExt};
use libfuzzer_sys::fuzz_target;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_websockets::{CloseCode, Config, Limits, Message, WebSocketStream};

#[derive(Arbitrary, Debug)]
enum ArbitraryMessage {
    Binary(Vec<u8>),
    Text(String),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close(u16, String),
}

impl From<ArbitraryMessage> for Message {
    fn from(message: ArbitraryMessage) -> Self {
        match message {
            ArbitraryMessage::Binary(payload) => Message::binary(payload),
            ArbitraryMessage::Text(payload) => Message::text(payload),
            ArbitraryMessage::Ping(mut payload) => {
                payload.truncate(125);
                Message::ping(payload)
            }
            ArbitraryMessage::Pong(mut payload) => {
                payload.truncate(125);
                Message::pong(payload)
            }
            ArbitraryMessage::Close(code, mut reason) => {
                for len in (119..=123).rev() {
                    if reason.is_char_boundary(len) {
                        reason.truncate(len);
                        break;
                    }
                }
                Message::close(
                    CloseCode::try_from(code)
                        .ok()
                        .filter(|code| !code.is_reserved()),
                    &reason,
                )
            }
        }
    }
}

#[derive(Arbitrary, Debug)]
struct Operation {
    kind: OperationKind,
    /// After polling this many times, the operation
    /// is canceled by dropping the future.
    max_polls: Option<u8>,
}

#[derive(Arbitrary, Debug)]
enum OperationKind {
    Read,
    Write(ArbitraryMessage),
    Flush,
    Close,
}

#[derive(Arbitrary, Debug)]
struct Workload {
    /// Fuzz the server, as opposed to the client.
    server: bool,
    data_to_read: Vec<u8>,
    io_behaviors: Vec<IoBehavior>,
    operations: Vec<Operation>,
    /// Zero would panic.
    frame_size: NonZeroUsize,
    /// Limit to a small int to avoid OOM.
    max_payload_len: u16,
}

struct DeterministicStream {
    /// The data that would be read by successful reads.
    data_to_read: Vec<u8>,
    /// The data written by successful writes.
    data_written: Arc<Mutex<Vec<u8>>>,
    /// Each stream IO will take one to decide how it behaves.
    io_behaviors: Vec<IoBehavior>,
    /// Return EOF. If false, returns pending when out of data.
    eof: bool,
}

#[derive(Arbitrary, Debug)]
enum IoBehavior {
    /// Instantly read/write at most this many bytes, if applicable.
    Limit(usize),
    /// Instantly return an error.
    Error,
    /// Wake the waker immediately but return pending.
    Pending,
}

impl Default for IoBehavior {
    fn default() -> Self {
        Self::Limit(usize::MAX)
    }
}

/// Ready once limit is expired.
struct PollLimiter {
    remaining_polls: Option<u8>,
}

impl Future for PollLimiter {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(remaining_polls) = &mut self.remaining_polls {
            if let Some(new) = remaining_polls.checked_sub(1) {
                *remaining_polls = new;
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        } else {
            Poll::Pending
        }
    }
}

fn new_stream(
    server: bool,
    config: Config,
    limits: Limits,
    stream: DeterministicStream,
) -> WebSocketStream<DeterministicStream> {
    if server {
        tokio_websockets::ServerBuilder::new()
            .config(config)
            .limits(limits)
            .serve(stream)
    } else {
        tokio_websockets::ClientBuilder::new()
            .config(config)
            .limits(limits)
            .take_over(stream)
    }
}

fn fuzz(
    Workload {
        server,
        data_to_read,
        io_behaviors,
        operations,
        frame_size,
        max_payload_len,
    }: Workload,
) {
    let data_written = Arc::new(Mutex::new(Vec::new()));
    let mut ws = new_stream(
        server,
        Config::default().frame_size(frame_size.get()),
        Limits::default().max_payload_len(Some(max_payload_len as _)),
        DeterministicStream {
            data_to_read,
            data_written: Arc::clone(&data_written),
            io_behaviors,
            eof: true,
        },
    );

    futures::executor::block_on(async move {
        for operation in operations {
            let future: Pin<Box<dyn Future<Output = ()>>> = match operation.kind {
                OperationKind::Read => Box::pin(ws.next().map(|_| ())),
                OperationKind::Write(message) => Box::pin(ws.feed(message.into()).map(|_| ())),
                OperationKind::Flush => Box::pin(ws.flush().map(|_| ())),
                OperationKind::Close => Box::pin(ws.close().map(|_| ())),
            };

            let limit = PollLimiter {
                remaining_polls: operation.max_polls,
            };
            pin_mut!(limit);
            tokio::select! {
                biased;
                _ = limit => {}
                _ = future => {}
            }
        }
    });

    let data_written = std::mem::take(&mut *data_written.lock().unwrap());

    // As a sanity check, make sure `data_written` is prefixed with 0 or more
    // *valid* messages, possibly followed by an incomplete *valid* message.
    let mut sanity_check = new_stream(
        !server,
        Default::default(),
        Default::default(),
        DeterministicStream {
            data_to_read: data_written.clone(),
            data_written: Default::default(),
            io_behaviors: Vec::new(),
            // Allow an incomplete message at the end.
            eof: false,
        },
    );

    futures::executor::block_on(async move {
        loop {
            let limit = PollLimiter {
                remaining_polls: Some(1),
            };
            pin_mut!(limit);
            tokio::select! {
                biased;
                _ = limit => {
                    // Pending means EOF; remaining message, if any, is incomplete.
                    break;
                }
                result = sanity_check.next() => {
                    // Any complete messages must be valid.
                    if let Some(result) = result {
                        result.unwrap_or_else(|e| panic!("{} {:?}", e, data_written));
                    } else {
                        // Received close.
                        break;
                    }
                }
            }
        }
    });
}

fuzz_target!(|workload: Workload| {
    fuzz(workload);
});

impl AsyncRead for DeterministicStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.io_behaviors.pop().unwrap_or_default() {
            IoBehavior::Limit(limit) => {
                if !self.eof && self.data_to_read.is_empty() {
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                let end = buf.remaining().min(limit).min(self.data_to_read.len());
                let remainder = self.data_to_read.split_off(end);
                buf.put_slice(&self.data_to_read);
                self.data_to_read = remainder;
                Poll::Ready(Ok(()))
            }
            IoBehavior::Error => Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, "???"))),
            IoBehavior::Pending => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }
}

impl AsyncWrite for DeterministicStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        match self.io_behaviors.pop().unwrap_or_default() {
            IoBehavior::Limit(limit) => {
                let amount = buf.len().min(limit);
                self.data_written
                    .lock()
                    .unwrap()
                    .extend_from_slice(&buf[..amount]);
                Poll::Ready(Ok(amount))
            }
            IoBehavior::Error => Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, "???"))),
            IoBehavior::Pending => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        match self.io_behaviors.pop().unwrap_or_default() {
            IoBehavior::Limit(_) => Poll::Ready(Ok(())),
            IoBehavior::Error => Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, "???"))),
            IoBehavior::Pending => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        match self.io_behaviors.pop().unwrap_or_default() {
            IoBehavior::Limit(_) => Poll::Ready(Ok(())),
            IoBehavior::Error => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "unsupported",
            ))),
            IoBehavior::Pending => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }
}
