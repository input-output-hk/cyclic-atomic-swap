# 1. Introduction and Goals – Business Analysis

## Executive Summary

**Cyclic Atomic N_party Swap** (CANS) work-stream aims to provide a cryptographic alternative
to traditional financial intermediaries by enabling multiple parties
to exchange multiple assets across multiple blockchains in a single, atomic operation.
Instead of relying on custodians or clearinghouses, CANS uses adaptor‑signature and multi‑signature techniques
to guarantee that either all legs of a complex trade settle simultaneously or none do,
ensuring that no honest participant is left worse off.

This capability transforms fragmented blockchain ecosystems into an interoperable liquidity network.
Markets can form directly between heterogeneous assets (e.g., BTC, ADA, ETH, Monero) without centralised exchanges,
reducing custody risk, operational overhead, and settlement latency.
Economic theory shows that decentralised trade usually decomposes into bilateral,
locally rational contracts that are globally inefficient; \
CANS overcomes this limitation by enforcing globally coordinated, synchronous settlement at the protocol level.

Strategically, this work positions IOG to supply the settlement layer for Intent‑Based Trading (IBT).
In IBT, users declare high‑level economic outcomes (“intents”) rather than low‑level transaction paths.
Solvers then construct composite, cross‑chain bundles that satisfy many intents at once.
CANS provides the all‑or‑nothing execution guarantee such bundles require, across chains and assets,
without introducing new trusted entities.
By combining adaptor‑signature‑based atomicity, formal verification of fairness and incentive properties,
and broad compatibility with standard signature schemes, the CANS workstream gives Cardano and collaborating chains
a unique capability: a universal, non‑custodial settlement primitive for multi‑asset, multi‑party,
cross‑chain intent fulfillment.

---

## From Intermediaries to Code

An atomic cross‑chain swap is a distributed coordination task where multiple parties exchange assets
across multiple blockchains—for example, trading Ada for Bitcoin for Ether.

An atomic swap protocol guarantees that

- if all parties follow the protocol, all swaps complete successfully;
- if any coalition deviates, no honest participant ends up worse off;
- and no rational group has incentive to deviate.

Traditionally implemented via hashed timelock contracts (HTLCs),
these protocols are now evolving toward more efficient cryptographic models using multi‑signature
or adaptor‑signature techniques that achieve the same atomicity with lower cost, broader compatibility,
and stronger privacy.

At first glance, this may appear to be an abstract exercise in distributed computing.
Yet its economic implications are profound: atomic swaps replace institutional trust with verifiable cryptography,
creating an infrastructure for decentralised trade among multiple, independent blockchains.
In an economy where digital assets span thousands of networks,
the ability to exchange value securely and instantly — without intermediaries — is a breakthrough on par
with the invention of clearinghouses in traditional finance.

---

## From Custodial Risk to Cryptographic Fairness

Trading lies at the heart of all economic systems.
Historically, markets have required intermediaries to guarantee fair exchange.
In the cryptocurrency ecosystem, centralised exchanges have reproduced these functions,
holding users’ coins in custody while performing trades.
This model revived all the familiar vulnerabilities associated with some kind of centralised finance:
theft, fraud, insolvency, censorship, and systemic fragility.
Indeed, some of the most catastrophic losses in the crypto industry stemmed from compromised
or poorly managed custodial exchanges.
In other words, we claim that any intermediary that is not part of a multilateral agreement among the swapping parties
is a weakness.

Atomic swaps provide an elegant alternative: a non‑custodial way to exchange assets between chains directly,
without ever surrendering control.
Early HTLC‑based protocols achieved this in principle but suffered from technical limitations.
Each blockchain had to support compatible scripting languages, identical hash functions, and time‑locking mechanisms.
These requirements excluded privacy‑preserving blockchains like Monero or Zcash, imposed high fees and storage costs,
and limited swaps to simple one‑to‑one, single‑asset exchanges.

