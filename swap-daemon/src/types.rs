use std::collections::{BTreeMap, HashMap, HashSet};
use tokio::task::AbortHandle;

use secp256k1::{PublicKey, SecretKey};

use crate::networking::ConnectionPool;

/// Enumeration representing various types of wire messages used for communication.
///
/// This enum provides a structured way of encoding messages in different stages
/// of a transaction coordination protocol. Each variant represents a specific
/// message with its associated data. Below is the list of variants and their descriptions.
///
/// # Variants
///
/// - `AdaptorPoint(String)`:
///   Contains a string representation of an adaptor point.
///
/// - `LeaderCommitment([u8; 32])`:
///   Represents a leader's cryptographic commitment, stored as a 32-byte array.
///
/// - `LeaderNonce([u8; 32])`:
///   Represents a leader's nonce, stored as a 32-byte array.
///
/// - `SchnorrNonce { role: TxRole, nonce: PubNonce }`:
///   Encapsulates a Schnorr nonce with the associated transaction role (`role`)
///   and the public nonce (`nonce`).
///
/// - `PartialSignature { role: TxRole, sig: PartialSig }`:
///   Represents a partial signature along with the associated transaction role (`role`)
///   and the generated partial signature (`sig`).
///
/// - `LockTxBroadcast`:
///   Signals that the locking transaction has been broadcasted to the network.
///
/// - `SecretReveal(Secret)`:
///   Contains a revealed secret in the form of a `Secret` type.
///
/// - `SpendTxBroadcast`:
///   Indicates that the leader is notifying peers about the on-chain presence of
///   their spending transaction. This variant is commonly used to finalize a transaction.
///
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum WireMessage {
    AdaptorPoint(String),
    LeaderElectionCommitment([u8; 32]),
    LeaderElectionNonce([u8; 32]),
    SchnorrNonce { role: TxRole, nonce: PubNonce },
    PartialSignature { role: TxRole, sig: PartialSig },
    LockTxBroadcast,
    SecretReveal(Secret),
    /// Leader notifies peers their spend tx is on-chain.
    SpendTxBroadcast,
}

/// Represents the role of a transaction (TxRole) within a multi-party protocol,
/// associated with a specific participant's locked UTXO.
///
/// This enum is used to indicate the context in which a transaction is being
/// co-signed by a participant, such as refunding or spending their locked UTXO.
///
/// # Variants
///
/// * `Refund(ParticipantId)`
///     - Indicates the role of co-signing the refund transaction that corresponds to
///       the participant's locked UTXO. This is usually used to return funds to the
///       participant in case of a protocol abort or timeout.
///
/// * `Spend(ParticipantId)`
///     - Indicates the role of co-signing the spend transaction that claims the
///       participant's locked UTXO. This is typically used to fulfill the intended
///       operation, such as transferring funds.
///
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TxRole {
    /// Co-signing the refund tx for this participant's locked UTXO
    Refund(ParticipantId),
    /// Co-signing the spend tx claiming this participant's locked UTXO
    Spend(ParticipantId),
}

impl TxRole {
    /// Retrieves the `ParticipantId` associated with a `TxRole`.
    ///
    /// # Description
    /// The `participant_id` method returns the `ParticipantId` by matching
    /// the `TxRole` variant. For both `TxRole::Refund` and `TxRole::Spend`,
    /// it extracts and returns the `id` contained in the variant.
    ///
    /// # Returns
    /// * `ParticipantId` - The identifier of the participant associated
    ///   with the transaction role.
    ///
    pub fn participant_id(self) -> ParticipantId {
        match self {
            TxRole::Refund(id) | TxRole::Spend(id) => id,
        }
    }
}

pub type PubNonce = String;
pub type PartialSig = String;
pub type Secret = String;
pub type StartBlock = u32;

/// The `Envelope` struct represents a container for transporting a `WireMessage`
/// along with associated metadata such as the session ID and participant ID.
///
/// This struct is designed to support serialization and deserialization for
/// seamless transmission across systems, and it derives important traits for
/// debugging, cloning, serialization, deserialization, and comparison.
///
/// # Fields
///
/// * `session_id` - A unique identifier of type `SessionId` that represents the current session.
/// * `participant_id` - A unique identifier of type `ParticipantId` that represents the sender or recipient of the message.
/// * `msg` - A `WireMessage` that encapsulates the actual data or command being transmitted.
///
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct Envelope {
    pub session_id: SessionId,
    pub participant_id: ParticipantId,
    pub msg: WireMessage,
}

