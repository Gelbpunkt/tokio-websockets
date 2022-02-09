#!/usr/bin/bash
set -euo pipefail
set -x

function cleanup() {
    kill -9 ${WSSERVER_PID}
}

trap cleanup TERM EXIT

$1 & WSSERVER_PID=$!

sleep 3

podman run --rm -it \
    -v "${PWD}/autobahn/config:/config" \
    -v "${PWD}/autobahn/reports:/reports" \
    --net host \
    --security-opt label=disable \
    --name autobahn \
    crossbario/autobahn-testsuite \
    wstest -m fuzzingclient -s /config/fuzzingclient.json
