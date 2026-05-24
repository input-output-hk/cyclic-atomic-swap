# 5.3 Building Blocks View – The Protocol

## Typographic Conventions

### Parties

For a swap involving N parties, each party is identified by a number &ge; 0 and &le; N-1

- P<sub>i</sub>: The party (who I am), for the transaction<sub>i</sub>,
  P<sub>i</sub> identifies the _depositor_ writing the lock<sup>tx</sup> on chain to deposit its funds.
- P<sub>i+1 mod N</sub>: for the transaction<sub>i</sub>
  P<sub>i+1 mod N</sub> is the _withdrawer_ writing the spend<sup>tx</sup> on chain to collect the funds
  deposited by P<sub>i</sub>.
- P<sub>j&ne;</sub>: Any other party who isn't P<sub>i</sub>.

### Transactions

The code names the on chain transactions as follows:

- `refund`: it is the refund<sup>tx</sup> P<sub>i</sub> writes on chain to _refund_ itself in case the swap fails.
- `lock`: it is the lock<sup>tx</sup> P<sub>i</sub> writes on chain to _deposit_ its funds \
  to be withdrawed by P<sub>i+1</sub>.
- `spend`: it is spend<sup>tx</sup> the transaction when P<sub>i+1</sub> writes on chain
  to  _withdraw_ the funds deposited by P<sub>i</sub>.

### Symbols

- **Annotation**
    - ✉ Off chain **message**: Unicode #9993
    - 🗎 **Note**: Unicode #128462
- **Concurrency and Parallelism**
    - ☈ **Interrupt** process: Unicode #9736
    - ↻ Periodic **poll** process: Unicode #8635
- **State**
    - 🔒 **MuSig round** transition: #128274
    - ⤞ Swap session **state transition**: Unicode #10526.

---

## 5.3.1 Protocol Phases

Phases for different transfers run concurrently but are synchronised at two global barriers **Lock** and **Claim**.

Phases are represented by the `types::SwapSession.state` of `SwapState` type.

```mermaid
---
title: "Figure 1: Protocol Phases" 
---
stateDiagram-v2
    [*] --> Config
    state Config {
        Initialized
    }
    state Setup {
        AwaitingAdaptorPoints --> AwaitingLeaderElectionCommitments
        AwaitingLeaderElectionCommitments --> LeaderElectionCommitments
        LeaderElectionCommitments --> AwaitingLeaderElectionNonces
        AwaitingLeaderElectionNonces --> RefundAndSpendTxsSigning
        RefundAndSpendTxsSigning --> Funding
    }
    Initialized --> Setup
    state lock_barrier <<fork>>
    Funding --> lock_barrier
    state Lock {
        AwaitingLockConfirmations
    }
    lock_barrier --> Lock
    state fork_claim_or_refund <<fork>>
    Lock --> fork_claim_or_refund
    state Claim {
        state Leader {
            AwaitingSecrets
        }
        state Followers {
            AwaitingLeaderSpend
        }
        Leader --> Claiming
        Followers --> Claiming
        Claiming --> Completed
    }  
    fork_claim_or_refund --> Claim: all lock<sup>tx</sup> deposit confirmed
    fork_claim_or_refund --> Refund: block ≥ W<sub>i</sub> no spend<sup>tx</sup> withdraw observed
    Claim --> [*]
    Claim --> Refund: block ≥ W<sub>i</sub>, no spend<sup>tx</sup> withdraw observed
    state Refund {
        Refunded
        Failure
    }
    Refund --> [*]
```

> **Figure 3** external boxes represent protocol phases,
> internal boxes represent the swap session state as `SwapState` instances.
> Once all lock txs are confirmed,
> the **leader** transitions to `AwaitingSecrets` (waiting for N−1 `SecretReveal` messages);
> **non-leaders** transition immediately to `AwaitingLeaderSpend` and simultaneously broadcast their adaptor secret.

---

## 5.3.2 Config Phase

**Goal.** Establish the swap parameters, have each party generate its private adaptor secret,
and confirm that all _N_ parties are willing to proceed before any cryptographic material is
exchanged between parties.

Config has two parts: a one-time global step and a per-transfer consent step.

**Part 1 — Swap description (once per swap).** The swap initiator distributes the **swap
description** to all _N_ parties: a unique swap identifier `swap_id`, the ordered party
list [P<sub>0</sub>, ..., P<sub>N −1</sub>],
the leader’s identity, and the proposed refund window heights W<sub>0</sub>, ..., W<sub>N −1</sub>.
Each party P<sub>i</sub> records swap identifier, parties, and leader = P<sub>leader</sub>,
and generates its private adaptor secret t<sub>i</sub> uniformly at random.

**Part 2 — Per-transfer consent.** For transfer<sub>i</sub>, P<sub>(i+1) mod N</sub> proposes the swap description
to all N parties; each party either accepts or withholds consent. P<sub>(i+1) mod N</sub> proceeds to
the **Setup** phase only after receiving consent from all N parties.

**Pre-condition (for a party to consent):**

- The party has received the swap description: the transfer cycle, the complete ordered
  party list, the leader’s identity, and the proposed refund window heights W<sub>0</sub>, ..., W<sub>N−1</sub>.
- The proposed windows satisfy the staggered invariant W<sub>0</sub> > W<sub>1</sub> > · · · > W<sub>N−1</sub> with gaps
  W<sub>i</sub> − W<sub>i+1</sub> ≥ ∆.

The two parts, the swap description distribution and the consent handshaking,
are mocked in the implementation.
Code tests show how the parties initialise the `types::SwapSession`.

The `type::SwapSession` describes the state of the swap session through the protocol phases
from the point of view of each partecipant.
The `SwapSession` properties and the properties of structures it links, are described in
the class diagram below.

```mermaid
---
title: "Figure 2: Swap Session Data Structure - Class Diagram"
---
classDiagram
    class SwapSession {
        +SessionId id
        +Participants participants
        +u32 start_block
        +u64 start_slot
        +SwapState state
        +Vec~SwapState~ state_history
        +u64 bitcoin_fee
        +u64 cardano_fee
        +u64 refund_window_secs
        +BTreeMap~ParticipantId, [u8; 32]~ leader_nonces
        +BTreeMap~ParticipantId, [u8; 32]~ leader_commitments
        +Option~ParticipantId~ leader
        +HashMap~TxRole, MusigRuntime~ musig_sessions
        +HashMap~ParticipantId, HashMap~ TxRole, PubNonce~~ schnorr_nonces
        +HashMap~ParticipantId, HashMap~ TxRole, PartialSig~~ partial_sigs
        +HashMap~ParticipantId, String~ lock_txs
        +HashMap~TxRole, String~ unsigned_txs
        +HashMap~TxRole, String~ signed_txs
        +HashMap~TxRole, String~ adaptor_sigs
        +HashSet~ParticipantId~ lock_txs_broadcast
        +HashSet~ParticipantId~ confirmed_lock_txs
        +HashMap~ParticipantId, String~ adaptor_secrets
        +HashMap~ParticipantId, String~ adaptor_points
        +HashMap~ParticipantId, CardanoCollateral~ cardano_collaterals
        +ConnectionPool connection_pool
    }

    class Participant {
        +ParticipantId id
        +Blockchain blockchain
        +Address tcp_address
        +ParticipantId target_participant
        +u64 amount_locking
        +u64 amount_claiming
        +bool is_me
        +String secp256k1_public_key
        +Vec~u8~ cardano_wallet_public_key
        +String funding_utxo_txid
        +u32 funding_utxo_vout
    }

    class SwapState {
        <<enumeration>>
        Initialized
        AwaitingAdaptorPoints
        AwaitingLeaderElectionCommitments
        AwaitingLeaderElectionNonces
        RefundAndSpendTxsSigning
        Funding
        AwaitingLockConfirmations
        AwaitingSecrets
        AwaitingLeaderSpend
        Claiming
        Completed
        Refunded
        Failed
    }

    class TxRole {
        <<enumeration>>
        Refund(ParticipantId)
        Spend(ParticipantId)
    }

    class MusigRuntime {
        <<enumeration>>
        RoundOne
        RoundTwo
    }

    class CardanoCollateral {
        +String utxo_txid
        +u32 utxo_index
    }

    class Blockchain {
        <<enumeration>>
        Bitcoin
        Cardano
    }

    class ConnectionPool

    class SessionId {

    <<typealias>>
        u64
    }
    
    class ParticipantId {
        <<typealias>>
        u8
    }
    
    class Participants {
        <<typealias>>
        BTreeMap~ParticipantId, Participant~
    }
    
    class PubNonce {
        <<typealias>>
        String
    }
    
    class PartialSig {
        <<typealias>>
        String
    }
    
    class Address {
        <<typealias>>
        String
    }

    SwapSession --> SessionId: id
    SwapSession --> Participants: participants
    SwapSession --> SwapState : current/history
    SwapSession --> ParticipantId: leader/election keys
    SwapSession --> TxRole: tx role keys
    SwapSession --> MusigRuntime: musig sessions
    SwapSession --> PubNonce: schnorr nonces
    SwapSession --> PartialSig : partial sigs
    SwapSession --> CardanoCollateral: cardano collaterals
    SwapSession --> ConnectionPool: connection pool
    
    Participants --> Participant: contains
    Participant --> ParticipantId: id/target
    Participant --> Blockchain: blockchain
    Participant --> Address: tcp address
    TxRole --> ParticipantId: role owner
```

