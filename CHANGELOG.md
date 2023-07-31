# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

# 0.4.0 (unreleased)

## Dependencies

## Added

- Added support for NEON-accelerated frame (un)masking (currently only on aarch64)
- Limits for frame and message size can be applied via `{ClientBuilder, ServerBuilder}::limits` to protect against malicious peers
- The websocket stream can now be configured via `{ClientBuilder, ServerBuilder}::config`. This currently only supports changing the frame payload size that outgoing messages are chunked into

## Changed

- Update `webpki` to 0.24, `webpki-roots` to 0.25 and `rust-webpki` to 0.101
- The crate now specifies and validates a MSRV of Rust 1.64
- The library now validates UTF-8 in partial continuation frames to a text message
- Several types now implement `Debug`
- The performance of the decoder has been improved
- `upgrade::Error` is now publicly accessible and can thus be matched on
- **[breaking]** `ClientBuilder` methods which perform a handshake now return a tuple of `(WebsocketStream, Response)` to retrieve e.g. response headers
- **[breaking]** `WebsocketStream` is now fully cancellation safe and implements `Stream`, therefore using `WebsocketStream::next` now requires having `StreamExt` in scope
- **[breaking]** `CloseCode` is now a wrapper around a `NonZeroU16` instead of an enum. Enum variants have been moved to associated constants. This fixes possible creation of disallowed close codes by the user
- **[breaking]** All error types are now marked as `non_exhaustive` to prevent consumer build breakage when new variants are added

## Removed

- **[breaking]** `{ClientBuilder, ServerBuilder}::fail_fast_on_invalid_utf8` has been removed and is now always enabled because our implementation for streaming UTF-8 validation is already efficient enough, benchmarks show no gains anymore
- **[breaking]** Some error variants have been removed and coalesced into other variants (mostly into `InvalidOpcode`). `ConnectionClosed` has been entirely removed in favor of returning `None` from `poll_next` because it is more idiomatic
- **[breaking]** `WebsocketStream::close` has been removed. The `SinkExt::close` implementation will send a default close message and properly shut down the sink. Custom close messages can be sent via `SinkExt::send` followed by `SinkExt::close`
- The `futures-util` dependency has been removed

## Fixed

- A few inconsistencies with the websocket RFC have been fixed
