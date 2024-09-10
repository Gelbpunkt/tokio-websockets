#![no_main]
use std::task::{Context, Poll};

use libfuzzer_sys::fuzz_target;
use std::io;
use futures::stream::StreamExt;
use std::pin::Pin;
extern crate tokio_websockets;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

fn fuzz(reads: Vec<Result<Vec<u8>, ()>>) {
    struct DeterministicStream {
        reads: Vec<Result<Vec<u8>, ()>>,
    }

    impl AsyncRead for DeterministicStream {
        fn poll_read(
                mut self: Pin<&mut Self>,
                _cx: &mut Context<'_>,
                buf: &mut ReadBuf<'_>,
            ) -> Poll<io::Result<()>> {
            if let Some(next) = self.reads.pop() {
                match next {
                    Ok(mut vec) => {
                        vec.truncate(buf.remaining());
                        buf.put_slice(&vec);
                        Poll::Ready(Ok(()))
                    }
                    Err(_) => {
                        Poll::Ready(Err(io::Error::new(io::ErrorKind::ConnectionReset, "permanently failed?")))
                    }
                }
            } else {
                Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, "out of data")))
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
            Poll::Ready(Err(io::Error::new(io::ErrorKind::Unsupported, "unsupported")))
        }
    }

    let mut ws = tokio_websockets::ServerBuilder::new().serve(DeterministicStream{
        reads
    });

    futures::executor::block_on(async move {
        for _ in 0..10 {
            let _ = ws.next().await;
        }
    })
}

fuzz_target!(|reads: Vec<Result<Vec<u8>, ()>>| {
    fuzz(reads);
});