impl Envelope {
    /// Creates a new instance of the struct with the provided session ID,
    /// participant ID, and wire message.
    ///
    /// # Parameters
    ///
    /// * `session_id` - A `SessionId` representing the unique identifier for the session.
    /// * `participant_id` - A `ParticipantId` representing the unique identifier for the participant.
    /// * `msg` - A `WireMessage` containing the message associated with the session and participant.
    ///
    /// # Returns
    ///
    /// Returns an instance of `Self` populated with the provided session ID, participant ID, and message.
    pub fn new(session_id: SessionId, participant_id: ParticipantId, msg: WireMessage) -> Self {
        Self {
            session_id,
            participant_id,
            msg,
        }
    }
}

/// Represents various events that a daemon can handle or process.
///
/// # Variants
///
/// - `PeerMessage`
///     Represents an incoming message from a peer.
///
///     Fields:
///     - `envelope`: The actual message contents wrapped in an `Envelope` struct.
///     - `from`: The identifier (e.g., address or ID) of the peer sending the message.
///
/// - `ChainPoll`
///     Represents an event triggered when polling the blockchain or chain-specific target.
///
///     Fields:
///     - `session_id`: A unique session identifier associated with the polling action.
///     - `target`: The specific target of the chain poll, represented as a `ChainPollTarget`.
///
pub enum DaemonEvent {
    PeerMessage {
        envelope: Envelope,
        from: String,
    },
    ChainPoll {
        session_id: SessionId,
        target: ChainPollTarget,
    },
}

pub type SessionId = u64;
pub type ParticipantId = u8;
pub type Address = String;

/// Represents a blockchain platform.
///
/// The `Blockchain` enum defines two specific blockchain platforms,
/// Bitcoin and Cardano. This enum is `Debug`, `Clone`, `Copy`,
/// `PartialEq`, and `Eq`, allowing various common operations to be
/// performed on its values.
///
/// # Variants
///
/// - `Bitcoin`: Represents the Bitcoin blockchain.
/// - `Cardano`: Represents the Cardano blockchain.
///
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blockchain {
    Bitcoin,
    Cardano,
}

pub type Participants = BTreeMap<ParticipantId, Participant>;

///
/// Represents the states in a swap protocol workflow.
///
/// This enum tracks the lifecycle of a swap through its various stages,
/// from initialization to completion, refund, or failure. Each state
/// corresponds to a specific phase of the protocol.
///
/// # Variants
///
/// - `Initialized`:
///     The swap has been initialized and is awaiting the start of the protocol.
///
/// - `AwaitingAdaptorPoints`:
///     The protocol is waiting for participants to exchange adaptor points.
///
/// - `AwaitingLeaderCommitments`:
///     The swap is in the phase of exchanging commitments with the leader.
///
/// - `AwaitingLeaderNonces`:
///     Participants are exchanging nonces to proceed with the protocol.
///
/// - `RefundTxsSigning`:
///     Signing refund and spend transactions is in progress to ensure safety.
///
/// - `Funding`:
///     The lock transactions are being broadcasted to the blockchain.
///
/// - `AwaitingLockConfirmations`:
///     Waiting for confirmations of the lock transactions on the blockchain.
///
/// - `AwaitingSecrets`:
///     Non-leaders are waiting for all secrets to be revealed after lock confirmations.
///
/// - `AwaitingLeaderSpend`:
///     Non-leaders are monitoring the blockchain for the leader's spend transaction
///     to extract the aggregate secret.
///
/// - `Claiming`:
///     Participants are broadcasting their own adapted spend transactions to claim funds.
///
/// - `Completed`:
///     The swap has successfully completed.
///
/// - `Refunded`:
///     A refund transaction has been submitted, reverting the swap process.
///
/// - `Failed`:
///     An error occurred during the swap, causing the process to fail.
///
/// # Notes
/// - Non-leaders before moving to Claiming: non-leaders reveal their secrets immediately
///   once lock txs are confirmed, then wait in AwaitingLeaderSpend to extract the
///    aggregate secret from the leader's spend tx on chain.
/// 
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapState {
    Initialized,

    AwaitingAdaptorPoints,

    AwaitingLeaderElectionCommitments,
    AwaitingLeaderElectionNonces,

    RefundAndSpendTxsSigning,
    Funding,                   // broadcasting lock txs
    AwaitingLockConfirmations, // polling for lock tx confirmations

    AwaitingSecrets,
    AwaitingLeaderSpend, // non-leaders watching for leader's spend tx on chain
    Claiming, // broadcasting own adapted spend tx

    Completed,
    Refunded,  // refund tx submitted
    Failed,    // something went wrong
}

