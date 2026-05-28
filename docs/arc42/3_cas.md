# 3. Context and Scope

The reference implementation provides a concrete demonstration of the CANS protocol 
between Bitcoin and Cardano blockchains. 
It showcases the integration of two blockchains and the use of smart contracts implementing the CANS protocol.

## 3.1 Business Context

The [`swap-daemon`](../../swap-daemon) provides the software _daemon_ to be integrated with the party's wallet.

In the business context, it is supposed each daemon for each party to represent runs in a separate process inside
a separate enterprise boundary.
The Enterprise boundary should bot be interpreted as on premisis services running in servers 
installed inside the walls of a company.
Imagining a Kubernetes cluster of pods on clouds, the set containers running the daemons representing a party are
a valid enterprise boundary.

The _daemon_ handles a swap descriptor for the party it represents and the other parties it interacts with to complete
the swap session.

- The index **i** identifies the party the daemon represents
- The index **j** identifies the other parties the daemon interacts with.


Each _daemon_ implements the [Finite State Machine](5_bbw_protocol.md) (FSM) the CANS protocol describes.

Each _daemon_ interacts with the blockchain.
This reference implementation provides integration code for Bitcoin and Cardano.
Each daemon interacts with Bitcoin or Cardano. 

Each _daemon_ exchanges messages with the other daemons, the messages allow the distributed FSM instances to evolve to 
the success or failure of the swap session. 

```mermaid
C4Context
    title "Figure 1: Reference Implementation Business Context"
    
    Enterprise_Boundary(party_i, "This Party i") {
        Person(wallet_i, "Party i", "wallet")
        System(swap_descriptor_i, "Swap Descriptor")
        System(swap_daemon_i, "Swap Daemon")
        Rel(wallet_i, swap_descriptor_i, "propose/agree")
        Rel(swap_descriptor_i, swap_daemon_i, "define/run")
    }
    BiRel(swap_daemon_i, bitcoin, "use")
    BiRel(swap_daemon_i, cardano, "use")
    BiRel(swap_daemon_i, network, "use")
    
    Enterprise_Boundary(party_j, "Other Party j") {
        Person(wallet_j, "Party j", "wallet")
        System(swap_descriptor_j, "Swap Descriptor")
        System(swap_daemon_j, "Swap Daemon")
        Rel(wallet_j, swap_descriptor_j, "propose/agree")
        Rel(swap_descriptor_j, swap_daemon_j, "define/run")
    }
    BiRel(swap_daemon_j, bitcoin, "use")
    BiRel(swap_daemon_j, cardano, "use")
    BiRel(swap_daemon_j, network, "exchange messages")


    System_Boundary(blockchain_environment, "Blockchain Environment") {
        Container_Boundary(btc_defi_atomic_swaps_test_env, "BTC DeFi Test Rig",, "https://github.com/input-output-hk/btc-defi-atomic-swaps-test-env") {
            ContainerDb(bitcoin, "Bitcoin", "Docker Container", "Blockchain")
            ContainerDb(cardano, "Cardano", "Docker Container", "Blockchain")
        }
    }
    
    SystemQueue(network, "Network")
```

## 3.2 Technical Context

The reference implementation doesn't provide a party's wallet.
The wallet (keys, assets) and the terms and conditions of the swap are represented by the 
[`SwapSession`](../../swap-daemon/src/types.rs) and injected into the [`swap-daemon`](../../swap-daemon/src/daemon.rs) 
via the `pub fn insert_session(&mut self, session: SwapSession)` method.

The [lib.rs](../../swap-daemon/src/lib.rs) exposes what is needed to build a crate representing
a party or building a runtime rig serving the multiple parties of the CANS protocol.

Tests code at [regtest](../../swap-daemon/tests/regtest) provides a runtime rig for testing the swap daemon
hosting up to twenty parties.

Daemons use a pluggable transport layer to connect through the network to other demons, TCP is used by default.
Daemons are blockchain API clients
- **Bitcoin**: Electrs API TCP Port 3002
- **Cardano**, Dolos API TCP Port 50051/50052"

```mermaid
C4Container
    title "Figure 2: Reference Implementation Technical Context"
    System_Boundary(test_rig, "Cyclic Atomic N-Party Swap Reference Implementation Demo Rig") {
        Container(swap_descriptor, "daemon.insertSession(session: SwapSession)", "`swap-daemon`")
        Container(swap_validator, "Swap Validator", "`swap-validator`", "Plutus Smart Contract")
        System_Boundary(lib_i, "swap-daemon/src/lib.rs") {
            Container_Boundary(daemon, "This Party i<br/>-<br/>`swap-daemon`") {
                System(swap_session, "Swap Session")
                System(protocol, "Protocol")
                Component(cryptography, "Cryptography<br/>MuSig2")
                Component(blockchain, "Blockchain")
                Component(dashboard_api_server, "Dashboard API Server")
                Component(networking, "Networking")
                BiRel(swap_session, protocol, "represent")
                Rel(protocol, blockchain, "use")
                Rel(protocol, cryptography, "use")
                Rel(protocol, networking, "use")
                Rel(protocol, dashboard_api_server, "use")
            }
        }
        System_Boundary(blockchain_environment, "Blockchain Environment") {
            Container_Boundary(btc_defi_atomic_swaps_test_env, "BTC DeFi Test Rig",, "https://github.com/input-output-hk/btc-defi-atomic-swaps-test-env") {
                ContainerDb(bitcoin, "Bitcoin", "Docker Container", "Blockchain")
                ContainerDb(cardano, "Cardano", "Docker Container", "Blockchain")
            }
        }
        Container_Boundary(dashboard_vite, "Vite Service<br/>-<br/>`dashboard`") {
            System(dashboard_client_ui, "Dashboard Client UI")
        }
        System_Boundary(lib_j, "swap-daemon/src/lib.rs") {
            Container_Boundary(p_j, "Other Party j<br/>-<br/>`swap-daemon`") {
                Container(p_j_daemon, "Swap Daemon")
            }
        }
    }

    Rel(swap_descriptor, swap_session, "define")
    Rel(blockchain, swap_validator, "use")
    Rel(dashboard_client_ui, dashboard_api_server, "use", "REST API")
    BiRel(blockchain, bitcoin, "read/write", "Electrs TCP Port 3002")
    BiRel(blockchain, cardano, "read/write", "Dolos TCP Port 50051/50052")
    BiRel(networking, p_j_daemon, "Pluggable Transport Layer", "Network")
    BiRel(p_j_daemon, bitcoin, "read/write", "Electrs TCP Port 3002")
    BiRel(p_j_daemon, cardano, "read/write", "Dolos TCP Port 50051/50052")


```