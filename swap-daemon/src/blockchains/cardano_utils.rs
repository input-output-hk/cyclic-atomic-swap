//! Generates a Cardano script-based address for a given network using a Plutus V3 script.
//!
//! This function creates a script address by using a predefined Plutus V3 script (`SWAP_SCRIPT_BYTES`),
//! calculates the script hash, and constructs an enterprise address based on the given network identifier.
//!
//! # Parameters
//! - `network`: A `u8` representing the network identifier.
//!     - Use [`CARDANO_TESTNET`] for the testnet.
//!     - Use [`CARDANO_MAINNET`] for the mainnet.
//!
//! # Returns
//! A `cardano_serialization_lib::Address` representing the script-based enterprise address for the specified network.
//!
//! # Notes
//! - Ensure that `SWAP_SCRIPT_BYTES` is correctly defined as the serialized bytes of a Plutus V3 script.
//! - The returned address is a non-staking enterprise address, suitable for certain use cases like smart contracts
//!   without staking requirements.
//!
//! # Errors
//! - This function assumes that `SWAP_SCRIPT_BYTES` is valid and correctly represents
//!   a Plutus V3 script. Errors in this data may result in invalid state or incorrect address generation.
//! - Use caution when supplying the `network` parameter to avoid generating an invalid
//!   network-specific address.
use blake2::{digest::consts::U32, Blake2b, Digest};
use cardano_serialization_lib::{
    BigNum, ConstrPlutusData, CostModel, Costmdls, Ed25519Signature, EnterpriseAddress, ExUnits,
    Int, Language, PlutusData, PlutusList, PlutusScript, PlutusScripts, PublicKey, Redeemer,
    RedeemerTag, Redeemers, Transaction, TransactionBody, TransactionHash, TransactionInput,
    TransactionInputs, TransactionOutputBuilder, TransactionOutputs, TransactionWitnessSet, Value,
    Vkey, Vkeywitness, Vkeywitnesses,
};
use ed25519_dalek::{Signer, SigningKey};

use serde::Deserialize;
use tracing::{error, info};
use crate::types::{CardanoNetwork, DaemonConfig, Participant, SwapKeys};

// Aiken-compiled Plutus V3 script: compiledCode from plutus.json (361 bytes).
// Compiled with Aiken v1.1.21. Validator has two spending paths:
//   Spend:  verifies schnorr(agg_pubkey, txid, sig)
//   Refund: checks validity_range.lower >= datum.refund_posix_ms, then verifies
//           schnorr(agg_pubkey, blake2b(0x01 || lock_txid), sig)
// The 0x01 prefix is the domain tag — prevents a refund sig from being replayed
// via the Spend redeemer (which signs the lock txid with no prefix).
//
// NOTE on units: validity_range.lower is POSIX milliseconds, so the datum's
// deadline must be too. A slot number there makes the timelock check vacuous.
//
// NOTE on what the signature covers: the signed message is derived from the LOCK
// tx's id only — it does not commit to the spending tx's body, so it constrains
// neither the refund tx's validity interval nor its outputs. The timelock is
// therefore enforced solely by the validity_range check above plus the ledger's
// phase-1 rule; it is NOT protected by the co-signed signature.
// Hash: afdc922d468f249b3bd5b3504a8f42bec5ac26a8f61a0e92543603e8
pub const SWAP_SCRIPT_BYTES: &[u8] = &[
    0x59, 0x01, 0x66, 0x01, 0x01, 0x00, 0x29, 0x80, 0x0a, 0xba, 0x2a, 0xba, 0x1a, 0xab, 0x9f, 0xaa,
    0xb9, 0xea, 0xab, 0x9d, 0xab, 0x9a, 0x48, 0x88, 0x88, 0x96, 0x60, 0x02, 0x64, 0x65, 0x30, 0x01,
    0x30, 0x07, 0x00, 0x19, 0x80, 0x39, 0x80, 0x40, 0x00, 0xcd, 0xc3, 0xa4, 0x00, 0x53, 0x00, 0x70,
    0x02, 0x48, 0x88, 0x96, 0x60, 0x02, 0x60, 0x04, 0x60, 0x10, 0x6e, 0xa8, 0x00, 0xe2, 0x65, 0x30,
    0x01, 0x30, 0x0c, 0x00, 0x19, 0x80, 0x61, 0x80, 0x68, 0x00, 0xcd, 0xc3, 0xa4, 0x00, 0x09, 0x11,
    0x19, 0x91, 0x2c, 0xc0, 0x04, 0xc0, 0x0c, 0x00, 0x62, 0x64, 0x64, 0xb3, 0x00, 0x13, 0x01, 0x40,
    0x02, 0x80, 0x24, 0x59, 0x01, 0x21, 0xba, 0xe3, 0x01, 0x20, 0x01, 0x30, 0x0f, 0x37, 0x54, 0x01,
    0x51, 0x59, 0x80, 0x09, 0x80, 0x40, 0x00, 0xc4, 0xc8, 0xc9, 0x66, 0x00, 0x26, 0x02, 0x80, 0x05,
    0x00, 0x48, 0xb2, 0x02, 0x43, 0x75, 0xc6, 0x02, 0x40, 0x02, 0x60, 0x1e, 0x6e, 0xa8, 0x02, 0xa2,
    0xc8, 0x06, 0x90, 0x0d, 0x0a, 0xcc, 0x00, 0x4c, 0x00, 0x4c, 0x03, 0x0d, 0xd5, 0x00, 0x14, 0x4c,
    0x96, 0x60, 0x02, 0x60, 0x04, 0x60, 0x1a, 0x6e, 0xa8, 0x02, 0x63, 0x30, 0x01, 0x37, 0x5c, 0x60,
    0x20, 0x60, 0x1c, 0x6e, 0xa8, 0x00, 0x66, 0xeb, 0x8c, 0x04, 0x0c, 0x03, 0x8d, 0xd5, 0x00, 0x24,
    0xdd, 0x71, 0x80, 0x81, 0x80, 0x71, 0xba, 0xa0, 0x09, 0x5d, 0xaa, 0x26, 0x4b, 0x30, 0x01, 0x30,
    0x08, 0x30, 0x0e, 0x37, 0x54, 0x00, 0x31, 0x59, 0x80, 0x09, 0x9b, 0x89, 0x37, 0x5a, 0x60, 0x22,
    0x60, 0x24, 0x60, 0x1e, 0x6e, 0xa8, 0x00, 0x8d, 0xd6, 0x98, 0x08, 0x98, 0x07, 0x9b, 0xaa, 0x00,
    0x18, 0xcc, 0x00, 0x4d, 0xd7, 0x18, 0x08, 0x98, 0x07, 0x9b, 0xaa, 0x00, 0x29, 0xb9, 0x43, 0x37,
    0x16, 0x90, 0x01, 0x1b, 0xae, 0x30, 0x11, 0x30, 0x0f, 0x37, 0x54, 0x00, 0xb3, 0x75, 0xc6, 0x02,
    0x26, 0x01, 0xe6, 0xea, 0x80, 0x29, 0x76, 0xa8, 0xb2, 0x01, 0xa8, 0xb2, 0x01, 0xa3, 0x01, 0x03,
    0x00, 0xe3, 0x75, 0x46, 0x02, 0x06, 0x01, 0xc6, 0xea, 0x8c, 0x04, 0x0c, 0x04, 0x4c, 0x04, 0x4c,
    0x04, 0x4c, 0x04, 0x4c, 0x04, 0x4c, 0x04, 0x4c, 0x04, 0x4c, 0x03, 0x8d, 0xd5, 0x00, 0x32, 0x01,
    0x83, 0x00, 0xf3, 0x00, 0xd3, 0x75, 0x40, 0x05, 0x16, 0x40, 0x2c, 0x60, 0x18, 0x6e, 0xa8, 0x02,
    0x06, 0x01, 0x26, 0xea, 0x80, 0x0e, 0x2c, 0x80, 0x38, 0x60, 0x0e, 0x00, 0x26, 0x00, 0x66, 0xea,
    0x80, 0x1e, 0x29, 0x34, 0x4d, 0x95, 0x90, 0x01, 0x01,
];

// Ceiling value used as placeholders before evaluate-tx runs.
// Real budgets are computed dynamically via attach_final_sig_evaluated.
const MEM_UNITS_CEILING: u64 = 14_000_000;

// Ceiling value used as placeholders before evaluate-tx runs.
// Real budgets are computed dynamically via attach_final_sig_evaluated.
const CPU_UNITS_CEILING: u64 = 10_000_000_000;

/// Fallback protocol fee parameter (Conway era, stable since Chang).
const DEFAULT_MIN_FEE_A: u64 = 44;

/// Fallback protocol fee parameters (Conway era, stable since Chang).
const DEFAULT_MIN_FEE_B: u64 = 155_381;

/// Fallback protocol fee parameters (Conway era, stable since Chang).
const DEFAULT_PRICE_MEM: f64 = 0.0577;

/// Fallback protocol fee parameters (Conway era, stable since Chang).
const DEFAULT_PRICE_STEP: f64 = 0.0000721;

// add_collateral_witness appends one Ed25519 vkey witness after attach_final_sig.
// Account for its CBOR size (~105 bytes) when computing the minimum fee.
const COLLATERAL_WITNESS_CBOR_BYTES: u64 = 105;

