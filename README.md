# tokio-websockets

WIP

## Status

The client implementation passes the Autobahn test suite entirely, with strict spec conformance, even more strict than tungstenite.

The server implementation does handshaking, but will currently always run into fail fast on invalid UTF8 checks because it does not unmask before the check.
