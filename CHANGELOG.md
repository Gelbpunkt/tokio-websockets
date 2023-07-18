# 0.4.0 (unreleased)

- Update `webpki` to 0.24 and `rust-webpki` to 0.101
- `upgrade::Error` is now publicly accessible and can thus be matched on
- `ClientBuilder` methods which perform a handshake now return a tuple of `(WebsocketStream, Response)` to retrieve e.g. response headers
- Several types now implement `Debug`
- `WebsocketStream` now implements `Stream`, therefore using `WebsocketStream::next` now requires having `StreamExt` in scope
- Limits for frame and message size can be applied via `{ClientBuilder,ServerBuilder}::limits` to protect against malicious peers
- Added support for NEON-accelerated frame (un)masking
- The crate now specifies and validates a MSRV of Rust 1.64