/// Represents the protocol parameters used in a blockchain context.
///
/// # Fields
///
/// * `cost_models` (`Costmdls`): A collection of cost models defining the
///   computational costs for various operations within the protocol.
/// * `min_fee_a` (`u64`): A constant base value representing a portion of
///   the minimum transaction fee.
/// * `min_fee_b` (`u64`): A constant additional value influencing the
///   minimum transaction fee calculation.
/// * `price_mem` (`f64`): The cost per unit of memory usage in transactions,
///   expressed as a floating-point value.
/// * `price_step` (`f64`): The cost per unit of computation step, expressed
///   as a floating-point value.
///
struct ProtocolParams {
    cost_models: Costmdls,
    min_fee_a: u64,
    min_fee_b: u64,
    price_mem: f64,
    price_step: f64,
}


/// Computes the minimum fee required for a given transaction based on the protocol parameters
/// and execution resource requirements.
///
/// # Parameters
/// - `tx`: A reference to the transaction for which the fee is being calculated. The transaction
///   should be of type `cardano_serialization_lib::Transaction`.
/// - `params`: A reference to the protocol parameters (`ProtocolParams`) that include various fee
///   constants and cost models.
/// - `mem`: The memory usage estimate (in bytes) for the transaction execution.
/// - `cpu`: The CPU steps estimate for the transaction execution.
///
/// # Returns
/// - A `u64` representing the minimum fee for the transaction, calculated as the sum of:
///   - A size-based fee component, derived from the transaction's serialized size (`tx_size`)
///     multiplied by `min_fee_a` and added to `min_fee_b`.
///   - An execution-based fee component, calculated using `price_mem` and `price_step`
///     for the corresponding memory and CPU usage.
///
fn compute_min_fee(
    tx: &cardano_serialization_lib::Transaction,
    params: &ProtocolParams,
    mem: u64,
    cpu: u64,
) -> u64 {
    let tx_size = tx.to_bytes().len() as u64 + COLLATERAL_WITNESS_CBOR_BYTES;
    let size_fee = params.min_fee_a * tx_size + params.min_fee_b;
    let exec_fee = (params.price_mem * mem as f64 + params.price_step * cpu as f64).ceil() as u64;
    size_fee + exec_fee
}

/// A constant representing the identifier for the Cardano mainnet.
pub const CARDANO_MAINNET: u8 = 1;

/// A constant representing the identifier for the Cardano testnet.
pub const CARDANO_TESTNET: u8 = 0;


/// Generates a Cardano script-based address for a given network using a Plutus V3 script.
///
/// This function creates a script address by using a predefined Plutus V3 script (`SWAP_SCRIPT_BYTES`),
/// calculates the script hash, and constructs an enterprise address based on the given network identifier.
///
/// # Parameters
/// - `network`: A `u8` representing the network identifier.
///     - Use [`CARDANO_TESTNET`] for the testnet.
///     - Use [`CARDANO_MAINNET`] for the mainnet.
///
/// # Returns
/// A `cardano_serialization_lib::Address` representing the script-based enterprise address for the specified network.
///
/// # Notes
/// - Ensure that `SWAP_SCRIPT_BYTES` is correctly defined as the serialized bytes of a Plutus V3 script.
/// - The returned address is a non-staking enterprise address, suitable for certain use cases like smart contracts
///   without staking requirements.
///
/// # Errors
/// - This function assumes that `SWAP_SCRIPT_BYTES` is valid and does not handle invalid or malformed script bytes.
/// - Ensure the `cardano_serialization_lib` and relevant types (e.g., `PlutusScript`, `EnterpriseAddress`) are correctly imported and configured in your project.
pub fn script_address(network: u8) -> cardano_serialization_lib::Address {
    let script = PlutusScript::new_v3(SWAP_SCRIPT_BYTES.to_vec());
    let script_hash = script.hash();
    EnterpriseAddress::new(
        network,
        &cardano_serialization_lib::Credential::from_scripthash(&script_hash),
    )
    .to_address()
}

/// Computes the Cardano transaction "sighash" (signature hash) using the Blake2b hashing algorithm.
///
/// # Parameters
///
/// * `tx` - A reference to a [`Transaction`] object. The `Transaction` must implement a method `body()`
///          that returns the transaction body, which in turn must provide a `to_bytes()` method to
///          serialize the body into a byte array.
///
/// # Returns
///
/// A `Vec<u8>` representing the 32-byte Blake2b hash (the sighash) of the transaction body. This hash
/// can be used as input for transaction signing or validation.
///
/// # Notes
///
/// - This function depends on the `Transaction` type and its `body` and `to_bytes` methods, which
///   must be implemented appropriately.
/// - The resulting hash is deterministic for a given transaction body.
pub fn compute_cardano_sighash(tx: &Transaction) -> Vec<u8> {
    use blake2::digest::consts::U32;
    use blake2::{Blake2b, Digest};

    let body_bytes = tx.body().to_bytes();
    let mut hasher = Blake2b::<U32>::new();
    hasher.update(&body_bytes);
    hasher.finalize().to_vec()
}


/// Builds a Cardano transaction to lock funds using a script address.
///
/// This function creates a transaction that locks a specified amount of ADA
/// using a script address and a provided aggregate public key. The transaction
/// also includes a refund mechanism that utilizes a refund slot parameter.
///
/// # Parameters
/// - `funding_utxo_txhash`: A hexadecimal string representing the transaction hash
///   of the UTXO (unspent transaction output) being used as an input.
/// - `funding_utxo_index`: Index of the UTXO in the specified transaction hash
///   to use as input.
/// - `amount`: The total amount of ADA (in lovelaces) to include in the
///   transaction. This amount includes both the funds being locked and the fee.
/// - `aggregate_pubkey`: A reference to the Musig2 aggregate public key used as
///   part of the Plutus datum to lock the transaction.
/// - `fee`: The amount (in lovelaces) to set aside as a fee for the transaction.
/// - `network`: A numeric representation of the network (e.g., testnet or
///   mainnet) used to determine the appropriate script address.
/// - `refund_deadline_posix_ms`: The earliest moment the locked UTxO may be
///   refunded, as **POSIX time in milliseconds**, encoded in the transaction
///   datum. NOT a slot number — see the note below.
///
/// # Returns
/// A `Transaction` object representing the newly constructed transaction to
/// lock funds.
///
/// # Notes
/// - This function assumes that the provided aggregate public key is valid and
///   does not perform validation.
/// - Script address generation is dependent on the network parameter. Ensure
///   that the correct network value is provided.
/// - The refund deadline goes into the datum in POSIX milliseconds because the
///   Plutus validator compares it against `Transaction.validity_range`, which the
///   ledger supplies as POSIX time. Passing a slot number makes the on-chain
///   timelock unenforceable. Use
///   [`crate::utils::refund_deadline_posix_ms`] to derive this value.
///
/// # Errors
/// - Panics if the `funding_utxo_txhash` is not a valid hexadecimal string.
/// - Panics if `refund_deadline_posix_ms` cannot be successfully parsed into a BigInt.
///
/// ```
pub fn build_lock_tx(
    funding_utxo_txhash: &str,
    funding_utxo_index: u32,
    amount: u64,
    aggregate_pubkey: &musig2::secp256k1::PublicKey,
    fee: u64,
    network: u8,
    refund_deadline_posix_ms: u64,
) -> Transaction {
    let mut inputs = TransactionInputs::new();
    inputs.add(&TransactionInput::new(
        &TransactionHash::from_hex(funding_utxo_txhash).unwrap(),
        funding_utxo_index,
    ));

    // datum: {aggregate_pubkey, refund_posix_ms}
    let xonly_bytes = aggregate_pubkey.serialize()[1..].to_vec();
    error!(
        "BUILD_LOCK_TX datum pubkey (xonly): {}",
        hex::encode(&xonly_bytes)
    );
    let mut datum_fields = PlutusList::new();
    datum_fields.add(&PlutusData::new_bytes(xonly_bytes));
    datum_fields.add(&PlutusData::new_integer(
        &cardano_serialization_lib::BigInt::from_str(&refund_deadline_posix_ms.to_string())
            .unwrap(),
    ));

    let datum = PlutusData::new_constr_plutus_data(
        &cardano_serialization_lib::ConstrPlutusData::new(&BigNum::zero(), &datum_fields),
    );

    let addr = script_address(network);
    let mut outputs = TransactionOutputs::new();
    outputs.add(
        &TransactionOutputBuilder::new()
            .with_address(&addr)
            .with_plutus_data(&datum)
            .next()
            .unwrap()
            .with_value(&Value::new(&BigNum::from(amount - fee)))
            .build()
            .unwrap(),
    );

    let body = TransactionBody::new_tx_body(&inputs, &outputs, &BigNum::from(fee));

    // lock tx signed by wallet externally — no script witness needed
    Transaction::new(&body, &TransactionWitnessSet::new(), None)
}


