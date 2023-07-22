use std::net::SocketAddr;

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_websockets::{Error, Limits, ServerBuilder};

fn get_port() -> u16 {
    #[cfg(feature = "simd")]
    {
        if std::env::var("SKIP_FAIL_FAST").is_ok() {
            9005
        } else {
            9004
        }
    }
    #[cfg(not(feature = "simd"))]
    {
        if std::env::var("SKIP_FAIL_FAST").is_ok() {
            9003
        } else {
            9002
        }
    }
}

async fn accept_connection(stream: TcpStream) {
    if let Err(e) = handle_connection(stream).await {
        match e {
            Error::Protocol(_) => (),
            err => eprintln!("Error processing connection: {err:?}"),
        }
    }
}

async fn handle_connection(stream: TcpStream) -> Result<(), Error> {
    let fail_fast_on_invalid_utf8 = std::env::var("SKIP_FAIL_FAST").is_err();
    let mut ws_stream = ServerBuilder::new()
        .limits(Limits::unlimited())
        .fail_fast_on_invalid_utf8(fail_fast_on_invalid_utf8)
        .accept(stream)
        .await?;

    while let Some(msg) = ws_stream.next().await {
        let msg = msg?;

        if msg.is_text() || msg.is_binary() {
            ws_stream.send(msg).await?;
        }
    }

    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let addr: SocketAddr = ([127, 0, 0, 1], get_port()).into();
    let listener = TcpListener::bind(&addr).await.expect("Can't listen");

    println!("Listening on: {addr}");

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(accept_connection(stream));
    }
}
