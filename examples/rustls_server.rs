use std::{
    fs::File,
    io::{self, BufReader},
    net::SocketAddr,
    sync::Arc,
};

use futures_util::SinkExt;
use rustls_pemfile::{certs, pkcs8_private_keys};
use tokio::net::TcpListener;
use tokio_rustls::{
    rustls::{self, Certificate, PrivateKey},
    TlsAcceptor,
};
use tokio_websockets::Message;

const PATH_TO_CERT: &str = "certs/localhost.crt";
const PATH_TO_KEY: &str = "certs/localhost.key";

fn load_certs(path: &str) -> io::Result<Vec<Certificate>> {
    certs(&mut BufReader::new(File::open(path)?))
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid cert"))
        .map(|mut certs| certs.drain(..).map(Certificate).collect())
}

fn load_keys(path: &str) -> io::Result<Vec<PrivateKey>> {
    pkcs8_private_keys(&mut BufReader::new(File::open(path)?))
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid key"))
        .map(|mut keys| keys.drain(..).map(PrivateKey).collect())
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    let certs = load_certs(PATH_TO_CERT)?;
    let mut keys = load_keys(PATH_TO_KEY)?;

    let config = rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(certs, keys.remove(0))
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;

    let acceptor = TlsAcceptor::from(Arc::new(config));

    let listener = TcpListener::bind(&addr).await?;

    loop {
        let (stream, _) = listener.accept().await?;
        let acceptor = acceptor.clone();

        let fut = async move {
            let stream = acceptor.accept(stream).await?;

            let mut ws = tokio_websockets::ServerBuilder::new()
                .accept(stream)
                .await?;

            // From here, do what you want with it
            ws.send(Message::text(String::from("Hello, world!")))
                .await?;

            ws.close().await?;

            Ok(()) as Result<(), tokio_websockets::Error>
        };

        tokio::spawn(async move {
            if let Err(err) = fut.await {
                eprintln!("{:?}", err);
            }
        });
    }
}