/// Signs a Cardano transaction using an Ed25519 wallet key and outputs the signed transaction
/// as a hexadecimal string.
///
/// # Parameters
///
/// * `unsigned_tx_hex` - A hexadecimal string representation of the unsigned Cardano transaction.
/// * `keys` - A `SwapKeys` struct containing the Cardano wallet's secret and public keys.
///
/// # Returns
///
/// Returns a hexadecimal string representation of the signed Cardano transaction.
///
/// # Errors
///
/// This function will panic if:
/// - The `unsigned_tx_hex` cannot be decoded into a valid byte array.
/// - The decoded transaction bytes cannot be parsed into a valid `Transaction`.
/// - The wallet's secret key cannot be converted into the expected format.
/// - The public key or resulting signature cannot be used to create a verification key witness.
/// - The signature or transaction body construction fails during process.
///
/// # Notes
///
/// - The transaction hash is computed using the Blake2b-256 hashing algorithm.
/// - The signing process relies on the Ed25519 cryptographic algorithm.
/// - The resulting transaction includes a `vkeywitness` to authenticate the signing process.
pub async fn sign_cardano_lock_tx(unsigned_tx_hex: &str, keys: &SwapKeys) -> String {
    let tx_bytes = hex::decode(unsigned_tx_hex).unwrap();
    let tx = Transaction::from_bytes(tx_bytes).unwrap();

    // compute tx body hash
    let mut hasher = Blake2b::<U32>::new();
    hasher.update(&tx.body().to_bytes());
    let tx_hash = hasher.finalize();

    // sign with Ed25519 wallet key
    let signing_key = SigningKey::from_bytes(
        keys.cardano_wallet_secret_key
            .as_slice()
            .try_into()
            .unwrap(),
    );
    let signature = signing_key.sign(&tx_hash);

    // attach vkey witness
    let vkey = Vkey::new(&PublicKey::from_bytes(&keys.cardano_wallet_public_key).unwrap());
    let sig = Ed25519Signature::from_bytes(signature.to_bytes().to_vec()).unwrap();
    let vkeywitness = Vkeywitness::new(&vkey, &sig);
    let mut vkeywitnesses = Vkeywitnesses::new();
    vkeywitnesses.add(&vkeywitness);

    let mut witness_set = TransactionWitnessSet::new();
    witness_set.set_vkeys(&vkeywitnesses);

    let signed_tx = Transaction::new(&tx.body(), &witness_set, None);
    hex::encode(signed_tx.to_bytes())
}

/// Constructs a Cardano transaction that spends funds from a given lock transaction output
/// and optionally includes a collateral UTXO for transaction validation.
///
/// # Parameters
/// - `participant`: A reference to the `Participant` which contains the Cardano wallet's public key.
/// - `lock_txhash`: A reference to the hash of the transaction containing the locked funds to be spent.
/// - `fee`: The transaction fee in lovelaces.
/// - `output_amount`: The amount to be transferred to the recipient in lovelaces.
/// - `collateral_utxo`: An optional tuple containing the transaction ID (as a hexadecimal string)
///   and output index of the collateral UTXO to include in the transaction for collateral purposes.
///
/// # Returns
/// - `Transaction`: The built transaction object, ready for submission to the Cardano blockchain.
///
/// # Errors
/// - If the collateral transaction hash cannot be converted from hexadecimal, the function will panic.
/// - If any part of the transaction output builder fails (e.g., building the output object),
///   the function will panic.
///
/// # Notes
///
/// - Used during signing to get the sighash.
///
pub fn build_spend_tx(
    participant: &Participant,
    lock_txhash: &TransactionHash,
    fee: u64,
    output_amount: u64,
    collateral_utxo: Option<(&str, u32)>,
) -> Transaction {
    let mut inputs = TransactionInputs::new();
    inputs.add(&TransactionInput::new(lock_txhash, 0));

    let recipient =
        cardano_address_from_ed25519(&participant.cardano_wallet_public_key, CARDANO_TESTNET);

    let mut outputs = TransactionOutputs::new();
    outputs.add(
        &TransactionOutputBuilder::new()
            .with_address(&recipient)
            .next()
            .unwrap()
            .with_value(&Value::new(&BigNum::from(output_amount)))
            .build()
            .unwrap(),
    );

    let mut body = TransactionBody::new_tx_body(&inputs, &outputs, &BigNum::from(fee));

    if let Some((txid, index)) = collateral_utxo {
        let mut collateral_inputs = TransactionInputs::new();
        collateral_inputs.add(&TransactionInput::new(
            &TransactionHash::from_hex(txid).unwrap(),
            index,
        ));
        body.set_collateral(&collateral_inputs);
    }

    Transaction::new(&body, &TransactionWitnessSet::new(), None)
}

/// Adds a collateral witness to a given transaction and returns the updated transaction as a hex-encoded string.
///
/// This function takes the transaction in its hex-encoded form, a signing key, and a verifying key.
/// It uses a provided collateral Ed25519 key to sign the transaction body hash, and creates a new
/// witness (vkeywitness) that is added to the transaction's witness set while preserving any existing
/// witnesses such as Plutus scripts or redeemers.
///
/// # Parameters
///
/// * `tx_hex` - A string slice that holds the hex-encoded transaction to which the collateral witness will be added.
/// * `signing_key` - A byte slice representing the private signing key (32 bytes).
/// * `verifying_key` - A byte slice representing the corresponding public verifying key (32 bytes).
///
/// # Returns
///
/// A `String` containing the hex-encoded updated transaction that includes the new collateral witness.
///
/// # Errors
///
/// This function will panic if:
/// - `tx_hex` cannot be successfully decoded into bytes.
/// - The decoded transaction bytes do not represent a valid transaction.
/// - Conversion of `signing_key` to a fixed-size byte array fails.
/// - Public or private keys cannot be converted or are invalid.
/// - The generated signature cannot be parsed into a valid `Ed25519Signature`.
///
/// # Notes
///
/// - The function assumes that the provided `tx_hex` is valid and that the `signing_key` and `verifying_key` correspond to the same key pair.
/// - If the transaction has existing witnesses (e.g., Plutus scripts or redeemers), they will be preserved in the updated transaction.
/// - Called after attach_final_sig to add the collateral signer's witness.
pub fn add_collateral_witness(tx_hex: &str, signing_key: &[u8], verifying_key: &[u8]) -> String {
    let tx_bytes = hex::decode(tx_hex).unwrap();
    let tx = Transaction::from_bytes(tx_bytes).unwrap();

    // sign tx body hash with collateral Ed25519 key
    let mut hasher = Blake2b::<U32>::new();
    hasher.update(&tx.body().to_bytes());
    let tx_hash = hasher.finalize();

    let signing_key_bytes: [u8; 32] = signing_key.try_into().unwrap();
    let collateral_signing_key = SigningKey::from_bytes(&signing_key_bytes);
    let signature = collateral_signing_key.sign(&tx_hash);

    let vkey = Vkey::new(&PublicKey::from_bytes(verifying_key).unwrap());
    let sig = Ed25519Signature::from_bytes(signature.to_bytes().to_vec()).unwrap();
    let vkeywitness = Vkeywitness::new(&vkey, &sig);

    // preserve existing witness set contents (Plutus script + redeemer)
    let existing = tx.witness_set();
    let mut new_witness = TransactionWitnessSet::new();

    let mut vkeywitnesses = Vkeywitnesses::new();
    if let Some(existing_vkeys) = existing.vkeys() {
        for i in 0..existing_vkeys.len() {
            vkeywitnesses.add(&existing_vkeys.get(i));
        }
    }
    vkeywitnesses.add(&vkeywitness);
    new_witness.set_vkeys(&vkeywitnesses);

    if let Some(scripts) = existing.plutus_scripts() {
        new_witness.set_plutus_scripts(&scripts);
    }
    if let Some(redeemers) = existing.redeemers() {
        new_witness.set_redeemers(&redeemers);
    }

    let signed_tx = Transaction::new(&tx.body(), &new_witness, None);
    hex::encode(signed_tx.to_bytes())
}

/// Generates a Cardano address from an Ed25519 public key.
///
/// # Parameters
///
/// * `wallet_public_key` - A byte slice representing the Ed25519 public key of the wallet.
/// * `network` - A `u8` value representing the network identifier (e.g., 1 for mainnet, or 0 for testnet).
///
/// # Returns
///
/// * A `cardano_serialization_lib::Address` object representing the generated Cardano address.
///
/// # Panics
///
/// This function will panic if:
/// * The provided `wallet_public_key` cannot be converted into a valid `PublicKey`.
///
/// # Notes
///
/// The generated address is of type `EnterpriseAddress`, which is a simple address type
/// without delegation to a stake pool.
fn cardano_address_from_ed25519(
    wallet_public_key: &[u8],
    network: u8,
) -> cardano_serialization_lib::Address {
    let vk = PublicKey::from_bytes(wallet_public_key).unwrap();
    let vk_hash = vk.hash();
    EnterpriseAddress::new(
        network,
        &cardano_serialization_lib::Credential::from_keyhash(&vk_hash),
    )
    .to_address()
}

