use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_websockets::{Config, Error, Limits, ServerBuilder};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Error> {
    let listener = TcpListener::bind("127.0.0.1:3000").await?;

    loop {
        let (conn, _) = listener.accept().await?;

        tokio::spawn(async move {
            let mut server = unsafe {
                ServerBuilder::new()
                    .config(Config::default().frame_size(usize::MAX))
                    .limits(Limits::unlimited())
                    .accept(conn)
                    .await
                    .unwrap_unchecked()
            };

            while let Some(Ok(item)) = server.next().await {
                if item.is_text() || item.is_binary() {
                    unsafe { server.send(item).await.unwrap_unchecked() };
                }
            }
        });
    }
}
