#!/bin/bash

# Configuration
SERVER_URL="ws://localhost:3000"
ITERATIONS=10
MASTERS_PER_ITERATION=5
MESSAGE_INTERVAL=0.5
RAW_DATA=$(head -c 1000 /dev/urandom | base64 | tr -d '\n')
DATA_PAYLOAD="{\"type\":\"telemetry\",\"blob\":\"$RAW_DATA\"}"

PIDS=()

# Helper function for organic delays
random_sleep() {
    local min=$1
    local max=$2
    local sleep_time=$(awk -v min="$min" -v max="$max" 'BEGIN{srand(); print min+rand()*(max-min)}')
    sleep "$sleep_time"
}

cleanup() {
    echo ""
    echo "Stopping all clients..."
    for pid in "${PIDS[@]}"; do
        kill $pid 2>/dev/null
    done
    wait 2>/dev/null
    echo "Done."
}

trap cleanup SIGINT SIGTERM EXIT

echo "Starting Data Load Test on $SERVER_URL"
echo "Spawning $ITERATIONS Devices with $MASTERS_PER_ITERATION Masters each"
echo "Message Interval: $MESSAGE_INTERVAL seconds"
echo "---------------------------------------------------"

for (( i=1; i<=ITERATIONS; i++ ))
do
    DEVICE_ID="DataTestDevice_$i"
    
    random_sleep 0.2 1.0
    echo "Initializing $DEVICE_ID..."

    # Spawn Slave with continuous data stream (echoing data periodically)
    (
        while true; do
            echo "$DATA_PAYLOAD"
            sleep $MESSAGE_INTERVAL
        done | wscat -c "$SERVER_URL/register?id=$DEVICE_ID" > /dev/null 2>&1
    ) &
    PIDS+=($!)
    echo "  [+] Slave connected & transmitting"

    random_sleep 0.1 0.4

    # Spawn Masters
    for (( j=1; j<=MASTERS_PER_ITERATION; j++ ))
    do
        (
            while true; do
                echo "$DATA_PAYLOAD"
                sleep $MESSAGE_INTERVAL
            done | wscat -c "$SERVER_URL/pair?id=$DEVICE_ID" > /dev/null 2>&1
        ) &
        PIDS+=($!)
        
        random_sleep 0.01 0.1
    done
    echo "  [+] $MASTERS_PER_ITERATION Masters connected & transmitting"
done

echo "---------------------------------------------------"
echo "All clients active and exchanging data."
echo "Press CTRL+C to stop the test."

wait
