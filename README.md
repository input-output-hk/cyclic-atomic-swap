# Cyclic Atomic N-party Swap Protocol

## Trustless Cross-chain Swaps via Adaptor Signatures

---

## Intro

This repository provides a **Rust** reference implementation for the [Cyclic Atomic N-party Swap (CANS) protocol](),
a trustless cross-chain swap mechanism that enables secure and efficient atomic swaps among multiple parties
without the need for intermediaries.
It leverages adaptor signatures to facilitate secure and non-interactive swaps across different blockchain networks.

- **Cyclic**: P1 → P2 → … → Pn → P1: each participant sends exactly once and receives exactly once.
- **Atomic**: Either all spend txs complete, or all refunds fire. If any participant aborts,
  time-locked refunds guarantee no participant loses funds N-party.
- **N-party**: The proposed implementatuib scales to N participants.
- **Swap**: The implementation supports same-chain or cross-chain legs (ADA ↔ ADA, BTC ↔ ADA, BTC ↔ ADA, BTC ↔ BTC).
- Participants need only trust the cryptographic protocol — not each other

## Getting Started

1. [Presentation (PDF)](docs/IOG_Cyclic_Atomic_N-Party_Swap_Protocol.pdf)
   1. [OpenDocument Presentation](docs/IOG_Cyclic_Atomic_N-Party_Swap_Protocol.odp)
   2. [WBEM Video Presentation](docs/IOG_Cyclic_Atomic_N-Party_Swap_Protocol.webm) (by Edmund Judge)
2. [Protocol Specification](docs/protocol-spec.pdf)
3. [Software Architecture](docs/arc42/toc.md)
4. [Reference Implementation](swap-daemon/README.md)
5. [Formal Methods](formal-methods/README.md) 

---

> ### ⚠️ Important Disclaimer & Acceptance of Risk
>
> **This repository contains proof-of-concept implementations** intended to evaluate the feasibility 
> of the CANS reference implementation.
> Unless required by applicable law or agreed to in writing, software distributed under the License is distributed on an
> "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
> This reference implementation has not been thoroughly tested and audited and is not intended for production use. 
> By using this code, you acknowledge and accept all associated risks, 
> and our company disclaims any liability for damages or losses.
> See the License for the specific language governing permissions and limitations under the License.
---

## License

Copyright 2025 Input Output Global

Licensed under the [Apache License, Version 2.0](LICENSE) (the "License").
You may not use this repository except in compliance with the License.
You may obtain a copy of the License at [link] http://www.apache.org/licenses/LICENSE-2.0

---

[Contributing](CONTRIBUTING.md)

[Code of Conduct](CODE-OF-CONDUCT.md)

#### Original Contributors

- Architecture: [Luca Debiasi](mailto:luca.debiasi@iohk.io), [Andrew Sutherland](mailto:andrew.sutherland@iohk.io)
- Code Development: [Edmund Judge](mailto:edmund.judge@iohk.io)
- Formal Methods: Lucas Escot, [Mauro Jaskelioff](mailto:mauro.jaskelioff@iohk.io)
- Scientific Research: [Lukas Aumayr](mailto:lukas.aumayr@iohk.io)


