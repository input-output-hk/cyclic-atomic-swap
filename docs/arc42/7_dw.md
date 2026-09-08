# 7. Deployment View

## 7.1 Intended Use Case

In this section 

- The index **i** identifies _this party_ the daemon represents;
- The index **j** identifies _the other parties_ this daemon interacts with.

[C4](https://c4model.com/) **enterprise boundary** means the boundary of a legal body.

The intended use case suggests

- one _enterprise boundry_ per party, encompassing
  - the daemon deployment node,
  - the optional [ELK Stack](https://www.elastic.co/elastic-stack) node,
  - the optional UI Dashboard node;
- one _enterprise boundry_ per blockchain infrastructure

Deployment nodes run in OS, on cloud, on container or on premises Operating System.

Inside the same enterprise boundary, daemons, ELK stack and UI Dashboard may or may not share the same
OS or container.

```mermaid
C4Deployment
    title "Figure 1: Intended Use Case Deployment"
    Enterprise_Boundary(party_j, "Party j") {
        Deployment_Node(party_node_j, "Party j Node", "Container/OS") {
            Deployment_Node(swap_daemon_node_j, "Swap Daemon j", "Executable") {
                Container(swap_daemon_j, "Swap Daemon j", "artifact")
            }
        }
    }
    Deployment_Node(blockchain_node, "Blockchain Node", "Container/OS") {
        Container(blockchain_validator, "Blockchain Validator", "Smart Contract")
        ContainerDb(blockchain, "Blockchain", "blockchain")
        Rel(blockchain_validator, blockchain, "loaded")
    }
    Enterprise_Boundary(party_i, "Party i") {
        Deployment_Node(party_node_i, "Party i Node", "Container/OS") {
            Deployment_Node(swap_daemon_node_i, "Swap Daemon i", "Executable") {
                Container(swap_daemon_i, "Swap Daemon i", "artifact")
                Container(dashboard_server_i, "Dashboard Server", "library")
                Container(open_telemetry_collector, "OpenTelemetry Collector Log", "Library")
                Container(open_telemetry_instrumentation_library_i, "Open Telemetry Instrumentation", "library")
                Container(transport_layer_i, "Pluggable Transport Layer", "library")
                Rel(swap_daemon_i, dashboard_server_i, "link")
                Rel(swap_daemon_i, open_telemetry_collector, "link")
                Rel(swap_daemon_i, open_telemetry_instrumentation_library_i, "link")
                Rel(swap_daemon_i, transport_layer_i, "link")
            }
        }
        Deployment_Node(dashboard_node, "Dashboard UI Node", "Container/OS") {
            Deployment_Node(dashboard_ui_vite, "Dashboard", "Vite") {
                Container(dashboard_ui, "Dashboard UI", "React")
            }
        }
        Deployment_Node(elk_node_i, "ELK Node", "Container/OS") {
            Container(elk_open_telemetry_collector, "OpenTelemetry Collector", "Agent")
            ContainerDb(elk_elasticsearch, "Elasticsearch", "Elasticsearch")
            Container(elk_kibana, "Kibana", "Kibana")
            ContainerQueue(elk_logstash, "Logstash", "Logstash")
            Rel(elk_open_telemetry_collector, elk_elasticsearch, "logs")
            Rel(elk_open_telemetry_collector, elk_kibana, "dashboard")
            Rel(elk_open_telemetry_collector, elk_logstash, "logs")
        }
        Rel(open_telemetry_instrumentation_library_i, elk_open_telemetry_collector, "Open Telemetry Protocol")
        Rel(dashboard_ui, dashboard_server_i, "REST API")
    }
    BiRel(swap_daemon_i, swap_daemon_j, "Pluggable Transport Layer")
    BiRel(swap_daemon_i, blockchain, "Read/Write")
    BiRel(swap_daemon_j, blockchain, "Read/Write")
```

## 7.2 Reference Implementation Demo/Test Rig

The reference implementation includes a demo/test rig running all daemons in the same deployment node 
and running two containers, one for Bitcoin and one for Cardano.

The containers for Bitcoin and Cardano are published in the 
[test-env/](../../test-env) directory of this repository, so the regression suite it drives needs
no other repository. See [test-env/README.md](../../test-env/README.md) for how to run it, and
[swap-daemon/README.md](../../swap-daemon/README.md#regression-tests) for the tests themselves.

```mermaid
C4Deployment
    title "Figure 2: Reference Implementation Test/Rig"
    Enterprise_Boundary(rig, "Demo/Test Rig") {
        Deployment_Node(runtime, "Runtime Environment", "OS") {
            Deployment_Node(test_env, "test-env/", "directory") {
                Deployment_Node(bitcoin_node, "Bitcoin Node", "Container") {
                    ContainerDb(bitcoin, "Bitcoin", "blockchain")
                }
                Deployment_Node(cardano_node, "Cardano Node", "Container") {
                    ContainerDb(cardano, "Cardano", "blockchain")
                    Container(swap_validator, "Swap Validator", "Smart Contract")
                    Rel(swap_validator, cardano, "loaded")
                }
            }
            Deployment_Node(cyclic-atomic-swap, "https://github.com/input-output-hk/cyclic-atomic-swap", "repository") {
                Deployment_Node(daemons_executable, "swap-daemon", "executable") {
                    Container_Boundary(party_j, "Party j") {
                        Container(swap_daemon_j, "Swap Daemon j", "agent")
                    }
                    Deployment_Node(dashboard_ui_vite, "Dashboard", "Vite") {
                        Container(dashboard_ui, "Dashboard UI", "React")
                    }
                    Container_Boundary(party_i, "Party i") {
                        Container(swap_daemon_i, "Swap Daemon j", "agent")
                        Container(dashboard_server_i, "Dashboard Server", "library")
                        Container(open_telemetry_collector, "OpenTelemetry Collector Log", "Library")
                        Container(open_telemetry_instrumentation_library_i, "Open Telemetry Instrumentation", "library")
                        Container(transport_layer_i, "Pluggable Transport Layer", "library")
                    }
                    Rel(swap_daemon_i, dashboard_server_i, "link")
                    Rel(swap_daemon_i, open_telemetry_collector, "link")
                    Rel(swap_daemon_i, open_telemetry_instrumentation_library_i, "link")
                    Rel(swap_daemon_i, transport_layer_i, "link")
                    BiRel(swap_daemon_i, swap_daemon_j, "TCP")
                }
            }
        }
    }
    Rel(dashboard_ui, swap_daemon_i, "REST API")
    Rel(dashboard_ui, swap_daemon_j, "REST API")
    BiRel(swap_daemon_i, bitcoin, "Electrs REST Port 3002")
    BiRel(swap_daemon_j, bitcoin, "Electrs REST Port 3002")
    BiRel(swap_daemon_i, cardano, "Dolos gRPC Port 50051/50052")
    BiRel(swap_daemon_j, cardano, "Dolos gRPC Port 50051/50052")
```