/// Constructs a multi-output transfer transaction.
///
/// This function creates a Cardano transaction that transfers funds from a single
/// input to multiple outputs, deducting a specified transaction fee. The output
/// addresses are generated from the provided Ed25519 public keys and network identifier.
///
/// # Parameters
///
/// - `input_txid`: A string representing the transaction ID of the input being spent.
/// - `input_vout`: The index of the output in the referenced transaction to be used as input.
/// - `outputs`: A slice of tuples where each tuple consists of:
///   - A byte slice (`&[u8]`) representing the recipient's Ed25519 public key.
///   - A 64-bit unsigned integer (`u64`) specifying the amount to transfer to the corresponding recipient.
/// - `fee`: A 64-bit unsigned integer (`u64`) specifying the amount to be deducted as a transaction fee.
/// - `network`: An 8-bit unsigned integer (`u8`) representing the Cardano network identifier
///   (e.g., 1 for mainnet or 0 for testnet).
///
/// # Returns
///
/// This function returns a `Transaction` object that represents the constructed
/// transfer transaction. The returned transaction includes all specified inputs, outputs,
/// and the computed fee.
///
/// # Panics
///
/// - The function will panic if:
///   - The given `input_txid` is not a valid transaction hash in hexadecimal format.
///   - Any of the `outputs` tuples fail to generate a valid Cardano address using the
///     provided Ed25519 public key and network.
///   - Address generation or transaction body construction encounters an unexpected error.
///
/// # Notes
///
/// - Ensure the `outputs` array is non-empty and all amounts are non-zero.
/// - The caller is responsible for ensuring that the input's total value exceeds
///   the total value of outputs plus the specified fee.
/// - This function does not attach any witnesses to the transaction; signing the
///   transaction must be handled separately.
/// - ADA transfer: 1 input → N wallet outputs.
//    Each entry in `outputs` is (verifying_key, lovelace_amount).
//    Used in test setup to split a genesis UTXO into multiple funding/collateral outputs.
pub fn build_multi_output_transfer_tx(
    input_txid: &str,
    input_vout: u32,
    outputs: &[(&[u8], u64)],
    fee: u64,
    network: u8,
) -> Transaction {
    let mut inputs = TransactionInputs::new();
    inputs.add(&TransactionInput::new(
        &TransactionHash::from_hex(input_txid).unwrap(),
        input_vout,
    ));

    let mut tx_outputs = TransactionOutputs::new();
    for (wallet_key, amount) in outputs {
        let addr = cardano_address_from_ed25519(wallet_key, network);
        tx_outputs.add(
            &TransactionOutputBuilder::new()
                .with_address(&addr)
                .next()
                .unwrap()
                .with_value(&Value::new(&BigNum::from(*amount)))
                .build()
                .unwrap(),
        );
    }

    let body = TransactionBody::new_tx_body(&inputs, &tx_outputs, &BigNum::from(fee));
    Transaction::new(&body, &TransactionWitnessSet::new(), None)
}

/// Builds a simple Cardano transfer transaction with one input, one receiver output, and one change output.
///
/// # Parameters
///
/// * `input_txid` - A string slice representing the transaction ID (TxId) of the input being spent.
/// * `input_vout` - An unsigned 32-bit integer indicating the index of the input within its transaction outputs array.
/// * `receiver_wallet_key` - A byte slice representing the Ed25519 public key of the receiver's wallet.
/// * `receiver_amount` - An unsigned 64-bit integer specifying the amount to send to the receiver in Lovelaces.
/// * `change_wallet_key` - A byte slice representing the Ed25519 public key of the sender's (change) wallet.
/// * `change_amount` - An unsigned 64-bit integer specifying the amount to allocate as change back to the sender's wallet, in Lovelaces.
/// * `fee` - An unsigned 64-bit integer representing the transaction fee in Lovelaces.
/// * `network` - An unsigned 8-bit integer specifying the network ID (e.g., 1 for Mainnet, or 0 for Testnet).
///
/// # Returns
///
/// Returns a constructed `Transaction` object representing the Cardano transaction, ready to be signed and submitted to the blockchain.
///
/// # Panics
///
/// This function will panic if:
/// * The `input_txid` cannot be parsed as a valid hexadecimal string.
/// * Any of the `TransactionInput` or `TransactionOutput` operations fail internally.
///
/// # Notes
///
/// - Simple ADA transfer: 1 input → 2 wallet outputs (receiver + change back to sender).
/// - Used in test setup to fund a participant from the genesis wallet.
pub fn build_simple_transfer_tx(
    input_txid: &str,
    input_vout: u32,
    receiver_wallet_key: &[u8],
    receiver_amount: u64,
    change_wallet_key: &[u8],
    change_amount: u64,
    fee: u64,
    network: u8,
) -> Transaction {
    let mut inputs = TransactionInputs::new();
    inputs.add(&TransactionInput::new(
        &TransactionHash::from_hex(input_txid).unwrap(),
        input_vout,
    ));

    let receiver_addr = cardano_address_from_ed25519(receiver_wallet_key, network);
    let change_addr = cardano_address_from_ed25519(change_wallet_key, network);

    let mut outputs = TransactionOutputs::new();
    outputs.add(
        &TransactionOutputBuilder::new()
            .with_address(&receiver_addr)
            .next()
            .unwrap()
            .with_value(&Value::new(&BigNum::from(receiver_amount)))
            .build()
            .unwrap(),
    );
    outputs.add(
        &TransactionOutputBuilder::new()
            .with_address(&change_addr)
            .next()
            .unwrap()
            .with_value(&Value::new(&BigNum::from(change_amount)))
            .build()
            .unwrap(),
    );

    let body = TransactionBody::new_tx_body(&inputs, &outputs, &BigNum::from(fee));
    Transaction::new(&body, &TransactionWitnessSet::new(), None)
}

/// Builds a refund transaction for a participant in a Cardano smart contract.
///
/// This function creates a transaction that refunds a participant's locked amount
/// from a previously locked transaction at a script address. The refunded amount
/// accounts for transaction fees and any collateral provided.
///
/// # Parameters
///
/// * `participant` - A reference to a `Participant` struct holding details about
///   the participant, such as their Cardano wallet public key and the amount locked.
/// * `lock_txhash` - A reference to a `TransactionHash` representing the hash
///   of the previously locked transaction.
/// * `refund_slot` - The blockchain slot at which this refund transaction becomes valid.
/// * `fee` - The fee deducted for executing the transaction.
/// * `collateral_utxo` - An optional tuple specifying a collateral input in the form
///   of a tuple (`&str`, `u32`), where the first element is the transaction ID in hex format
///   and the second element is the index of the UTXO.
///
/// # Returns
///
/// Returns a new `Transaction` object.
///
/// # Errors
///
/// - This function will panic if:
///   - Any provided transaction hash or collateral transaction ID is invalid.
///   - Address building or transaction building fails due to incorrect inputs.
///
/// # Notes
///
/// - Refund tx — reclaims funds after timeout using MuSig2 signature.
/// - Collateral is required because the lock UTxO is at a Plutus script address.
pub fn build_refund_tx(
    participant: &Participant,
    lock_txhash: &cardano_serialization_lib::TransactionHash,
    refund_slot: u64,
    fee: u64,
    collateral_utxo: Option<(&str, u32)>,
) -> Transaction {
    let mut inputs = TransactionInputs::new();
    inputs.add(&TransactionInput::new(lock_txhash, 0));

    let recipient =
        cardano_address_from_ed25519(&participant.cardano_wallet_public_key, CARDANO_TESTNET);

    let mut outputs = TransactionOutputs::new();
    // The lock tx deposited (amount_locking - fee) at the script address.
    // This refund tx spends that input, so output = input_value - refund_fee
    //                                              = (amount_locking - fee) - fee
    //                                              = amount_locking - 2*fee
    let output_amount = participant.amount_locking - 2 * fee;
    outputs.add(
        &TransactionOutputBuilder::new()
            .with_address(&recipient)
            .next()
            .unwrap()
            .with_value(&Value::new(&BigNum::from(output_amount)))
            .build()
            .unwrap(),
    );

    let mut body = TransactionBody::new_tx_body(&inputs, &outputs, &BigNum::from(fee));
    body.set_validity_start_interval_bignum(&BigNum::from(refund_slot));

    if let Some((txid, index)) = collateral_utxo {
        let mut collateral_inputs = TransactionInputs::new();
        collateral_inputs.add(&TransactionInput::new(
            &TransactionHash::from_hex(txid).unwrap(),
            index,
        ));
        body.set_collateral(&collateral_inputs);
    }

    Transaction::new(&body, &TransactionWitnessSet::new(), None)
}

/// Hardcoded Plutus V3 cost model — 251 entries fetched from Blockfrost on 2026-04-16.
/// Used as a fallback when the network fetch fails.
fn plutus_v3_cost_model_hardcoded() -> Costmdls {
    let values: Vec<i64> = vec![
        100788, 420, 1, 1, 1000, 173, 0, 1, 1000, 59957, 4, 1, 11183, 32, 201305, 8356, 4, 16000,
        100, 16000, 100, 16000, 100, 16000, 100, 16000, 100, 16000, 100, 100, 100, 16000, 100,
        94375, 32, 132994, 32, 61462, 4, 72010, 178, 0, 1, 22151, 32, 91189, 769, 4, 2, 85848,
        123203, 7305, -900, 1716, 549, 57, 85848, 0, 1, 1, 1000, 42921, 4, 2, 24548, 29498, 38, 1,
        898148, 27279, 1, 51775, 558, 1, 39184, 1000, 60594, 1, 141895, 32, 83150, 32, 15299, 32,
        76049, 1, 13169, 4, 22100, 10, 28999, 74, 1, 28999, 74, 1, 43285, 552, 1, 44749, 541, 1,
        33852, 32, 68246, 32, 72362, 32, 7243, 32, 7391, 32, 11546, 32, 85848, 123203, 7305, -900,
        1716, 549, 57, 85848, 0, 1, 90434, 519, 0, 1, 74433, 32, 85848, 123203, 7305, -900, 1716,
        549, 57, 85848, 0, 1, 1, 85848, 123203, 7305, -900, 1716, 549, 57, 85848, 0, 1, 955506,
        213312, 0, 2, 270652, 22588, 4, 1457325, 64566, 4, 20467, 1, 4, 0, 141992, 32, 100788, 420,
        1, 1, 81663, 32, 59498, 32, 20142, 32, 24588, 32, 20744, 32, 25933, 32, 24623, 32,
        43053543, 10, 53384111, 14333, 10, 43574283, 26308, 10, 16000, 100, 16000, 100, 962335, 18,
        2780678, 6, 442008, 1, 52538055, 3756, 18, 267929, 18, 76433006, 8868, 18, 52948122, 18,
        1995836, 36, 3227919, 12, 901022, 1, 166917843, 4307, 36, 284546, 36, 158221314, 26549, 36,
        74698472, 36, 333849714, 1, 254006273, 72, 2174038, 72, 2261318, 64571, 4, 207616, 8310, 4,
        1293828, 28716, 63, 0, 1, 1006041, 43623, 251, 0, 1,
    ];
    costmdls_from_values(&values)
}

