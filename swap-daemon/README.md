# Bitcoin/Cardano Atomic Swap Daemon

A Rust implementation of a multi-party cyclic atomic swap protocol between Bitcoin and Cardano using MuSig2 multi-signatures and adaptor signatures.

---

## Overview

This daemon enables trustless cross-chain atomic swaps in a cyclic pattern (A→B→C→A) where participants on different blockchains exchange assets without a trusted intermediary. The protocol uses:

- **MuSig2** — n-of-n Schnorr multi-signatures for locking and spending funds
- **Adaptor signatures** — cryptographic primitive that atomically links secret revelation to on-chain spending
- **Bitcoin Taproot** (P2TR) — keypath spending with tweaked aggregate pubkey
- **Cardano Plutus** — secp256k1 Schnorr signature verification via on-chain Aiken script

---

## Protocol Overview

```
Participant 1 (Bitcoin)  →  Participant 2 (Cardano)  →  Participant 3 (Bitcoin)  →  back to 1
```

Each participant locks funds on their chain. Funds can only be claimed by providing a valid MuSig2 adapted Schnorr signature. The adaptor signature scheme ensures that when any participant claims their funds, they reveal a secret that allows all other participants to claim theirs — atomically and without trust.

---

## Swap Setup and Coordination

Before the protocol can begin, all participants must agree on the swap parameters out-of-band. There is no on-chain or in-protocol coordination step for this — it is assumed that participants have already negotiated and shared their details through some external channel.

Each daemon constructs an identical `Participants` map and `SwapSession` before starting. The fields that must be coordinated are split into two levels:

### Per-Participant Fields

Every participant broadcasts the following to all others:

| Field                      | Description                                                            |
| -------------------------- | ---------------------------------------------------------------------- |
| `id`                       | Unique participant index in the cycle (e.g. 1, 2, 3)                  |
| `blockchain`               | Which chain this participant locks on (`Bitcoin` or `Cardano`)         |
| `tcp_address`              | Address other daemons connect to for message passing                   |
| `target_participant`       | The participant whose lock this participant will claim                  |
| `amount_locking`           | Amount (sats / lovelace) this participant locks                        |
| `amount_claiming`          | Amount this participant expects to claim from `target_participant`     |
| `secp256k1_public_key`     | MuSig2 public key — used in aggregate key on both chains               |
| `cardano_wallet_public_key`| Ed25519 public key — used to derive Cardano receive / refund address   |
| `funding_utxo_txid`        | Txid of the UTXO being locked                                          |
| `funding_utxo_vout`        | Output index of the UTXO being locked                                  |
| `funding_utxo_value`       | Value of the UTXO (sats or lovelace)                                   |
| `funding_utxo_script`      | Hex script pubkey of the UTXO (Bitcoin only)                           |

### Session-Level Fields

The following apply to the swap as a whole and must also be agreed before starting:

| Field                 | Description                                                                           |
| --------------------- | ------------------------------------------------------------------------------------- |
| `id`                  | Unique session identifier shared by all participants                                  |
| `start_block`         | Bitcoin block height at session creation — anchor for BTC refund locktimes            |
| `start_slot`          | Cardano slot at session creation — anchor for ADA refund locktimes                    |
| `refund_window_secs`  | Per-hop refund window duration; staggered locktimes are derived from this value        |
| `bitcoin_fee`         | Satoshis deducted as miner fee on Bitcoin spend and refund txs — **consensus-critical** |
| `cardano_fee`         | Lovelace fee on the Cardano lock tx — **consensus-critical**                          |
| `cardano_collaterals` | Per-participant Cardano collateral UTxOs, used when executing Plutus scripts           |

