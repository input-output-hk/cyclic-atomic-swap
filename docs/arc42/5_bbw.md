# 5. Building Blocks View

## 5.1 Context Setting

### 5.1.1 Parties

The solution defines a finite set of N parties 

P = {P<sub>0</sub>, P<sub>1</sub>, ..., P<sub>N−1</sub> }.

An index identifies each party. In this document the index

- **i** represents the party from its subjecting point of view: "who I am";
- **j** represents the other parties from the perspective P<sub>i</sub>: "who I am not".
- **leader** represents the party elected as leader at runtime via a commit-reveal scheme.

Each party is represented by a daemon process, running concurrently, observing the blockchain it uses to swap
and exchanging messages with the other parties' daemon.

Each party operates on one of two supported blockchains: 
Bitcoin (Taproot / MuSig2 key-path spend) or Cardano (Plutus V2 script).
Parties may use different blockchains, enabling cross-chain swaps.

### 5.1.2 Directed Cycle

The parties are arranged in a directed cycle encoded by each participant’s 
`types::Participant.target_participant` field: 

P<sub>0</sub> → P<sub>1</sub> → P<sub>2</sub> → · · · → P<sub>N−1</sub> → <sub>P0</sub>.

Each directed edge defines one transfer.

### 5.1.3 Transfers

**Transfer<sub>i</sub>** is the transfer from P<sub>i</sub> to P<sub>(i+1) mod N</sub>, 
for i = 0, ..., N−1.

**Roles**. Because the swap is cyclic, every party plays all three of the following roles
simultaneously.

- **Depositor** for transfer<sub>i</sub>: P<sub>i</sub> signs and writes the **lock<sup>tx</sup>**<sub>i</sub>
  to _deposit_ funds on-chain.
- **Claimant** for transfer<sub>(i−1) mod N</sub>: P<sub>i</sub> signs with all P<sub>j</sub>,  
  then it writes the **spend<sup>tx</sup>**<sub>(i−1) mod N</sub> on chain 
  to _withdraw_ the funds deposited by the lock<sup>tx</sup><sub>(i−1) mod N</sub>.
- **Refunded** for transfer<sub>i</sub>: P<sub>i</sub> signs with all P<sub>j</sub>,
  then it writes the **refund<sup>tx</sup>**<sub>i</sub> on chain;
  the transaction will be finalised to recover the funds in the case any swap of the cycle fails.
- **Co-signer** for all N transfers: P<sub>i</sub> contributes its public key, nonce, and partial 
  pre-signature to the multi-signed spend<sup>tx</sup>. 
  The spend<sup>tx</sup> cannot be committed if any party disagrees, where the full consensus
  among participants is the purpose of this protocol. 

The terms _sender_ and _receiver_ are used below only as shorthand for the **depositor** and
**claimant** roles within a specific transfer. They do not denote distinct categories of parties.

The leader P<sub>leader</sub> is the claimant in transfer<sub>leader−1</sub> 
Until the Claim phase, all parties execute identical protocol steps, 
differing only in which transfer index they are depositing into and claiming from.
The only asymmetry in the protocol is in the
Claim phase where the leader’s behaviour diverges from that of all other parties.

```mermaid
---
title: "Figure 1: Three-party cyclic swap structure (N = 3, leader = P₀)"
---
graph TD
    P0((P<sub>0</sub><br><b>leader</b>))
    P1((P<sub>1</sub>))
    P2((C))
    L0[lock<sub>0</sub>]
    L1[lock<sub>1</sub>]
    L2[lock<sub>2</sub>]
 
    P0 -->|deposit<sup>tx</sup><sub>0→1</sub>| L0
    P1 -->|deposit<sup>tx</sup><sub>1→2</sub>| L1
    P2 -->|deposit<sup>tx</sup><sub>2→0</sub>| L2
 
    L0 -->|spend<sup>tx</sup><sub>0→1</sub>| P1
    L1 -->|spend<sup>tx</sup><sub>1→2</sub>| P2
    L2 -->|<b>trigger</b> spend<sup>tx</sup><sub>2→0</sub>| P0
 
    L0 -.->|refund<sub>0</sub>| P0
    L1 -.->|refund<sub>1</sub>| P1
    L2 -.->|refund<sub>2</sub>| P2
```

