use futures_util::SinkExt;
use tokio::net::{TcpListener, TcpStream};
use tokio_websockets::{accept, Error};

use std::net::SocketAddr;

#[cfg(feature = "simd")]
const PORT: u16 = 9003;
#[cfg(not(feature = "simd"))]
const PORT: u16 = 9002;

async fn accept_connection(stream: TcpStream) {
    if let Err(e) = handle_connection(stream).await {
        match e {
            Error::ConnectionClosed | Error::Protocol(_) => (),
            err => eprintln!("Error processing connection: {:?}", err),
        }
    }
}

async fn handle_connection(stream: TcpStream) -> Result<(), Error> {
    let mut ws_stream = accept(stream).await?;

    while let Some(msg) = ws_stream.read_message().await {
        let msg = msg?;

        if msg.is_text() || msg.is_binary() {
            ws_stream.send(msg).await?;
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    let addr: SocketAddr = ([127, 0, 0, 1], PORT).into();
    let listener = TcpListener::bind(&addr).await.expect("Can't listen");

    println!("Listening on: {}", addr);

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(accept_connection(stream));
    }
}
