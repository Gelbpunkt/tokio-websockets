# tokio-websockets

Semi-WIP websockets implementation for Tokio 1.x with a focus on very high performance and tiny dependencies.

## Status

Both the client and server implementations pass the Autobahn test suite entirely, with strict spec conformance, even more strict than tungstenite.

You can find automated benchmark results [here](https://gelbpunkt.github.io/tokio-websockets/index.html).

## TODO

- Find a way to implement `Stream` and `Sink` for `WebsocketStream`
- Maybe remove client and/or server implementation in favor of only allowing usage with hyper for security reasons