/// Constructs a `Costmdls` object from a slice of integer values.
///
/// This function initializes a `CostModel` instance, sets the cost model values
/// based on the provided integers, and inserts the configured cost model into a
/// `Costmdls` object associated with the Plutus V3 language.
///
/// # Parameters
/// - `values`: A reference to a slice of `i64` integers representing cost model values.
///   Each value is mapped to an index in the cost model.
///
/// # Returns
/// - Returns a `Costmdls` object populated with the cost model for the Plutus V3 language.
///
/// # Panics
/// - Panics if setting a value in the `CostModel` fails due to an internal error
///   (e.g., invalid index).
///
fn costmdls_from_values(values: &[i64]) -> Costmdls {
    let mut cost_model = CostModel::new();
    for (i, &v) in values.iter().enumerate() {
        let int_val = if v >= 0 {
            Int::new(&BigNum::from(v as u64))
        } else {
            Int::new_negative(&BigNum::from((-v) as u64))
        };
        cost_model.set(i, &int_val).unwrap();
    }
    let mut costmdls = Costmdls::new();
    costmdls.insert(&Language::new_plutus_v3(), &cost_model);
    costmdls
}

/// Fetches the protocol parameters from the network or uses hardcoded defaults as a fallback.
///
/// This asynchronous function queries the appropriate Cardano network endpoint, retrieves the
/// latest epoch protocol parameters, and parses them into a `ProtocolParams` struct. If the
/// network call fails or the response is invalid, it falls back to hardcoded default values.
///
/// # Parameters
///
/// * `config` - A reference to a [`DaemonConfig`] instance, which contains configuration
///   information including the Cardano network type and API keys.
///
/// # Returns
///
/// An instance of [`ProtocolParams`] containing the processed protocol parameters.
///
/// # Network Behavior
///
/// - Depending on the network type specified in `config.cardano_network`, the appropriate
///   endpoint URL is constructed:
///     - For `CardanoNetwork::Preprod`, `Preview`, or `Mainnet`, it uses the blockfrost API with
///       the corresponding API key from the configuration.
///     - For `CardanoNetwork::Custom`, it uses a custom REST URL specified in the configuration.
/// - A GET request is sent to the constructed URL.
/// - On success:
///     - Extracts and parses protocol parameters (`min_fee_a`, `min_fee_b`, `price_mem`,
///       `price_step`, and `cost_models`).
///     - Logs key information and returns a populated `ProtocolParams`.
/// - On failure:
///     - Logs the error and falls back to hardcoded default values for `ProtocolParams`:
///         - `min_fee_a` and `min_fee_b` use `DEFAULT_MIN_FEE_A` and `DEFAULT_MIN_FEE_B`.
///         - `price_mem` and `price_step` use `DEFAULT_PRICE_MEM` and `DEFAULT_PRICE_STEP`.
///         - `cost_models` uses `plutus_v3_cost_model_hardcoded()`.
///
/// # Logging
///
/// - Logs errors, warnings, or successful actions with detailed context, including HTTP status
///   codes or fallback metadata.
///
/// # Errors
///
/// - Logs if the network request fails or the response is invalid.
/// - Recovers gracefully by falling back to hardcoded default parameters.
///
/// # Notes

