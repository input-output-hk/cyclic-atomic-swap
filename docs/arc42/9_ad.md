# 9. Architecture Decisions

The [Architecture Constraints](9_ad.md) dictates the choices explained in the [Solution Strategy](4_ss.md) section.

### [MuSig2](https://eprint.iacr.org/2020/1261) adaptor signature

---
status: accepted
date: January 2026 
decision-makers: [Edmund Judge](mailto:edmund.judge@iohk.io); [Andrew Sutherland](mailto:andrew.sutherland@iohk.io)
informed: [Mauro Jaskelioff](mauro.jaskelioff@iohk.io)
---

# <!-- short title, representative of solved problem and found solution -->

## Context and Problem Statement

Use the Rust [MuSig2](https://docs.rs/musig2/latest/musig2/) crate

## Decision Drivers

Adaptor 

## Considered Options

* <!-- option -->

## Decision Outcome

Chosen option: "", because

### Consequences

* Good, because
* Bad, because

### Confirmation

* Good, because
* Neutral, because
* Bad, because

| Cryptography       | [MuSig2](https://eprint.iacr.org/2020/1261) adaptor signature.                                 | [MuSig2](https://docs.rs/musig2/latest/musig2/) Rust crate.          |
| Networking         | [Ports and Adapters](https://en.wikipedia.org/wiki/Hexagonal_architecture_(software)) pattern. | Pluggable transport layer for network communication.                 |
| Protocol modelling | [Finite State Machine](https://en.wikipedia.org/wiki/Finite-state_machine) pattern.            | [Tokio](https://tokio.rs/) asynchronous application runtime pattern. |
| Party modelling    | [Daemon](https://en.wikipedia.org/wiki/Daemon_(computing)) pattern.                            | [Tokio](https://tokio.rs/) asynchronous application runtime pattern. |                                                                    |



Daemon: intention-based agents sandbox...


