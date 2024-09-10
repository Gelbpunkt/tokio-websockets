#![no_main]
use std::{
    io,
    num::NonZeroUsize,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{stream::StreamExt, SinkExt};
use libfuzzer_sys::fuzz_target;
use tokio_websockets::{CloseCode, Config, Limits, Message};
extern crate tokio_websockets;
use std::convert::TryFrom;

use arbitrary::Arbitrary;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

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
            ArbitraryMessage::Ping(payload) => Message::ping(payload),
            ArbitraryMessage::Pong(payload) => Message::pong(payload),
            ArbitraryMessage::Close(code, reason) => {
                Message::close(CloseCode::try_from(code).ok(), &reason)
            }
        }
    }
}

#[derive(Arbitrary, Debug)]
enum Operation {
    Read,
    Write(ArbitraryMessage),
}

#[derive(Arbitrary, Debug)]
struct Workload {
    reads: Vec<u8>,
    read_limit_or_error: Vec<Result<usize, ()>>,
    operations: Vec<Operation>,
    /// Zero would panic.
    frame_size: NonZeroUsize,
    /// Limit to a small int to avoid OOM.
    max_payload_len: u16,
}

struct DeterministicStream {
    /// The data that would be read by successful reads.
    reads: Vec<u8>,
    /// Each stream read will take one to decide how it behaves.
    read_limit_or_error: Vec<Result<usize, ()>>,
}

fn fuzz(
    Workload {
        reads,
        read_limit_or_error,
        operations,
        frame_size,
        max_payload_len,
    }: Workload,
) {
    let mut ws = tokio_websockets::ServerBuilder::new()
        .config(Config::default().frame_size(frame_size.get()))
        .limits(Limits::default().max_payload_len(Some(max_payload_len as _)))
        .serve(DeterministicStream {
            reads,
            read_limit_or_error,
        });

    futures::executor::block_on(async move {
        for operation in operations {
            match operation {
                Operation::Read => {
                    let _ = ws.next().await;
                }
                Operation::Write(message) => {
                    let _ = ws.send(message.into()).await;
                }
            }
        }
        let _ = ws.flush().await;
    });
}

fuzz_target!(|workload: Workload| {
    fuzz(workload);
});

impl AsyncRead for DeterministicStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if let Ok(limit) = self.read_limit_or_error.pop().unwrap_or(Ok(usize::MAX)) {
            let end = buf.remaining().min(limit).min(self.reads.len());
            let remainder = self.reads.split_off(end);
            buf.put_slice(&self.reads);
            self.reads = remainder;
            Poll::Ready(Ok(()))
        } else {
            Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, "???")))
        }
    }
}

impl AsyncWrite for DeterministicStream {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "unsupported",
        )))
    }
}