A newer family of protocols addresses these shortcomings using adaptor signatures and related multi‑signature schemes.
In these systems, trades are triggered by the coordinated revelation of partial digital signatures
rather than the disclosure of hash pre‑images or on‑chain scripts.
When one party finalises a signature to claim an asset, that act itself releases the cryptographic information
allowing the counterparty to claim theirs—maintaining perfect atomicity.
This signature‑level coordination works across any blockchain that supports standard transaction‑verification
signatures,
such as those based on ECDSA or Schnorr.

---

## The Multi‑Asset and Multi‑Party Revolution

Recent research (e.g. generalised multi‑asset swap frameworks) shows that it is possible to atomically trade
any combination of assets, across any set of blockchains, among multiple participants.
Instead of merely exchanging one coin for another, such protocols enable trading structured portfolios:
Alice might offer some Ada leveraging to parties wishing Ada and willing to trade among Ether,
Monero, and Ripple in exchange for Bob’s Bitcoin Alice wants, in a single atomic operation.
This multi‑asset atomic swap represents a conceptual leap—transforming swaps from bilateral trades into flexible,
multi‑party marketplaces built entirely on cryptographic rules.

Equally important, these modern solutions are universal and lightweight.
They require no specialised smart‑contract languages, no third‑party coordination ledgers, and no trusted hardware.
The only cryptographic primitive each blockchain must support is its native signature system.
This means that even distinct blockchains with minimal scripting support can interoperate securely,
provided they recognise valid digital signatures—a property shared by nearly all of them.

For multi‑party configurations, the benefit becomes even clearer.
Traditional financial clearing imposes exponential complexity on N‑party arrangements: more trust, more contracts,
and higher settlement risk.
A multi‑signature atomic swap, by contrast, extends fairness to an arbitrary cycle of participants.
The result mimics what financial economists call synchronous settlement—a simultaneous clearing of obligations—with
no clearinghouse required. Every participant knows that either everyone’s transaction is valid or none count at all,
an assurance that normally demands a central institution.

The described use case is considered invaluable when users exchange and provide liquidity trading multiple tokens
among multiple parties, at lower cost, latency, and need of trust.
Even in the simpler case, when only two cryptocurrencies are involved in the swap, consider the deals involving
more than two parties. This is the most typical case in the real estate market;
when Alice buys the house signing a mortgage deal with Bob the bank, sending to Bob the deposit,
then Bob pays the price of the house to Carol, the seller.
In the example above the parties are at least three, but in countries with positive laws,
there is also a notary authority involved, mapped as NFT in blockchain terminology;
the parties federate in a single atomic financial transaction parties agreeing in different terms among each pair of
them.
Consider the importance relating to the atomic swap of the signed agreement,
more important than the amount and comparable values of any liquidity swapped.
Alice trades her future earnings and binds a mortgage (NFT) with Bob related to the ownership transfer (NFT)
from Carol to Alice, guaranteed by the simultaneous swap of Bob the Bank payment to Carol for the full price of
the estate.

Quoting Hatfield, J. W., & Milgrom [^1]

_Across economic literature, the recurring insight is that multi‑party trade in
decentralized markets typically decomposes into bilateral contracts (pairwise exchanges).
However, because each bilateral agreement reflects local incentives rather than global coordination,
the resulting network of trades is not necessarily transitive, comparable, or Pareto‑efficient—each contract
is private and context‑specific._

---

## Economic and Systemic Implications

From an economic perspective, the value of universal, multi‑party atomic swaps can be distilled into three key aspects:

1. **Efficiency and Cost Reduction**
   By removing intermediaries, settlements occur almost instantly, reducing administrative, custody,
   and compliance overhead.
   Prototype implementations already perform in under one second on commodity hardware—faster than most
   centralised exchanges can finalise trades on‑chain.
2. **Interoperability and Liquidity**
   Atomic swaps transform disconnected blockchains into a cohesive financial network.
   Markets can emerge directly between any two (or more) assets without onboarding to a centralised platform,
   fostering global liquidity and fairer price discovery.
3. **Resilience and Inclusivity**
   Because trust is distributed among participants, there is no single point of failure, corruption, or censorship.
   Individuals and institutions everywhere can transact on equal footing.
   This self‑enforcing property democratises market participation,
   particularly in jurisdictions underserved by traditional banking or subject to capital controls.

