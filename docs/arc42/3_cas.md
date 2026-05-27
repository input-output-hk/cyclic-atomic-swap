# 3. Context and Scope

The reference implementation is a demonstration of a decentralised swap protocol between Bitcoin and Cardano blockchains. 
It showcases the integration of two blockchains and the use of smart contracts implementing the CANS protocol.



## 3.1 Business Context

[`swap-daemon`](../../swap-daemon) provides the software to be integrated with the party's wallet

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
    BiRel(swap_daemon_j, network, "use")

    SystemQueue(network, "Network")
    
    System_Boundary(blockchain_environment, "Blockchain Environment") {
        Container_Boundary(btc_defi_atomic_swaps_test_env, "BTC DeFi Test Rig",, "https://github.com/input-output-hk/btc-defi-atomic-swaps-test-env") {
            ContainerDb(bitcoin, "Bitcoin", "Docker Container", "Blockchain")
            ContainerDb(cardano, "Cardano", "Docker Container", "Blockchain")
        }
    }
```

## 3.2 Technical Context

```mermaid
C4Container
    title "Figure 2: Reference Implementation Technical Context"
    System_Boundary(test_rig, "Cyclic Atomic N-Party Swap Reference Implementation Demo Rig") {
        Container(swap_descriptor, "Swap Descriptor", "`swap-daemon`")
        Container(swap_validator, "Swap Validator", "`swap-validator`", "Plutus Smart Contract")
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
        System_Boundary(blockchain_environment, "Blockchain Environment") {
            Container_Boundary(btc_defi_atomic_swaps_test_env, "BTC DeFi Test Rig",, "https://github.com/input-output-hk/btc-defi-atomic-swaps-test-env") {
                ContainerDb(bitcoin, "Bitcoin", "Docker Container", "Blockchain")
                ContainerDb(cardano, "Cardano", "Docker Container", "Blockchain")
            }
        }
        Container_Boundary(dashboard_vite, "Vite Service<br/>-<br/>`dashboard`") {
            System(dashboard_client_ui, "Dashboard Client UI")
        }
        Container_Boundary(p_j, "Other Party j<br/>-<br/>`swap-daemon`") {
            Container(p_j_daemon, "Swap Daemon")
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