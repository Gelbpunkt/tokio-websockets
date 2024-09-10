#!/bin/sh

load_test() {
    conn=$1
    port=$2
    bytes=$3
    load_test "$conn" 0.0.0.0 "$port" 0 0 "$bytes"
}

# Define targets (no associative arrays in ash, using lists instead)
targets="8080 fastwebsockets /usr/bin/fastwebsockets
9001 uWebSockets /usr/bin/uWebSockets
9002 rust-websocket /usr/bin/rust-websocket
3000 tokio-websockets /usr/bin/tokio-websockets"

# Define test cases
cases="100 20
10 1024
10 $((16 * 1024))
200 $((16 * 1024))
500 $((16 * 1024))"

generate_chart() {
    conn=$1
    bytes=$2
    results_file=$3
    output_file="${conn}-${bytes}-chart.svg"

    gnuplot <<EOF
    set terminal svg size 600,400
    set output '$output_file'
    set boxwidth 0.5
    set style fill solid
    set title "Connections: $conn, Payload size: $bytes bytes"
    set xlabel "WebSocket Implementations"
    set ylabel "Messages per second"
    set xtics rotate by -45
    plot '$results_file' using 2:xtic(1) with boxes title 'Msg/sec', lc rgb "blue"
EOF
}

for case in $cases; do
    conn=$(echo "$case" | cut -d' ' -f1)
    bytes=$(echo "$case" | cut -d' ' -f2)

    results=""
    results_file="/tmp/results_${conn}_${bytes}.txt"

    echo "$targets" | while read port name server; do
        logs=""

        echo "Waiting for $name to start..."
        $server & server_pid=$!
        sleep 1

        client=$(load_test "$conn" "$port" "$bytes")

        # Collect logs, skip the first 5 lines (warmup), and capture the next 20
        logs=$(echo "$client" | grep 'Msg/sec' | tail -n +6 | head -20)

        kill -9 $server_pid

        # Calculate average messages per second
        mps=$(echo "$logs" | awk '{print $2}')
        sum=0
        count=0
        for value in $mps; do
            sum=$((sum + value))
            count=$((count + 1))
        done

        if [ "$count" -gt 0 ]; then
            avg=$((sum / count))
        else
            avg=0
        fi

        echo "$name $avg" >> "$results_file"
    done

    # Generate the chart
    generate_chart "$conn" "$bytes" "$results_file"

    # Clean up the temporary results file
    rm "$results_file"
done