> Solid arrows: lock (deposit) and spend (claim) paths.
> Dashed arrows: refund path (time-locked, unhappy path only).
> The **trigger** is the first spend<sup>tx</sup> broadcast, by leader P<sub>0</sub> claiming P<sub>N-1</sub>'s locked funds.

---

## 5.2 Core Concepts Data Representation

### 5.2.1 Keys

Each party P<sub>i</sub> holds two key pairs, stored in a `types::Daemon.SwapKeys` struct.

- A **Secp256k1** key pair used for MuSig2 multi-signature operations and Plutus script verification on Cardano.
- An **Ed25519** key pair used to sign the Cardano lock<sup>tx</sup> input itself (the funding UTxO spending witness) 
  and to provide collateral witnesses on Cardano spend and refund transactions.

Makes sense that there is just one pair of private and public keys for the daemon.
These will probably come from a wallet.

The **Secp256k1** public keys are exchanged during the Config phase of the swap session;
the **Ed25519** keys are used locally and not exchanged.


### 5.2.2 Aggregate Adaptor Secret

Each party P<sub>i</sub> generates a private **adaptor secret** t<sub>i</sub> 
uniformly at random during session initialisation. 
The corresponding public **adaptor point** is:

T<sub>i</sub> = t<sub>i</sub> · G

where G is the **Secp256k1** generator.
T<sub>i</sub> is broadcast to all parties as the start of the Setup phase with the first wire message (`AdaptorPoint`).
The **aggregate adaptor point** T<sub>>agg</sub> is the sum of all individual adaptor points.
The corresponding **aggregate adaptor secret** is:

t<sub>agg</sub> = t<sub>0</sub> + t<sub>1</sub> + … + t<sub>N−1</sub>

This single secret adapts the pre-signature for **every** spend<sup>tx</sup>.
It is distributed: no strict subset of parties can compute it unilaterally.

### 5.2.3 Transactions Per Transfer

For each transfer<sub>i</sub>, three transactions are constructed during the signing phase 
(`types::SwapState::RefundAndSpendTxsSigning`):

| Transaction                      | Spends                       | Produces                                | Authorisation                                    |
|----------------------------------|------------------------------|-----------------------------------------|--------------------------------------------------|
| **Deposit: lock<sup>tx</sup>**   | P<sub>i</sub>'s funding UTXO | Locked account for transfer<sub>i</sub> | Signature of P<sub>i</sub>'s alone.              |
| **Withdraw: spend<sup>tx</sup>** | Locked account               | P<sub>(i+1) mod N</sub>'s address       | MuSig2 aggregate sig + t<sub>agg</sub> (adaptor) |
| **Refund: refund<sup>tx</sup>**  | Locked account               | P<sub>i</sub>'s address                 | MuSig2 aggregate sig + locktime ≥ W<sub>i</sub>  |

- The **spend tx** is co-signed as an _adaptor pre-signature_:
  it becomes a valid signature only when t<sub>agg</sub> is applied.
- The **refund tx** is fully co-signed during the Setup phase 
- but time-locked and cannot be broadcast before block height W<sub>i</sub>.

### 5.2.4 Locked Account

Each transfer’s locked account is an N-of-N multi-signature account.
Its unlocking key is an aggregated public key derived from all N parties’ individual public keys. 
Spending it requires a single aggregated Schnorr signature committing all N parties.

**Mutual exclusion assumption.** For each transfer’s locked account, the underlying chain
guarantees that at most one of the spend<sup>tx</sup> and the refund<sup>tx</sup> can be confirmed.
In UTXO-based chains this holds structurally (spending a UTXO consumes it, invalidating any
other transaction that references it); other chain models must enforce it explicitly.

