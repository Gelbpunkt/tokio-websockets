use http::Uri;
use tokio_websockets::{ClientBuilder, Error};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let uri = Uri::from_static("wss://gateway.discord.gg");
    let mut client = ClientBuilder::from_uri(uri).connect().await?;

    while let Some(item) = client.next().await {
        println!("{:?}", item);
    }

    Ok(())
}
