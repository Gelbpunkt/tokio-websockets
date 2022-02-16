use futures_util::SinkExt;
use http::Uri;
use tokio_websockets::{ClientBuilder, CloseCode, Connector, Error};

use std::str::FromStr;

#[cfg(feature = "simd")]
const AGENT: &str = "tokio-websockets-avx2";
#[cfg(not(feature = "simd"))]
const AGENT: &str = "tokio-websockets";

async fn get_case_count() -> Result<u32, Error> {
    let uri = Uri::from_static("ws://localhost:9001/getCaseCount");
    let mut stream = ClientBuilder::from_uri(uri)
        .set_connector(&Connector::Plain)
        .connect()
        .await?;
    let msg = stream.read_message().await.unwrap()?;

    stream
        .close(Some(CloseCode::NormalClosure), None)
        .await
        .unwrap();

    Ok(msg.as_text().unwrap().parse::<u32>().unwrap())
}

async fn update_reports() -> Result<(), Error> {
    let uri = Uri::from_str(&format!(
        "ws://localhost:9001/updateReports?agent={}",
        AGENT
    ))
    .unwrap();
    let mut stream = ClientBuilder::from_uri(uri)
        .set_connector(&Connector::Plain)
        .connect()
        .await?;

    stream.close(Some(CloseCode::NormalClosure), None).await?;

    Ok(())
}

async fn run_test(case: u32) -> Result<(), Error> {
    println!("Running test case {}", case);

    let uri = Uri::from_str(&format!(
        "ws://localhost:9001/runCase?case={}&agent={}",
        case, AGENT
    ))
    .unwrap();

    let mut stream = ClientBuilder::from_uri(uri)
        .set_connector(&Connector::Plain)
        .connect()
        .await?;

    while let Some(msg) = stream.read_message().await {
        let msg = msg?;

        if msg.is_text() || msg.is_binary() {
            stream.send(msg).await?;
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let total = get_case_count().await?;
    println!("Running {} tests", total);

    for case in 1..=total {
        if let Err(e) = run_test(case).await {
            match e {
                Error::Protocol(_) => {}
                _ => eprintln!("Testcase failed: {:?}", e),
            }
        };
    }

    update_reports().await?;

    Ok(())
}
