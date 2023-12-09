//! Helper for creating [`http::Request`] objects for HTTP/1.1 Upgrade requests
//! with WebSocket servers.
use http::{
    header::{CONNECTION, SEC_WEBSOCKET_KEY, SEC_WEBSOCKET_VERSION, UPGRADE},
    HeaderValue, Request, Uri,
};

use crate::client::make_key;

/// Create a HTTP upgrade request for use with HTTP libraries.
///
/// This can be sent with a client and then waiting for the upgrade to complete.
///
/// For example, using [hyper](https://docs.rs/hyper/latest/hyper/):
///
/// ```rust
/// use http::{StatusCode, Uri};
/// use hyper_util::{
///     client::legacy::{connect::HttpConnector, Client},
///     rt::tokio::{TokioExecutor, TokioIo},
/// };
/// use tokio_websockets::{upgrade_request, ClientBuilder};
///
/// # use futures_util::{SinkExt, StreamExt};
/// # use tokio_websockets::ServerBuilder;
/// # use tokio::net::TcpListener;
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// # let listener = TcpListener::bind("127.0.0.1:3333").await?;
/// # tokio::spawn(async move {
/// #     let (stream, _) = listener.accept().await?;
/// #     let mut ws_stream = ServerBuilder::new()
/// #         .accept(stream)
/// #         .await.unwrap();
/// #
/// #     while let Some(msg) = ws_stream.next().await {
/// #         let msg = msg?;
/// #
/// #         if msg.is_text() || msg.is_binary() {
/// #             ws_stream.send(msg).await?;
/// #         }
/// #     }
/// #
/// #     Ok::<_, tokio_websockets::Error>(())
/// # });
/// #
/// // Create a new hyper client, ideally you'd reuse an existing one
/// // The HTTP Connector does not allow ws:// per default
/// let mut connector = HttpConnector::new();
/// connector.enforce_http(false);
/// // `upgrade_request` supports any `Body` type that implements `Default`
/// let client: Client<_, String> = Client::builder(TokioExecutor::new()).build(connector);
///
/// let uri = Uri::from_static("ws://localhost:3333");
/// let response = client.request(upgrade_request(uri)?).await?;
///
/// assert!(
///     response.status() == StatusCode::SWITCHING_PROTOCOLS,
///     "Our server didn't upgrade: {}",
///     response.status()
/// );
///
/// let stream = match hyper::upgrade::on(response).await {
///     Ok(upgraded) => ClientBuilder::new().take_over(TokioIo::new(upgraded)),
///     Err(e) => panic!("upgrade error: {}", e),
/// };
///
/// // stream is a WebSocket stream like any other one
///
/// # Ok(()) }
/// ```
///
/// # Errors
///
/// This method returns a [`http::Error`] if assembling the request failed,
/// which should never happen.
pub fn upgrade_request<T, B>(uri: T) -> Result<Request<B>, http::Error>
where
    Uri: TryFrom<T>,
    <Uri as TryFrom<T>>::Error: Into<http::Error>,
    B: Default,
{
    let key_bytes = make_key();

    Request::builder()
        .uri(uri)
        .header(CONNECTION, "Upgrade")
        .header(UPGRADE, "websocket")
        .header(SEC_WEBSOCKET_VERSION, "13")
        .header(SEC_WEBSOCKET_KEY, HeaderValue::from_bytes(&key_bytes)?)
        .body(B::default())
}
