use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Error};

const AGENT: &str = "tokio-tungstenite";

async fn get_case_count() -> Result<u32, Error> {
    let (mut stream, _) = connect_async("ws://localhost:9001/getCaseCount").await?;
    let msg = stream.next().await.unwrap()?;

    stream.close(None).await.unwrap();

    Ok(msg.into_text().unwrap().parse::<u32>().unwrap())
}

async fn update_reports() -> Result<(), Error> {
    let (mut stream, _) =
        connect_async(format!("ws://localhost:9001/updateReports?agent={}", AGENT)).await?;

    stream.close(None).await?;

    Ok(())
}

async fn run_test(case: u32) -> Result<(), Error> {
    println!("Running test case {}", case);

    let (mut stream, _) = connect_async(format!(
        "ws://localhost:9001/runCase?case={}&agent={}",
        case, AGENT
    ))
    .await?;

    while let Some(msg) = stream.next().await {
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
                Error::ConnectionClosed | Error::Protocol(_) | Error::Utf8 => {}
                _ => eprintln!("Testcase failed: {:?}", e),
            }
        };
    }

    update_reports().await?;

    Ok(())
}