Each party calls `SwapSession::new` with:

- a unique `session_id`;
- the `participants` map (must form a valid directed cycle, verified by `check_cyclic`);
- `start_block` and `start_slot` (current chain tips);
- `bitcoin_fee` and `cardano_fee` in the respective chain's smallest units.

During the configuration each party:

- Generates 32 random bytes and interprets them as a **Secp256k1 scalar** t<sub>i</sub> (the adaptor secret).
- Computes the adaptor point T<sub>i</sub> = t<sub>i</sub> · G and stores both.
- Sets the initial state to `SwapState::Initialized`.
- Sets `refund_window_secs` = 604 800 (7 days).

**Post-condition:** Every party holds the swap description and its own private adaptor secret t<sub>i</sub>.
All N parties have agreed on the swap description, and P<sub>(i+1) mod N</sub> may proceed to Setup
for transfer<sub>i</sub>.

**Post-condition:** every party holds its own adaptor secret t<sub>i</sub> and adaptor point T<sub>i</sub>.
State = `Initialized`.

```mermaid
---
title: "Figure 3: Daemon Initialization and Session Configuration - State Diagram"
---
flowchart TD
    start([
        <b>Start</b>
])
    init_daemon["
        <i>initialize daemon</i>
        
        blockchain {bitcoin | cardano}
        network address
        x<sub>i</sub>, X<sub>i</sub>
        -
        </code>daemon::Daemon::new(...)</code>
    "]
    init_daemon_note[
        🗎
        The party has
        • id 0,...,N-1
        • private key x<sub>i</sub>
        • public key X<sub>i</sub>
    ]
    init_swap_session_note("
        🗎
        • The party has received the swap description: the transfer cycle, the complete ordered
        party list, the leader’s identity, and the proposed refund window heights W<sub>0</sub>,...,W<sub>N−1</sub> .
        • The proposed windows satisfy the staggered invariant W<sub>0</sub> > W<sub>1</sub> > ··· > W<sub>N−1</sub>
        with gaps W<sub>i</sub> − W<sub>i+1</sub> ≥ ∆.
        • session id <code>SwapSession.id</code>
        • party list &#91P<sub>0</sub>,...,P<sub>N-1</sub>&#93 <code>SwapSesssion.participants</code>
        • Bitcoin blockchain start <code>SwapSession.start_block</code>
        • Cardano blockchain start <code>SwapSession.start_slot</code>
        • refund windows height &#91W<sub>0</sub>,...,W<sub>N-1</sub>&#93 <code>SwapSession.refund_window_secs</code>
    ")
    init_swap_session["
        <b>Part 1 - Swap Description</b>
        <i>configure swap session</i>
        -
        <code>types::SwapSession::new(...)</code>
    "]
    check_swap_session("
        <b>Part 2 - Per-transfer consent</b>
        <i>validate swap session</i>
        -
        <code>session::SwapSession::new(...) { check_cyclic(...)</code>
    ")
    set_adaptor_secret_and_points["
        <i>set</i> t<sub>i</sub>, T<sub>i</sub>
        -
        <code>session::SwapSession::new(...)</code>
    "]
    swap_state_initialized("
        ⤞ <u>SwapState::Initialized</u>
    ")
    run_daemon["
        <i>run daemon</i>
        -
        <code>daemon.insert_session(session: SwapSession)</code>
        <code>daemon.run()</code>
    "]
    setup_adaptor_point_exchange([
        <b>Setup - Adaptor Point Exchange</b>
    ])
    failure([
        <b>End - Failure</b>
    ])
    
    start --> init_daemon
    init_daemon_note --- init_daemon
    init_swap_session_note --- init_swap_session
    subgraph All
    init_daemon --> init_swap_session
    init_swap_session --> check_swap_session
    check_swap_session -- valid --> set_adaptor_secret_and_points
    set_adaptor_secret_and_points --> swap_state_initialized
    swap_state_initialized --> run_daemon
    end
    run_daemon --> setup_adaptor_point_exchange
    check_swap_session -- invalid --> failure
```

---

## 5.3.3 Setup Phase

**Goal.** Construct and mutually verify all three transactions for every transfer so that:

1. Everyone can independently compute T<sub>agg</sub>.
2. For each transfer<sub>i, P<sub>(i+1) mod N</sub> holds a verified pre-signature for the spend<sup>tx</sup>
   (completable once t<sub>agg</sub> is known, and not before).
3. For each transfer<sub>i</sub>, P<sub>i</sub> holds a fully co-signed, time-locked refund<sup>tx</sup>.
4. All parties have verified that every refund window W<sub>i</sub> is correctly set.

### Adaptor Point Exchange

Setup begins with a one-time preliminary step in which every party broadcasts its individual
adaptor point T<sub>i</sub> to all other parties.
This happens once per swap, before any per-transfer rounds begin.

When the daemon calls `start_swap_session`, it optionally validates all participants' funding 
UTxOs (`validate_utxos` config flag via `validate_funding_utxos`),
then calls `broadcast_adaptor_point` and transitions the session to `AwaitingAdaptorPoints`.

Each party broadcasts its adaptor point T<sub>i</sub> as an `AdaptorPoint` wire message to all other participants.

On receiving an `AdaptorPoint` from participant j, each party:

1. Store T<sub>j</sub> in `adaptor_points`.
2. If all N adaptor points have been received and the state is `AwaitingAdaptorPoints`,
3. transition to `AwaitingLeaderElectionCommitments` and call `start_leader_election`.

> **Note on concurrency:**
> if the first `LeaderElectionCommitment` arrives before all adaptor points are collected
> (possible under concurrent delivery), `start_leader_election` is triggered on the first commitment as well,
> ensuring it runs exactly once regardless of message ordering.

**Post-condition:** every party holds {T<sub>0</sub>, …, T<sub>N−1</sub>} and can compute T<sub>agg</sub> = T<sub>
i</sub> + ΣT<sub>j</sub>.
State = `AwaitingLeaderElectionCommitments`.