// - Uses cost_models_raw.PlutusV3 — an already-ordered integer array — rather than
//   cost_models.PlutusV3 (a named map whose key order varies by implementation).
// - Falls back to hardcoded defaults on any failure.
async fn fetch_protocol_params(config: &DaemonConfig) -> ProtocolParams {
    #[derive(Deserialize)]
    struct EpochParams {
        cost_models_raw: CostModelsRaw,
        min_fee_a: Option<u64>,
        min_fee_b: Option<u64>,
        price_mem: Option<String>,
        price_step: Option<String>,
    }
    #[derive(Deserialize)]
    struct CostModelsRaw {
        #[serde(rename = "PlutusV3")]
        plutus_v3: Vec<i64>,
    }

    let (url, api_key) = match &config.cardano_network {
        CardanoNetwork::Preprod | CardanoNetwork::Preview | CardanoNetwork::Mainnet => (
            format!(
                "{}/api/v0/epochs/latest/parameters",
                config.cardano_network.blockfrost_base_url()
            ),
            Some(config.blockfrost_api_key.as_str()),
        ),
        CardanoNetwork::Custom { rest_url, .. } => {
            (format!("{}/epochs/latest/parameters", rest_url), None)
        }
    };

    let result: Option<ProtocolParams> = async {
        let mut req = reqwest::Client::new().get(&url);
        if let Some(key) = api_key {
            req = req.header("project_id", key);
        }
        let resp = req.send().await.ok()?;
        if !resp.status().is_success() {
            error!(
                "fetch_protocol_params: {} — {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            );
            return None;
        }
        let p: EpochParams = resp.json().await.ok()?;
        let cost_models = costmdls_from_values(&p.cost_models_raw.plutus_v3);
        let min_fee_a = p.min_fee_a.unwrap_or(DEFAULT_MIN_FEE_A);
        let min_fee_b = p.min_fee_b.unwrap_or(DEFAULT_MIN_FEE_B);
        let price_mem = p.price_mem.as_deref().and_then(|s| s.parse().ok()).unwrap_or(DEFAULT_PRICE_MEM);
        let price_step = p.price_step.as_deref().and_then(|s| s.parse().ok()).unwrap_or(DEFAULT_PRICE_STEP);
        info!(
            "fetch_protocol_params: {} PlutusV3 entries, min_fee_a={min_fee_a}, min_fee_b={min_fee_b}",
            p.cost_models_raw.plutus_v3.len()
        );
        Some(ProtocolParams { cost_models, min_fee_a, min_fee_b, price_mem, price_step })
    }
    .await;

    result.unwrap_or_else(|| {
        error!("fetch_protocol_params: using hardcoded fallback");
        ProtocolParams {
            cost_models: plutus_v3_cost_model_hardcoded(),
            min_fee_a: DEFAULT_MIN_FEE_A,
            min_fee_b: DEFAULT_MIN_FEE_B,
            price_mem: DEFAULT_PRICE_MEM,
            price_step: DEFAULT_PRICE_STEP,
        }
    })
}

/// Evaluate the memory and CPU units for the redeemer associated with a given transaction hex string.
///
/// This function interacts with the Blockfrost API or Ogmios through the provided configuration to evaluate
/// the required computational resources for a transaction redeemer. If the given `DaemonConfig` specifies
/// a custom network, the function returns ceiling values (`MEM_UNITS_CEILING` and `CPU_UNITS_CEILING`)
/// as the evaluation is only supported for Preprod, Preview, or Mainnet networks.
///
/// # Parameters
///
/// - `config`: A reference to the [`DaemonConfig`] instance containing network configurations, such as
///   the Cardano network and Blockfrost API key.
/// - `tx_hex`: A hexadecimal string representation of the transaction to be evaluated.
///
/// # Returns
///
/// A tuple `(u64, u64)` representing:
/// - Memory units (as a `u64` value).
/// - CPU units (as a `u64` value).
///
/// In case of a failure (e.g., an invalid response, network error, or an unsupported network type),
/// the function returns default ceiling values (`MEM_UNITS_CEILING`, `CPU_UNITS_CEILING`).
///
/// # Logging
///
/// The function logs the following:
/// - `info`: When the evaluation is successful, including the evaluated `mem` and `cpu` values.
/// - `error`: When evaluation fails, including the HTTP status code and response details (if available),
///   or when default ceiling values are used as a fallback.
///
/// # Notes
///
/// - The function relies on the `Blockfrost` API endpoint that wraps an Ogmios evaluation service.
///   Ensure that the provided API key has the necessary permissions.
///
/// - The `spend:0` field is used in the response to extract memory and CPU. Adjust this logic
///   if the API response format changes in the future.
///
/// - `MEM_UNITS_CEILING` and `CPU_UNITS_CEILING` are assumed to be global constants,
///   representing the fallback default values.
///
/// - Custom (Dolos) networks do not expose a REST evaluate endpoint; ceiling values
//    are used there, matching the same fallback behaviour as for spend transactions.
///
async fn evaluate_redeemer_units(config: &DaemonConfig, tx_hex: &str) -> (u64, u64) {
    let (base_url, api_key) = match &config.cardano_network {
        CardanoNetwork::Preprod | CardanoNetwork::Preview | CardanoNetwork::Mainnet => (
            config.cardano_network.blockfrost_base_url(),
            config.blockfrost_api_key.as_str(),
        ),
        CardanoNetwork::Custom { .. } => return (MEM_UNITS_CEILING, CPU_UNITS_CEILING),
    };

    let result: Option<(u64, u64)> = async {
        let tx_bytes = hex::decode(tx_hex).ok()?;
            let url = format!("{}/api/v0/utils/txs/evaluate", base_url);
        let resp = reqwest::Client::new()
            .post(&url)
            .header("project_id", api_key)
            .header("content-type", "application/cbor")
            .body(tx_bytes)
            .send()
            .await
            .ok()?;

        if !resp.status().is_success() {
            error!(
                "evaluate-tx: {} — {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            );
            return None;
        }

        let json: serde_json::Value = resp.json().await.ok()?;
        // Blockfrost wraps the Ogmios response: {"result":{"EvaluationResult":{"spend:0":{...}}}}
        let spend = json
            .get("result")?
            .get("EvaluationResult")?
            .get("spend:0")?;
        let mem = spend.get("memory")?.as_u64()?;
        let cpu = spend.get("steps").or_else(|| spend.get("cpu"))?.as_u64()?;
        info!("evaluate-tx: mem={mem}, cpu={cpu}");
        Some((mem, cpu))
    }
    .await;

    result.unwrap_or_else(|| {
        error!("evaluate-tx: failed, using ceiling values");
        (MEM_UNITS_CEILING, CPU_UNITS_CEILING)
    })
}


/// Attaches a final Plutus signature, updates transaction outputs, and constructs
/// a fully signed transaction in hexadecimal format with updated details.
///
/// # Parameters
/// - `unsigned_tx_hex`: The hexadecimal string representation of the unsigned transaction.
/// - `final_sig`: The final signature or cryptographic proof (Plutus-compatible) as a byte array.
/// - `mem`: The memory allocation (execution limit) for the Plutus script in the transaction.
/// - `cpu`: The CPU allocation (execution limit) for the Plutus script in the transaction.
/// - `fee`: The updated transaction fee (in lovelace) to be imposed on the signed transaction.
/// - `cost_models`: A reference to the cost models used for Plutus script evaluation.
/// - `is_refund`: A boolean flag indicating whether the transaction is for a refund or a standard operation.
///
/// # Returns
/// Returns a string containing the hexadecimal representation of the fully signed transaction with
/// the final signature and updated fee/outputs.
///
/// # Panics
/// This function may panic in the following cases:
/// - If the `unsigned_tx_hex` cannot be decoded into valid bytes.
/// - If the transaction deserialization or manipulation fails.
/// - If parsing `fee`, `original_fee`, or `original_output` values from strings into integers fails.
///
/// # Notes
/// - This function is specific to the Cardano blockchain and assumes Plutus-compatible
///   data structures and scripts.
/// - Ensure the `SWAP_SCRIPT_BYTES` constant is defined and corresponds to the intended Plutus script.
fn attach_final_sig_with_units(
    unsigned_tx_hex: &str,
    final_sig: &[u8],
    mem: u64,
    cpu: u64,
    fee: u64,
    cost_models: &Costmdls,
    is_refund: bool,
) -> String {
    let unsigned_tx =
        cardano_serialization_lib::Transaction::from_bytes(hex::decode(unsigned_tx_hex).unwrap())
            .unwrap();

    let input = unsigned_tx.body().inputs().get(0);
    let msg_bytes = input.transaction_id().to_bytes();
    info!("attach_final_sig sig: {}", hex::encode(final_sig));
    info!("attach_final_sig msg: {}", hex::encode(&msg_bytes));

    let mut redeemer_fields = PlutusList::new();
    redeemer_fields.add(&PlutusData::new_bytes(final_sig.to_vec()));
    let constr_index = if is_refund { BigNum::one() } else { BigNum::zero() };
    let redeemer_data = PlutusData::new_constr_plutus_data(&ConstrPlutusData::new(
        &constr_index,
        &redeemer_fields,
    ));

    let redeemer = Redeemer::new(
        &RedeemerTag::new_spend(),
        &BigNum::zero(),
        &redeemer_data,
        &ExUnits::new(&BigNum::from(mem), &BigNum::from(cpu)),
    );

    let mut redeemers = Redeemers::new();
    redeemers.add(&redeemer);

    let script = PlutusScript::new_v3(SWAP_SCRIPT_BYTES.to_vec());
    let mut plutus_scripts = PlutusScripts::new();
    plutus_scripts.add(&script);

    // script_data_hash = Blake2b256(redeemers || datums || language_views)
    // required for phase-1 validation of any Plutus tx
    let script_data_hash =
        cardano_serialization_lib::hash_script_data(&redeemers, cost_models, None);

    // Recompute output so that output + fee = original output + original fee (total input value).
    let original_fee: u64 = unsigned_tx.body().fee().to_str().parse().unwrap();
    let original_output: u64 = unsigned_tx.body().outputs().get(0).amount().coin().to_str().parse().unwrap();
    let new_output = original_output + original_fee - fee;
    let recipient = unsigned_tx.body().outputs().get(0).address();

    let mut new_outputs = TransactionOutputs::new();
    new_outputs.add(
        &TransactionOutputBuilder::new()
            .with_address(&recipient)
            .next()
            .unwrap()
            .with_value(&Value::new(&BigNum::from(new_output)))
            .build()
            .unwrap(),
    );

    let mut body = TransactionBody::new_tx_body(
        &unsigned_tx.body().inputs(),
        &new_outputs,
        &BigNum::from(fee),
    );
    if let Some(collateral) = unsigned_tx.body().collateral() {
        body.set_collateral(&collateral);
    }
    if let Some(slot) = unsigned_tx.body().validity_start_interval_bignum() {
        body.set_validity_start_interval_bignum(&slot);
    }
    body.set_script_data_hash(&script_data_hash);

    let mut witness_set = TransactionWitnessSet::new();
    witness_set.set_plutus_scripts(&plutus_scripts);
    witness_set.set_redeemers(&redeemers);

    let signed_tx = cardano_serialization_lib::Transaction::new(&body, &witness_set, None);
    hex::encode(signed_tx.to_bytes())
}

/// Attaches a final signature to an unsigned Cardano transaction, calculates appropriate fees,
/// and finalizes the transaction ready for broadcasting to the blockchain.
///
/// # Parameters
///
/// * `unsigned_tx_hex` - A string slice containing the hexadecimal representation of the unsigned transaction.
/// * `final_sig` - A byte slice containing the final signature to attach to the transaction.
/// * `config` - A reference to a `DaemonConfig` object holding protocol configurations.
/// * `cardano_fee` - A placeholder fee (in lovelace) provided as input for initial computations.
/// * `is_refund` - A boolean flag indicating whether the transaction is a refund.
///
/// # Returns
///
/// A `String` containing the hexadecimal representation of the finalized transaction ready for submission.
///
/// # Workflow
///
/// 1. Fetch the protocol parameters using the provided `DaemonConfig`.
/// 2. Create a draft transaction using ceiling values for memory units, CPU units, and a placeholder fee.
/// 3. Evaluate the memory and CPU usage of the draft transaction using the Cardano network.
/// 4. Create a second transaction with the evaluated memory and CPU units and the provided `cardano_fee` as
///    a placeholder to ensure accurate CBOR encoding of the fee field.
/// 5. Compute the exact minimum fee for the transaction using the evaluated memory and CPU units.
/// 6. Construct the finalized transaction by attaching the final signature and the computed fee.
/// 7. Return the hexadecimal representation of the finalized transaction.
///
/// # Logging
///
/// Logs the memory units, CPU units, and the calculated minimum fee for the finalized transaction.
///
/// # Errors
///
/// This function will panic if:
/// * The `hex::decode` operation fails to decode the draft transaction.
/// * The `Transaction::from_bytes` operation fails to deserialize the transaction.
///
/// # Dependencies
///
/// This function relies on utilities such as `fetch_protocol_params`, `evaluate_redeemer_units`,
/// `attach_final_sig_with_units`, and `compute_min_fee` to perform its operations.

/// Builds the witnessed Plutus transaction with an accurately evaluated execution budget
/// and a dynamically computed minimum fee.
///
pub async fn attach_final_sig(
    unsigned_tx_hex: &str,
    final_sig: &[u8],
    config: &DaemonConfig,
    cardano_fee: u64,
    is_refund: bool,
) -> String {
    let params = fetch_protocol_params(config).await;

    // Draft with ceiling units so the evaluate endpoint can assess the script.
    let draft = attach_final_sig_with_units(
        unsigned_tx_hex,
        final_sig,
        MEM_UNITS_CEILING,
        CPU_UNITS_CEILING,
        MEM_UNITS_CEILING * DEFAULT_MIN_FEE_A + DEFAULT_MIN_FEE_B, // placeholder fee for draft
        &params.cost_models,
        is_refund,
    );

    let (mem, cpu) = evaluate_redeemer_units(config, &draft).await;

    // Use cardano_fee as the placeholder so the CBOR encoding of the fee field matches the
    // final tx — both are in the multi-byte range, giving an accurate size measurement.
    let sized_tx_hex = attach_final_sig_with_units(
        unsigned_tx_hex,
        final_sig,
        mem,
        cpu,
        cardano_fee,
        &params.cost_models,
        is_refund,
    );
    let sized_tx = cardano_serialization_lib::Transaction::from_bytes(
        hex::decode(&sized_tx_hex).unwrap(),
    )
    .unwrap();
    let fee = compute_min_fee(&sized_tx, &params, mem, cpu);
    info!("attach_final_sig: mem={mem}, cpu={cpu}, fee={fee}");

    attach_final_sig_with_units(unsigned_tx_hex, final_sig, mem, cpu, fee, &params.cost_models, is_refund)
}

/// Submits a Cardano transaction to the appropriate network based on the provided configuration.
///
/// # Parameters
///
/// * `tx_hex` - A string slice representing the transaction in hexadecimal format.
/// * `config` - A reference to the `DaemonConfig` object containing network and API configuration.
///
/// # Returns
///
/// * A `bool` indicating the success or failure of the transaction submission.
///   - `true`: If the transaction submission was successful.
///   - `false`: If the transaction submission failed.
///
/// # Errors
///
/// The function may fail for the following reasons:
/// - Invalid or malformed `tx_hex`.
/// - Incorrect or missing configuration in the `DaemonConfig`.
/// - Network/API failures during the transaction submission process.
///
/// # Notes
///
/// - Ensure that the proper API key (`blockfrost_api_key`) and/or gRPC URL (`dolos_grpc_url`) are configured
///   for the specified network in the `DaemonConfig`.
/// - The return value only indicates whether the transaction submission was initiated successfully; it does not
///   guarantee on-chain confirmation of the transaction.
pub async fn submit_cardano_tx(tx_hex: &str, config: &DaemonConfig) -> bool {
    match &config.cardano_network {
        CardanoNetwork::Preprod | CardanoNetwork::Preview | CardanoNetwork::Mainnet => {
            submit_cardano_tx_blockfrost(
                tx_hex,
                &config.cardano_network.blockfrost_base_url(),
                &config.blockfrost_api_key,
            )
            .await
        }
        CardanoNetwork::Custom { .. } => {
            let url = config.cardano_network.dolos_grpc_url().unwrap();
            submit_cardano_tx_dolos(tx_hex, url).await
        }
    }
}

/// Asynchronously submits a Cardano transaction using the Blockfrost API.
///
/// # Parameters
/// - `tx_hex`: A string slice containing the transaction in hexadecimal format.
/// - `base_url`: A string slice holding the base URL of the Blockfrost API endpoint (e.g., "https://cardano-mainnet.blockfrost.io").
/// - `api_key`: A string slice with the Blockfrost project API key for authorization.
///
/// # Returns
/// - `true` if the transaction is successfully submitted (HTTP status code indicates success).
/// - `false` if the submission fails (either due to a network issue, invalid API response, or unsuccessful status code).
///
/// # Errors
/// - If the provided `tx_hex` cannot be decoded into bytes, the function will panic since `unwrap()` is called on the result.
/// - An error message will be logged if the request fails due to a network issue or a non-successful HTTP status response.
/// - The function handles HTTP errors by logging the status code and response body, if available.
///
/// # Dependencies
/// This function depends on the `reqwest` crate for HTTP requests and the `hex` crate for decoding hexadecimal strings.
/// Ensure these crates are added to your `Cargo.toml`:
/// ```toml
/// [dependencies]
/// reqwest = "0.11"
/// hex = "0.4"
/// log = "0.4"     # For logging messages
/// ```
async fn submit_cardano_tx_blockfrost(tx_hex: &str, base_url: &str, api_key: &str) -> bool {
    let url = format!("{}/api/v0/tx/submit", base_url);
    let client = reqwest::Client::new();
    let tx_bytes = hex::decode(tx_hex).unwrap();
    match client
        .post(url)
        .header("project_id", api_key)
        .header("Content-Type", "application/cbor")
        .body(tx_bytes)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() {
                info!("cardano tx submitted: {status}");
                true
            } else {
                let body = resp.text().await.unwrap_or_default();
                error!("cardano tx submission failed: {status} — {body}");
                false
            }
        }
        Err(e) => {
            error!("cardano tx submission failed: {e}");
            false
        }
    }
}

