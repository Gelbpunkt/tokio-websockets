use tokio::net::{TcpListener, TcpStream};
use tokio_websockets::{accept, Error};

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
            ws_stream.write_message(msg).await?;
        } else if msg.is_close() {
            break;
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    let addr = "127.0.0.1:9002";
    let listener = TcpListener::bind(&addr).await.expect("Can't listen");
    println!("Listening on: {}", addr);

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(accept_connection(stream));
    }
}
