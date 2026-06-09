# 8 Crosscutting Concepts

The notable crosscutting concepts of the reference implementation are 

- [**Daemon**](../../swap-daemon/src/daemon.rs) orchestrating instances of
- [**Swap Session**](../../swap-daemon/src/types.rs) 
  implemented in the [session.rs](../../swap-daemon/src/protocol/session.rs) file;
- **Instrumentation**, implemented with [tracing](https://docs.rs/tracing/latest/tracing/) 
  and [tracing-subscriber](https://docs.rs/tracing-subscriber/latest/tracing-subscriber/):
  read at the linked documentation for additional info and how to integrate and customize
  instrumentation adapting this reference implementation to a production project.
 

## 8.1 Daemon and Swap Session

_Daemon_ and _Swap Session_ are the pivotal concepts associated with the software architecture
of this reference implementation for the CANS protocol; those address the following responsibilities.

- **Orchestration**:
  - `daemon.rs`: Implements the `Daemon` struct, which orchestrates events, manages multiple `SwapSession` instances, handles networking connections, and spawns background tasks like chain monitoring.

- **Protocol**:
  - `protocol/session.rs`: The heart of the protocol, implementing the state machine for a single _swap session_. It handles transitions and coordinates with other protocol submodules.
  - `protocol/chain_monitor.rs`: Responsible for checking on-chain events (confirmations, spends, refund windows) for both Bitcoin and Cardano.
  - Specialised submodules (`leader_election`, `lock_funds`, `refund`, `spend`, `adaptor_nonce`, `secret`) handle specific phases of the atomic swap protocol.

- **Infrastructure**:
  - `networking.rs`: Handles TCP communication between participants, including connection pooling and broadcasting messages.
  - `cryptography/multisig.rs`: Implements MuSig2 and adaptor signature logic used for co-signing transactions.
  - `blockchains/`: Contains blockchain-specific utility functions for Bitcoin and Cardano.

- **Core**:
  - `types.rs`: Defines shared data structures, enums (like `SwapState`, `WireMessage`), and common types.
  - `utils.rs`: General-purpose helper functions.
  - `config.rs`: Handles application configuration.


```mermaid
---
title: "Figure 1: Crosscutting Concepts and Responsabilities"
---
graph TD
    subgraph Orchestration
        Daemon[daemon.rs]
    end
    subgraph "Protocol (Domain Logic)"
        Session[protocol/session.rs]
        
        Session --> LE[protocol/leader_election.rs]
        Session --> LF[protocol/lock_funds.rs]
        Session --> RF[protocol/refund.rs]
        Session --> SP[protocol/spend.rs]
        Session --> AN[protocol/adaptor_nonce.rs]
        Session --> SC[protocol/secret.rs]
        
        Daemon --> Session
        Daemon --> CM[protocol/chain_monitor.rs]
    end

    subgraph Infrastructure
        NW[networking.rs]
        CRY[cryptography/multisig.rs]
        BC[blockchains/mod.rs]
        BC --> BC_B[blockchains/bitcoin_utils.rs]
        BC --> BC_C[blockchains/cardano_utils.rs]
    end

    subgraph Core
        Types[types.rs]
        Utils[utils.rs]
        Config[config.rs]
    end

    Daemon --> NW
    Daemon --> Types
    Daemon --> Utils

    Session --> CRY
    Session --> NW
    Session --> Types
    Session --> Utils

    LF --> BC
    CM --> BC
    CM --> Types
    
    BC --> Types
    BC --> Config

    CRY --> Types
    NW --> Types
    Utils --> Types
```

## 8.2 Daemon and Swap Session API.

The following class diagram illustrates the public structures and logical service interfaces of the `swap-daemon`
reference implementation.

See [Building Blocks View](5_bbw.md) for additional details.

```mermaid
---
title: "Figure 2: Daemon and Swap Session API"
---
classDiagram
  class Daemon {
    +HashMap~SessionId, SwapSession~ sessions
    +SwapKeys swap_keys
    +HashMap~Tuple, AbortHandle~ active_pollers
    +DaemonConfig config
    +new(SwapKeys, DaemonConfig) Daemon
    +insert_session(SwapSession)
    +start_swap_session(u64) Result
    +run() Result
  }

  class SwapSession {
    +SessionId id
    +Participants participants
    +SwapState state
    +new(SessionId, Participants, ...) SwapSession
    +transition_to(SwapState)
    +handle_session_message(WireMessage, ParticipantId, SwapKeys, DaemonConfig)
  }

  class Participant {
    +ParticipantId id
    +Blockchain blockchain
    +Address tcp_address
    +u64 amount_locking
    +bool is_me
  }

  class SwapKeys {
    +BitcoinKeys bitcoin
    +CardanoKeys cardano
  }

  class DaemonConfig {
    +String bitcoin_rpc_url
    +String cardano_node_url
    +NetworkConfig network
  }

  class WireMessage {
    <<enumeration>>
    LeaderElection
    AdaptorNonce
    LockFunds
    Refund
    Spend
    Secret
  }

  class SwapState {
    <<enumeration>>
    Init
    LeaderElected
    FundsLocked
    Refunding
    Spending
    Completed
    Aborted
  }

  class ChainMonitor {
    <<interface>>
    +validate_funding_utxos(SwapSession, DaemonConfig) bool
    +check_refund_window_open(SwapSession, ParticipantId, DaemonConfig) bool
    +check_lock_tx_confirmed(SwapSession, ParticipantId, DaemonConfig)
    +check_leader_spend_confirmed(SwapSession, ParticipantId, DaemonConfig) bool
  }

  class Networking {
    <<interface>>
    +new_connection_pool() ConnectionPool
    +handle_connection(TcpStream, String, Sender) Result
    +broadcast(Vec~Address~, Envelope, ConnectionPool) Result
  }

  class ProtocolCore {
    <<interface>>
    +start_leader_election(SwapSession)
    +broadcast_my_lock_tx(SwapSession)
    +begin_refund_signing(SwapSession)
    +begin_spend_signing(SwapSession)
    +reveal_secret(SwapSession)
  }

  class Multisig {
    <<interface>>
    +aggregate_public_keys(Vec~PublicKey~) AggregatedKey
    +sign_adaptor(SecretKey, Message, AdaptorPoint) AdaptorSignature
    +verify_adaptor(PublicKey, Message, AdaptorSignature) bool
    +adapt_signature(AdaptorSignature, SecretKey) Signature
    +extract_secret(Signature, AdaptorSignature) SecretKey
  }

%% Relationships
  Daemon "1" *-- "many" SwapSession : manages
  Daemon --> SwapKeys : owns
  Daemon --> DaemonConfig : owns
  SwapSession "1" *-- "many" Participant : involves
  SwapSession --> SwapState : has
  SwapSession ..> WireMessage : processes
  Daemon ..> Networking : uses
  Daemon ..> ChainMonitor : uses
  SwapSession ..> ProtocolCore : uses
  ProtocolCore ..> Multisig : uses
```