Economically, this shift parallels moving from relationship‑based finance—where trust and reputation govern
participation—to protocol‑based finance, where correctness is mathematically verifiable.
Multi‑party atomic swaps realize this ideal at the infrastructure level,
hard‑coding fairness into the fabric of digital markets.

---

## Remaining Challenges and Outlook

While the theoretical framework for universal, multi‑asset swaps is mature, real‑world deployment remains underway.
Widely‑used blockchains must adopt compatible signature schemes like Schnorr or ECDSA,
and standardisation across diverse networks is still evolving.
Moreover, usability challenges persist: coordinating multi‑party signatures securely and transparently
for non‑expert users requires advanced wallet software and clear user interfaces.
Nevertheless, the trajectory is clear.
As more blockchains upgrade their cryptographic primitives, the barrier to atomic interoperability continues to fall.
These developments point to a decentralised trading infrastructure that could rival centralised exchanges—not
through regulation or trust, but through code and proof.

---

## Multi-parties Atomic Swap solution as an enabler for the Intent-Based Trading.

Multi‑party atomic swaps—particularly those built on adaptor‑signature primitives—represent a critical step
toward a universal, non‑custodial economy.
They combine mathematical rigour with economic freedom, enabling seamless, private,
and multi‑asset exchange across any set of blockchains.
By removing intermediaries while preserving coordination, they promise to reduce systemic risk,
expand financial inclusion, and unlock a more efficient, interconnected marketplace.
In the evolution from centralised ledgers to truly decentralised value networks, multi‑party atomic swaps are the
protocol‑level foundation for global, trust-less trade.

The strategic goal of the Generalised Bitcoin (and others) multi-parties atomic swap work-stream is to enable IOG
to propose a solution that economic and DeFi literature
(partially in the references below) shows no other blockchain offers yet.
Nevertheless, Generalised multi-parties atomic swap, albeit it addresses valuable use cases, like Intent-based Trading.

### Intent‑Based Trading (IBT)

Intent‑Based Trading (IBT) is a paradigm shift in decentralised markets where participants express what economic
outcome they want to achieve — their intent — rather than how to execute it on‑chain.

Instead of submitting explicit step‑by‑step transactions (e.g. “swap 10 ADA for BTC via DEX X using route Y”),
traders broadcast intents representing desired state transitions in abstract form
(e.g. “I want to hold BTC worth 10 ADA in the next block”).
A specialised coordination layer — usually an “intent solver,” “matcher,” or “searcher” — then finds the optimal,
mutually satisfiable set of intents and constructs a composite transaction or bundle that fulfills all these intents
simultaneously and atomically.

The IBT goal is **to maximise efficiency and liquidity by matching interdependent intents among multiple agents
while preserving fairness, verifiability, and atomic completion across all participants.**

### The Problem: Trust-less Multi‑Agent Coordination

The challenge IBT faces is that realising many intents often requires
**multi‑party coordination** across **heterogeneous chains, protocols, and assets**.

For instance:

1. Alice’s intent: exchange ADA for BTC;
2. Bob’s intent: borrow ADA using ETH collateral;
3. Carol’s intent: supply ETH liquidity if she receives ADA and BTC exposure.

A rational solver could build a **single transaction bundle** that satisfies all three intents
— but only if it can guarantee atomic completion: either all commitments succeed, or none occur.
Otherwise, partial fulfillment leads to reversion risk, unfair outcomes, or exploit vectors
(e.g. front-running, sandwiching, partial‑settlement losses).

In centralised exchanges, this role is handled by trusted clearinghouses and custodians.
In decentralized IBT systems, that trust anchor must be replaced by provable cryptographic synchronisation mechanisms
— precisely the domain of multi‑party atomic swaps.

### Why Multi‑Party Atomic Swaps Are Critical for IBT

Multi‑party atomic swaps (especially those using adaptor‑signature or multi‑signature primitives)
provide the mathematical machinery to realise the execution layer of Intent‑Based Trading systems on a non‑custodial,
protocol‑agnostic basis.