```mermaid
---
title: "Figure 4: Setup - Adaptor Point Exchange - State Diagram"
---
flowchart TD
    setup_adaptor_point_exchange([     
        <b>Setup - Adaptor Point Exchange</b>
    ])
    validate_utxo["
        <i>validate UTxO</i>
        -
        <code>daemon.start_swap_session(session_id: u64) { </code>
        <code>protocol::chain_monitor::validate_funding_utxos(...)</code>
    "]
    broadcast_adaptor_point["
        <i>broadcast</i> T<sub>i</sub>
        -
        <code>protocol::adaptor_nonce::broadcast_adaptor_point(...)</code>
    "]
    swap_state_awaiting_adaptor_points("
        ⤞ <u>SwapState::AwaitingAdaptorPoints</u>
    ")
    wire_message_adaptor_point("
        ✉ WireMessage::AdaptorPoint(my_point)
    ")
    
    receive_all_adaptor_points["
        <i>receive all</i> T<sub>j</sub>
        -
        <code>daemon.run(...) { loop {{ event_rx.recv().await { </code>
        <code>daemon.handle_event(...) {{ DaemonEvent::PeerMessage { </code>
        <code>session.handle_session_message(...) {{ WireMessage::AdaptorPoint { </code>
        <code>utils.all_adaptor_points_received(...) { </code> ⇒ true
    "]
    swap_state_awaiting_leader_election_commitments("
        ⤞ <u>SwapState::AwaitingLeaderElectionCommitments</u>
    ")

    setup_adaptor_point_exchange --> validate_utxo
    subgraph All - Broadcast
        validate_utxo -- valid UTxO --> broadcast_adaptor_point
        broadcast_adaptor_point --> swap_state_awaiting_adaptor_points
    end
    validate_utxo -- invalid UtxO --> failure
    broadcast_adaptor_point -.-> wire_message_adaptor_point
    failure([
        <b>End - Failure</b>
    ])
    setup_leader_election([
        <b>Setup - Leader Election</b>
    ])
    
    swap_state_awaiting_adaptor_points --> receive_all_adaptor_points
    wire_message_adaptor_point -.-> receive_all_adaptor_points
    subgraph All - Receive
        receive_all_adaptor_points --> swap_state_awaiting_leader_election_commitments
    end
    swap_state_awaiting_leader_election_commitments --> setup_leader_election
```

---

### Leader Election

**Goal:** elect a single leader via a commit-reveal scheme without any pre-assignment.

#### Commit Phase

On `start_leader_election`, each party P<sub>i</sub>:

1. Generates a 32-byte random **nonce** n<sub>i</sub> via `OsRng`.
2. Computes the commitment c<sub>i</sub> = SHA256(n<sub>i</sub>) using the `bitcoin::hashes` crate.
3. Stores the nonce n<sub>i</sub> in `leader_nonces`
   and the commitment c<sub>i</sub> in `leader_commitments` under its own `ParticipantId`.
4. Broadcasts `LeaderElectionCommitment(c[i])` to all other parties.

On receiving a `LeaderElectionCommitment` from participant j:

1. If this party has not yet started leader election (own commitment is absent),
   call `start_leader_election` and transition to `AwaitingLeaderElectionCommitments`.
2. Store c<sub>j</sub> in `leader_commitments`.
3. If all N commitments have been received and the state is not yet `AwaitingLeaderElectionNonces`,
   broadcast `LeaderElectionNonce(n_self)` and transition to `AwaitingLeaderElectionNonces`.

#### Reveal Phase

On receiving a `LeaderElectionNonce` from participant j:

1. Verify SHA256(n<sub>j</sub>) = c<sub>j</sub>; **panic** if mismatch (`"Nonce invalid with commitment"`).
2. Store n<sub>j</sub> in `leader_nonces`.
3. If all N nonces have been received and `musig_sessions` is empty (i.e. signing has not yet been initialised),
   call `compute_leader`, transition to `RefundAndSpendTxsSigning`,
   call `build_lock_txs`, then concurrently call `begin_refund_signing` and `begin_spend_signing`.

#### Leader Computation

```mermaid
---
title: "Figure 5: Leader Computation Algorithm - Flowchart"
---
flowchart LR
    A["Concatenate all nonces in BTreeMap order\ninput = n₀ ‖ n₁ ‖ … ‖ n<sub>N−1</sub>"]
    B["seed = SHA256(input)"]
    C["v = u64::from_be_bytes(seed[0..8])"]
    D["leader_index = v mod N"]
    E["Select idx-th key from\nsorted participants map"]
    A --> B --> C --> D --> E
```

**Post-condition:** all parties agree on the same leader identity (stored in `leader`). State =
`RefundAndSpendTxsSigning`.

```mermaid
---
title: "Figure 6: Setup - Leader Election - State Diagram"
---
flowchart TD
    setup_leader_election([
        <b>Setup - Leader Election</b>
    ])
    start_leader_election_note("
        🗎
        nonce n<sub>i</sub>
        commitment c<sub>i</sub>
    ")
    %%  All - Commit Phase    
    start_leader_election["
        <i>set</i> n<sub>i</sub>, c<sub>i</sub>
        -
        <code>daemon.run(...) { loop {{ event_rx.recv().await { </code>
        <code>daemon.handle_event(...) {{ DaemonEvent::PeerMessage { </code>
        <code>session.handle_session_message(...) {{ WireMessage::AdaptorPoint { </code>
        <code>utils::all_adaptor_points_received(...) { </code> ⇒ true
        <code>protocol::leader_election::start_leader_election(...)</code>
    "]
    broadcast_leader_election_commitment["
        <i>broadcast</i> c<sub>i</sub>
        -
        <code>protocol::leader_election::start_leader_election(...)</code>
    "]
    wire_message_leader_election_commitment("
        ✉ WireMessage::WireMessage::LeaderElectionCommitment
    ")
    receive_all_leader_election_commitment["
        <i>receive all</i> c<sub>j</sub>
        -
        <code>daemon.run(...) { loop {{ event_rx.recv().await { </code>
        <code>daemon.handle_event(...) {{ DaemonEvent::PeerMessage { </code>
        <code>session.handle_session_message(...) {{ WireMessage::LeaderElectionCommitment { </code>
        <code>protocol::leader_election::received_leader_commitment(...) { </code>
        <code>utils::all_leader_commitments_received(...) { ⇒ true </code>
    "]
    broadcast_leader_nonce["
        <i>broadcast</i> n</i>
        -
        <code>leader_election::broadcast_leader_nonce(...)</code
    "]
    swap_state_awaiting_leader_election_nonce("
        ⤞ <u>SwapState::AwaitingLeaderElectionNonces</u>
    ")
    wire_message_leader_election_nonce("
        ⤞ <u>WireMessage::LeaderElectionNonce</u>
    ")
    
    %%  All - Reveal Phase    
    receive_all_leader_election_nonce["
        <i>receive all</i> n<sub>j</sub>
        -
        <code>daemon.run(...) { loop {{ event_rx.recv().await { </code>
        <code>daemon.handle_event(...) {{ DaemonEvent::PeerMessage { </code>
        <code>session.handle_session_message(...) {{ WireMessage::LeaderElectionNonce { </code>
        <code>protocol::leader_election::received_leader_nonce(...)</code>
        <code>utils::all_leader_nonces_received(...) { ⇒ true</code>
    "]
    compute_leader["
        <i>compute leader</i>
        
        input = n₀ ‖ n₁ ‖ … ‖ n<sub>N−1</sub>
        seed = SHA256(input)
        v = u64::from_be_bytes(seed[0..8])
        leader_index = v mod N
        -
        <code>daemon.run(...) { loop {{ event_rx.recv().await { </code>
        <code>daemon.handle_event(...) {{ DaemonEvent::PeerMessage { </code>
        <code>session.handle_session_message(...) {{ WireMessage::LeaderElectionNonce { </code>
        <code>utils::all_leader_nonces_received(...) { </code> ⇒ true
        <code>protocol::leader_election::compute_leader(...)</code>
    "]
    swap_state_refund_and_spend_txs_signing("
        ⤞ </u>SwapState::RefundAndSpendTxsSigning</u>
    ")
    setup_musig2_rounds(["
        <b>Setup - MuSig2 Rounds</b>
    "])
    
    
    setup_leader_election --> start_leader_election
    start_leader_election_note --- start_leader_election
    subgraph All - Commit Phase
        start_leader_election --> broadcast_leader_election_commitment
        broadcast_leader_election_commitment -.-> wire_message_leader_election_commitment
        wire_message_leader_election_commitment -.-> receive_all_leader_election_commitment
        receive_all_leader_election_commitment -- all commitment<sub>j</sub> received --> broadcast_leader_nonce
        receive_all_leader_election_commitment -- if c<sub>j</sub> preceeds c<sub>i</sub> broadcast --> broadcast_leader_election_commitment
        broadcast_leader_nonce --> swap_state_awaiting_leader_election_nonce
    end
    broadcast_leader_nonce -.-> wire_message_leader_election_nonce
    swap_state_awaiting_leader_election_nonce --> receive_all_leader_election_nonce
    wire_message_leader_election_nonce -.-> receive_all_leader_election_nonce
    subgraph All - Revel Phase
        receive_all_leader_election_nonce
    end
    receive_all_leader_election_nonce --> compute_leader
    
    subgraph All - Leader Computation
        compute_leader --> swap_state_refund_and_spend_txs_signing
    end
    swap_state_refund_and_spend_txs_signing --> setup_musig2_rounds
```

