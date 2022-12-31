use futures_util::SinkExt;
use tokio::net::TcpListener;
use tokio_websockets::{Error, ServerBuilder};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let listener = TcpListener::bind("127.0.0.1:3000").await?;

    loop {
        let (conn, _) = listener.accept().await?;
        println!("1");
        let mut server = ServerBuilder::new().accept(conn).await?;
        println!("2");

        while let Some(Ok(item)) = server.next().await {
            println!("Received: {item:?}");
            server.send(item).await?;
        }
    }
}