Fields marked **consensus-critical** must be identical across all participants. See [Fee Model](#fee-model) below.

Each daemon configures `is_me: true` for its own entry and `is_me: false` for all others. Everything else is identical across all daemons. The daemon will panic if the `Participants` map does not form a valid cycle.

---

## Architecture

### Directory Structure

```
src/
  blockchains/
    bitcoin_utils.rs      — Bitcoin tx building, signing, submission via Electrs
    cardano_utils.rs      — Cardano tx building, signing, Plutus script, Dolos/Blockfrost
  cryptography/
    multisig.rs           — MuSig2 signing: nonce aggregation, partial sig computation,
                            finalization, adaptor signing, secret extraction
  protocol/
    leader_election.rs    — Commit-reveal leader election
    refund.rs             — Refund tx signing (safety net before locking)
    spend.rs              — Spend tx adaptor signing and broadcast
    lock_funds.rs         — Lock tx building and broadcasting
    chain_monitor.rs      — On-chain confirmation polling
    secret.rs             — Secret extraction from confirmed spend txs
    session.rs            — Session state machine and message handling
    adaptor_nonce.rs      — Adaptor point broadcast
  types.rs                — Core types: SwapSession, SwapKeys, Participant, WireMessage
  daemon.rs               — Daemon entry point, TCP listener, event loop
  networking.rs           — TCP broadcast utilities
  utils.rs                — Session utility functions

validators/
  swap.ak                 — Aiken smart contract for Cardano lock tx

tests/
  common/mod.rs                                     — Shared test helpers
  spend_tx_integration_test.rs                      — Spend tx signing and adaptation integration tests
  regtest/
    mod.rs                                          — Directory index: local regtest environment tests
    completed_regression_test.rs                    — End-to-end happy-path test (4 participants)
    refunded_regression_test.rs                     — End-to-end refund test: leader absent, non-leaders reach Refunded
    failed_regression_test.rs                       — End-to-end failure test: leader absent + refund txs missing
    20_party_completed_regression_test.rs           — Happy-path test at scale: 20-participant ring
  preprod/
    mod.rs                                          — Directory index: public testnet tests
    preprod_integration_test.rs                     — Integration tests against Cardano preprod network
```

### State Machine

All participants share a common setup phase before the paths diverge based on whether the participant was elected leader.

#### Common setup (all participants)

```
Initialized
  → AwaitingAdaptorPoints        start_swap_session called; each participant broadcasts their adaptor point Ti
  → AwaitingLeaderElectionCommitments    all adaptor points received; commit-reveal leader election begins
  → AwaitingLeaderElectionNonces         all commitments received; nonces revealed, leader determined
  → RefundAndSpendTxsSigning             leader elected; MuSig2 signing of refund txs and adaptor spend txs begins
  → Funding                      all partial sigs collected; each participant builds and submits their lock tx
  → AwaitingLockConfirmations    lock txs submitted; chain polled until all are confirmed
```

#### Non-leader path

Once all lock txs are confirmed, non-leaders have no further information the leader needs — so they reveal their adaptor secret immediately and then wait passively for the leader to act first.

```
  → AwaitingLeaderSpend          secrets revealed to leader; polling begins for leader's spend tx on chain
  → Claiming                 leader's spend tx detected; aggregate secret extracted from its witness/redeemer;
                                 own spend tx adapted with the secret and broadcast
  → Completed
```

#### Leader path

The leader waits to collect all non-leaders' secrets before they can adapt their own spend tx. Once they have them all, they go directly to broadcasting — they already hold everything they need.

```
  → AwaitingSecrets              all lock txs confirmed; waiting to receive SecretReveal from every non-leader
  → Claiming                 all secrets received; aggregate secret assembled; own spend tx adapted and broadcast;
                                 SpendTxBroadcast signal sent to peers
  → Completed
```

#### Failure paths (all participants)

A refund poller runs from the moment a participant's own lock tx is confirmed. If the swap stalls for any reason — leader offline, secrets withheld — each participant can reclaim their funds once the locktime passes.

```
  → Refunded                     refund window opened; refund tx submitted successfully
  → Failed                       refund window opened; refund tx submission failed
```

---

## Cryptographic Protocol

### MuSig2 Signing

Each transaction requires a complete MuSig2 signing round across all participants:

1. **Nonce generation** — each participant generates a `SecNonce` (two random scalars) and derives a `PubNonce` (two elliptic curve points) from it. The `PubNonce` is broadcast to all peers. The `SecNonce` bytes are stored in the session and consumed exactly once during signing.
2. **Partial signing** — once all `PubNonce`s are collected they are summed into an `AggNonce`. Each participant calls `musig2::sign_partial` with their `SecNonce` (consumed here, never reused), private key, `AggNonce`, and message to produce a 32-byte partial signature. This is broadcast to all peers.
3. **Finalization** — once all partial signatures are collected, any participant can call `musig2::aggregate_partial_signatures` to produce a single 64-byte Schnorr signature valid against the aggregate public key.

The signing path uses the musig2 low-level API directly rather than the `FirstRound`/`SecondRound` state machine types. Those types are convenient but do not implement `Clone` or `Serialize`, which makes them impossible to persist across the async message-handling calls that separate the two rounds. Instead, `SecNonce` is serialized to 64 bytes and stored in `MusigRuntime::RoundOne`. When signing occurs the bytes are deserialized once and the resulting `SecNonce` is consumed by `sign_partial`. The session then transitions to `MusigRuntime::RoundTwo`, which carries no nonce bytes — making double-use impossible.

For **refund txs**, standard MuSig2 is used — the combined signature is immediately valid.

For **spend txs**, adaptor signing is used — `musig2::adaptor::sign_partial` and `musig2::adaptor::aggregate_partial_signatures` are used instead. The combined signature is an `AdaptorSignature` encrypted under the aggregate adaptor point `T = T1 + T2 + ... + Tn`. It becomes a valid Schnorr signature only when the aggregate secret `t = t1 + t2 + ... + tn` is applied via `adaptor_sig.adapt(t)`.

### Taproot Tweak

Bitcoin taproot addresses apply a BIP341 tweak to the aggregate key:

```
tweaked_key = internal_key + H_TapTweak(internal_key) * G
```

MuSig2 signing must use `with_tweak(t, true)` (x-only parity normalization) to produce signatures valid under the key embedded in the `script_pubkey`. Failure to normalize parity causes verification to fail approximately 50% of the time — whenever the aggregate key happens to have an odd y-coordinate.

Cardano secp256k1 script verification uses the untweaked aggregate key directly — no taproot tweak is applied.

### Adaptor Signatures

Each participant generates a secret scalar `ti` and public point `Ti = ti * G` at session start. The aggregate adaptor point `T = T1 + T2 + T3` is used during spend tx adaptor signing.

When the leader broadcasts their spend tx on chain, the full signature `s'` is publicly visible. Any participant can then compute:

```
t = s' - s
```

where `s` is the adaptor (pre-)signature they already hold. This reveals the aggregate secret `t`, allowing every participant to adapt their own spend tx and claim their funds.

### Staggered Refund Locktimes

Refund locktimes are staggered according to two rules:

- The leader's refund window opens last — any cheating requires the leader's cooperation, so they bear the cost of delay.
- Each party's refund window opens after their claimant's — if your claimant takes both their refund and your output, you can still claim your own output since the refund window of the person you are claiming from has not yet opened. The only party left without either their refund or their claim is the leader.

Distance is measured by following each participant's claim target starting from the leader: the leader's direct claim target gets distance 1 (earliest window), and the leader itself gets distance N (latest window).

- **Bitcoin**: `locktime = start_block + distance * ceil(refund_window_secs / 600)`
- **Cardano**: `locktime = start_slot + distance * refund_window_secs`

---

## Fee Model

Not all fees need to be agreed in advance. Whether a fee is consensus-critical depends on whether it affects the message that participants sign.

| Transaction     | Fee source                        | Consensus-critical | Reason                                                                                   |
| --------------- | --------------------------------- | ------------------ | ---------------------------------------------------------------------------------------- |
| BTC lock        | implicit (UTXO remainder)         | No                 | No fee field; miner takes whatever is left after `amount_locking`                        |
| BTC spend       | `session.bitcoin_fee`             | **Yes**            | Fee determines output amount, which is part of the Bitcoin sighash all participants sign |
| BTC refund      | `session.bitcoin_fee`             | **Yes**            | Same reason as BTC spend                                                                 |
| ADA lock        | `session.cardano_fee`             | **Yes**            | Fee is in the lock tx body; the lock tx txid (Blake2b of the body) is the message signed by MuSig2 and verified on-chain by the Plutus script — a different fee produces a different txid and the aggregate signature fails |
| ADA spend       | computed dynamically              | No                 | The MuSig2 message is the lock txid, not the spend tx body; fee can be corrected freely after signing |
| ADA refund      | computed dynamically              | No                 | Same reason as ADA spend                                                                 |

### Cardano Spend and Refund Fee Computation

For Cardano Plutus transactions (spend and refund), the fee is not taken from the session — it is computed at submission time by `attach_final_sig`. The process:

1. Build a draft transaction with ceiling execution units (`14_000_000` mem, `10_000_000_000` CPU steps)
2. Evaluate real execution units via the node's evaluate endpoint
3. Compute the minimum fee: `min_fee_a × tx_size + min_fee_b + ⌈price_mem × mem + price_step × cpu⌉` where the linear fee coefficients and execution unit prices are fetched from `/epochs/latest/parameters`
4. Rebuild the transaction with the accurate fee and the corrected output amount

The collateral witness (an Ed25519 signature over the tx body hash) is added after this step and covers the final corrected body.

On Custom (Dolos) networks the evaluate endpoint is not supported, so ceiling execution units are used and the fee is computed from those ceiling values.

---

## Cardano Smart Contract

The Aiken contract (`validators/swap.ak`) locks funds at a script address. The datum stores two fields:

| Field               | Type        | Description                                                                    |
| ------------------- | ----------- | ------------------------------------------------------------------------------ |
| `aggregate_pubkey`  | `ByteArray` | 32-byte x-only secp256k1 aggregate pubkey                                      |
| `refund_slot`       | `Int`       | Cardano slot before which the Refund path is rejected by the ledger            |

The redeemer has two variants, each carrying a 64-byte MuSig2 Schnorr signature:

- **`Spend { signature }`** — claims funds in the happy path. The validator verifies `schnorr(datum.aggregate_pubkey, utxo.transaction_id, signature)`. No timelock check is applied.
- **`Refund { signature }`** — reclaims funds if the swap stalls. The validator first checks that the transaction's validity range lower bound is `>= datum.refund_slot` (enforced by the Cardano ledger — the tx cannot be included in a block before that slot). It then verifies `schnorr(datum.aggregate_pubkey, blake2b(utxo.transaction_id || be64(datum.refund_slot)), signature)`.

The domain-separated refund message (`blake2b(txid || be64(refund_slot))`) is critical for security: both the spend and refund signatures are valid Schnorr signatures under the same aggregate key, so without message separation a participant holding a pre-signed refund signature could submit it via the `Spend` redeemer to bypass the timelock entirely. Because the two paths sign different messages, a signature produced for one path cannot be replayed on the other.

The `refund_slot` baked into the datum is the same value used to set `invalid_before` on the refund transaction, and is computed at lock tx creation time from `start_slot + distance × refund_window_secs`. It is part of the on-chain state and cannot be altered by the party submitting the refund transaction.

### Plutus Cost Model and Execution Units

Any Cardano transaction that executes a Plutus script must include a `script_data_hash` in the transaction body — a commitment over the redeemers, datums, and cost model. If this hash does not match what the node computes, the transaction is rejected at phase-1 validation before the script even runs.

**Cost model** — the price list for each UPLC operation (e.g. how many CPU steps `addInteger` costs). This is a protocol parameter set by Cardano governance and changes only at hard forks. The daemon fetches it at transaction build time from the network API (`/epochs/latest/parameters` → `cost_models_raw.PlutusV3`), using a hardcoded 251-entry fallback if the fetch fails. The `cost_models_raw` field is used rather than `cost_models` because the named map returned by some implementations (including Dolos) has keys in alphabetical order rather than canonical Cardano positional order, which would corrupt the hash.

**Execution units** — the memory and CPU budget declared for a specific script invocation. These are set per-transaction by the submitter. The daemon uses a two-pass approach for spend transactions on Blockfrost networks:

1. Build a draft transaction with ceiling values (`14_000_000` mem, `10_000_000_000` CPU steps)
2. Submit the draft CBOR to `/utils/txs/evaluate` — the node runs the script and returns the actual units consumed
3. Rebuild with the real units, reducing the execution fee to the minimum required

For Custom (Dolos) networks the evaluate endpoint is not supported, so ceiling values are used for both spend and refund transactions. The cost model is still fetched dynamically from Dolos.

For refund transactions on Blockfrost networks, the evaluate endpoint is called but may reject the draft if `invalid_before` is in the future (the tx is not yet valid at the current chain tip). In that case the daemon falls back to ceiling values. This only affects the fee estimate, not correctness.

### Compiling the Contract

```bash
cd swap_validator
aiken build
```

Extract the compiled bytes for embedding in Rust:

```bash
cat plutus.json | python3 -c "
import json, sys
code = json.load(sys.stdin)['validators'][0]['compiledCode']
bytes_list = [f'0x{code[i:i+2]}' for i in range(0, len(code), 2)]
print('pub const SWAP_SCRIPT_BYTES: &[u8] = &[')
print(', '.join(bytes_list))
print('];')
"
```

Paste the output into `src/blockchains/cardano_utils.rs`.

---

## Network Architecture

### Bitcoin

```
Bitcoin Core  (port 18443)   ← validates transactions, stores blockchain
      ↑
Electrs       (port 3002)    ← indexes blockchain, exposes mempool.space REST API
      ↑
Swap Daemon                  ← submits txs:          POST /api/tx
                             ← checks confirmation:  GET  /api/tx/{txid}/status
                             ← fetches witness:      GET  /api/tx/{txid}/hex
```

### Cardano

```
Cardano Node  (port 3001)    ← validates transactions, stores blockchain
      ↑
Dolos         (port 50051/50052) ← indexes blockchain, exposes UTxO REST + gRPC
      ↑
Swap Daemon                  ← submits txs:         POST /txs (REST)
                             ← checks confirmation: GET  /txs/{txid} (REST)
                             ← fetches witness:     CardanoQueryClient gRPC
```

For public networks (preprod/mainnet) Blockfrost is used instead of Dolos.

### TCP Networking

Daemons communicate peer-to-peer over persistent TCP connections. Each `SwapSession` holds a `ConnectionPool` — a map from peer address to a shared `Arc<Mutex<TcpStream>>`. The first `broadcast` call to a given peer establishes the connection; all subsequent calls reuse it.

This is important at scale: a 20-participant ring produces roughly 20×19 directed pairs. Without pooling, each message would open and tear down a new connection, exhausting OS ephemeral ports (TIME_WAIT backlog) and overflowing the kernel listen queue under concurrent load.

The daemon's `run_shared` entry point separates the accept loop into its own `tokio::spawn` task so that incoming connection handling is never blocked by event processing.

### Non-leader Spend Tx Discovery

Non-leaders enter the `AwaitingLeaderSpend` state once signing completes and begin polling for the leader's spend tx independently. When the leader submits their spend tx they also broadcast a `SpendTxBroadcast` signal, but this is redundant with the polling — it exists as an explicit protocol step to make the handshake readable. If the leader goes offline after claiming their funds and never sends the message, non-leaders make progress through polling alone.

The poll checks whether the leader's lock UTxO (`lock_txid#0`) has been spent:

- **Cardano** — queries the script address UTxO set; absence of `lock_txid#0` means spent. The spending tx is then located by scanning the script address transaction history to retrieve its CBOR and extract the adaptor secret from the redeemer.
- **Bitcoin** — polls the outspend endpoint (`/tx/{lock_txid}/outspend/0`) for `"spent": true`.

---

## Configuration

```rust
DaemonConfig {
    tcp_address: "127.0.0.1:9301".to_string(),
    bitcoin_network: BitcoinNetwork::Custom("http://localhost:3002".to_string()),
    cardano_network: CardanoNetwork::Custom {
        grpc_url: "http://localhost:50052".to_string(),
        rest_url: "http://localhost:50051".to_string(),
    },
    blockfrost_api_key: "".to_string(),
}
```

### Bitcoin Networks

| Network  | URL                               |
| -------- | --------------------------------- |
| Mainnet  | `https://mempool.space`           |
| Signet   | `https://mempool.space/signet`    |
| Testnet4 | `https://mempool.space/testnet4`  |
| Regtest  | `http://localhost:3002` (Electrs) |
| Custom   | provided URL                      |

### Cardano Networks

| Network                 | API                    |
| ----------------------- | ---------------------- |
| Preprod/Preview/Mainnet | Blockfrost REST        |
| Custom                  | Dolos REST + gRPC      |

---

## Local Development

### Prerequisites

- Rust (stable)
- Docker + Docker Compose
- Aiken (for contract compilation)

### Start the Private Network

The [btc-defi-atomic-swaps-test-env](https://github.com/input-output-hk/btc-defi-atomic-swaps-test-env) must be
installed to provide the Bitcoin and Cardano blockchain services needed to swap among parties.

The `/path/to/` refers to the directory where the `btc-defi-atomic-swaps-test-env` repository is cloned.

```bash
cd /path/to/btc-defi-atomic-swaps-test-env
./cli/testenv start
./cli/testenv status
```

---

## Running Tests

```bash
# unit tests and non-regtest integration tests
cargo test

# spend tx signing integration test
cargo test --test spend_tx_integration_test

# all regression tests against the private network (see Regression Tests section below)
./scripts/reg_suite.sh
```

### Regression Tests

The regression tests run against the private network (Bitcoin regtest + Dolos). Each test spins up multiple daemon instances in-process, connects them over TCP, and drives the full swap protocol from start to finish — including chain interaction.

**Prerequisites:** the private network must be running before each test (`./cli/testenv start`). The environment is stateful: each test mines blocks, submits transactions, and leaves the chain in a new state, so the environment should be restarted between tests to ensure a clean baseline. `reg_suite.sh` handles this automatically.

| Test file                                    | Participants | Scenario                                                               |
| -------------------------------------------- | ------------ | ---------------------------------------------------------------------- |
| `completed_regression_test.rs`               | 4 (BTC+ADA)  | Happy path: all participants complete the swap                         |
| `20_party_completed_regression_test.rs`      | 20 (BTC+ADA) | Happy path at scale: 20-participant ring, 10 BTC + 10 ADA              |
| `refunded_regression_test.rs`                | 4 (BTC+ADA)  | Leader absent: non-leaders reclaim funds via refund txs                |
| `failed_regression_test.rs`                  | 3 (BTC+ADA)  | Leader absent + refund txs missing: non-leaders reach Failed           |

**Run a single test:**

```bash
./cli/testenv start
cargo test --features regtest --test completed_regression_test -- --nocapture
```

**Run all regression tests in sequence** (restarts the environment between each):

```bash
./scripts/reg_suite.sh
```

### Live Dashboard

Each regression test includes a live visualisation dashboard showing the swap ring, per-participant state, and a real-time state transition log. `reg_suite.sh` enables it by default.

**Terminal 1** — start the React dev server (once, leave it running):
```bash
cd /path/to/btcdefi-lpqp/dashboard
npm install  # first time only
npm run dev
```

**Terminal 2** — run the suite or a single test:
```bash
./scripts/reg_suite.sh
# or individually:
./cli/testenv start
cargo test --features "regtest,dashboard" --test completed_regression_test -- --nocapture
```

Then open **http://localhost:5173** in a browser. The dashboard shows "Waiting for daemon…" until the test starts, then updates live throughout the run. After the swap completes the dashboard remains available for 120 seconds so the final state can be inspected before the process exits.

---

## Key Types

```rust
pub struct SwapKeys {
    pub secret_key: SecretKey,               // secp256k1 — MuSig2 + Bitcoin signing
    pub public_key: PublicKey,               // secp256k1
    pub cardano_wallet_secret_key: Vec<u8>,  // Ed25519 — signs Cardano wallet inputs
    pub cardano_wallet_public_key: Vec<u8>,  // Ed25519 — Cardano wallet address derivation
}

pub struct Participant {
    pub id: ParticipantId,
    pub blockchain: Blockchain,              // Bitcoin or Cardano
    pub tcp_address: Address,
    pub target_participant: ParticipantId,   // participant you claim funds from
    pub amount_locking: u64,
    pub amount_claiming: u64,
    pub is_me: bool,
    pub secp256k1_public_key: String,        // secp256k1 pubkey — MuSig2 on both chains
    pub cardano_wallet_public_key: Vec<u8>,  // Ed25519 pubkey — Cardano receive address
    pub funding_utxo_txid: String,
    pub funding_utxo_vout: u32,
    pub funding_utxo_value: u64,             // sats (Bitcoin) or lovelace (Cardano)
    pub funding_utxo_script: String,         // hex script_pubkey (Bitcoin only)
}
```

---

## Wire Protocol

Messages exchanged over TCP between daemons (JSON-encoded, newline-delimited):

| Message                          | Direction | Purpose                                          |
| -------------------------------- | --------- | ------------------------------------------------ |
| `AdaptorPoint(String)`           | broadcast | Share public adaptor point `Ti` at session start |
| `LeaderElectionCommitment([u8;32])` | broadcast | Commit phase: hash of leader nonce            |
| `LeaderElectionNonce([u8;32])`      | broadcast | Reveal phase: raw nonce for leader election   |
| `SchnorrNonce { role, nonce }`   | broadcast | MuSig2 RoundOne public nonce                     |
| `PartialSignature { role, sig }` | broadcast | MuSig2 RoundTwo partial signature                |
| `LockTxBroadcast`                | broadcast | Notify peers lock tx was submitted               |
| `SecretReveal(String)`           | broadcast | Reveal adaptor secret after lock txs confirmed   |
| `SpendTxBroadcast`               | broadcast | Leader signals their spend tx is on chain; carries no data and triggers no state change — non-leaders are already polling independently; the message exists for protocol clarity only |

---

## Security Notes

- **Refund txs are signed before locking funds** — every participant has a safe exit before committing funds on chain
- **Adaptor secrets are never shared directly** — they are revealed implicitly when a spend tx appears on chain
- **Commit-reveal leader election** — prevents any participant from biasing the leader selection
- **Adaptor point guard** — the daemon will not begin signing until all participants' adaptor points are received
- **Taproot parity normalization** — `with_tweak(t, true)` is used for MuSig2 signing to match `Address::p2tr`
- **MuSig2 nonce single-use** — `SecNonce` is stored as bytes in `MusigRuntime::RoundOne` and consumed exactly once by `sign_partial`. The session immediately transitions to `MusigRuntime::RoundTwo` (which holds no nonce bytes), preventing any code path from reusing the same nonce for a different message. Reusing a nonce across two signatures would expose the private key.
- **Cardano refund timelock enforced on-chain** — the `refund_slot` is stored in the datum at lock tx creation time and checked by the Plutus validator against the transaction's validity range lower bound, which the Cardano ledger enforces. A participant cannot submit the refund tx before the slot is reached regardless of what they put in the redeemer. The spend and refund paths sign different messages (`txid` vs `blake2b(txid || be64(refund_slot))`), so a pre-signed refund signature cannot be replayed via the `Spend` redeemer to bypass the timelock.

---

## Status

| Feature                             | Status                 |
| ----------------------------------- | ---------------------- |
| Leader election                     | ✅ Complete            |
| MuSig2 refund tx signing            | ✅ Complete            |
| MuSig2 adaptor spend tx signing     | ✅ Complete            |
| Bitcoin lock tx                     | ✅ Complete            |
| Bitcoin refund tx                   | ✅ Complete            |
| Bitcoin spend tx                    | ✅ Complete            |
| Cardano lock tx (Aiken)             | ✅ Complete            |
| Cardano refund tx                   | ✅ Complete            |
| Cardano spend tx                    | ✅ Complete            |
| Chain monitoring (Bitcoin/Electrs)  | ✅ Complete            |
| Chain monitoring (Cardano/Dolos)    | ✅ Complete            |
| Secret extraction from chain        | ✅ Complete            |
| Completed/Refunded/Failed states    | ✅ Complete            |
| End-to-end regtest regression tests | ✅ Complete            |
| Preprod integration                 | 🚧 Pending             |
| Bitcoin fee                         | ✅ Configurable per session (`bitcoin_fee` satoshis)            |
| Cardano lock fee                    | ✅ Configurable per session (`cardano_fee` lovelace)            |
| Cardano spend/refund fee            | ✅ Dynamic — computed from execution units + tx size + protocol params |
| Cardano ExUnits                     | ✅ Dynamic via evaluate-tx (Blockfrost) / ⚠️ Ceiling fallback (Dolos) |