- **Bitcoin:** A Pay-to-Taproot output whose internal key is the MuSig2 aggregate public key 
X<sub>agg</sub> = X<sub>0</sub> + … + X<sub>N−1</sub> (key-path spend, with Taproot tweak applied).

**Cardano:** A Plutus V2 script address. The script validates `verify_schnorr(agg_pubkey, lock_txid, sig)`, 
where `agg_pubkey` is derived from all N **Secp256k1** public keys and `lock_txid`
is the transaction ID of the lock tx itself, 
computed calling `protocol::chain_monitor::bitcoin_txid(tx_hex: &str) -> String`.
Cardano transactions require _collateral_ UTxO; each participant provides their own, stored in `cardano_collaterals`.

### 5.2.5 Refund Windows

Each transfer<sub>i</sub> has an associated refund window open height W<sub>i</sub>: 
the minimum block height of a chosen blockchain at which the refund tx for transfer<sub>i</sub> becomes valid.

**Staggered window invariant (cyclic case):**

W<sub>0</sub> > W<sub>1</sub> > W<sub>2</sub> > ··· > W<sub>N−1</sub>.

The leader P<sub>leader</sub> ’s refund window opens **last**. P<sub>leader−1</sub>’s
refund window (for the deposit into the leader) opens **first**.

**Minimum gap requirement.**
Let ∆ be the claim propagation time: 
the maximum number of blocks required for a party to 
(1) observe a broadcast transaction at the required finality depth, 
(2) construct and sign the corresponding spend<sup>tx</sup>, and 
(3) have it confirmed to the required finality depth.
The staggered ordering alone is not enough for safety; the gaps must satisfy:

W<sub>i</sub> − W<sub>i+1</sub> ≥ ∆ for all i = 1, 2, ..., N-2.

∆ is a protocol parameter that must be fixed before the Config phase and depends on
the confirmation-depth policies of the relevant blockchains. This value **must be chosen
conservatively** as the security of the protocol depends entirely on it.

> **Security note.**
> If W<sub>i</sub> − W<sub>i+1</sub> < ∆, a malicious leader (the only party that controls the
> trigger broadcast) can allow W<sub>i+1</sub> to expire before P<sub>(i+1) mod N</sub> can confirm its spend<sup>tx</sup>.
> P<sub>(i+1) mod N</sub> then claims a refund, but the adversary also claims P<sub>(i+1) mod N</sub> ’s deposit via
> the spend<sup>tx</sup>: that party loses their deposit and receives nothing, breaking atomicity.

**Griefing opportunity cost asymmetry.**
Because refund windows are staggered, an
abort imposes unequal waiting costs on different parties. If any party aborts after the Lock
phase, every party eventually recovers their principal via the refund path; no principal is lost.
However, a party with a small W<sub>j</sub> (refund available early) that withholds their adaptor secret
forces the leader, who holds the largest window W<sub>0</sub>, to keep funds locked for the longest
period. The aborting party bears a shorter opportunity cost than the leader does. This
asymmetry is a liveness concern, not an atomicity violation.

**Optionality.** 
The temporal gap between the Lock phase (when all funds are committed)
and the leader’s decision to broadcast the trigger creates a free option for the leader.
During this gap the leader can observe price movements and choose to complete the swap (if prices
moved favourably) or let it expire via refund (if prices moved against them). The non-leader
parties are locked in and cannot exit.
The protocol does not require the leader to broadcast the trigger within any deadline, 
nor does it impose a premium to compensate non-leaders for this optionality.
The refund path ensures no principal is lost, but expected value is transferred from non-leaders to
the leader through the option itself.
This is not specific to this protocol: it is a structural property of all atomic swap protocols 
(HTLC or adaptor-signature-based) in which one party commits last.

```mermaid
block-beta
    columns 4
    Claim_Window["<b>Claim window</b>"]:4
    space C["P<sub>2</sub>'s refund window"]:3
    space space P1["P<sub>1</sub>'s refund window"]:2
    space space space P0["P<sub>leader=0</sub>'s refund window"]
    w2 W1["W<sub>1</sub>"] W0["W<sub>0</sub>"] block_height["..."]
    
```

