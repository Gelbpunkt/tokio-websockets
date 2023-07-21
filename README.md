# tokio-websockets

High performance, strict, tokio-util based websockets implementation.

## Why use tokio-websockets?

- Built with tokio-util, intended to be used with tokio from the ground up
- Minimal dependencies: The base only requires:
  - tokio, tokio-util, bytes, futures-core, futures-sink
  - SHA1 backend, e.g. sha1_smol (see [Feature flags](#feature-flags))
- Big selection of features to tailor dependencies to any project (see [Feature flags](#feature-flags))
- SIMD support: AVX2, SSE2 or NEON for frame (un)masking and accelerated UTF-8 validation
- Strict conformance with the websocket specification, passes the [Autobahn test suite](https://github.com/crossbario/autobahn-testsuite) without relaxations [by default](https://gelbpunkt.github.io/tokio-websockets/index.html) (some can be enabled for performance)
- TLS support
- Reusable TLS connectors
- Uses widely known crates from the ecosystem for types, for example `Uri` from `http` in the client
- Cheaply clonable messages due to `Bytes` as payload storage
- Tuned for performance: no unnecessary duplicate UTF-8 validation, no duplicate bounds checking (this however heavily uses unsafe code, which is sound to my knowledge, if not, open an issue!)

## Feature flags

Feature flags in tokio-websockets are added to allow tailoring it to your needs.

- `simd` will enable AVX2, SSE2 or NEON accelerated masking and UTF-8 validation
- `client` enables a tiny client implementation
- `server` enables a tiny server implementation
- `http-integration` enables a method for websocket upgrade [`http::Request`](https://docs.rs/http/latest/http/request/struct.Request.html) generation

TLS support is supported via any of the following feature flags:

- `native-tls` for a [`tokio-native-tls`](https://docs.rs/tokio-native-tls/latest/tokio_native_tls/) backed implementation
- `rustls-webpki-roots` for a [`tokio-rustls`](https://docs.rs/tokio-rustls/latest/tokio_rustls/) backed implementation with [`webpki-roots`](https://docs.rs/webpki-roots/latest/webpki_roots/)
- `rustls-native-roots` for a [`tokio-rustls`](https://docs.rs/tokio-rustls/latest/tokio_rustls/) backed implementation with [`rustls-native-certs`](https://docs.rs/rustls-native-certs/latest/rustls_native_certs/)

One SHA1 implementation is required, usually provided by the TLS implementation:

- [`ring`](https://docs.rs/ring/latest/ring/) is used if `rustls` is the TLS library
- The `openssl` feature will use [`openssl`](https://docs.rs/openssl/latest/openssl/), usually prefered on most Linux/BSD systems with `native-tls`
- The [`sha1_smol`](https://docs.rs/sha1_smol/latest/sha1_smol/) feature can be used as a fallback if no TLS is needed

The `client` feature requires enabling one random number generator:

- [`fastrand`](https://docs.rs/fastrand/latest/fastrand) is the default used and a `PRNG`
- [`getrandom`](https://docs.rs/getrandom/latest/getrandom) can be used as a cryptographically secure RNG
- [`rand`](https://docs.rs/rand/latest/rand) can be used as an alternative to `fastrand` and should be preferred if it is already in the dependency tree

For these reasons, I recommend disabling default features and using a configuration that makes sense for you, for example:

```toml
# Tiny client
tokio-websockets = { version = "*", default-features = false, features = ["client", "fastrand", "sha1_smol"] }
# Client with SIMD, cryptographically secure RNG and rustls
tokio-websockets = { version = "*", default-features = false, features = ["client", "getrandom", "simd", "rustls-webpki-roots"] }
```

## Example

This is a simple websocket echo server without any proper error handling.

More examples can be found in the [examples folder](https://github.com/Gelbpunkt/tokio-websockets/tree/main/examples).

```rust
use futures_util::{SinkExt, StreamExt};
use http::Uri;
use tokio::net::TcpListener;
use tokio_websockets::{ClientBuilder, Error, Message, ServerBuilder};

#[tokio::main]
async fn main() -> Result<(), Error> {
  let listener = TcpListener::bind("127.0.0.1:3000").await?;

  tokio::spawn(async move {
    while let Ok((stream, _)) = listener.accept().await {
      let mut ws_stream = ServerBuilder::new()
        .accept(stream)
        .await?;

      tokio::spawn(async move {
        // Just an echo server, really
        while let Some(Ok(msg)) = ws_stream.next().await {
          if msg.is_text() || msg.is_binary() {
            ws_stream.send(msg).await?;
          }
        }

        Ok::<_, Error>(())
      });
    }

    Ok::<_, Error>(())
  });

  let uri = Uri::from_static("ws://127.0.0.1:3000");
  let (mut client, _) = ClientBuilder::from_uri(uri).connect().await?;

  client.send(Message::text(String::from("Hello world!"))).await?;

  while let Some(Ok(msg)) = client.next().await {
    if let Ok(text) = msg.as_text() {
      assert_eq!(text, "Hello world!");
      // We got one message, just stop now
      client.close().await?;
    }
  }

  Ok(())
}
```

## MSRV

The current MSRV for all feature combinations is Rust 1.64.

## Caveats / Limitations / ToDo

Websocket compression is currently unsupported.