---

### MuSig2 Rounds

`lock_funds::build_lock_txs` builds a lock tx for every participant:

- **Bitcoin:** P2TR output sending `amount_locking` satoshis to the aggregate Taproot address
  derived from all N secp256k1 public keys.
- **Cardano:** Plutus script output sending `amount_locking` lovelace to the script address, minus `cardano_fee`.

#### Round 1 – Schnorr Nonce Exchange for spend<sup>tx</sup>

**Pre-condition:** Config phase represents the fact all parties agree about all trasfers.
Setup phase completed, the leader elected.

For each participant i, the refund locktime is computed by `refund_locktime_btc` / `refund_locktime_cardano`:

- W<sub>i</sub>(BTC) = start_block + d<sub>i</sub>> · (window_secs / 600)
- W<sub>i</sub>(ADA) = start_slot + d<sub>i</sub> · window_secs

where d<sub>i</sub> = distance_from_leader(P_k)

Signing message for refund<sup>tx</sup> and spend<sup>tx</sup>:

- **Bitcoin:** Taproot sighash of the unsigned refund<sup>tx</sup> (`compute_sighash`).
- **Cardano:** raw 32-byte lock tx transaction ID (blake2b of the original body bytes,
  computed via `FixedTransaction` to preserve byte-level accuracy).
  The Plutus script checks `verify_schnorr(agg_pubkey, lock_txid, sig)`.

**Initiator** for transfer<sub>i</sub>: P<sub<(i+1) mod N</sub> broadcasts R<sub>i</sub> to all N parties.

Every party P<sub>j</sub> responds with its public nonce R<sub>j</sub>.
All parties compute the aggregate nonce R<sub>agg</sub> = R<sub>0</sub> + ··· + R<sub>N−1</sub>.

**Post-condition:** All parties are committed to specific nonce 
for partial signature computation for transfer<sub>i</sub>.
Changing nonce after this point invalidates all collected partial
signatures, making the commitment binding by construction.

#### Round 2 – Partial Pre-Signature Exchange for spend<sup>tx</sup>

**Pre-condition:** Round 1 complete; all parties hold the individual adaptor points T<sub>j</sub> for all j
(broadcast in the Preliminary step).

All parties collect the Schnorr nonce R<sub>j</sub> and compute R<sub>agg</sub> = R<sub>i</sub> + ΣR<sub>j</sub>
for refund<sup>tx</sup><sub>j</sub> and spend<sup>tx</sup><sub>j</sub>.

Each party then computes and broadcasts the partial pre-signature s<sub>i</sub>
using their own private key and R<sub>agg</sub> for refund<sup>tx</sup> and spend<sup>tx</sup>.

Before computing its partial pre-signature, each responding party **must verify**:

- T<sub>agg</sub> = T<sub>0</sub> + T<sub>1</sub> + ··· + T<sub>N−1</sub>, 
  computed independently of the individual adaptor points T<sub>j</sub> broadcast in Round 1. 
- The spend<sup>tx</sup> being pre-signed spends the correct locked account 
  and credits P<sub>(i+1) mod N</sup>’s address for the agreed amount.

> **Security note.** 
> If co-signers do not verify T<sub>agg</sub>, the initiating party can substitute 
> T<sub>agg</sub> = T<sub>agg</sub> + δ · G for an arbitrary scalar δ. 
> The resulting pre-signature requires t<sub>agg</sub> + δ to complete.
> That value is extractable on-chain after the trigger, but it is not t<sub>agg</sub>; 
> all other parties’ spend<sup>tx</sup>, which require t<sub>agg</sub>, become permanently invalid, 
> and the initiating party alone can claim.

Every responding party P<sub>j</sub> computes its partial pre-signature s′j over the (verified) spend<sup>tx</sup>
for transfer<sub>i</sub> and sends it to P<sub>(i+1) mod N</sub>. 
P<sub>(i+1) mod N</sub> assembles the aggregate pre-signature from all N responses 
and verifies it against the (verified) T<sub>agg</sub>.

**Post-condition:** <sub>P(i+1) mod N</sub> holds a verified aggregate pre-signature 
for the spend<sup>tx</sup> of transfer<sub>i</sub>.
This pre-signature is not a valid signature; it becomes one if and only if t<sub>agg/sub> is applied to it. 
No party can complete this withdraw tx without knowing t<sub>agg</sub>.

### MuSig2 Pre-Signature for spend<sup>tx</sup>

Pre-condition: Round 3 complete for transfer<sub>i</sub>.

Once all N partial signatures for a refund<sup>tx</sup> and spend<sup>tx</sup> role are collected,
`finalize_role` assembles the full MuSig2 aggregate signature and stores the signed tx in `signed_txs`.

For each participant i, the spend<sup>tx</sup> spends the locked account of (i-1 (mod N))'s `target_participant`
(the party i is *receiving from*).

The target's blockchain determines the sighash/message:

- **Bitcoin target:** Taproot sighash of the unsigned spend tx.
- **Cardano target:** raw 32-byte lock tx transaction ID (same `FixedTransaction` approach).

For Cardano spend txs, the fee is fixed at 2 000 000 lovelace (`cardano_spend_fee`)
to cover the Plutus execution unit cost;
the output amount is `lock_utxo_value − cardano_spend_fee`.

Spend txs are signed as **adaptor pre-signatures** via `finalize_adaptor`: T<sub>agg</sub> is mixed into the signing,
producing a pre-signature s'<sub>i</sub> that becomes valid only when t<sub>agg</sub> is applied.
The pre-signature s'<sub>i</sub> is stored in `adaptor_sigs`; no entry is written to `signed_txs` yet.