/// Represents a participant in a blockchain-based transaction or protocol.
///
/// The `Participant` struct contains detailed information about a participant,
/// including their identification, addresses, cryptographic keys, and transaction details.
///
/// # Fields
///
/// * `id` - The unique identifier for the participant.
/// * `blockchain` - The blockchain associated with the participant (e.g., Bitcoin, Ethereum, Cardano).
/// * `tcp_address` - The TCP address of the participant, used for communication.
/// * `target_participant` - The identifier of the participant from whom this participant is claiming an amount.
/// * `amount_locking` - The amount locked by the participant in the transaction, measured in smallest units of the blockchain's currency.
/// * `amount_claiming` - The amount the participant is claiming from the target participant.
/// * `is_me` - A boolean flag to identify if this participant represents the current user.
/// * `secp256k1_public_key` - A secp256k1 public key associated with the participant.
///   This is used for MuSig2 signature schemes and Plutus script verification.
/// * `cardano_wallet_public_key` - An Ed25519 public key used to derive Cardano receive addresses.
/// * `funding_utxo_txid` - The transaction ID of the UTXO (Unspent Transaction Output) used for funding.
/// * `funding_utxo_vout` - The output index (vout) of the UTXO used in the funding transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Participant {
    pub id: ParticipantId,
    pub blockchain: Blockchain,
    pub tcp_address: Address,
    pub target_participant: ParticipantId, //participant you're claming from
    pub amount_locking: u64,
    pub amount_claiming: u64,
    pub is_me: bool,
    pub secp256k1_public_key: String,           // secp256k1 key — MuSig2 + Plutus script verification
    pub cardano_wallet_public_key: Vec<u8>,   // Ed25519 key — used to derive Cardano receive address
    pub funding_utxo_txid: String,
    pub funding_utxo_vout: u32,
}

