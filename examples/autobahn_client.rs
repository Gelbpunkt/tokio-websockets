use std::str::FromStr;

use futures_util::{SinkExt, StreamExt};
use http::Uri;
use tokio_websockets::{ClientBuilder, CloseCode, Connector, Error, Limits, Message};

fn get_agent() -> &'static str {
    #[cfg(feature = "simd")]
    {
        if std::env::var("SKIP_FAIL_FAST").is_ok() {
            "tokio-websockets-avx2-skip-fail-fast"
        } else {
            "tokio-websockets-avx2"
        }
    }
    #[cfg(not(feature = "simd"))]
    {
        if std::env::var("SKIP_FAIL_FAST").is_ok() {
            "tokio-websockets-skip-fail-fast"
        } else {
            "tokio-websockets"
        }
    }
}

async fn get_case_count() -> Result<u32, Error> {
    let uri = Uri::from_static("ws://localhost:9001/getCaseCount");
    let (mut stream, _) = ClientBuilder::from_uri(uri)
        .connector(&Connector::Plain)
        .connect()
        .await?;
    let msg = stream.next().await.unwrap()?;

    stream
        .feed(Message::close(Some(CloseCode::NormalClosure), ""))
        .await
        .unwrap();
    stream.close().await.unwrap();

    Ok(msg.as_text().unwrap().parse::<u32>().unwrap())
}

async fn update_reports() -> Result<(), Error> {
    let uri = Uri::from_str(&format!(
        "ws://localhost:9001/updateReports?agent={}",
        get_agent()
    ))
    .unwrap();
    let (mut stream, _) = ClientBuilder::from_uri(uri)
        .connector(&Connector::Plain)
        .connect()
        .await?;

    stream
        .feed(Message::close(Some(CloseCode::NormalClosure), ""))
        .await?;
    stream.close().await?;

    Ok(())
}

async fn run_test(case: u32) -> Result<(), Error> {
    println!("Running test case {case}");

    let fail_fast_on_invalid_utf8 = std::env::var("SKIP_FAIL_FAST").is_err();

    let uri = Uri::from_str(&format!(
        "ws://localhost:9001/runCase?case={}&agent={}",
        case,
        get_agent()
    ))
    .unwrap();

    let (mut stream, _) = ClientBuilder::from_uri(uri)
        .fail_fast_on_invalid_utf8(fail_fast_on_invalid_utf8)
        .limits(Limits::unlimited())
        .connector(&Connector::Plain)
        .connect()
        .await?;

    while let Some(msg) = stream.next().await {
        let msg = msg?;

        if msg.is_text() || msg.is_binary() {
            stream.send(msg).await?;
        }
    }

    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Error> {
    let total = get_case_count().await?;
    println!("Running {total} tests");

    for case in 1..=total {
        if let Err(e) = run_test(case).await {
            match e {
                Error::Protocol(_) => {}
                _ => eprintln!("Testcase failed: {e:?}"),
            }
        };
    }

    update_reports().await?;

    Ok(())
}