Before co-signing, each party must verify all the following:
- The lock<sup>tx</sup> sends funds to the locked account address (not a substituted address).
- The lock<sup>tx</sup> amount matches the agreed transfer amount for transfer<sub>i</sup>.
- The refund<sup>tx</sup> spends the correct locked account for transfer<sub>i</sub>.
- The refund<sup>tx</sup> credits P<sub>i</sub> ’s address.
- The refund<sup>tx</sup> time-lock is exactly W<sub>i</sub>.
- W<sub>i</sub> is consistent with the window heights agreed in the Config phase and satisfies the
  staggered invariant (W<sub>i−1</sub> > W<sub>i</sub> > W<sub>i+1</sub>, 
  with boundary conditions at i = 0 and i = N −1).

> **Security note.** Without these checks, a malicious depositor can present a lock<sup>tx</sup> that
> sends to an attacker-controlled address, or a refund<sup>tx</sup> with a shortened time-lock, and get
> co-signatures on it from all other parties. A mis-addressed lock<sup>tx</sup> allows the attacker
> to claim the locked funds directly; a shortened refund lock allows a refund before the claim
> window closes, in both cases forfeiting the rightful claimant’s output.
> A party that fails any of the above checks withholds its spend<sup>tx</sup>, causing Setup to stall;
> that party waits for its refund window to open and broadcasts its refund<sup>tx</sup>.

**Post-condition:** once all N refund and all N spend partial signatures have been collected
(checked by `all_partial_sigs_received_for_all_refund_and_spend_txs`),
the state transitions to `Funding` and `broadcast_my_lock_tx` is called.

**Post-condition for P<sub>i</sub>:** Holds the lock<sup>tx</sup> (ready to broadcast) and a fully co-signed
time-locked refund<sup>tx</sup> valid at block height ≥ W<sub>i</sub>.
**Post-condition for all parties**: Have verified the lock<sup>tx</sup> destination, the refund<sup>tx</sup>
recipient and time-lock, and the refund window W<sub>i</sub> against the agreed protocol parameters.

```mermaid
---
title: "Figure 7: MuSig2 Rounds and Pre-Signature - State Diagram"
---
flowchart TD
    %%  All - Build Lock Transaction  
     setup_musig2_rounds(["
            <b>Setup - MuSig2 Rounds</b>
    "])
    build_lock_txs["
        <i>build</i> lock<sup>tx</sup>
        -
        <code>daemon.run(...) { loop {{ event_rx.recv().await { </code>
        <code>daemon.handle_event(...) {{ DaemonEvent::PeerMessage { </code>
        <code>handle_session_message(...) {{ WireMessage::LeaderElectionNonce { </code>
        <code>utils::all_leader_nonces_received(...) { </code> ⇒ true
        <code>protocol::lock_funds::build_lock_txs(...) { </code>
    "]
    
    %%  All - MuSig2 Round 1
    begin_refund_signing["
        <i>build and sign</i> refund<sup>tx</sup>
        -
        <code>protocol::refund::begin_refund_signing::begin_refund_signing(...)</code>
    "]
    musig_round_one_refund("
        🔒 MusigRuntime::RoundOne(TxRole::Refund, R<sub>i</sub>)
    ")
    broadcast_refund_schnorr["
        <i>broadcast</i> refund<sup>tx</sup> R<sub>i</sub>
        -
        <code>protocol::refund::begin_refund_signing(...)</code>
    "]
    wire_message_schnorr_nonce_refund("
        ✉ WireMessage::SchnorrNonce(TxRole::Refund, R<sub>i</sub>)
    ")
    begin_spend_signing["
        <i>build and sign</i> spend<sup>tx</sup>
        -
        <code>protocol::spend::begin_spend_signing(...)</code>
    "]
    musig_round_one_spend("
        🔒 MusigRuntime::RoundOne(TxRole::Spend, R<sub>i</sub>)
    ")
    broadcast_spend_schnorr["
        <i>broadcast</i> spend<sup>tx</sup> R<sub>i</sub>
        -
        <code>protocol::spend::begin_spend_signing(...)</code>
    "]
    wire_message_schnorr_nonce_spend("
        ✉ WireMessage::SchnorrNonce(TxRole::Spend, R<sub>i</sub>)
    ")
    
    %%  All - MuSig2 Round 2
    all_schnorr_nonces_received_for_refund["
        <i>receive all</i> R<sub>j</sub> refund<sup>tx</sup>
        -
        <code>daemon.run(...) { loop {{ event_rx.recv().await { </code>
        <code>daemon.handle_event(...) {{ DaemonEvent::PeerMessage { </code>
        <code>session.handle_session_message(...) {{ WireMessage::SchnorrNonce { </code>
        <code>all_schnorr_nonces_received_for(role = TxRole::Refund)</code>
    "]
    all_schnorr_nonces_received_for_spend["
        <i>receive all</i> R<sub>j</sub> spend<sup>tx</sup>
        -
        <code>daemon.run(...) { loop {{ event_rx.recv().await { </code>
        <code>daemon.handle_event(...) {{ DaemonEvent::PeerMessage { </code>
        <code>session.handle_session_message(...) {{ WireMessage::SchnorrNonce { </code>
        <code>all_schnorr_nonces_received_for(role = TxRole::Spend)</code>
    "]
    musig_round_two_refund("
        🔒 MusigRuntime::RoundTwo(TxRole::Refund, R<sub>agg</sub>)
    ")
    musig_round_two_spend("
        🔒 MusigRuntime::RoundTwo(TxRole::Spend, R<sub>agg</sub>)
    ")
    broadcast_aggregated_schnorr_nonce_refund["
        <i>compute/i> R<sub>agg</sub> for refund<sup>tx</sup>
        <i>broadcast/i> s<sub>i</sub> for refund<sup>tx</sup>
        -
        <code>daemon.run(...) { loop {{ event_rx.recv().await { </code>
        <code>daemon.handle_event(...) {{ DaemonEvent::PeerMessage { </code>
        <code>session.handle_session_message(...) {{ WireMessage::SchnorrNonce { </code>
        <code>transition_to_round_two(..., role = TxRole::Refund)</code>
    "]
    broadcast_aggregated_schnorr_nonce_spend["
        <i>compute/i> R<sub>agg</sub> for spend<sup>tx</sup>
        <i>broadcast/i> s<sub>i</sub> for refund<sup>tx</sup>
        -
        <code>daemon.run(...) { loop {{ event_rx.recv().await { </code>
        <code>daemon.handle_event(...) {{ DaemonEvent::PeerMessage { </code>
        <code>session.handle_session_message(...) {{ WireMessage::SchnorrNonce { </code>
        <code>transition_to_round_two(.... role = TxRole::Spend) </code>
    "]
    wire_message_partial_siganture_refund("
        ✉ WireMessage::PartialSignature(TxRole::Refund, s)
    ")
    wire_message_partial_siganture_spend("
        ✉ WireMessage::PartialSignature(TxRole::Refund, s)
    ")
    
    %%  All - MuSig2 Pre-Signature
    receive_all_partial_signature_refund["
        <i>receive all</i> s<sub>j</sub> for refund<sup>tx</sup>
        <i>pre-sign</i> s'<sub>i</sub> for refund<sup>tx</sup>
        -
        <code>daemon.run() { loop {{ </code>
        <code>daemon.handle_event(...) {{ DaemonEvent::PeerMessage { </code>
        <code>session.handle_session_message(...) {{ WireMessage::PartialSignature { </code>
        <code>all_partial_sigs_received_for(role = TxRole::Refund)</code>
    "]
    receive_all_partial_signature_spend["
        <i>receive all</i> s<sub>j</sub> for spend<sup>tx</sup>
        <i>pre-sign</i> s'<sub>i</sub> for spend<sup>tx</sup>
        -
        <code>daemon.run(...) { loop {{ event_rx.recv().await { </code>
        <code>daemon.handle_event(...) {{ DaemonEvent::PeerMessage { </code>
        <code>session.handle_session_message(...) {{ WireMessage::PartialSignature { </code>
        <code>all_partial_sigs_received_for(role = TxRole::Spend)</code>
    "]
    swap_state_funding("
        ⤞ <u>SwapState::Funding</u>
        -
        <code>all_partial_sigs_received_for_all_refund_and_spend_txs(...) { ⇒ true </code>
    ")
    lock(["
        <b>Lock</b>
    "])
    
    
    setup_musig2_rounds --> build_lock_txs
    subgraph All - Build Lock Transaction
        build_lock_txs
    end
    build_lock_txs --> begin_refund_signing
    build_lock_txs --> begin_spend_signing
    subgraph All - MuSig2 Round 1: Schnorr Nonce Exchange
        begin_refund_signing --> musig_round_one_refund
        musig_round_one_refund --> broadcast_refund_schnorr
        begin_spend_signing --> musig_round_one_spend
        musig_round_one_spend --> broadcast_spend_schnorr
    end
    broadcast_refund_schnorr -.-> wire_message_schnorr_nonce_refund
    broadcast_spend_schnorr -.-> wire_message_schnorr_nonce_spend
    
    wire_message_schnorr_nonce_refund -.-> all_schnorr_nonces_received_for_refund
    wire_message_schnorr_nonce_spend -.-> all_schnorr_nonces_received_for_spend
    subgraph All - MuSig2 Round 2 - Partial Pre-Signature Exchange
        all_schnorr_nonces_received_for_refund --> musig_round_two_refund
        all_schnorr_nonces_received_for_spend --> musig_round_two_spend
        musig_round_two_refund --> broadcast_aggregated_schnorr_nonce_refund
        musig_round_two_spend --> broadcast_aggregated_schnorr_nonce_spend
    end
    broadcast_aggregated_schnorr_nonce_refund -.-> wire_message_partial_siganture_refund
    broadcast_aggregated_schnorr_nonce_spend -.-> wire_message_partial_siganture_spend
    
    wire_message_partial_siganture_refund -.-> receive_all_partial_signature_refund
    wire_message_partial_siganture_spend -.-> receive_all_partial_signature_spend
    subgraph All - MuSig2 Pre-Signature
        receive_all_partial_signature_refund --> swap_state_funding
        receive_all_partial_signature_spend --> swap_state_funding
    end
    swap_state_funding --> lock
```