///
/// Represents a swap session, which is a coordinated protocol involving multiple participants
/// for exchanging assets across Bitcoin and Cardano blockchains.
///
/// # Fields
///
/// - **id**: `SessionId`
///   The unique identifier for this swap session.
///
/// - **participants**: `Participants`
///   A collection of participants involved in the swap session.
///
/// - **start_block**: `u32`
///   The Bitcoin block height at the time of session creation.
///
/// - **start_slot**: `u64`
///   The Cardano slot number at the time of session creation.
///
/// - **state**: `SwapState`
///   Current state of the swap session.
///
/// - **state_history**: `Vec<SwapState>`
///   Chronological log of all previous states of the swap session.
///
/// - **fee**: `u64`
///   The fee amount required for the swap.
///
/// - **refund_window_secs**: `u64`
///   Duration (in seconds) per hop for the refund window, converted to blocks/slots for each chain.
///
/// ## Leader Election
///
/// - **leader_nonces**: `BTreeMap<ParticipantId, [u8; 32]>`
///   Nonces submitted by participants during the leader election process.
///
/// - **leader_commitments**: `BTreeMap<ParticipantId, [u8; 32]>`
///   Commitments submitted by participants during leader election.
///
/// - **leader**: `Option<ParticipantId>`
///   The identifier of the elected leader, if any.
///
/// ## MuSig Protocol State
///
/// - **musig_sessions**: `HashMap<TxRole, MusigRuntime>`
///   Current MuSig runtime state for each transaction role.
///
/// - **schnorr_nonces**: `HashMap<ParticipantId, HashMap<TxRole, PubNonce>>`
///   Schnorr nonces associated with each participant per transaction role.
///
/// - **partial_sigs**: `HashMap<ParticipantId, HashMap<TxRole, PartialSig>>`
///   Partial Schnorr signatures provided by participants for each transaction role.
///
/// ## Transactions
///
/// - **lock_txs**: `HashMap<ParticipantId, String>`
///   Hex-encoded lock transactions submitted by participants.
///
/// - **unsigned_txs**: `HashMap<TxRole, String>`
///   Hex-encoded unsigned transactions organized by their roles.
///
/// - **signed_txs**: `HashMap<TxRole, String>`
///   Hex-encoded fully signed transactions organized by their roles.
///
/// - **adaptor_sigs**: `HashMap<TxRole, String>`
///   Hex-encoded pre-signatures (adaptor signatures) for transactions.
///
/// - **lock_txs_broadcast**: `HashSet<ParticipantId>`
///   Tracks participants who have successfully submitted their lock transactions to the blockchain.
///
/// - **confirmed_lock_txs**: `HashSet<ParticipantId>`
///   Tracks participants whose lock transactions have been confirmed on the blockchain.
///
/// ## Adaptor Secrets
///
/// - **adaptor_secrets**: `HashMap<ParticipantId, String>`
///   Hex-encoded secrets either received via messages or extracted from the blockchain.
///
/// - **adaptor_points**: `HashMap<ParticipantId, String>`
///   Points for adaptors associated with each participant.
///
/// ## Cardano Collateral
///
/// - **cardano_collaterals**: `HashMap<ParticipantId, CardanoCollateral>`
///   Collateral UTXOs for Cardano Plutus transactions, keyed by the participant who provides and signs them.
///   Each participant supplies their own collateral UTXO, used in both spend and refund transactions.
///
/// ## Connection Pool
///
/// - **connection_pool**: `ConnectionPool`
///   Persistent TCP connections to peers, reused across all broadcasts to prevent exhaustion of OS ephemeral ports
///   under high message volume.
///
#[derive(Debug)]
pub struct SwapSession {
    pub id: SessionId,
    pub participants: Participants,
    /// Bitcoin block height at session creation — the anchor from which every
    /// participant's Bitcoin refund locktime is derived
    /// ([`crate::utils::refund_locktime_btc`]). Stays a block height: `nLockTime`
    /// is enforced by consensus in block heights.
    pub start_block: u32,
    /// Cardano slot at session creation — the anchor from which every
    /// participant's Cardano refund locktime is derived
    /// ([`crate::utils::refund_locktime_cardano`]).
    ///
    /// Deliberately a **slot**, and not interchangeable with
    /// [`Self::cardano_system_start_secs`]. The derived locktime is used where
    /// the chain speaks slots: as a refund tx's `validity_start_interval`
    /// (enforced by the ledger in phase 1) and when comparing against the node's
    /// current tip slot. Only the datum's deadline is converted to POSIX
    /// milliseconds, by [`crate::utils::refund_deadline_posix_ms`], because that
    /// value is read by the Plutus script rather than by the ledger.
    ///
    /// This is per-session (where this swap began); the network start below is a
    /// per-network constant.
    pub start_slot: u64,
    pub state: SwapState,
    pub state_history: Vec<SwapState>,
    pub bitcoin_fee: u64,
    pub cardano_fee: u64,
    pub refund_window_secs: u64, // per-hop refund window in seconds; converted to blocks/slots per chain

    // leader election
    pub leader_nonces: BTreeMap<ParticipantId, [u8; 32]>,
    pub leader_commitments: BTreeMap<ParticipantId, [u8; 32]>,
    pub leader: Option<ParticipantId>,

    // musig protocol state
    pub musig_sessions: HashMap<TxRole, MusigRuntime>,
    pub schnorr_nonces: HashMap<ParticipantId, HashMap<TxRole, PubNonce>>,
    pub partial_sigs: HashMap<ParticipantId, HashMap<TxRole, PartialSig>>,

    // transactions
    pub lock_txs: HashMap<ParticipantId, String>, // hex encoded
    pub unsigned_txs: HashMap<TxRole, String>,    // hex encoded
    pub signed_txs: HashMap<TxRole, String>,      // hex encoded
    pub adaptor_sigs: HashMap<TxRole, String>,    // hex encoded pre-signatures
    pub lock_txs_broadcast: HashSet<ParticipantId>, // tracks which participants have submitted their lock tx to the chain.
    pub confirmed_lock_txs: HashSet<ParticipantId>, // tracks which participants' lock txs have been confirmed on chain.

    // adaptor secrets — received via message or extracted from chain
    pub adaptor_secrets: HashMap<ParticipantId, String>, // hex encoded Scalar
    pub adaptor_points: HashMap<ParticipantId, String>,

    /// Collateral UTXOs for Cardano Plutus txs, keyed by the participant who signs them.
    /// Each participant provides their own collateral UTXO, used for both spend and refund txs.
    pub cardano_collaterals: HashMap<ParticipantId, CardanoCollateral>,

    /// Persistent TCP connections to peers — reused across all broadcasts to avoid
    /// exhausting OS ephemeral ports under high message volume.
    pub connection_pool: ConnectionPool,
}

