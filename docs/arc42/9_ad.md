# 9. Architecture Decisions

The [Architecture Constraints](9_ad.md) dictates the choices explained in the [Solution Strategy](4_ss.md) section.

## 9.1 Cryptography – [MuSig2](https://eprint.iacr.org/2020/1261) adaptor signature

| Status   | Date         | Decision-makers                                                                                       | Informed                                                                                                                        |
|----------|--------------|-------------------------------------------------------------------------------------------------------|---------------------------------------------------------------------------------------------------------------------------------|
| accepted | January 2026 | [Edmund Judge](mailto:edmund.judge@iohk.io)<br/>[Andrew Sutherland](mailto:andrew.sutherland@iohk.io) | Nicolas Biri- VP of ARC<br/>[Luca Debiasi](mailto:luca.debiasi@iohk.io)</br>[Mauro Jaskelioff](mailto:mauro.jaskelioff@iohk.io) |

### Context

Multi-signatures enable a group of signers to produce a joint signature on a joint message.

### Decision Drivers

- The robust [MuSig2](https://docs.rs/musig2/latest/musig2/) crate library is available,
- it natively fits in Bitcoin cryptography.

### Decision Outcome

- Good because the [formal method](../../formal-methods) modelling confirms MuSig2 is suitable for the CANS protocol.
- Good because [swap_validator](../../swap_validator) provides a smart contract for Cardano bridging MuSig2
  with Cardano Ed25519 cryptography.

---

## 9.2 Code Instrumentation – [Open Telemetry](https://opentelemetry.io/)

| Status   | Date       | Decision-makers                             | Informed                                                                                                                          |
|----------|------------|---------------------------------------------|-----------------------------------------------------------------------------------------------------------------------------------|
| accepted | March 2026 | [Luca Debiasi](mailto:luca.debiasi@iohk.io) | Nicolas Biri- VP of ARC</br>[Edmund Judge](mailto:edmund.judge@iohk.io)<br/>[Andrew Sutherland](mailto:andrew.sutherland@iohk.io) |

### Context

Daemons running on behalf of n-parties must provide a robust instrumentation layer to provide debugging tools able to
analyse all daemon processes as a whole.

### Decision Drivers

Open Telemetry is a vendor-neutral observability framework that provides a unified approach to monitoring, logging,
and tracing.
It allows for the collection and aggregation of metrics, logs, and traces from various sources,
making it easier to understand and troubleshoot complex systems.
By adopting Open Telemetry, we can ensure that our instrumentation layer is flexible, scalable,
and interoperable with other monitoring tools.

### Decision Outcome

- Good because [Tokyo Tracing](https://github.com/tokio-rs/tracing-opentelemetry) crate library offered Open Telemetry
  functionalities without asking any compromise or additoonal complexitty in the reference implementation.

---

## 9.3 Networking – [Ports and Adapters](https://en.wikipedia.org/wiki/Hexagonal_architecture_(software)) pattern.

| Status   | Date       | Decision-makers                             | Informed                                                                                                                          |
|----------|------------|---------------------------------------------|-----------------------------------------------------------------------------------------------------------------------------------|
| accepted | March 2026 | [Luca Debiasi](mailto:luca.debiasi@iohk.io) | Nicolas Biri- VP of ARC</br>[Edmund Judge](mailto:edmund.judge@iohk.io)<br/>[Andrew Sutherland](mailto:andrew.sutherland@iohk.io) |

### Context

A wide range of use cases for the CANS protocol, including but not limited to decentralised finance (DeFi),
supply chain management, and identity verification, require for robust networking capabilities.

### Decision Drivers

Support "usability", help to demo, prevent the innovative qualities of the CANS protocol
and its reference implementation are dented by the incapability to meet regulated industry requirements in terms
of messaging.

### Decision Outcome

- Good because we reached wanted flexibility of the reference implementation.

---

## 9.4 Party modelling – [Daemon](https://en.wikipedia.org/wiki/Daemon_(computing)) pattern.

| Status   | Date          | Decision-makers                                                                                                                                       | Informed                |
|----------|---------------|-------------------------------------------------------------------------------------------------------------------------------------------------------|-------------------------|
| accepted | February 2026 | [Luca Debiasi](mailto:luca.debiasi@iohk.io)<br/>[Edmund Judge](mailto:edmund.judge@iohk.io)<br/>[Andrew Sutherland](mailto:andrew.sutherland@iohk.io) | Nicolas Biri- VP of ARC |

### Context

Represent as independent processes the participnts of the [N-Party Cyclic Atomic Swap Protocol](../protocol-spec.pdf)
Allow each participant to represent multiple swap sessions.
Allow the reference implementation to run in the same computer/container for demo and experimentation purposes.
Aim for an _Intent-Based Agent_ implementation.

### Decision Driver

The availability of the [Tokio](https://tokio.rs/) create library facilitates developing code combining FSM and deemon
patterns.

A daemon architecture is a natural fit for intent-based agents for several interconnected reasons:

- **Persistent State & Context**
  A daemon runs continuously as a background process, which means it maintains live state across interactions.
  Intent-based agents need to track evolving context — partial fulfillment of goals, intermediate results,
  memory of prior steps — without reconstructing everything from scratch on each invocation.
  A daemon holds this state in memory cheaply and consistently.
- **Event-Driven Reactivity**
  Daemons excel at listening for events (signals, sockets, file changes, timers) and responding asynchronously.
  Intent-based agents are inherently reactive: they receive a high-level goal, then orchestrate actions across
  time as conditions change.
  A daemon's event loop maps cleanly onto this — it can re-evaluate intent fulfillment whenever new information arrives,
  without polling or external orchestration.
- **Decoupled Intent from Execution**
  The daemon acts as a long-lived broker: the intent (what the user wants) is declared once, while the execution
  (how it's achieved) is handled by background processes the daemon spawns, monitors, and restarts.
  This separation is architecturally clean — the caller doesn't need to care about retry logic, partial failures,
  or sequencing.
- **Lifecycle & Supervision**
  Daemons are designed to run under process supervisors (systemd, launchd, supervisor).
  This gives the agent automatic recovery from crashes, graceful shutdown hooks, and health monitoring —
  all critical when an agent is mid-execution of long-running intent like "deploy this service"
  or "reprocess this dataset."
- **Resource Efficiency for Long-Horizon Goals**
  Intent-based agents often deal with goals that span minutes, hours, or longer.
  A daemon avoids the overhead of spawning a new process per request — it amortises initialisation cost 
  (loading models, establishing connections, warming caches) across the entire lifetime of the agent, 
  making long-horizon execution practical.
- **Interprocess Communication**
  Daemons expose stable IPC surfaces (Unix sockets, named pipes, D-Bus, HTTP on loopback) abstracted by the
  Pluggable Transport Layer covered in [Networking](#93-networking--ports-and-adapters-pattern).
  Multiple clients — a CLI, a UI, another service — can all issue intents to the same agent instance and receive 
  streamed progress, which is far more natural than stateless request/response for goals that unfold over time.
- **Concurrency & Parallelism**
  A daemon naturally manages a pool of workers or coroutines, letting an intent-based agent pursue subgoals in
  parallel (e.g., fan out research tasks, then synthesise results), coordinate dependencies, and throttle execution —
  all within a single coherent process boundary.

### Decision Outcome

- Good because  [Tokio](https://tokio.rs/) create library effectively speeded up development.
- Good because the reference implementation is well-documented and easy to understand, an excellent
  starting point to implemenent an Intent-Based Agent service.

---

## 9.4 Protocol modelling – [Finite State Machine](https://en.wikipedia.org/wiki/Finite-state_machine) pattern.

| Status   | Date         | Decision-makers                                                                                       | Informed                                                                                                                        |
|----------|--------------|-------------------------------------------------------------------------------------------------------|---------------------------------------------------------------------------------------------------------------------------------|
| accepted | January 2026 | [Edmund Judge](mailto:edmund.judge@iohk.io)<br/>[Andrew Sutherland](mailto:andrew.sutherland@iohk.io) | Nicolas Biri- VP of ARC<br/>[Luca Debiasi](mailto:luca.debiasi@iohk.io)</br>[Mauro Jaskelioff](mailto:mauro.jaskelioff@iohk.io) |

### Context

The [N-Party Cyclic Atomic Swap Protocol Specification](../protocol-spec.pdf)
and its [formal method](../../formal-methods) modelling.

### Decision Driver

The Finite State Machine (FSM) pattern is a widely used approach for modelling and implementing protocols,
especially in scenarios where the protocol's behaviour can be described as a series of states and transitions between
them.
This pattern is particularly suitable for protocols that involve complex state transitions and conditional logic,
such as the CANS protocol, which requires precise and reliable handling of various states and transitions.

### Decision Outcome

- Good because FSM fits naturally the protocol theory, the formal methods modelling challenges, and combines
  well with the daemon architectural pattern chosen to represent the participants in the protocol.

---
