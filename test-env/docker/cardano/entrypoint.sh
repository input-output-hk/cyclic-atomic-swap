#!/bin/bash
set -e

chmod 600 /keys/*
chmod +x /busybox
chmod 777 /shared

target_time=$(( ($(date +%s) / 30 + 1) * 30 ))
echo "$target_time" > /shared/cardano.start
byron_startTime=$target_time
shelley_systemStart=$(date --utc +"%Y-%m-%dT%H:%M:%SZ" --date="@$target_time")

/busybox sed "s/\"startTime\": [0-9]*/\"startTime\": $byron_startTime/" /shared/byron/genesis.json.base > /shared/byron/genesis.json
echo "Updated startTime value in Byron genesis.json to: $byron_startTime"

/busybox sed "s/\"systemStart\": \"[^\"]*\"/\"systemStart\": \"$shelley_systemStart\"/" /shared/shelley/genesis.json.base > /shared/shelley/genesis.json
echo "Updated systemStart value in Shelley genesis.json to: $shelley_systemStart"

cp /shared/conway/genesis.conway.json.base /shared/conway/genesis.conway.json
cp /shared/shelley/genesis.alonzo.json.base /shared/shelley/genesis.alonzo.json
echo "Created /shared/conway/genesis.conway.json and /shared/shelley/genesis.alonzo.json"

byron_hash=$(/bin/cardano-cli byron genesis print-genesis-hash --genesis-json /shared/byron/genesis.json)
shelley_hash=$(/bin/cardano-cli latest genesis hash --genesis /shared/shelley/genesis.json)
alonzo_hash=$(/bin/cardano-cli latest genesis hash --genesis /shared/shelley/genesis.alonzo.json)
conway_hash=$(/bin/cardano-cli latest genesis hash --genesis /shared/conway/genesis.conway.json)

/busybox sed "s/\"ByronGenesisHash\": \"[^\"]*\"/\"ByronGenesisHash\": \"$byron_hash\"/" /shared/node-1-config.json.base > /shared/node-1-config.json.base.byron
/busybox sed "s/\"ShelleyGenesisHash\": \"[^\"]*\"/\"ShelleyGenesisHash\": \"$shelley_hash\"/" /shared/node-1-config.json.base.byron > /shared/node-1-config.base.shelley
/busybox sed "s/\"AlonzoGenesisHash\": \"[^\"]*\"/\"AlonzoGenesisHash\": \"$alonzo_hash\"/" /shared/node-1-config.base.shelley > /shared/node-1-config.json.base.conway
/busybox sed "s/\"ConwayGenesisHash\": \"[^\"]*\"/\"ConwayGenesisHash\": \"$conway_hash\"/" /shared/node-1-config.json.base.conway > /shared/node-1-config.json

echo "Updated ByronGenesisHash value in config files to: $byron_hash"
echo "Updated ShelleyGenesisHash value in config files to: $shelley_hash"
echo "Updated ConwayGenesisHash value in config files to: $conway_hash"

# Start cardano-node
cardano-node run \
  --topology /shared/node-1-topology.json \
  --database-path /data/db \
  --socket-path /ipc/node.socket \
  --host-addr 0.0.0.0 \
  --port 3001 \
  --config /shared/node-1-config.json \
  --shelley-kes-key /keys/kes.skey \
  --shelley-vrf-key /keys/vrf.skey \
  --shelley-operational-certificate /keys/node.cert &

echo "Waiting for node.socket..."

while true; do
    if [ -e "/ipc/node.socket" ]; then
        break
    else
        sleep 1
    fi
done

echo "Found node.socket..."

# ---------------------------------------------------------------------------
# Fund the user-facing funded_address from the genesis UTXO
# ---------------------------------------------------------------------------
fund_address() {
  local SOCKET="/ipc/node.socket"
  local MAGIC="42"
  local FUND_AMOUNT="1000000000000"  # 1,000,000 ADA in lovelace

  # Wait until the node can answer queries (may need a few slots)
  echo "Waiting for node to be query-ready..."
  for i in $(/busybox seq 1 60); do
    if cardano-cli latest query tip --socket-path "$SOCKET" --testnet-magic "$MAGIC" 2>/dev/null | /busybox grep -q '"slot"'; then
      break
    fi
    sleep 1
  done

  # Build funded_address from its verification key
  local FUNDED_ADDR
  FUNDED_ADDR=$(cardano-cli latest address build \
    --payment-verification-key-file /keys/funded_address.vkey \
    --testnet-magic "$MAGIC")
  echo "Funded address: $FUNDED_ADDR"

  # Check if already funded (idempotent on container restart)
  local EXISTING
  EXISTING=$(cardano-cli latest query utxo \
    --address "$FUNDED_ADDR" \
    --socket-path "$SOCKET" \
    --testnet-magic "$MAGIC" --out-file /dev/stdout)
  if echo "$EXISTING" | /busybox grep -q "lovelace"; then
    echo "funded_address already has UTXOs — skipping funding"
    return 0
  fi

  # Find the genesis UTXO
  local GENESIS_ADDR
  GENESIS_ADDR=$(cat /shared/shelley/genesis-utxo.addr)
  local GENESIS_UTXO_JSON
  GENESIS_UTXO_JSON=$(cardano-cli latest query utxo \
    --address "$GENESIS_ADDR" \
    --socket-path "$SOCKET" \
    --testnet-magic "$MAGIC" --out-file /dev/stdout)

  # Parse the first UTXO txhash#ix (format: "txhash#ix": { ... })
  local TX_IN
  TX_IN=$(echo "$GENESIS_UTXO_JSON" | /busybox grep -oE '"[a-f0-9]{64}#[0-9]+"' | /busybox head -1 | /busybox tr -d '"')
  if [ -z "$TX_IN" ]; then
    echo "WARNING: No genesis UTXO found — cannot fund funded_address"
    return 1
  fi
  echo "Using genesis UTXO: $TX_IN"

  # Query protocol parameters
  cardano-cli latest query protocol-parameters \
    --socket-path "$SOCKET" \
    --testnet-magic "$MAGIC" \
    --out-file /tmp/protocol-params.json

  # Build, sign, submit (use conway era explicitly)
  cardano-cli conway transaction build \
    --socket-path "$SOCKET" \
    --testnet-magic "$MAGIC" \
    --tx-in "$TX_IN" \
    --tx-out "$FUNDED_ADDR+$FUND_AMOUNT" \
    --change-address "$GENESIS_ADDR" \
    --out-file /tmp/fund-tx.raw

  cardano-cli conway transaction sign \
    --testnet-magic "$MAGIC" \
    --tx-body-file /tmp/fund-tx.raw \
    --signing-key-file /shared/shelley/genesis-utxo.skey \
    --out-file /tmp/fund-tx.signed

  cardano-cli conway transaction submit \
    --socket-path "$SOCKET" \
    --testnet-magic "$MAGIC" \
    --tx-file /tmp/fund-tx.signed

  echo "Funded funded_address with $(( FUND_AMOUNT / 1000000 )) ADA"
}

fund_address || echo "WARNING: funded_address funding failed — genesis-utxo keys still available at /shared/shelley/"

touch /shared/cardano.ready

wait