/// Enum representing the various targets for polling the blockchain state in a multi-party transaction protocol.
///
/// This enum is used to specify the specific transaction or state to monitor during blockchain operations.
/// Each variant corresponds to a specific type of transaction or condition that may require polling.
///
/// # Variants
///
/// - `LockTx`:
///   Monitors the status of the locking transaction for a specific participant.
///   - `participant_id`: The unique identifier of the participant associated with the locking transaction.
///
/// - `LeaderSpendTx`:
///   Monitors the spend transaction initiated by the leader of the protocol.
///   - `leader_id`: The unique identifier of the leader associated with the spend transaction.
///
/// - `RefundWindow`:
///   Polls whether the blockchain has reached the refund locktime for a specific participant.
///   This is used to determine if the participant is eligible for a refund according to the protocol's rules.
///   - `participant_id`: The unique identifier of the participant associated with this refund condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChainPollTarget {
    LockTx { participant_id: ParticipantId },
    LeaderSpendTx { leader_id: ParticipantId },
    /// Poll whether the blockchain has reached the refund locktime for this participant.
    RefundWindow { participant_id: ParticipantId },
}

/// Represents the runtime state of a MuSig (Multi-Signature) protocol instance.
///
/// This enum is used to track the progress of a MuSig operation, offering two stages:
/// `RoundOne` for initializing the operation and `RoundTwo` for completing it.
/// It also supports serialization and deserialization via `serde`.
///
/// # Variants
///
/// ## `RoundOne`
/// Represents the initial round of MuSig protocol execution.
///
/// - `all_pubkeys` - A vector containing the public keys of all parties involved in the signing.
/// - `signer_index` - The index of the current signer within the `all_pubkeys` vector.
/// - `msg` - The message to be signed, represented as a vector of bytes.
/// - `taproot_tweak` - A boolean indicating whether Taproot tweaking is applied.
/// - `sec_nonce` - A vector containing the signer's secret nonce.
///
/// ## `RoundTwo`
/// Represents the second round of MuSig protocol execution.
///
/// - `all_pubkeys` - A vector containing the public keys of all parties involved in the signing.
/// - `signer_index` - The index of the current signer within the `all_pubkeys` vector.
/// - `msg` - The message to be signed, represented as a vector of bytes.
/// - `taproot_tweak` - A boolean indicating whether Taproot tweaking is applied.
///
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MusigRuntime {
    RoundOne {
        all_pubkeys: Vec<String>,
        signer_index: usize,
        msg: Vec<u8>,
        taproot_tweak: bool,
        sec_nonce: Vec<u8>,
    },
    RoundTwo {
        all_pubkeys: Vec<String>,
        signer_index: usize,
        msg: Vec<u8>,
        taproot_tweak: bool,
    },
}

/// Represents a Cardano blockchain collateral input.
///
/// A collateral is used in Cardano to cover transaction fees or to ensure
/// a transaction is valid. It includes the transaction ID and the specific
/// index of the UTXO (Unspent Transaction Output) being used.
///
/// # Struct Fields
///
/// * `utxo_txid` - A `String` representing the unique transaction ID of the collateral input.
/// * `utxo_index` - A `u32` representing the index of the UTXO within the transaction.
///
/// # Derives
///
/// * `Debug` - Enables formatting the struct using the `{:?}` formatter.
/// * `Clone` - Generates a `clone` method to create a copy of the struct.
/// * `PartialEq` - Allows for equality comparisons (`==` and `!=`) between two instances of the struct.
/// * `Eq` - Indicates that the struct implements full equivalence.
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardanoCollateral {
    pub utxo_txid: String,
    pub utxo_index: u32,
}

/// Represents the configuration settings for a daemon process.
///
/// This structure holds various configuration parameters required to initialize
/// and operate the daemon, including network details, API keys, and flag options.
///
/// # Fields
///
/// - `tcp_address`:
///   The TCP address on which the daemon will listen for incoming connections.
///   This should be a valid IP address or domain name with a port specified (e.g., "127.0.0.1:8080").
///
/// - `bitcoin_network`:
///   The Bitcoin network configuration specifying which network (e.g., mainnet, testnet) the daemon will interact with.
///
/// - `cardano_network`:
///   The Cardano network configuration indicating which environment (e.g., mainnet, testnet) is used for Cardano blockchain interactions.
///
/// - `blockfrost_api_key`:
///   The API key for authenticating with the Blockfrost API, which is used for interacting with the Cardano blockchain.
///
/// - `validate_utxos`:
///   A boolean flag indicating whether to perform validation on unspent transaction outputs (UTXOs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonConfig {
    pub tcp_address: String,
    pub bitcoin_network: BitcoinNetwork,
    pub cardano_network: CardanoNetwork,
    pub blockfrost_api_key: String,
    pub validate_utxos: bool,
}

