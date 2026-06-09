# 4. Solution Strategy

## 4.1 [Architecture Constraints](2_ac.md)

| Goal/Requirement               | Architectural Approach                      | Details                                                                                                                                                                      |
|--------------------------------|---------------------------------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| 1.Apache 2.0 Licence           | [arc42](https://docs.arc42.org/) template   | Use arc2 as guideline to document the reference implementation.<br/> Architecural documents as software manual to learn and adapt the published reference implementation.    |
| 2.Atomic Swap Test Environment | Contenarised services.                      | Reference implementation reuses [btc-defi-atomic-swaps-test-env](https://github.com/input-output-hk/btc-defi-atomic-swaps-test-env) to provide a dockerised test environment |
| 3.Code in Rust                 | [Tokio](https://tokio.rs/)                  | Asynchronous application runtime pattern.                                                                                                                                    |
| 4.Code Instrumentation         | [Open telemetry](https://opentelemetry.io/) | [Tokyo Tracing](https://github.com/tokio-rs/tracing-opentelemetry) crate.                                                                                                    |
| 5.Use Bitcoin                  | [Electrum](https://electrum.org/) API       | [Bitcoin](https://docs.rs/crate/bitcoin/) transaction and wallet handing.                                                                                                    |
| 6.Use Cardano                  | Dolos API                                   | [Dolos](https://github.com/input-output-hk/dolos) gRPC API for Cardano blockchain interaction.                                                                               |

## 4.2 Architectural Patterns and Technology Decisions

| Goal/Requirement   | Architectural Approach                                                                         | Details                                                              |
|--------------------|------------------------------------------------------------------------------------------------|----------------------------------------------------------------------|
| Cryptography       | [MuSig2](https://eprint.iacr.org/2020/1261) adaptor signature.                                 | [MuSig2](https://docs.rs/musig2/latest/musig2/) Rust crate.          |
| Networking         | [Ports and Adapters](https://en.wikipedia.org/wiki/Hexagonal_architecture_(software)) pattern. | Pluggable transport layer for network communication.                 |
| Party modelling    | [Daemon](https://en.wikipedia.org/wiki/Daemon_(computing)) pattern.                            | [Tokio](https://tokio.rs/) asynchronous application runtime pattern. |                                                                    |
| Protocol modelling | [Finite State Machine](https://en.wikipedia.org/wiki/Finite-state_machine) pattern.            | [Tokio](https://tokio.rs/) asynchronous application runtime pattern. |


See [Architecture Decisions](9_ad.md) for additional details.

## 4.3 Quality Goals

### Goal

**Provide a reference implementation IOG can be proud to showcase live, documented well enough to be used as a reference
for other projects.**

### How To Achieve

- Distribute the software with a [test rig](../../swap-daemon/tests) and a UI [dashboard](../../dashboard) to visualise
  the reference implementation running live.
- Use the [architectural documentation](toc.md) as a map between the
  theoretical [protocol specification](../protocol-spec.pdf)
  and its concrete [reference implementation](../../swap-daemon), use UML diagrams to visualise the relation between
  theory and code, see [5. Building Blocks View](5_bbw.md) and [6. Runtime View](6_rtw.md).
- Full document the [reference implementation](../../swap-daemon) code.

 