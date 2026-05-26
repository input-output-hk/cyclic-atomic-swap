# 5 Building Blocks View - Protocol Examples

## Example Trace: N = 3, Happy Path

**Parties:** A, B, C — elected leader = A
**Cycle:** A → B → C → A
**Transfers:** tr₀ = (A→B), tr₁ = (B→C), tr₂ = (C→A) *(leader's received transfer)*
**Refund windows:** W_A > W_B > W_C *(leader's window latest)*

Once all locks are confirmed, B and C immediately broadcast their secrets and enter `AwaitingLeaderSpend`; A enters `AwaitingSecrets`.

```mermaid
---
title: "Figure 1: N = 3, Happy Path"
---
sequenceDiagram
    participant A as A (leader)
    participant B as B
    participant C as C
    participant Chain as Chain

    Note over A,Chain: INIT
    Note over A,C: Each party generates t_i, computes T_i ? state = Initialized

    Note over A,Chain: ADAPTOR POINT EXCHANGE
    A->>B: AdaptorPoint(T_A)
    A->>C: AdaptorPoint(T_A)
    B->>A: AdaptorPoint(T_B)
    B->>C: AdaptorPoint(T_B)
    C->>A: AdaptorPoint(T_C)
    C->>B: AdaptorPoint(T_C)
    Note over A,C: All collect {T_A, T_B, T_C}, compute T_agg

    Note over A,Chain: LEADER ELECTION
    A->>B: LeaderElectionCommitment(c_A)
    A->>C: LeaderElectionCommitment(c_A)
    B->>A: LeaderElectionCommitment(c_B)
    B->>C: LeaderElectionCommitment(c_B)
    C->>A: LeaderElectionCommitment(c_C)
    C->>B: LeaderElectionCommitment(c_C)
    Note over A,C: All commitments received ? broadcast nonces
    A->>B: LeaderElectionNonce(n_A)
    A->>C: LeaderElectionNonce(n_A)
    B->>A: LeaderElectionNonce(n_B)
    B->>C: LeaderElectionNonce(n_B)
    C->>A: LeaderElectionNonce(n_C)
    C->>B: LeaderElectionNonce(n_C)
    Note over A,C: Verify commitments ? compute_leader ? elect A

    Note over A,Chain: SIGNING (2N = 6 MuSig2 sessions)
    Note over A,C: build_lock_txs begin_refund_signing + begin_spend_signing
    A->>B: SchnorrNonce { role, R_A }  [all 6 roles]
    A->>C: SchnorrNonce { role, R_A }  [all 6 roles]
    B->>A: SchnorrNonce  [all 6 roles]
    B->>C: SchnorrNonce  [all 6 roles]
    C->>A: SchnorrNonce  [all 6 roles]
    C->>B: SchnorrNonce  [all 6 roles]
    Note over A,C: All nonces per role ? transition_to_round_two ? send PartialSignature
    A->>B: PartialSignature { role, s'_A }  [all 6 roles]
    A->>C: PartialSignature { role, s'_A }  [all 6 roles]
    B->>A: PartialSignature  [all 6 roles]
    C->>A: PartialSignature  [all 6 roles]
    Note over A,C: Refund roles ? finalize_role ? signed_txs
    Note over A,C: Spend roles  ? finalize_adaptor ? adaptor_sigs
    Note over A,C: All 6 roles done ? Funding

    Note over A,Chain: LOCK
    A->>Chain: lock tx A
    B->>Chain: lock tx B
    C->>Chain: lock tx C
    A->>B: LockTxBroadcast
    A->>C: LockTxBroadcast
    B->>A: LockTxBroadcast
    C->>A: LockTxBroadcast
    Note over A,C: daemon polls ChainPollTarget::LockTx for each participant
    Chain-->>A: all deposits confirmed
    Chain-->>B: all deposits confirmed
    Chain-->>C: all deposits confirmed

    Note over A,Chain: SECRET REVEAL
    Note over A: A ? AwaitingSecrets
    Note over B,C: B, C ? AwaitingLeaderSpend: immediately broadcast secrets
    B->>A: SecretReveal(t_B)
    B->>C: SecretReveal(t_B)
    C->>A: SecretReveal(t_C)
    C->>B: SecretReveal(t_C)
    Note over A: all_adaptor_secrets_received ? t_agg = t_A + t_B + t_C

    Note over A,Chain: CLAIM
    A->>Chain: spend?: C?A claim  (TRIGGER)
    A->>B: SpendTxBroadcast
    A->>C: SpendTxBroadcast
    Note over B,C: ChainPollTarget::LeaderSpendTx fires ? extract_secret_and_adapt ? adapt_role_with
    Chain-->>B: trigger confirmed
    Chain-->>C: trigger confirmed
    B->>Chain: spend?: A?B claim
    C->>Chain: spend?: B?C claim

    Note over A,Chain: DONE: A claimed C's deposit � B claimed A's deposit � C claimed B's deposit
```

---

## Example Trace: N = 3, Refund Path

**Parties:** A = leader, B, C
**Scenario:** B withholds t_B during the Secret Reveal phase. A never receives all secrets and does not broadcast the trigger. All parties fall through to the Refund phase via `ChainPollTarget::RefundWindow`.

```mermaid
---
title: "Figure 2: N = 3, Refund Path"
---
sequenceDiagram
    participant A as A (leader)
    participant B as B
    participant C as C
    participant Chain as Chain

    Note over A,Chain: INIT, ADAPTOR POINT EXCHANGE, LEADER ELECTION, SIGNING, and LOCK
    Note over A,Chain: proceed identically to the happy-path trace.

    Chain-->>A: all deposits confirmed
    Chain-->>B: all deposits confirmed
    Chain-->>C: all deposits confirmed

    Note over A,Chain: SECRET REVEAL
    Note over A: A ? AwaitingSecrets
    Note over B,C: B, C ? AwaitingLeaderSpend: broadcast secrets
    C->>A: SecretReveal(t_C)
    C->>B: SecretReveal(t_C)
    Note over B: B withholds t_B ? no SecretReveal broadcast
    Note over A: Only t_C received: all_adaptor_secrets_received = false ? no trigger
    Note over B,C: RefundWindow poller active: no trigger seen on-chain

    Note over A,Chain: REFUND 
    Note over C: ChainPollTarget::RefundWindow fires (W_C ? earliest)
    Chain-->>C: slot/block >= W_C
    C->>Chain: refund?: C reclaims (C?A locked funds)

    Note over B: ChainPollTarget::RefundWindow fires (W_B)
    Chain-->>B: slot/block >= W_B
    B->>Chain: refund?: B reclaims (B?C locked funds)

    Note over A: ChainPollTarget::RefundWindow fires (W_A ? latest)
    Chain-->>A: slot/block >= W_A
    A->>Chain: refund?: A reclaims (A?B locked funds)

    Note over A,Chain: DONE (refund): A, B, C each recovered their own deposit ? no principal lost
```

> Refund windows fire in order W<sub>B</sub> < W<sub>C</sub> < W<sub>A</sub>
> (earlier window for parties farther from the leader).
> Each party independently broadcasts their refund tx via `broadcast_my_refund_tx`. 
> No principal is lost.

---
