# 11. Glossary

**Adaptor Signature / Secret**: A cryptographic technique used to link multiple transactions across different blockchains. 
Atomicity is guaranteed because finalising a signature on one chain reveals a secret that allows other participants 
to claim their funds on other chains.

**Atomic**: A core protocol property ensuring that either all transfers in a swap are successfully completed, 
or all participants can recover their original funds via time-locked refunds, leaving no honest participant worse off.

**Bridges**: Interoperability solutions that typically use wrapped tokens and often rely on trusted third parties,
custodians, or complex smart contracts to manage cross-chain transfers.


**CANS Protocol (Cyclic Atomic N-party Swap)**: A trust-minimised, cross-chain interoperability protocol 
enabling multiple parties ($N \ge 2$) to exchange assets in a cyclic arrangement 
with high-assurance security and cryptographic fairness.

**Chain Monitor**: A component within the swap daemon responsible for observing on-chain events such as transaction 
confirmations, spends, and refund windows across different blockchains (e.g., Bitcoin and Cardano).
 
**Claim phase**: The stage of the protocol where participants coordinate to broadcast **spend txs**. 
It is triggered by the leader and results in the final settlement of the swap.

**Claimant (Withdrawer)**: The role a party plays when signing and broadcasting a **spend tx** 
to withdraw funds deposited by the previous party in the cycle.

**Co-signer**: A participant who contributes cryptographic material, including public keys, nonces, 
and partial signatures, to create a valid multi-signature transaction.

**Cyclic**: A topological arrangement where participants are ordered in a closed loop 
($P_0 \to P_1 \to \dots \to P_{N-1} \to P_0$). 
Each party $P_i$ sends funds to $P_{i+1}$ and receives funds from $P_{i-1}$.

**Daemon**: A background process that orchestrates swap sessions, handles peer-to-peer networking, 
manages cryptographic material, and monitors blockchain activity.

**Depositor**: The role a party plays when signing and broadcasting a **lock tx** 
to deposit funds on-chain for the next participant in the cycle.
 
**Dolos**: A gRPC-based API used for high-performance interaction with the Cardano blockchain.

**Electrum (Electrs)**: A lightweight API used for interaction with the Bitcoin blockchain, 
typically used for querying UTXOs and broadcasting transactions.
 
**Finite State Machine (FSM)**: An architectural pattern used to model the lifecycle of a swap session
as a series of discrete, well-defined states and transitions.

**Follower**: Any participant in a swap session who is not the elected leader.

**HTLC (Hash-based Time Lock Contract)**: A traditional atomic swap mechanism that relies 
on hash pre-images and staggered time-locks, often resulting in larger on-chain footprints 
and lower privacy compared to adaptor signatures.
 
**IBT (Intent-Based Trading)**: A trading paradigm where users specify desired economic outcomes ("intents") instead 
of explicit transaction paths, leaving the execution details to solvers.

**Leader ($P_{leader}$)**: A party elected at runtime (via a commit-reveal scheme) who is responsible for initiating
the Claim phase by broadcasting the first **spend tx**.

**lock barrier**: A global synchronisation point where all participants must verify that all **lock txs** 
have been confirmed on their respective blockchains before the protocol moves to the next phase.

**lock tx**: An on-chain transaction that deposits and locks a participant's funds into a multi-signature output,
which can be spent either by the claimant or by a refund transaction after a timeout.

**MuSig2**: A state-of-the-art, multi-round Schnorr-based multi-signature scheme that allows multiple parties 
to create a single joint signature that is indistinguishable from a regular signature on-chain.

**N-party**: The property of the protocol that allows it to scale to an arbitrary number of participants ($N$) 
without requiring changes to its core cryptographic structure.

**Party ($P_i$)**: An individual participant in a swap, identified by a unique index within the cyclic arrangement.

**Ports and Adapters (Hexagonal Architecture)**: A design pattern used to decouple the core protocol logic 
from external concerns like networking (TCP/IP) and blockchain APIs (Bitcoin/Cardano).

**refund tx**: An on-chain transaction that allows a depositor to reclaim their funds from a **lock tx** 
if the swap fails or if the claimant does not act within the agreed-upon refund window.

**Refunded**: The terminal state of a swap session for a participant who has successfully 
recovered their funds via a **refund tx**.

**Solver**: An entity (often automated) that finds and constructs valid multi-party cycles 
to satisfy various user intents in an intent-based trading ecosystem.

**spend tx**: An on-chain transaction used by a claimant to withdraw and take ownership of funds deposited in a **lock tx**.

**Swap Description / Descriptor**: A data structure containing all the parameters of a swap, 
including the list of participants, assets, amounts, and blockchain configurations.

**Swap Session**: A single, stateful execution of the CANS protocol managed by a daemon.

**Synchronous Settlement**: The simultaneous clearing of multiple financial obligations among several parties without
the need for a central intermediary or clearinghouse.

**Transfer ($Transfer_i$)**: The logical movement of funds from party $P_i$ to party $P_{i+1 \pmod N}$ within the swap cycle.

**W_i (Refund Window)**: The specific block height or time duration after 
which a participant's **refund tx** becomes valid on the blockchain.