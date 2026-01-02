use futures_util::{SinkExt, StreamExt};
use http::Uri;
use tokio_websockets::{ClientBuilder, Error, Message};

#[tokio::main]
async fn main() -> Result<(), Error> {
    // Will set the default crypto provider for libraries which configure rustls
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .unwrap();

    // Connecting to a echo server with TLS set up
    let uri = Uri::from_static("wss://ws.vi-server.org/mirror");
    let (mut client, _) = ClientBuilder::from_uri(uri).connect().await?;

    // Print out if its a plain or TLS socket
    // Requires a TLS feature to be turned on to not be plain
    eprintln!("{:?}", client.get_ref());

    client.send(Message::text("Hello, world!")).await?;

    let msg = client.next().await;

    println!("Got message: {msg:?}");

    client.close().await?;

    Ok(())
}