/// Asynchronously submits a Cardano transaction to the Dolos backend.
///
/// # Parameters
/// - `tx_hex`: A string slice containing the hexadecimal representation of the Cardano transaction.
/// - `base_url`: A string slice specifying the base URL for the Dolos API endpoint.
///
/// # Returns
/// - A boolean value indicating whether the transaction submission was successful (`true`)
///   or failed (`false`).
///

/// # Errors
/// - If the `tx_hex` string fails to decode into bytes, the function will panic as `unwrap` is used
///   to handle the decoding process.
/// - If the client fails to initialize or the transaction submission fails, an error is logged,
///   and the function will return `false`.
///
async fn submit_cardano_tx_dolos(tx_hex: &str, base_url: &str) -> bool {
    use utxorpc::{CardanoSubmitClient, ClientBuilder};

    info!("submitting cardano tx hex: {tx_hex}");
    let tx_bytes = hex::decode(tx_hex).unwrap();

    let mut client: CardanoSubmitClient = ClientBuilder::new().uri(base_url).unwrap().build().await;

    match client.submit_tx(tx_bytes).await {
        Ok(tx_ref) => {
            info!("cardano tx submitted via dolos, ref: {:?}", tx_ref);
            true
        }
        Err(e) => {
            error!("cardano tx submission failed: {e:#?}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use blake2::digest::consts::U32;
    use blake2::{Blake2b, Digest};

    use cardano_serialization_lib::FixedTransaction;

    use ed25519_dalek::{Verifier, VerifyingKey};

    use super::*;
    use crate::{
        blockchains::bitcoin_utils::aggregate_pubkey,
        test_utils::{make_participant, make_swap_keys},
    };

    use std::sync::Once;

    static TRACING: Once = Once::new();

    fn init_tracing() {
        TRACING.call_once(|| {
            tracing_subscriber::fmt()
                .with_max_level(tracing::Level::DEBUG)
                .with_test_writer()
                .init();
        });
    }

    #[test]
    fn cardano_lock_tx_has_correct_structure() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();
        
        let mut participants = BTreeMap::new();
        participants.insert(1, make_participant(1, true));
        participants.insert(2, make_participant(2, false));

        let agg_pubkey = crate::blockchains::bitcoin_utils::aggregate_pubkey(&participants);

        let lock_tx = build_lock_tx(
            "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2",
            0,
            5_000_000,
            &agg_pubkey,
            200_000,
            CARDANO_MAINNET,
            1_000,
        );

        // verify input
        let inputs = lock_tx.body().inputs();
        assert_eq!(inputs.len(), 1);
        assert_eq!(
            inputs.get(0).transaction_id().to_hex(),
            "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"
        );
        assert_eq!(inputs.get(0).index(), 0);

        // verify output
        let outputs = lock_tx.body().outputs();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs.get(0).amount().coin().to_str(), "4800000");

        // verify output goes to script address
        let expected_addr = script_address(CARDANO_MAINNET);
        assert_eq!(
            outputs.get(0).address().to_bytes(),
            expected_addr.to_bytes()
        );

        // verify datum contains aggregate pubkey and refund_slot
        let datum = outputs.get(0).plutus_data().expect("should have datum");
        let constr = datum.as_constr_plutus_data().unwrap();
        let fields = constr.data();
        assert_eq!(fields.len(), 2, "datum should have 2 fields");
        let pubkey_bytes = fields.get(0).as_bytes().unwrap();
        assert_eq!(pubkey_bytes.len(), 32, "xonly pubkey should be 32 bytes");
        assert_eq!(pubkey_bytes, agg_pubkey.serialize()[1..].to_vec());
        let slot_int = fields.get(1).as_integer().unwrap();
        assert_eq!(slot_int.to_str(), "1000", "refund_slot should match");

        // verify fee
        assert_eq!(lock_tx.body().fee().to_str(), "200000");

        // verify witness set is empty — signing happens separately
        assert!(lock_tx.witness_set().vkeys().is_none());
    }

    #[test]
    fn cardano_lock_tx_cbor_roundtrips() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();
        
        let mut participants = BTreeMap::new();
        participants.insert(1, make_participant(1, true));
        participants.insert(2, make_participant(2, false));
        let agg_pubkey = crate::blockchains::bitcoin_utils::aggregate_pubkey(&participants);

        let lock_tx = build_lock_tx(
            "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2",
            0,
            5_000_000,
            &agg_pubkey,
            200_000,
            CARDANO_MAINNET,
            1_000,
        );

        let tx_hex = hex::encode(lock_tx.to_bytes());

        // verify FixedTransaction produces stable txid
        let fixed_tx1 = FixedTransaction::from_bytes(hex::decode(&tx_hex).unwrap()).unwrap();
        let fixed_tx2 = FixedTransaction::from_bytes(hex::decode(&tx_hex).unwrap()).unwrap();

        let txid1 = hex::encode(fixed_tx1.transaction_hash().to_bytes());
        let txid2 = hex::encode(fixed_tx2.transaction_hash().to_bytes());

        assert_eq!(txid1, txid2, "txid should be stable");
        assert_eq!(txid1.len(), 64, "txid should be 64 hex chars");
    }

    #[tokio::test]
    async fn sign_cardano_lock_tx_adds_vkey_witness() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();
        
        let keys = make_swap_keys(); // just need valid Ed25519 keys for signing

        let mut participants = BTreeMap::new();
        participants.insert(1, make_participant(1, true));
        participants.insert(2, make_participant(2, false));
        let agg_pubkey = aggregate_pubkey(&participants);

        let lock_tx = build_lock_tx(
            "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2",
            0,
            5_000_000,
            &agg_pubkey,
            200_000,
            CARDANO_MAINNET,
            1_000,
        );
        let unsigned_tx_hex = hex::encode(lock_tx.to_bytes());

        let signed_tx_hex = sign_cardano_lock_tx(&unsigned_tx_hex, &keys).await;

        let signed_tx_bytes = hex::decode(&signed_tx_hex).unwrap();
        let signed_tx = Transaction::from_bytes(signed_tx_bytes).unwrap();

        // verify vkey witness was added
        let vkeys = signed_tx.witness_set().vkeys().expect("should have vkeys");
        assert_eq!(vkeys.len(), 1);

        // verify the public key in the witness matches our wallet key
        let witness_pubkey = vkeys.get(0).vkey().public_key().as_bytes();
        assert_eq!(witness_pubkey, keys.cardano_wallet_public_key);
    }

    #[tokio::test]
    async fn sign_cardano_lock_tx_signature_verifies() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();
        
        let keys = make_swap_keys(); // just need valid Ed25519 keys for signing

        let mut participants = BTreeMap::new();
        participants.insert(1, make_participant(1, true));
        participants.insert(2, make_participant(2, false));
        let agg_pubkey = aggregate_pubkey(&participants);

        let lock_tx = build_lock_tx(
            "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2",
            0,
            5_000_000,
            &agg_pubkey,
            200_000,
            CARDANO_MAINNET,
            1_000,
        );
        let unsigned_tx_hex = hex::encode(lock_tx.to_bytes());

        let signed_tx_hex = sign_cardano_lock_tx(&unsigned_tx_hex, &keys).await;

        let signed_tx_bytes = hex::decode(&signed_tx_hex).unwrap();
        let signed_tx = Transaction::from_bytes(signed_tx_bytes).unwrap();

        // compute expected tx body hash
        let mut hasher = Blake2b::<U32>::new();
        hasher.update(&signed_tx.body().to_bytes());
        let tx_hash = hasher.finalize();

        // extract sig and pubkey from witness
        let vkeys = signed_tx.witness_set().vkeys().unwrap();
        let witness = vkeys.get(0);
        let sig_bytes = witness.signature().to_bytes();
        let pub_bytes: [u8; 32] = keys
            .cardano_wallet_public_key
            .as_slice()
            .try_into()
            .unwrap();

        // verify signature
        let verifying_key = VerifyingKey::from_bytes(&pub_bytes).unwrap();
        let signature = ed25519_dalek::Signature::from_bytes(&sig_bytes.try_into().unwrap());
        verifying_key
            .verify(&tx_hash, &signature)
            .expect("signature should verify");
    }

    #[tokio::test]
    async fn sign_cardano_lock_tx_preserves_tx_body() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();
        
        let keys = make_swap_keys(); // just need valid Ed25519 keys for signing

        let mut participants = BTreeMap::new();
        participants.insert(1, make_participant(1, true));
        participants.insert(2, make_participant(2, false));
        let agg_pubkey = aggregate_pubkey(&participants);

        let lock_tx = build_lock_tx(
            "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2",
            0,
            5_000_000,
            &agg_pubkey,
            200_000,
            CARDANO_MAINNET,
            1_000,
        );
        let unsigned_tx_hex = hex::encode(lock_tx.to_bytes());

        let signed_tx_hex = sign_cardano_lock_tx(&unsigned_tx_hex, &keys).await;
        let signed_tx = Transaction::from_bytes(hex::decode(&signed_tx_hex).unwrap()).unwrap();
        let unsigned_tx = Transaction::from_bytes(hex::decode(&unsigned_tx_hex).unwrap()).unwrap();

        // signing should not change the tx body
        assert_eq!(
            signed_tx.body().to_bytes(),
            unsigned_tx.body().to_bytes(),
            "tx body should be unchanged after signing"
        );
    }

    #[test]
    fn script_hash_matches_aiken_compiled_hash() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();

        // Aiken's plutus.json reports hash = afdc922d... (compiled with Aiken v1.1.21)
        // CSL's new_v3(raw_flat_bytes) should produce the same hash
        let script = PlutusScript::new_v3(SWAP_SCRIPT_BYTES.to_vec());
        let hash = hex::encode(script.hash().to_bytes());
        assert_eq!(
            hash, "afdc922d468f249b3bd5b3504a8f42bec5ac26a8f61a0e92543603e8",
            "script hash must match Aiken plutus.json hash"
        );
    }

    fn make_cardano_participant(id: u8, wallet_public_key: Vec<u8>) -> crate::types::Participant {
        crate::types::Participant {
            id,
            blockchain: crate::types::Blockchain::Cardano,
            tcp_address: format!("127.0.0.1:91{id:02}"),
            target_participant: id % 2 + 1,
            amount_locking: 5_000_000,
            amount_claiming: 5_000_000,
            is_me: false,
            secp256k1_public_key: String::new(),
            cardano_wallet_public_key: wallet_public_key,
            funding_utxo_txid: "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2".to_string(),
            funding_utxo_vout: 0,
        }
    }

    // Verify the redeemer PlutusData constructor index.
    // CBOR tag encoding for 2-byte tags: 0xd8 0xNN where NN = tag number in hex.
    //   Constr 0 = tag 121 = 0xd8 0x79
    //   Constr 1 = tag 122 = 0xd8 0x7a
    #[test]
    fn attach_final_sig_with_units_redeemer_constr_index() {
        let keys = make_swap_keys();
        let participant = make_cardano_participant(2, keys.cardano_wallet_public_key.clone());

        let lock_txhash = cardano_serialization_lib::TransactionHash::from_hex(
            "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2",
        )
        .unwrap();

        let refund_tx = build_refund_tx(&participant, &lock_txhash, 1_000, 200_000, None);
        let unsigned_tx_hex = hex::encode(refund_tx.to_bytes());

        let dummy_sig = [0u8; 64];
        let cost_models = plutus_v3_cost_model_hardcoded();

        // Constr 0 (Spend)  = CBOR tag 121 = bytes [0xd8, 0x79]
        // Constr 1 (Refund) = CBOR tag 122 = bytes [0xd8, 0x7a]
        for (is_refund, expected_second) in [(false, 0x79u8), (true, 0x7au8)] {
            let signed_hex = attach_final_sig_with_units(
                &unsigned_tx_hex,
                &dummy_sig,
                MEM_UNITS_CEILING,
                CPU_UNITS_CEILING,
                200_000,
                &cost_models,
                is_refund,
            );

            let tx_bytes = hex::decode(&signed_hex).unwrap();
            let tx = cardano_serialization_lib::Transaction::from_bytes(tx_bytes).unwrap();
            let redeemers = tx.witness_set().redeemers().expect("must have redeemers");
            let cbor = redeemers.get(0).data().to_bytes();
            assert_eq!(cbor[0], 0xd8u8, "is_refund={is_refund}: first byte should be 0xd8");
            assert_eq!(
                cbor[1], expected_second,
                "is_refund={is_refund}: second byte should be 0x{expected_second:02x} (constr {})",
                if is_refund { 1 } else { 0 }
            );
        }
    }

    // Verify add_collateral_witness preserves the redeemer (constr 1 for refund).
    #[test]
    fn add_collateral_witness_preserves_refund_redeemer() {
        let keys = make_swap_keys();
        let participant = make_cardano_participant(2, keys.cardano_wallet_public_key.clone());
        let lock_txhash = cardano_serialization_lib::TransactionHash::from_hex(
            "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2",
        )
        .unwrap();
        let refund_tx = build_refund_tx(&participant, &lock_txhash, 1_000, 200_000, None);
        let unsigned_tx_hex = hex::encode(refund_tx.to_bytes());

        let dummy_sig = [0u8; 64];
        let cost_models = plutus_v3_cost_model_hardcoded();
        let signed_hex = attach_final_sig_with_units(
            &unsigned_tx_hex, &dummy_sig,
            MEM_UNITS_CEILING, CPU_UNITS_CEILING, 200_000, &cost_models, true,
        );

        // add_collateral_witness appends an Ed25519 vkey — redeemer must survive
        let after_collateral = add_collateral_witness(
            &signed_hex,
            &keys.cardano_wallet_secret_key,
            &keys.cardano_wallet_public_key,
        );

        let tx_bytes = hex::decode(&after_collateral).unwrap();
        let tx = cardano_serialization_lib::Transaction::from_bytes(tx_bytes).unwrap();
        let redeemers = tx.witness_set().redeemers().expect("must have redeemers after collateral");
        let cbor = redeemers.get(0).data().to_bytes();
        // Constr 1 (Refund) = tag 122 = 0xd8 0x7a
        assert_eq!(cbor[0], 0xd8, "first byte should be 0xd8 after add_collateral_witness");
        assert_eq!(cbor[1], 0x7a, "redeemer should still be constr 1 (0x7a = tag 122) after add_collateral_witness");
    }
}
