use hyper::{
    client::HttpConnector,
    header::{CONNECTION, SEC_WEBSOCKET_KEY, SEC_WEBSOCKET_VERSION, UPGRADE},
    upgrade::Upgraded,
    Body, Client, Request, StatusCode, Uri,
};
use rand::{
    distributions::{Alphanumeric, DistString},
    thread_rng,
};

use crate::proto::{Role, WebsocketStream};

pub async fn client(uri: Uri) -> WebsocketStream<Upgraded> {
    let mut rng = thread_rng();
    let ws_key_raw = Alphanumeric {}.sample_string(&mut rng, 16);

    let req = Request::builder()
        .uri(uri)
        .header(CONNECTION, "Upgrade")
        .header(UPGRADE, "websocket")
        .header(SEC_WEBSOCKET_VERSION, "13")
        .header(SEC_WEBSOCKET_KEY, base64::encode(ws_key_raw))
        .body(Body::empty())
        .unwrap();

    let mut connector = HttpConnector::new();
    connector.enforce_http(false);

    let res = Client::builder()
        .build(connector)
        .request(req)
        .await
        .unwrap();

    if res.status() != StatusCode::SWITCHING_PROTOCOLS {
        panic!("Our server didn't upgrade: {}", res.status());
    }

    match hyper::upgrade::on(res).await {
        Ok(upgraded) => WebsocketStream::new(upgraded, Role::Client),
        Err(e) => panic!("upgrade error: {}", e),
    }
}