---

## 5.3.4. Lock Phase

**Goal:** all parties deposit their funds into their respective locked accounts on-chain.

**Pre-condition:** The party has completed all four Setup rounds for all N transfers and has
verified pre-signatures and refund windows for every transfer.
A party does not lock funds unless satisfied that it can either claim its output or get a refund.

**Actions:**

1. `broadcast_my_lock_tx` signs and submits the party's lock<sup>tx</sup>:
    - **Bitcoin:** fetches the prevout via the mempool API, applies a P2TR key-path witness (`sign_bitcoin_lock_tx`),
      and submits via `submit_bitcoin_tx`.
    - **Cardano:** applies the Ed25519 wallet signature to the funding UTxO input (`sign_cardano_lock_tx`) and submits
      via `submit_cardano_tx`.
2. On success: insert own ID into `lock_txs_broadcast`, broadcast `LockTxBroadcast` to all other parties, and transition
   to `AwaitingLockConfirmations`.
3. On failure: transition to `Failed`.

On receiving `LockTxBroadcast` from participant j: insert j into `lock_txs_broadcast`. The daemon's
`maybe_spawn_pollers` subsequently spawns a `ChainPollTarget::LockTx` poller for j.

**Post-condition:** for each transfer<sub>i</sub>, the locked account holds P<sub>i</sub>'s funds.
The only ways to spend those funds are (a) the spend<sup>tx</sup> (requiring t<sub>agg</sub>),
or (b) the refund<sup>tx</sup> (after slot/block W<sub>i</sub>, with the already co-signed MuSig2 signature).
State = `AwaitingLockConfirmations`.

```mermaid
---
title: "Figure 8: Lock - State Diagram"
---
flowchart
    lock(["
        <b>Lock</b>
    "])
    %%  All - depositor
    write_lock[("
        <i>write</i> lock<sup>tx</sup><sub>i</sub>
        -
        <code>daemon.run(...) { loop {{ event_rx.recv().await { </code>
        <code>daemon.handle_event(...) {{ DaemonEvent::PeerMessage { </code>
        <code>session.handle_session_message(...) {{ WireMessage::PartialSignature { </code>
        <code>utils::all_partial_sigs_received_for_all_refund_and_spend_txs(...) { </code> ⇒ true
        <code>protocol::lock_funds::broadcast_my_lock_tx(...)</code>
    ")]
    broadcast_lock["
        <i>broadcast</i> lock<sup>tx</sup><sub>i</sub>
        -
        <code>protocol::lock_funds::broadcast_my_lock_tx(...)</code>
    "]
    swap_state_awaiting_lock_confirmations(["
        ⤞ <u>SwapState::AwaitingLockConfirmations</u>
        -
        <code>protocol::lock_funds::broadcast_my_lock_tx(...) { </code> ⇒ true
    "])
    swap_state_failed("
        ⤞ <u>SwapState::Failed</u>
        -
        <code>protocol::lock_funds::broadcast_my_lock_tx(...) { </code> ⇒ false
    ")
    fail([
        <b>End - Failure</b>
    ])
    
    wire_message_lock_transaction_broadcast[
        ✉  WireMessage::LockTxBroadcast
    ]
    
    %%  All - withdrawer    
    receive_lock["
        <i>receive</i> lock<sup>tx</sup><sub>j</sub>
        -
        <code>daemon.run(...) { loop {{ event_rx.recv().await { </code>
        <code>daemon.handle_event(...) {{ DaemonEvent::PeerMessage { </code>
        <code>session.handle_session_message(...) {{ WireMessage::LockTxBroadcast { </code>
    "]
    receive_lock_note("
        🗎
        The daemon spaws a chain poller for this party to monitor lock<sup>tx</sup> are on chain.
    ")
    read_lock[("
        ↻ <i>read<i/> lock<sup>tx</sup><sub>j</sub>
        -
        <code>daemon.run(...) { loop {{ event_rx.recv().await { </code>
        <code>daemon.hande_event(...) {{ DaemonEvent::ChainPoll {{ ChainPollTarget::LockTx { </code>
        <code>protocol::chain_monitor::check_lock_tx_confirmed(...)</code>
    ")]
    read_all_lock("
        <i>confirm all</i> lock<sup>tx</sup><sub>j</sub>
        -
        <code>utils::all_lock_txs_confirmed(...) == true { </code> ⇒ true
    ")
    claim_secret_reveal(["
        <b>Claim - Secret Reveal</b>
    "])
    refund(["
        <b>Refund</b>
    "])
    
    
    lock -- in parallel --> write_lock
    lock -- in parallel --> read_lock
    subgraph "All - depositor P<sub>i</sub> of lock<sup>tx</sup><sub>i</sub>"
    write_lock -- success --> broadcast_lock
    write_lock -- failure --> swap_state_failed
    broadcast_lock --> swap_state_awaiting_lock_confirmations
    swap_state_failed --> fail
    end
    broadcast_lock -.-> wire_message_lock_transaction_broadcast
    
    wire_message_lock_transaction_broadcast -.-> receive_lock
    subgraph "All - withdrawer P<sub>i+1 mod N</sub>"
    swap_state_awaiting_lock_confirmations --> receive_lock
    receive_lock -.- receive_lock_note
    receive_lock_note -.- read_lock
    read_lock -- check --> read_all_lock
    read_all_lock -- all not confirmed yet --> read_lock
    end
    read_all_lock -- all confirmed --> claim_secret_reveal
    read_all_lock -- refund window timeout trigger --> refund
```

