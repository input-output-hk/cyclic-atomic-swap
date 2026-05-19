# 6. Runtime View

## 6.1 Daemon

[`types::Daemon`](../../swap-daemon/src/daemon.rs)

```mermaid
flowchart TD
    new("new")
    insert_session("insert_session(session: SwapSession)")
    start_swap_session("start_swap_session(session_id: u64)")
    run("run()")
    %% Connect
    listener.accept("
        tokio::spawn ({
        loop {
        listener.accept() {
    ")
    handle_connection("
        networking::handleConnection(socket: TcpStream, from: String, event_tx: mpsc::Sender<DaemonEvent>)
    ")
    
    new --> insert_session
    insert_session --> start_swap_session
    start_swap_session --> run
    run -- spawn --> listener.accept
    subgraph connect
        listener.accept --> listener.accept
        listener.accept -- spawn --> handle_connection
    end
    
    
    
```
    
---

This phase runs concurrently for all N refund roles and all N spend roles (2N `TxRole` instances).
The sub-protocol for each role is a standard two-round MuSig2 exchange:

```mermaid
---
title: Transaction Build and Signing - Sequence Diagram
---
sequenceDiagram
    participant P as Party P[i]
    participant All as All N parties
    Note over P, All: MuSig2 - Round 1<br>Nonce Exchange
    P ->> All: SchnorrNonce { role, nonce }
    All ->> P: SchnorrNonce { role, nonce }
    Note over All: All parties compute R aggregated<br> → transition to MuSig2 Round 2
    Note over P, All: MuSig2 Round 2<br>Partial Signature Exchange
    P ->> All: PartialSignature { role, sig }
    All ->> P: PartialSignature { role, sig }
    Note over P, All: Refund role → finalize_role → full MuSig2 sig → signed_txs
    Note over P, All: Spend role → finalize_adaptor → adaptor pre-sig → adaptor_sigs
    Note over P, All: After all 2N roles finalised → transition to Funding → broadcast_my_lock_tx
```

> This exchange runs **2N** times in total: N `TxRole::Refund` instances and N `TxRole::Spend` instances.
> Every party participates in every role.