| IBT Requirement                     | How Multi‑Party Atomic Swaps addresses it.                                                                                                                                         |
|-------------------------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Atomic Execution                    | Guarantee “all‑or‑nothing” fulfillment of interdependent intents across chains and assets.                                                                                         |
| Trust Minimization                  | Replace off‑chain custodians or clearing agents with cryptographic fairness — no participant can complete their leg unless others’ counterpart actions are valid.                  |
| Composability Across Blockchains    | Adaptor signatures require no smart‑contract homogeneity — they work wherever standard signature schemes exist (ECDSA, Schnorr). Thus IBT can span Bitcoin, Ethereum, Cardano, etc |
| Privacy and Scalability             | Scriptless / signature‑based swaps hide execution logic, reducing on‑chain complexity and preserving user  confidentiality.                                                        |                                                        
| Efficiency for Multi‑Party Matching | Instead of sequential bilateral settlements, one cryptographic multi‑signature operation can simultaneously clear a full cycle of intents.                                         |

In essence, multi‑party atomic swaps are the cryptographic substrate that makes intents executable.
They transform an abstract economic coordination problem
(“many agents wish to exchange value under various constraints”) into a
deterministic distributed protocol with guaranteed fairness, termination, and auditability.

---

## Conclusions

The **Cyclic Atomic N-Party Swap** work-stream builds the theoretical foundations for the settlement layer
of the pillars exposed in the Cardano 2030 Strategic Framework, included the Intents-Based Trading.

- **Adaptor‑signature** architecture ensures standardisation across ecosystems — crucial for cross‑chain intents.
- **Formal methods integration** ensures proof of fairness, incentive compatibility, and synchronous settlement.

### Alignment with [Cardano 2030 Strategic Framework](https://product.cardano.intersectmbo.org/vision/strategy-2030/)

