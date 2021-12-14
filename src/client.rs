#[cfg(feature = "http-integration")]
use bytes::Bytes;
#[cfg(feature = "http-integration")]
use http::{
    header::{CONNECTION, SEC_WEBSOCKET_KEY, SEC_WEBSOCKET_VERSION, UPGRADE},
    Request, Uri,
};
#[cfg(feature = "http-integration")]
use http_body::Empty;
#[cfg(feature = "http-integration")]
use rand::{thread_rng, RngCore};

#[cfg(feature = "http-integration")]
/// Create a HTTP upgrade request for use with HTTP libraries.
///
/// This can be sent with a client and then waiting for the upgrade to complete.
///
/// ```rust
/// # use hyper::{self, Client, client::HttpConnector, Uri, StatusCode};
/// # use tokio_websockets::{
/// #     client::upgrade_request,
/// #     proto::{Role, WebsocketStream},
/// # };
/// #
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// /// Create a new hyper client, ideally you'd reuse an existing one
/// /// The HTTP Connector does not allow ws:// per default
/// let mut connector = HttpConnector::new();
/// connector.enforce_http(false);
///
/// let client = Client::builder().build(connector);
///
/// let uri = Uri::from_static("ws://localhost:9001/getCaseCount");
/// let response = client.request(upgrade_request(uri)?).await?;
///
/// assert!(
///     response.status() == StatusCode::SWITCHING_PROTOCOLS,
///     "Our server didn't upgrade: {}",
///     response.status()
/// );
///
/// let stream = match hyper::upgrade::on(response).await {
///     Ok(upgraded) => WebsocketStream::from_raw_stream(upgraded, Role::Client),
///     Err(e) => panic!("upgrade error: {}", e),
/// };
///
/// /// Do magic with stream
///
/// # Ok(()) }
/// ```
pub fn upgrade_request<T>(uri: T) -> Result<Request<Empty<Bytes>>, http::Error>
where
    Uri: TryFrom<T>,
    <Uri as TryFrom<T>>::Error: Into<http::Error>,
{
    let mut ws_key_raw = [0; 16];
    let mut rng = thread_rng();
    rng.fill_bytes(&mut ws_key_raw);

    Request::builder()
        .uri(uri)
        .header(CONNECTION, "Upgrade")
        .header(UPGRADE, "websocket")
        .header(SEC_WEBSOCKET_VERSION, "13")
        .header(SEC_WEBSOCKET_KEY, base64::encode(ws_key_raw))
        .body(Empty::new())
}