> Figure 2: Staggered refund windows for N = 3. W<sub>2</sub> < W<sub>1</sub> < W<sub>0</sub>. 
> The claim window is the period during which spend<sup>tx</sup> transactions may be broadcast. 

### 5.2.5 Refund Window Safety Argument

The staggered window invariant ensures that no coalition of parties can cheat an honest party
out of both their deposit and their output, _provided_ the minimum gap requirement is met.

**Formal invariant.** For the cycle P<sub>0</sub> → P<sub>1</sub> → ··· → P<sub>N−1</sub> → P<sub>0</sub> 
with P<sub>0</sub> as leader:

W<sub>0</sub> > W<sub>1</sub> > W</sub>2</sub> > ··· > W<sub>N−1</sub>, 
with W<sub>i<.sub> − W<sub>i+1</sub> ≥ ∆ for all i = 0, ..., N−2.

The claim propagation time ∆ is defined in Section 2.4. If ∆ gap condition is not met,
an adversary can exploit the timing t.

### 5.2.6 Data Model

### Daemon Data Model Class Diagram

The `types::Daemon` class (Rust struct and its implementation) is the pivotal object of the
CANS implementation. The **daemon** represents a party participating the swap session,
described by the `types::SwapSession` instance. The daemon evolves the state of the
`SwapSession` instances because the exchaning of messages with the daemons
representing the other participants and because the operations
performed on the chain.  

The following Mermaid diagram represents the data model of the `Daemon` and `SwapSession` structs 
and their associated proeprties and types as defined in [types.rs](../../swap-daemon/src/types.rs).
The data model represents the elements, concepts, and data described so far.
The rest of this document and the linked ones refer to the data model as shown in the diagram.

```mermaid
---
title: Figure 3 - Daemon Data Model - Class Diagram
---
classDiagram
    class Daemon {
        +HashMap~SessionId, SwapSession~ sessions
        +SwapKeys swap_keys
        +HashMap~tuple, AbortHandle~ active_pollers
        +DaemonConfig config
    }

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
        +BTreeMap~ParticipantId, bytes32~ leader_nonces
        +BTreeMap~ParticipantId, bytes32~ leader_commitments
        +Option~ParticipantId~ leader
        +HashMap~TxRole, MusigRuntime~ musig_sessions
        +HashMap~ParticipantId, HashMap~ schnorr_nonces
        +HashMap~ParticipantId, HashMap~ partial_sigs
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

    class SwapKeys {
        +SecretKey secret_key
        +PublicKey public_key
        +Vec~u8~ cardano_wallet_secret_key
        +Vec~u8~ cardano_wallet_public_key
    }

    class DaemonConfig {
        +String tcp_address
        +BitcoinNetwork bitcoin_network
        +CardanoNetwork cardano_network
        +String blockfrost_api_key
        +bool validate_utxos
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

    class Blockchain {
        <<enumeration>>
        Bitcoin
        Cardano
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

    Daemon "1" -- "*" SwapSession : manages
    Daemon "1" -- "1" SwapKeys : uses
    Daemon "1" -- "1" DaemonConfig : configured by
    SwapSession "1" -- "*" Participant : involves
    SwapSession "1" -- "1" SwapState : current state
    SwapSession "1" -- "*" MusigRuntime : signing state
    Participant "1" -- "1" Blockchain : belongs to
    SwapSession "1" -- "*" CardanoCollateral : requires
```

#### Key Components:
- `Daemon`: The central orchestrator managing multiple swap sessions, configuration, and global keys.
- `SwapSession`: Represents an active atomic swap protocol instance, tracking its state, participants, and cryptographic material (MuSig2, adaptor signatures).
- `Participant`: Contains metadata about a counterparty in a swap, including their blockchain addresses and public keys.
- `SwapState`: An enumeration of the various phases of the BBW (Bitcoin-to-Cardano) protocol.
- `SwapKeys`: The daemon's own cryptographic keys used for signing transactions on both Bitcoin (secp256k1) and Cardano (Ed25519).

---