---

## 5.3.5 Claim Phase

### Secret Reveal

**Goal:** once all lock transactions are confirmed on-chain,
non-leader parties immediately broadcast their adaptor secrets and begin watching for the trigger;
the leader waits to collect those secrets before triggering.

**Pre-condition:** the daemon's `ChainPollTarget::LockTx` pollers have confirmed all N lock transactions
(`all_lock_txs_confirmed` returns `true`).

The behaviour diverges by role at this point:

**Non-leader P<sub>i ≠ leader</sub>:**

1. Transition to `AwaitingLeaderSpend`.
2. Broadcast `SecretReveal(t<sub>j</sub>)` (hex-encoded scalar) to all other parties.
3. The daemon spawns a `ChainPollTarget::LeaderSpendTx` poller to watch for the trigger on-chain.

**Leader P<sub>leader</leader>:**

1. Transition to `AwaitingSecrets`.
2. Wait for `SecretReveal` messages from the N−1 non-leaders (received via `handle_session_message`).

On receiving `SecretReveal(t<sub>j</sub>)` from participant j (any party):

1. Store t<sub>j</sub> in `adaptor_secrets`.
2. If `all_adaptor_secrets_received` is true (leader only — see below), proceed to the Claim phase.

> **Critical constraint:** the leader must **never** share t<sub>leader</sub> off-chain.
> P<sub>leader</sub>'s adaptor secret is disclosed *only* through the on-chain broadcast of the trigger spend tx.
> Were P<sub>leader</sub> to share t<sub>leader</sub> off-chain,
> the remaining N−1 parties could reconstruct t<sub>agg</sub> independently
> and time the trigger broadcast to expire just before P<sub>leader</sub>'s refund window W<sub>leader</sub>,
> cheating P<sub>leader</sub>.

> **Note on `all_adaptor_secrets_received`:** this condition becomes true only for the *leader*,
> because the leader holds its own t<sub>leader</sub> from initialisation
> and receives t<sub>j</sub> from every non-leader via `SecretReveal`.
> Non-leaders never reach this condition via peer messages (since the leader never broadcasts its secret);
> instead, they recover t<sub>agg</sub> directly from the on-chain trigger signature.

```mermaid
---
title: "Figure 9: Claim - Secret Reveal - State Diagram"
---
flowchart TD
    claim_secret_reveal(["
        <b>Claim - Secret Reveal</b>
    "])

    %%  Leader    
    swap_state_awaiting_secrets("
        ⤞ <u>SwapState::AwaitingSecrets</u>
        -
        <code>daemon.run(...) { loop {{ event_rx.recv().await { </code>
        <code> daemon.handle_event(...) {{ DaemonEvent::ChainPoll {{{ ChainPollTarget::LockTx { </code>
        <code>utils::all_lock_txs_confirmed(session) { </code> ⇒ true
        <code>session.leader == Some(my_id)</code> ⇒ true
    ")
    receive_all_secrets["
        <i>receive all</i> t<sub>j</sub>
        -
        <code>daemon.run(...) { loop {{ event_rx.recv().await { </code>
        <code>daemon.handle_event(...) {{ DaemonEvent::PeerMessage { </code>
        <code>session.handle_session_message(...) {{ WireMessage::SecretReveal { </code>
        <code>utils::all_adaptor_secrets_received() { </code> ⇒ true
    "]
    swap_state_claiming("
        ⤞ <u>SwapState::Claiming</u>
        -
        <code>my_id == leader_id { </code> ⇒ true
    ")
    
    wire_message_secret_reveal("
        ✉ WireMessage::SecretReveal
    ")
    
    %%  All Not Leader
    cast_secret["
        <i>cast to P<sub>leader</sub></i> s<sub>i</sub>
        -
        <code>daemon.run(...) { loop {{ event_rx.recv().await { </code>
        <code>daemon.handle_event(...) {{ DaemonEvent::PeerMessage { </code>
        <code>session.handle_session_message(...) {{ WireMessage::CastSecret { </code>
        <code>my_id != leader_id { </code> ⇒ true
    "]
    swap_state_awaiting_leader_spend("
        ⤞ <u>SwapState::AwaitingLeaderSpend</u>
    ")
    
    claim_spend(["
        <b>Claim - Spend</b>
    "])
    refund(["
        <b>Refund</b>
    "])
    
    claim_secret_reveal -- P<sub>i ≠ leader</sub> --> cast_secret
    claim_secret_reveal -- P<sub>leader</sub> --> swap_state_awaiting_secrets
    subgraph All Not Leader
        cast_secret --> swap_state_awaiting_leader_spend
    end
    cast_secret -.-> wire_message_secret_reveal
    wire_message_secret_reveal -.-> receive_all_secrets
    subgraph Leader Only
        swap_state_awaiting_secrets --> receive_all_secrets
        receive_all_secrets --> swap_state_claiming
    end
    swap_state_claiming --> claim_spend
    swap_state_awaiting_leader_spend --> claim_spend
    swap_state_claiming -- refund windows timeout trigger --> refund
    swap_state_awaiting_leader_spend -- refund windows timeout trigger --> refund
```

### Spend Transaction

**Goal:** every party claims the funds locked for the transfer it is receiving.

#### Leader's Role (P<sub>leader</sub>)

**Post-condition:** trigger tx on-chain; t<sub>agg</sub> publicly extractable from its Schnorr signature.

#### Non-Leader Roles (P<sub>i ≠ leader</sub>)

Non-leaders are in `AwaitingLeaderSpend`, with the daemon polling `ChainPollTarget::LeaderSpendTx`.

On `check_leader_spend_confirmed` returning `true`, `extract_secret_and_adapt` is called:

> **Deviation — trigger never arrives:** if the daemon's `ChainPollTarget::RefundWindow`
> fires (slot/block ≥ W<sub>j</sub>) before the trigger is observed, P_j falls through to the Refund phase.

**Post-condition (happy path):** all N spend txs broadcast; each party has claimed their output.
State = `Completed`.

