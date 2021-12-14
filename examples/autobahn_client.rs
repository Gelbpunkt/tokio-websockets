use bytes::Bytes;
use http::{StatusCode, Uri};
use hyper::{
    client::{Client as HttpClient, HttpConnector},
    upgrade::Upgraded,
};
use tokio_websockets::{upgrade_request, CloseCode, Error, Role, WebsocketStream};

use std::str::FromStr;

#[cfg(feature = "simd")]
const AGENT: &str = "tokio-websockets-avx2";
#[cfg(not(feature = "simd"))]
const AGENT: &str = "tokio-websockets";

type Client = HttpClient<HttpConnector, http_body::Empty<Bytes>>;

async fn client(client: &Client, uri: Uri) -> WebsocketStream<Upgraded> {
    let request = upgrade_request(uri).unwrap();
    let response = client.request(request).await.unwrap();

    assert!(
        response.status() == StatusCode::SWITCHING_PROTOCOLS,
        "Our server didn't upgrade: {}",
        response.status()
    );

    match hyper::upgrade::on(response).await {
        Ok(upgraded) => WebsocketStream::from_raw_stream(upgraded, Role::Client),
        Err(e) => panic!("upgrade error: {}", e),
    }
}

async fn get_case_count(http_client: &Client) -> Result<u32, Error> {
    let uri = Uri::from_static("ws://localhost:9001/getCaseCount");
    let mut stream = client(http_client, uri).await;
    let msg = stream.read_message().await.unwrap()?;

    stream
        .close(Some(CloseCode::NormalClosure), None)
        .await
        .unwrap();

    Ok(msg.into_text().unwrap().parse::<u32>().unwrap())
}

async fn update_reports(http_client: &Client) -> Result<(), Error> {
    let uri = Uri::from_str(&format!(
        "ws://localhost:9001/updateReports?agent={}",
        AGENT
    ))
    .unwrap();
    let mut stream = client(http_client, uri).await;

    stream.close(Some(CloseCode::NormalClosure), None).await?;

    Ok(())
}

async fn run_test(http_client: &Client, case: u32) -> Result<(), Error> {
    println!("Running test case {}", case);

    let uri = Uri::from_str(&format!(
        "ws://localhost:9001/runCase?case={}&agent={}",
        case, AGENT
    ))
    .unwrap();

    let mut stream = client(http_client, uri).await;

    while let Some(msg) = stream.read_message().await {
        let msg = msg?;

        if msg.is_text() || msg.is_binary() {
            stream.write_message(msg).await?;
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let mut http_connector = HttpConnector::new();
    http_connector.enforce_http(false);

    let http_client = HttpClient::builder().build(http_connector);

    let total = get_case_count(&http_client).await?;
    println!("Running {} tests", total);

    for case in 1..=total {
        if let Err(e) = run_test(&http_client, case).await {
            match e {
                Error::Protocol(_) => {}
                _ => eprintln!("Testcase failed: {:?}", e),
            }
        };
    }

    update_reports(&http_client).await?;

    Ok(())
}