## 5.3 Protocol Phases

Each transfer independently progresses through five phases.

- [Config](5_bbw_protocol.md#532-config-phase)
- [Setup](5_bbw_protocol.md#533-setup-phase)
- [Lock](5_bbw_protocol.md#534-lock-phase)
- [Claim](5_bbw_protocol.md#535-claim-phase)
- [Refund](5_bbw_protocol.md#536-refund-phase)

Phases for different transfers run concurrently but are synchronised at two global barriers **Lock** and **Claim**.

```mermaid
---
title: "Figure 4: Protocol Phases - State Diagram" 
---
stateDiagram-v2
    [*] -->Config
    Config --> Setup
    state lock_barrier <<fork>>
    Setup --> lock_barrier
    lock_barrier --> Lock
    state fork_claim_or_refund <<fork>>
    Lock --> fork_claim_or_refund
    fork_claim_or_refund --> Claim: all lock<sup>tx</sup> deposit confirmed
    fork_claim_or_refund --> Refund: block ≥ W<sub>i</sub> no spend<sup>tx</sup> withdraw observed
    Claim --> [*]
    Claim --> Refund: block ≥ W<sub>i</sub>, no spend<sup>tx</sup> withdraw observed
    Refund --> [*]
```

Follow the above links for the [detailed explanation of the protocol](5_bbw_protocol.md) building blocks,
the comprehensive state diagram of the protocol is visible in
[horizontal](5_bbw_fsm_lr.mmd) or
[vertical](5_bbw_fsm_td.mmd) orientation.


---

## 5.4 Wire Messages

All off-chain communication uses `Envelope`-wrapped `WireMessage` values serialised with `serde_json` and sent over 
persistent TCP connections managed by the `ConnectionPool` (`Arc<Mutex<HashMap<String, Arc<Mutex<TcpStream>>>>>`).
Reusing connections avoids exhausting OS ephemeral ports under high message volume. 
Each message is wrapped in an `Envelope` carrying the `session_id` and the sender's `participant_id`.

| Message                          | Description                                                                       |
|----------------------------------|-----------------------------------------------------------------------------------|
| `AdaptorPoint(point)`            | Broadcasts this party's adaptor point T_i (hex string)                            |
| `LeaderElectionCommitment(c)`    | Broadcasts SHA-256 commitment to this party's 32-byte election nonce (`[u8; 32]`) |
| `LeaderElectionNonce(n)`         | Reveals the 32-byte nonce after all commitments received (`[u8; 32]`)             |
| `SchnorrNonce { role, nonce }`   | MuSig2 Round 1: public nonce for a specific `TxRole`                              |
| `PartialSignature { role, sig }` | MuSig2 Round 2: partial (pre-)signature for a specific `TxRole`                   |
| `LockTxBroadcast`                | Notification that this party has submitted its lock tx                            |
| `SecretReveal(secret)`           | Non-leaders reveal their adaptor secret t_j (hex-encoded secp256k1 scalar)        |
| `SpendTxBroadcast`               | Leader notifies peers that its spend tx (trigger) is on-chain                     |

---

## 5.5 Per-Party State

The complete local state of party P<sub>i</sub> throughout the protocol (`types::SwapSession`):

| Field                   | Type                | Set in                        | Description                                           |
|-------------------------|---------------------|-------------------------------|-------------------------------------------------------|
| `adaptor_secrets`       | `HashMap`           | Config                        | Own t<sub>i</sub>; others' t<sub>j</sub> on receipt   |
| `bitcoin_fee`           | `u64`               | Config                        | Bitcoin tx fee (satoshis)                             |
| `cardano_collaterals`   | `HashMap`           | Config                        | Collateral UTxOs per party                            |
| `cardano_fee`           | `u64`               | Config                        | Cardano base fee (lovelace)                           |
| `connection_pool`       | `ConnectionPool`    | Config                        | Persistent TCP connections to peers                   |
| `id`                    | `SessionId`         | Config                        | Unique session identifier                             |
| `participants`          | `BTreeMap`          | Config                        | Ordered party list                                    |
| `refund_window_secs`    | `u64`               | Config                        | Per-hop window (604 800 s)                            |
| `start_block`           | `u32`               | Config                        | Bitcoin block at session creation                     |
| `start_slot`            | `u64`               | Config                        | Cardano slot at session creation                      |
| ----------------------- | ------------------- | -----------------             | ----------------------------------------------------- |
| `state`                 | `SwapState`         | Runtime                       | Current state                                         |
| `state_history`         | `Vec<SwapState>`    | Runtime                       | Chronological state log                               |
| ----------------------- | ------------------- | -----------------             | ----------------------------------------------------- |
| `adaptor_points`        | `HashMap`           | Setup: Adaptor Point Exchange | All T<sub>j</sub>                                     |
| `adaptor_sigs`          | `HashMap<TxRole>`   | Setup: MuSig2 Pre-Signature   | Adaptor pre-sigs for spend txs                        |
| `leader_commitments`    | `BTreeMap`          | Setup: Leader Election        | c<sub>j</sub> from each party                         |
| `leader_nonces`         | `BTreeMap`          | Setup: Leader Election        | n<sub>j</sub> from each party                         |
| `leader`                | `Option<PID>`       | Setup: Leader Election        | Elected leader                                        |
| `lock_txs`              | `HashMap`           | Setup: MuSig2 Pre-Signature   | Hex-encoded unsigned lock txs                         |
| `musig_sessions`        | `HashMap<TxRole>`   | Setup: MuSig2 Rounds          | Per-role MuSig2 runtime                               |
| `partial_sigs`          | `HashMap`           | Setup: MuSig2 Pre-Signature   | Collected partial sigs                                |
| `schnorr_nonces`        | `HashMap`           | Setup: MuSig2 Rounds          | Collected public nonces                               |
| `signed_txs`            | `HashMap<TxRole>`   | Setup: MuSig2 Pre-Signature   | Fully signed refund txs; adapted spend txs            |
| `unsigned_txs`          | `HashMap<TxRole>`   | Setup: MuSig2 Pre-Signature   | Unsigned refund / spend txs                           |
| ----------------------- | ------------------- | -----------------             | ----------------------------------------------------- |
| `confirmed_lock_txs`    | `HashSet`           | Lock                          | Parties with confirmed lock tx                        |
| `lock_txs_broadcast`    | `HashSet`           | Lock                          | Parties that broadcast lock tx                        |

---

## 5.6 Notes

**UTXO validation:** the daemon optionally validates each participant's funding UTXO 
before the session starts (`validate_utxos` flag, implemented in `chain_monitor::validate_funding_utxos`). 
Bitcoin UTXOs are checked against the mempool API (`/tx/{txid}/outspend/{vout}`); 
Cardano UTXOs are checked via Blockfrost or Dolos REST. 
This check is a liveness guard, not a security requirement.

**Non-cyclic swaps:** the staggered window invariant does not straightforwardly generalise to non-cyclic transfer graphs. 
Out of scope for this specification.

**Cross-chain finality:** the definition of "confirmed" is chain-specific. 
Bitcoin confirmation is checked via the mempool API (`/tx/{txid}/status`); 
Cardano confirmation is checked via Blockfrost (`/api/v0/txs/{txid}`) or Dolos REST (`/txs/{txid}`).

**Cardano collateral:** Plutus V2 transactions require collateral UTXO. 
Each party provides their own collateral; 
the Ed25519 collateral witness is added immediately before submission 
in `broadcast_my_spend_tx` and `broadcast_my_refund_tx` via `add_collateral_witness`.

**Cryptographic soundness:** the MuSig2 multi-signature scheme (via the `musig2` crate) 
and the adaptor signature construction are assumed correct and secure. 
The combination follows the scheme described in the MuSig2 paper.

**Leader bias:** the commit-reveal leader election is unbiasable by any single party 
but can be biased by a coalition that aborts after seeing unfavourable nonces (last-actor problem). 
Mitigations such as Verified-Random-Function-based election are out of scope.
