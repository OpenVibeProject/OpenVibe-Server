#!/bin/bash

# Configuration
SERVER_URL="ws://localhost:3000"
ITERATIONS=10
MASTERS_PER_ITERATION=20
SLAVES_PER_ITERATION=1

PIDS=()

# Helper function for organic delays
# Usage: random_sleep <min> <max>
random_sleep() {
    local min=$1
    local max=$2
    # Generates a random float between min and max
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

echo "Starting Organic Load Test on $SERVER_URL"
echo "Spawning $ITERATIONS Groups (Devices)"
echo "---------------------------------------------------"

for (( i=1; i<=ITERATIONS; i++ ))
do
    DEVICE_ID="TestDevice_$i"
    
    # Random delay before a new device "boots up" (0.2 to 1.5 seconds)
    random_sleep 0.2 1.5
    echo "Initializing $DEVICE_ID..."

    # Spawn Slave
    tail -f /dev/null | wscat -c "$SERVER_URL/register?id=$DEVICE_ID" > /dev/null 2>&1 &
    PIDS+=($!)
    echo "  [+] Slave connected"

    # Brief pause while the "Slave" registers before Masters start jumping in
    random_sleep 0.1 0.4

    # Spawn Masters
    for (( j=1; j<=MASTERS_PER_ITERATION; j++ ))
    do
        tail -f /dev/null | wscat -c "$SERVER_URL/pair?id=$DEVICE_ID" > /dev/null 2>&1 &
        PIDS+=($!)
        
        # Micro-delays between individual master connections (10ms to 100ms)
        # This prevents a "thundering herd" on a single CPU cycle
        random_sleep 0.01 0.1
    done
    echo "  [+] $MASTERS_PER_ITERATION Masters connected"
done

echo "---------------------------------------------------"
echo "All clients connected. Testing active..."
echo "Press CTRL+C to stop the test."

wait