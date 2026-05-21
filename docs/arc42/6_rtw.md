# 6. Runtime View

## 6.1 Daemon

[`types::Daemon`](../../swap-daemon/src/daemon.rs)
 
---

## 6.2 Messages

The [Protocol Reference Implementation](5_bbw_protocol.md) describes phase progress through [`types::SwapState`](../../swap-daemon/src/types.rs)
because [`types::WireMessage`](../../swap-daemon/src/types.rs) exchaghed among parties off the chain through the network.

[Mermaid Sequence Diagram](https://mermaid.ai/open-source/syntax/sequenceDiagram.htm) syntax has
limited support for HTML, hence the following typographic convetions

- `P_i` is P<sub>i</sub>, the _deamon_'s party receiving messaged from other parties and sending messages to them.
- `P_j` are <sub>j</sub> j≠i, are any of all _other deamons_' parties, sending messages to P<sub>i</sub>
  and receving the messages P<sub>i</sub> broadcats to them.
- `tx` means _transaction_, `txs` means _transactions_.


### 6.2.1 [Config Phase](5_bbw_protocol.md#532-config-phase)

The configuration supposes all parties have the public key of each other parties 
and agree about the swap description.
The reference implementation injects the public keys with the
[`pub fn new(swap_keys: SwapKeys, config: DaemonConfig) -> Self`](../../swap-daemon/src/daemon.rs) 
function, then the swap description is injected with the
[`pub fn insert_session(&mut self, session: SwapSession)`](../../swap-daemon/src/daemon.rs).

No messages among parties are exchanged during this phase.

### 6.2.2 [Setup Phase](5_bbw_protocol.md#533-setup-phase)
 
```mermaid
sequenceDiagram
%%  Adaptor Point
    participant P_j as All other parties P_j
    participant P_i as This daemon's party P_i
    par Adaptor Point Exchange
        Note over P_j, P_i: All participants exchange Adaptor Points.
        P_j->>P_i: ✉ AdaptorPoint
        Note right of P_i: Stores Adaptor point.
        P_i->>P_i: All Adaptor Points received?
        opt If all received...
            P_i->>P_i: Transition to ⤞ AwaitingLeaderElectionCommitments.
        end
    end

%%  Leader Election
    participant P_j
    participant P_i
    par Leader Commitment Exchange
        Note over P_j, P_i: Leader Election - Round 1<br>Commitment Exchange
        P_j->>P_i: ✉ LeaderElectionCommitment
        Note right of P_i: Stores Leader Commitment.
        P_i->>P_i: All Leader Commitments received?
        opt If all received...
            P_i->>P_i: Transition to ⤞ AwaitingLeaderElectionNonces.
            P_i-->>P_j: ✉ LeaderElectionNonce
        end
    end
    par Leader Nonce Exchange
        Note over P_j, P_i: Leader Election - Round 2<br>Nonce Exchange
        P_j->>P_i: ✉ LeaderElectionNonce
        Note right of P_i: Stores Leader Nonce.
        P_i->>P_i: All Leader Nonces received?
        opt If all received...
            P_i->>P_i: Compute Leader.
            P_i->>P_i: Transition to ⤞ RefundAndSpendTxsSigning.
            P_i->>P_i: Start signing refund and spend txs.
        end
    end

%%  Schnorr Nonce
    participant P_j
    participant P_i
    par Musig2 Round 1 Schnorr Nonce Exchange
        Note over P_j, P_i: Musig2 Round 1
        P_j->>P_i: ✉ SchnorrNonce(role, nonce)
        Note right of P_i: Stores nonce for role.
        P_i->>P_i: All Schnorr Nonces received for role?
        opt If all received...
            P_i->>P_i: Transition to round two for the role.
            P_i-->>P_j: ✉ PartialSignature(role, sig)
        end
    end
    
%%  Partial Signature
    participant P_j
    participant P_i
    par MuSig2 Round 2 Partial Signature Exchange
        Note over P_j, P_i: Musig2 Round 2
        P_j->>P_i: ✉ PartialSignature(role, sig)
        Note right of P_i: Stores partial signature.
        P_i->>P_i: All Partial Signatures received for the role?
        opt If all received...
            P_i->>P_i: Finalize role (finalize tx).
        end
        P_i->>P_i: All Partial Signatures Received for all txs?
        opt If all received...
            P_i->>P_i: Transition to ⤞ Funding.
            P_i->>P_i: Broadcast Lock Tx.
            P_i-->>P_j: LockTxBroadcast.
        end
    end
```

### 6.2.3 [Lock Phase](5_bbw_protocol.md#534-lock-phase)

```mermaid
sequenceDiagram
    par Lock Tx Broadcast
        participant P_j as All other parties P_j
        participant P_i as This daemon's party P_i
        Note over P_j, P_i: Funding Phase
        P_j->>P_i: ✉ LockTxBroadcast
        Note right of P_i: Mark peer's lock tx as broadcasted.
        Note right of P_i: Daemon spawns chain poller for peer's lock tx.
    end
```

### 6.2.4 [Claim Phase](5_bbw_protocol.md#535-claim-phase)
```mermaid
sequenceDiagram
    participant P_j as All other parties P_j
    participant P_leader as This daemon's party P_i is LEADER
    participant P_i as This daemon's party P_i is not leader
    par Claim
        Note over P_j, P_i: After Lock Confirmations
        P_j->>P_leader: ✉ SecretReveal(secret)
        P_j->>P_i: ✉ SecretReveal(secret)
        Note right of P_i: Stores adaptor secret
        P_leader->>P_leader: All Adaptor Secrets received?
        P_i->>P_i: All Adaptor Secrets Received?
        opt If all received...
            alt If Leader
                P_leader->>P_leader: Adapt spend tx.
                P_leader->>P_leader: Broadcast spend tx
                P_leader-->>P_j: ✉ SpendTxBroadcast
                P_leader->>P_leader: Transition to ⤞ Completed.
            else If NOT Leader
                Note over P_leader, P_i: Final Protocol Step
                P_leader->>P_i: ✉ SpendTxBroadcast
                P_i->>P_i: Transition to ⤞ AwaitingLeaderSpend
                Note right of P_i: P_i knows Leader broadcasted spend tx.
                Note right of P_i: P_i extracts secret from on-chain tx.
            end
        end
    end
```

### 6.2.5 [Refund Phase](5_bbw_protocol.md#536-refund-phase)

The Refund phase runs on chain only and doesn't involve any `WireMessage` exchange off the chain.

---
