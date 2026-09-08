#!/bin/bash
set -e

# Set data directory
BITCOIN_DATA="/home/bitcoin/.bitcoin"

# Ensure data directory exists
mkdir -p "$BITCOIN_DATA"

# Wait for configuration file to be available
if [ ! -f "$BITCOIN_DATA/bitcoin.conf" ]; then
    echo "Configuration file not found at $BITCOIN_DATA/bitcoin.conf"
    exit 1
fi

echo "Starting Bitcoin Core in regtest mode..."
echo "Data directory: $BITCOIN_DATA"

# Background process to make cookie file readable when it's created
(
    while [ ! -f "$BITCOIN_DATA/regtest/.cookie" ]; do
        sleep 1
    done
    chmod 644 "$BITCOIN_DATA/regtest/.cookie"
    chmod 755 "$BITCOIN_DATA/regtest"
    echo "Cookie file and directory permissions updated"
) &

# Execute bitcoind with proper configuration
exec bitcoind \
    -datadir="$BITCOIN_DATA" \
    -conf="$BITCOIN_DATA/bitcoin.conf" \
    "$@"