/// Represents the various Bitcoin networks that can be used.
///
/// # Variants
///
/// - `Mainnet`: The primary Bitcoin network used for actual transactions with real value.
/// - `Testnet4`: A testing network designed for experimentation and development, with no real monetary value.
/// - `Custom(String)`: A user-defined custom Bitcoin network represented as a `String`, allowing for non-standard or alternative configurations.
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BitcoinNetwork {
    Mainnet,
    Testnet4,
    Custom(String),
}

/// Enum representing the different Cardano networks that can be used.
///
/// # Variants
///
/// * `Mainnet` - Represents the main public Cardano blockchain network.
/// * `Preprod` - Represents the pre-production network used for testing prior to deployment.
/// * `Preview` - Represents the preview network used for testing new features and changes.
/// * `Custom` - Represents a custom or private Cardano network (e.g., local network like Dolos).
///   - `grpc_url` - The gRPC endpoint URL for the custom network.
///   - `rest_url` - The REST endpoint URL for the custom network.
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardanoNetwork {
    Mainnet,
    Preprod,
    Preview,
    /// Local/private network served by Dolos.
    ///
    /// `system_start_secs` is the network's Shelley genesis `systemStart` in
    /// POSIX seconds, which the public networks carry as constants (see
    /// [`CardanoNetwork::system_start_secs`]) but a private chain stamps afresh
    /// on every start. Read it from the indexer's `/genesis` endpoint.
    Custom {
        grpc_url: String,
        rest_url: String,
        system_start_secs: u64,
    },
}

/// Represents the cryptographic keys required for a swap transaction.
///
/// The `SwapKeys` struct contains both secp256k1 keys for MuSig2 signing
/// and Ed25519 keys for signing Cardano lock transaction inputs. These keys
/// are used to secure and validate the transaction process.
///
/// # Fields
///
/// * `secret_key` - A `SecretKey` from the secp256k1 key pair that is used for MuSig2 signing.
/// * `public_key` - A `PublicKey` from the secp256k1 key pair that corresponds to the `secret_key`.
/// * `cardano_wallet_secret_key` - A byte vector representing the Ed25519 secret key,
///   which is used to sign inputs in Cardano lock transactions.
/// * `cardano_wallet_public_key` - A byte vector representing the Ed25519 public key,
///   which corresponds to the `cardano_wallet_secret_key`.
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwapKeys {
    pub secret_key: SecretKey,              // secp256k1 — MuSig2 signing
    pub public_key: PublicKey,              // secp256k1
    pub cardano_wallet_secret_key: Vec<u8>, // Ed25519 — signs Cardano lock tx input
    pub cardano_wallet_public_key: Vec<u8>, // Ed25519
}

/// Represents the main structure of the daemon responsible for managing swap sessions,
/// handling cryptographic keys, maintaining active pollers, and keeping daemon configurations.
///
/// # Fields
///
/// * `sessions` - A `HashMap` containing active swap sessions, where the key is a `SessionId`
///   and the value is a `SwapSession`. This map is used to store and manage the lifecycle
///   of all the ongoing sessions.
///
/// * `swap_keys` - Represents a single set of private and public keys (`SwapKeys`) for the daemon.
///   These keys are likely derived from a wallet and are utilized for cryptographic operations
///   required during swap activities. Having one set ensures centralized management of keys
///   for the daemon's operations.
///
/// * `active_pollers` - A `HashMap` that manages active polling processes. The key is a tuple
///   of `SessionId` and `ChainPollTarget` (specifies the specific target to poll for a session),
///   and the value is an `AbortHandle`, which allows the daemon to stop individual pollers
///   when required.
///
/// * `config` - A `DaemonConfig` instance that encapsulates configuration details for the
///   daemon, such as runtime parameters, environment settings, and other configurable options
///   necessary for its operation.
///
pub struct Daemon {
    pub sessions: HashMap<SessionId, SwapSession>,
    pub swap_keys: SwapKeys, // Makes sense that there is just one private and public key for the daemon. This will probably come from a wallet.
    pub active_pollers: HashMap<(SessionId, ChainPollTarget), AbortHandle>,
    pub config: DaemonConfig,
}