| Cardano 2030 Vision Area | Relevant Rationale Topic | Alignment Commentary |
| ----- | ----- | ----- |
| **Interoperability and Cross‑Chain Infrastructure**<br/>[I.1. Scalability and Interoperability](https://product.cardano.intersectmbo.org/vision/strategy-2030/#i1-scalability--interoperability)<br/>_Cross-chain interoperability_ | CANS enables direct, atomic exchange of multiple assets across multiple blockchains using adaptor signatures and multi‑signature schemes. | This directly fulfills Cardano 2030’s goal to *“connect Cardano with other ecosystems through standardised, trust-less interoperability frameworks.”*<br/>CANS provides the cryptographic mechanism to make that vision concrete, creating an economic bridge layer across chains. |
| **Trustless and Decentralized Value Exchange**<br/>[A.1. High-Value Vertical](https://product.cardano.intersectmbo.org/vision/strategy-2030/#a1-high-value-verticals)<br/>_DeFi RWA Supply chain / provenance_ | CANS replaces intermediaries with verifiable cryptography, guaranteeing atomic completion without custodians.<br/> CANS is critical to address concrete DeFi cases like mortgage industry, RWA like insurance and risk trading industry and supply-chain like [Trade Finance](https://en.wikipedia.org/wiki/Trade_finance) | Cardano’s mission emphasises decentralisation and verifiable fairness as foundational design principles.<br/>CANS operationalises these principles, removing human trust dependencies and embedding fairness at the transaction layer. |
| **Financial Inclusion through Open Infrastructure**<br/>[A.2. Experience (Business & Consumer)](https://product.cardano.intersectmbo.org/vision/strategy-2030#a2-experience-business--consumer)<br/>_Invisible technology Enterprise security & compliance_<br/>[C.2. Global Engagement & Market Adoption](https://product.cardano.intersectmbo.org/vision/strategy-2030#c2-global-engagement--market-adoption)<br/>_Proactively Demonstrate Ecosystem Value_<br/>[E.1 Financial Stewardship & Tokenomics](https://use.ai/project/e34795e7-2774-4a3f-8ac7-b379fd3adbdf/2d2f246f-fa9c-4d8c-9692-73a741a85630)<br/>_Multi-Asset Treasury Managed treasury_ | CANS and Intent‑Based Trading (IBT) open global, permissionless access to synchronised, fair trading — scalable to users without access to conventional finance. | This supports the strategic goal of using blockchain as open economic infrastructure for all, expanding access to non‑custodial, low‑latency trading in regions where intermediated finance is unavailable. |
| **Sustainable and Scalable Innovation**<br/>[I.1. Scalability and Interoperability](https://product.cardano.intersectmbo.org/vision/strategy-2030/#i1-scalability--interoperability)<br/>_L1 protocol improvements L2 integration_ | CANS is a lightweight (signature‑based), efficient, and composable across chains—no complex smart‑contract logic or heavy resource use. | Cardano’s design ethos prizes scalability, efficiency, and sustainability (both technical and economic).<br/>GAS contributes to a low‑friction interoperability layer that minimises cost and energy footprints. |
| **Cardano as a Secure, Reliable, and Fair Platform**<br/>[A.2. Experience (Business & Consumer)](https://product.cardano.intersectmbo.org/vision/strategy-2030#a2-experience-business--consumer)<br/>_Enterprise security & compliance_ | CANS highlights formal verification, fairness proofs, and incentive‑compatibility analysis integral to CANS design.<br/>The rationale notes that no blockchain currently offers full generalised, multi‑party atomic swaps with protocol‑level proofs of fairness | This aligns perfectly with Cardano’s 2030 strategy of basing core protocols on formal methods, mathematical rigour, and verifiable correctness.<br/> CANS continues that scientific lineage by formally proving atomicity and fairness. This establishes Cardano’s potential competitive edge under the 2030 strategic goal of leading in cross‑chain financial infrastructure and verifiably fair DeFi primitives. |
| **Multi‑Asset and Composable DeFi Ecosystem**<br/>[I.1. Scalability and Interoperability](https://product.cardano.intersectmbo.org/vision/strategy-2030/#i1-scalability--interoperability)<br/>_Cross-chain interoperability_<br/>[A.1. High-Value Vertical](https://product.cardano.intersectmbo.org/vision/strategy-2030/#a1-high-value-verticals)<br/>_DeFi_<br/>[C.2. Global Engagement & Market Adoption](https://product.cardano.intersectmbo.org/vision/strategy-2030/#c2-global-engagement--market-adoption)<br/>_Proactively Demonstrate Ecosystem Value Localized adoption_<br/>[E.1. Financial Stewardship & Tokenomics](https://product.cardano.intersectmbo.org/vision/strategy-2030/#e1-financial-stewardship--tokenomics)<br/>_Multi-Asset Treasury_ | CANS enables universal, cross‑chain, multi‑asset liquidity and fair multi‑party settlement—forming the execution layer for Intent‑Based Trading (IBT). | Strategy 2030 calls for composable, interoperable DeFi primitives. GAS and IBT can jointly form a protocol-based financial substrate that supports complex trades, lending, liquidity, and structured portfolios across ecosystems. |
| **Governance and Ecosystem Collaboration**<br/>[C.2. Global Engagement & Market Adoption](https://product.cardano.intersectmbo.org/vision/strategy-2030/#c2-global-engagement--market-adoption)<br/>_Proactively Demonstrate Ecosystem Value Localised adoption_ | CANS calls out collaboration among Cardano and other blockchains, emphasising open standards (ECDSA, Schnorr). | This aligns with Cardano’s open collaboration and standard development goals, ensuring interoperability isn’t proprietary but ecosystem-driven. |

---

## References

* Alt, S., et al.(2021). Universal Atomic Swaps: Secure, Non‑Custodial, Multi‑Asset Exchanges via Adaptor Signatures and
  Time‑Lock Puzzles.
* Antonopoulos, A. M., & Wood, G. (2020). Mastering Ethereum: Building Smart Contracts and DApps. O’Reilly Media.
* Poelstra, A., et al.(2019). Schnorr Signatures and Scriptless Scripts. Blockstream Research.
* Herlihy, M. (2018). Atomic Cross‑Chain Swaps. Proceedings of the 2018 ACM Symposium on Principles of Distributed
  Computing (PODC ’18).
* Narayanan, A. et al.(2016). Bitcoin and Cryptocurrency Technologies. Princeton University Press.
* Hatfield, J. W., & Milgrom, P. R. (2005). “Matching with Contracts.” American Economic Review, 95(4), 913–935.