```mermaid
---
title: "Figure 10: Claim - Spend - State Diagram"
---
flowchart TD
    claim_spend(["
        <b>Claim - Spend</b>
    "])

    %%  Leader Only
    write_spend_tx_leader[("
        <i>sign and write</i> spend<sup>tx</sup><sub>leader</sub>
        -
        <code>daemon.run(...) { loop {{ event_rx.recv().await { </code>
        <code>daemon.handle_event(...) {{ DaemonEvent::PeerMessage { </code>
        <code>session.handle_session_message(...) {{ WireMessage::SecretReveal { </code>
        <code>all_adaptor_secrets_received(...) { </code> ⇒ true
        <code>my_id == leader_id{ </code> ⇒ true
        <code>protocol:spend::broadcast_my_spend_tx(...)</code>
    ")]
    broadcast_spend_tx_leader["
        <i>broadcast</i> spend<sup>tx</sup><sub>leader</sub>
        -
        <code>protocol:spend::broadcast_my_spend_tx(...)</code>
    "]
    wire_message_spend_tx_broadcast_leader("
        ✉ WireMessage::SpendTxBroadcast
    ")
    swap_state_completed_leader(["
        ⤞ <u>SwapState::Completed</u>
    "])
    refund_leader([
        <b>Refund</b>
    ])
    success_leader(["
        <b>End - Success</b>
    "])
    
    %%  All Not Leader
    receive_spend_tx_leader["
        <i>receive</i> spend<sup>tx</sup><sub>leader</sub>
        -
        <code>daemon.run(...) { loop {{ event_rx.recv().await { </code>
        <code>daemon.handle_event(...) {{ DaemonEvent::PeerMessage { </code>
        <code>session.handle_session_message(...) {{ match WireMessage::SpendTxBroadcast { </code>
    "]
    receive_spend_tx_leader_note("
        🗎
        The daemon spaws a chain poller for this party to monitor lock<sup>tx</sup> are on chain.
    ")
    read_spend_tx_leader[("
        ↻ <i>read<i/> spend<sup>tx</sup><sub>leader</sub>
        -
        <code>daemon.run(...) { loop {{ event_rx.recv().await { </code>
        <code>daemon.handle_event(...) {{ DaemonEvent::ChainPoll {{{ ChainPollTarget::LeaderSpendTx { </code>
        <code>protocol::chain_monitor::check_leader_spend_confirmed(...)</code>
    ")]
    read_leader_spend_tx_confirmed["
        <i>confirm<i/> spend<sup>tx</sup><sub>leader</sub>
        -
        <code>protocol::chain_monitor::check_leader_spend_confirmed(...) { </code> ⇒ true
    "]
    write_spend_tx[("
        <i>sign and write<i/> spend<sup>tx</sup><sub>i</sub>
        -
        <code>protocol::secret::extract_secret_and_adapt(...) { ⇒ true</code>
        <code>broadcast_my_spend_tx(...)</code>
    ")]
    swap_state_claiming(["
        ⤞ <u>SwapState::Claiming</u>
        -
        <code>protocol::secret::extract_secret_and_adapt(...) { </code> ⇒ true
    "])
    broadcast_spend_tx["
        <i>broadcast<i/> spend<sup>tx</sup><sub>i</sub>
        -
        <code>protocol::spend::broadcast_my_spend_tx(...)</code>
    "]
    swap_state_completed(["
        ⤞ <u>SwapState::Completed</u>
    "])
    refund([
        <b>Refund</b>
    ])
    success(["
        <b>End - Success</b>
    "])
    
    %%  All
    receive_spend_tx["
        <i>receive</i> spend<sup>tx</sup><sub>j</sub>
        -
        <code>daemon.run(...) { loop {{ event_rx.recv().await { </code>
        <code>daemon.handle_event(...) {{ DaemonEvent::PeerMessage { </code>
        <code>session.handle_session_message(...) {{ WireMessage::SpendTxBroadcast { </code>
    "]
    receive_spend_tx_note("
        🗎
        Log
    ")
    
    
    wire_message_spend_tx_broadcast("
        ✉ WireMessage::SpendTxBroadcast
    ")
    
    
    claim_spend -- P<sub>leader</sub> --> write_spend_tx_leader
    claim_spend -- P<sub>i ≠ leader</sub> --> receive_spend_tx_leader
    wire_message_spend_tx_broadcast_leader -.-> receive_spend_tx_leader
    subgraph All Not Leader
        receive_spend_tx_leader -.- receive_spend_tx_leader_note
        receive_spend_tx_leader_note -.- read_spend_tx_leader
        read_leader_spend_tx_confirmed -- not yet confirmed --> read_spend_tx_leader
        read_spend_tx_leader -- check --> read_leader_spend_tx_confirmed
        read_leader_spend_tx_confirmed -- success --> swap_state_claiming
        swap_state_claiming -- success --> write_spend_tx
        broadcast_spend_tx --> swap_state_completed
        write_spend_tx --> broadcast_spend_tx
    end
    read_leader_spend_tx_confirmed -- failure<br>refund windows timeout trigger --> refund
    write_spend_tx -- failure<br>refund windows timeout trigger --> refund
    broadcast_spend_tx -.-> wire_message_spend_tx_broadcast
    wire_message_spend_tx_broadcast -.-> receive_spend_tx
    swap_state_completed --> success
    success -- interrupt --> refund
    subgraph All
        receive_spend_tx -.- receive_spend_tx_note
    end
    subgraph Leader Only
        write_spend_tx_leader --> broadcast_spend_tx_leader
        broadcast_spend_tx_leader --> swap_state_completed_leader
    end
    broadcast_spend_tx_leader -.-> wire_message_spend_tx_broadcast_leader
    swap_state_completed_leader --> success_leader
    success_leader -- interrupt --> refund_leader
```

---

## 5.3.6. Refund Phase

**Goal:** any party whose refund window has opened and whose locked funds have not yet been claimed reclaims their
deposit.

This phase is triggered by `ChainPollTarget::RefundWindow` whenever the current block height
or slot exceeds W<sub>i</sub> and the session is not in a terminal state.

**Pre-condition for P<sub>i<.sub> to broadcast their refund<sup>tx</sup>:**

- Bitcoin: block height ≥ W<sub>i</sub>(BTC).
- Cardano: current slot **>** W<sub>i</sub>(ADA) (strict — see [Refund Windows](5_bbw.md#525-refund-windows)).
- Session state is not `Completed`, `Refunded`, or `Failed`.

**Action:** `broadcast_my_refund_tx` submits the fully co-signed refund tx:

- **Bitcoin:** submit the pre-assembled MuSig2-signed tx directly via `submit_bitcoin_tx`.
- **Cardano:** if a collateral UTxO is registered in `cardano_collaterals`,
- attach the Ed25519 collateral witness via `add_collateral_witness` before submitting.

On success the session transitions to `Refunded`; on failure to `Failed`.
All active pollers are cancelled.

**Liveness obligation:** the daemon's `ChainPollTarget::RefundWindow` poller is spawned
as soon as the party's own lock tx is confirmed (`confirmed_lock_txs.contains(&my_id)`)
and runs continuously until the session reaches a terminal state.

**Post-condition:** P<sub>i</sub>'s deposit is returned.
State = `Refunded`.
No party has lost their principal.

```mermaid
---
title: "Figure 11: Refund - State Diagram"
---
flowchart TD
    refund([
        <b>Refund</b>
    ])
    read_block_height[("
        ↻ <i>read block height</i>
        -
        <code>daemon.run(...) { loop {{ event_rx.recv().await { </code>
        <code>daemon.handle_event(...) {{ DaemonEvent::ChainPoll {{{ ChainPollTarget::RefundWindow { </code>
    ")]
    refund_window_open["
        <i>block height ≥ W<sub>i</sub>?</i>
        -
        <code>protocol::chain_monitor::check_refund_window_open(...)</code>
    "]
    write_refund_tx[("
        <i>sign and write</i> refund<sup>tx</sup>
        -
        <code>protocol::refund::broadcast_my_refund_tx(...)</code>
    ")]
    swap_state_refunded("
        ⤞ <u>SwapState::Refunded</u>
    ")
    success(["
        <b>End - Success</b>
    "])
    swap_state_failed("
        ⤞ <u>SwapState::Failed</u>
    ")
    failure(["
        <b>End - Failure</b>
    "])
    claim([
        <b>Claim</b>
    ])
    
    refund --> read_block_height
    subgraph All
        refund_window_open -- block height < W<sub>i</sub> ⇒ false --> read_block_height
        read_block_height -- check --> refund_window_open
        refund_window_open -- block height ≥ W<sub>i</sub> ⇒ true --> write_refund_tx
        write_refund_tx -- success --> swap_state_refunded
        write_refund_tx -- failure --> swap_state_failed
    end
    swap_state_refunded --> success
    success -- interrupt --> claim
    swap_state_failed --> failure
```
---


