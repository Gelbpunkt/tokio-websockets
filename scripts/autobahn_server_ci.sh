#!/usr/bin/bash
set -euo pipefail
set -x

function cleanup() {
    kill -9 ${WSSERVER1_PID}
    kill -9 ${WSSERVER2_PID}
    kill -9 ${WSSERVER3_PID}
}

trap cleanup TERM EXIT

target/release/examples/autobahn_server & WSSERVER1_PID=$!
target/release/examples/autobahn_server_simd & WSSERVER2_PID=$!
target/release/examples/autobahn_server_tungstenite & WSSERVER3_PID=$!

sleep 3

podman run --rm -it \
    -v "${PWD}/autobahn/config:/config" \
    -v "${PWD}/autobahn/reports:/reports" \
    --net host \
    --security-opt label=disable \
    --name autobahn \
    crossbario/autobahn-testsuite \
    wstest -m fuzzingclient -s /config/fuzzingclient.json
