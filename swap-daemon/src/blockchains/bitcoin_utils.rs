use bitcoin::absolute::LockTime;
// use bitcoin::consensus::encode::serialize_hex;
use bitcoin::hashes::Hash;
use bitcoin::key::TapTweak;
use bitcoin::sighash::{SighashCache, TapSighashType};
use bitcoin::transaction::Version;
use bitcoin::{
    Address, Amount, Network, OutPoint, ScriptBuf, Sequence, TxIn, Witness, XOnlyPublicKey,
};
use bitcoin::{Transaction, TxOut};
use musig2::{secp::Scalar, secp256k1::PublicKey, KeyAggContext};
use tracing::{error, info};
use crate::types::{Participant, Participants, SwapKeys};

/// Performs Taproot key aggregation using a context-based approach.
///
/// This function takes a vector of public keys and creates a [`KeyAggContext`]
/// which facilitates the aggregation of public keys in the MuSig2 protocol. The resulting
/// aggregated key is converted to an `XOnlyPublicKey` for Taproot compatibility,
/// and a tweak is applied to the context for Taproot key tweaking.
///
/// # Parameters
///
/// * `pubkeys` - A `Vec` of [`PublicKey`] instances that represent the participants in the key aggregation process.
///
/// # Returns
///
/// Returns a [`KeyAggContext`] that includes the tweaked aggregated public key. The tweak is determined
/// based on the aggregated public key and a `None` tweak input (i.e., without any additional internal key modifications).
///
/// # Errors
///
/// This function will panic if:
/// - The initialization of a [`KeyAggContext`] with the provided public keys fails.
/// - The conversion of the aggregated public key to an `XOnlyPublicKey` fails.
/// - The creation of the tweak hash and its subsequent conversion to a scalar fails.
/// - The application of the tweak to the context fails.
///
/// # Notes
///
/// - Build a `KeyAggContext` with the BIP341 keypath tweak applied.
/// - Used when signing Bitcoin taproot outputs — the aggregate key must be tweaked
///   before it can be used as the internal key for a P2TR output.
pub fn taproot_key_agg_ctx(pubkeys: Vec<PublicKey>) -> KeyAggContext {
    let ctx = KeyAggContext::new(pubkeys).unwrap();
    let agg_pk = ctx.aggregated_pubkey::<musig2::secp256k1::PublicKey>();
    let xonly = bitcoin::XOnlyPublicKey::from_slice(&agg_pk.serialize()[1..]).unwrap();
    let tweak_hash = bitcoin::taproot::TapTweakHash::from_key_and_tweak(xonly, None);
    let tweak_bytes: [u8; 32] = tweak_hash.to_raw_hash().to_byte_array();
    let tweak = Scalar::from_slice(&tweak_bytes).unwrap();
    ctx.with_tweak(tweak, true).unwrap()
}

/// Retrieves a list of Bitcoin public keys from the given participants.
///
/// This function iterates through the provided `Participants` map and extracts the
/// secp256k1 public keys for each participant. The public keys are parsed from their
/// string representation and collected into a `Vec`.
///
/// # Parameters
///
/// * `participants` - A reference to a `Participants` collection. The `Participants` type
///   is expected to be a map-like structure where the values can provide a `secp256k1_public_key`
///   field that holds the public key as a string.
///
/// # Returns
///
/// A `Vec` containing `musig2::secp256k1::PublicKey` instances corresponding to each
/// participant's secp256k1 public key.
///
/// # Panics
///
/// This function will panic if:
/// - A participant's `secp256k1_public_key` cannot be parsed into a valid
///   `musig2::secp256k1::PublicKey`.
///
pub fn bitcoin_pubkeys(participants: &Participants) -> Vec<musig2::secp256k1::PublicKey> {
    participants
        .values()
        .map(|p| p.secp256k1_public_key.parse().unwrap())
        .collect()
}

/// Aggregates multiple public keys into a single Taproot address.
///
/// # Parameters
/// - `all_pubkeys`: A vector containing public keys (`Vec<PublicKey>`) that are to be aggregated.
/// - `network`: The Bitcoin network (`Network`) for which the Taproot address will be generated
///   (e.g., Mainnet, Testnet, Regtest).
///
/// # Returns
/// - An `Address` object representing the Taproot address created from the aggregated public keys
///   and network type.
///
/// # Errors
/// - Will panic if:
///   - The `KeyAggContext::new` creation fails.
///   - The X-only public key conversion (`XOnlyPublicKey::from_slice`) fails.
///
pub fn aggregate_address(all_pubkeys: Vec<PublicKey>, network: Network) -> Address {
    let key_agg_ctx = KeyAggContext::new(all_pubkeys).unwrap();
    let agg_pk = key_agg_ctx.aggregated_pubkey::<musig2::secp256k1::PublicKey>();
    let xonly = bitcoin::XOnlyPublicKey::from_slice(&agg_pk.serialize()[1..]).unwrap();
    let secp = bitcoin::secp256k1::Secp256k1::new();
    Address::p2tr(&secp, xonly, None, network)
}

/// Aggregates the public keys of all participants into a single public key.
///
/// This function takes a list of participants, extracts their respective public keys,
/// and combines them into a single aggregated public key using MuSig2 key aggregation.
///
/// # Parameters
///
/// * `participants` - A reference to a `Participants` object that contains the
///   information about all participants whose public keys are to be aggregated.
///
/// # Returns
///
/// - A `musig2::secp256k1::PublicKey` representing the aggregated public key.
///
/// # Errors
///
/// This function uses the `KeyAggContext` for key aggregation initialization.
/// If the creation of the `KeyAggContext` fails, the function will panic.
///
/// # Notes
///
/// Ensure that all participants are properly initialized, and their public keys
/// are correctly shared before calling this function to prevent any errors
/// during the key aggregation process.
pub fn aggregate_pubkey(participants: &Participants) -> musig2::secp256k1::PublicKey {
    let pubkeys = bitcoin_pubkeys(participants);
    let key_agg_ctx = KeyAggContext::new(pubkeys).unwrap();
    key_agg_ctx.aggregated_pubkey()
}

/// Computes the sighash for a Taproot key-path spend.
///
/// This function calculates the signature hash, which is used for signing
/// Taproot transactions in Bitcoin. It specifically handles the case where
/// the Taproot script path is bypassed, resulting in a direct key-path spend.
///
/// # Parameters
///
/// * `tx`: A reference to the [`Transaction`] object corresponding to the
///   transaction being signed.
/// * `lock_tx_output`: A reference to the [`TxOut`] object representing the
///   UTXO (unspent transaction output) being consumed in the transaction.
///
/// # Returns
///
/// A 32-byte array representing the computed sighash.
///
/// # Panics
///
/// This function will panic if the `taproot_key_spend_signature_hash` method
/// returns an error, which may happen if the input index or Prevouts configuration
/// is invalid.
pub fn compute_sighash(
    tx: &Transaction,
    lock_tx_output: &TxOut, // the output being spent from the lock tx
) -> [u8; 32] {
    let mut sighash_cache = SighashCache::new(tx);

    let sighash = sighash_cache
        .taproot_key_spend_signature_hash(
            0, // input index
            &bitcoin::sighash::Prevouts::All(&[lock_tx_output]),
            TapSighashType::Default,
        )
        .unwrap();

    sighash.to_byte_array()
}

/// Constructs a Bitcoin transaction that locks a specified amount of funds to a provided address.
///
/// This function creates a transaction with a single input and a single output. It uses the
/// given `funding_utxo` as the input and locks the specified `amount` to the `aggregate_address`.
/// The remaining balance in the input UTXO is considered the transaction fee, so the caller
/// must ensure that the funding UTXO's value is equal to `amount` plus the desired fee.
///
/// # Parameters
///
/// * `funding_utxo` - The previously confirmed UTXO to be spent, represented as an `OutPoint`.
/// * `amount` - The amount in satoshis to lock to the `aggregate_address`.
/// * `aggregate_address` - The Bitcoin address to which the funds will be sent.
///
/// # Returns
///
/// Returns a `Transaction` object with the specified input and output. The transaction is
/// unsigned, so it must be signed with the appropriate private key(s) before broadcasting.
///
/// # Notes
///
/// * Ensure that the funding UTXO is valid and contains sufficient funds (amount + fee).
/// * The function does not perform input validation or UTXO confirmation checks.
/// * The returned `Transaction` must be signed and validated before broadcasting.
///
pub fn build_lock_tx(
    funding_utxo: OutPoint,
    amount: u64,
    aggregate_address: Address,
) -> Transaction {
    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: funding_utxo,
            script_sig: bitcoin::ScriptBuf::default(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::default(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(amount), // this means everything left in the utxo is used as the fee. So make sure starting utxo is just amount + fee.
            script_pubkey: aggregate_address.script_pubkey(),
        }],
    }
}

/// Fetches the previous output (UTxO) of a Bitcoin transaction identified by its transaction ID (`txid`)
/// and output index (`vout`) using an external block explorer API.
///
/// # Parameters
/// - `txid`: A string slice representing the transaction ID of the UTxO to fetch.
/// - `vout`: A 32-bit unsigned integer representing the index of the UTxO in the transaction's outputs.
/// - `base_url`: A string slice representing the base URL of the block explorer API endpoint.
///
/// # Returns
/// - `Option<bitcoin::TxOut>`:
///   - Returns `Some(bitcoin::TxOut)` if the transaction output was successfully fetched and parsed.
///   - Returns `None` if there was an error during the API request, JSON parsing, or script decoding.
///
/// # Errors Logged
/// - Logs an error if the HTTP GET request fails (e.g., due to network issues).
/// - Logs an error if the JSON response from the API is invalid or missing required fields.
/// - Logs an error if the UTxO's scriptPubKey cannot be decoded from its hex representation.
///
/// # Notes
/// - Make sure the `base_url` points to a valid Bitcoin block explorer API conforming to the expected format.
/// - The function assumes the API provides a JSON response where transaction outputs are under the `vout` key,
///   and each output contains `scriptpubkey` (hex-encoded) and `value` (in satoshis).
/// - The function is asynchronous and must be used in an `async` context.
async fn fetch_bitcoin_utxo_prevout(
    txid: &str,
    vout: u32,
    base_url: &str,
) -> Option<bitcoin::TxOut> {
    let url = format!("{}/tx/{}", base_url, txid);
    let resp = match reqwest::get(&url).await {
        Ok(r) => r,
        Err(e) => {
            error!("failed to fetch UTxO {txid}:{vout}: {e}");
            return None;
        }
    };
    let data: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(e) => {
            error!("invalid JSON for UTxO {txid}:{vout}: {e}");
            return None;
        }
    };
    let out = &data["vout"][vout as usize];
    let script_hex = out["scriptpubkey"].as_str()?;
    let value = out["value"].as_u64()?;
    Some(bitcoin::TxOut {
        value: bitcoin::Amount::from_sat(value),
        script_pubkey: bitcoin::ScriptBuf::from_bytes(hex::decode(script_hex).ok()?),
    })
}

/// Signs a Bitcoin Taproot lock transaction using the provided participant, swap keys, and base URL for UTXO fetching.
///
/// This function takes an unsigned Bitcoin transaction in hexadecimal format, retrieves the
/// necessary previous output details (UTXO) associated with the participant's funding transaction,
/// computes the appropriate signature using Taproot key spend, and appends it to the transaction input's witness.
/// The signed transaction is returned in hexadecimal format.
///
/// # Parameters
///
/// * `unsigned_tx_hex` - A hexadecimal string representing the unsigned transaction data.
/// * `participant` - A reference to the `Participant`, which contains the funding UTXO information used for signature calculation.
/// * `keys` - A reference to the `SwapKeys`, providing the secret key required for signing.
/// * `base_url` - A base URL used to interact with external services for retrieving UTXO data.
///
/// # Returns
///
/// An `Option<String>`:
/// * `Some(String)` containing the signed transaction in hexadecimal format if signing is successful.
/// * `None` if any step in the signing process fails.
///
/// # Errors
///
/// This function will return `None` in the following cases:
/// * Failure to deserialize the unsigned transaction from the provided hexadecimal string.
/// * Failure to fetch the previous output (UTXO) details from the external service.
/// * Problems encountered during the signature creation process (e.g., invalid keys, invalid sighash).
///
/// # Notes
///
/// * This method assumes the Taproot signing scheme using a single-sig (key spend) approach.
/// * The function uses `bitcoin::sighash`, `bitcoin::secp256k1`, and other relevant Bitcoin libraries for transaction manipulation and signing.
pub async fn sign_bitcoin_lock_tx(
    unsigned_tx_hex: &str,
    participant: &Participant,
    keys: &SwapKeys,
    base_url: &str,
) -> Option<String> {
    use bitcoin::sighash::{Prevouts, SighashCache, TapSighashType};

    let mut tx: bitcoin::Transaction =
        bitcoin::consensus::encode::deserialize_hex(unsigned_tx_hex).unwrap();

    let prevout = fetch_bitcoin_utxo_prevout(
        &participant.funding_utxo_txid,
        participant.funding_utxo_vout,
        base_url,
    )
    .await?;

    let mut sighash_cache = SighashCache::new(&tx);
    let sighash = sighash_cache
        .taproot_key_spend_signature_hash(0, &Prevouts::All(&[prevout]), TapSighashType::Default)
        .unwrap();

    let secp = bitcoin::secp256k1::Secp256k1::new();

    // convert secret key from musig2::secp256k1 to bitcoin::secp256k1 via bytes
    let secret_key_bytes = keys.secret_key.secret_bytes();
    let bitcoin_secret_key = bitcoin::secp256k1::SecretKey::from_slice(&secret_key_bytes).unwrap();
    let keypair = bitcoin::secp256k1::Keypair::from_secret_key(&secp, &bitcoin_secret_key);

    let tweaked_keypair = keypair.tap_tweak(&secp, None);

    let msg = bitcoin::secp256k1::Message::from_digest(sighash.to_byte_array());
    let sig = secp.sign_schnorr_no_aux_rand(&msg, &tweaked_keypair.to_keypair());

    tx.input[0].witness = bitcoin::Witness::from_slice(&[sig.as_ref()]);
    Some(bitcoin::consensus::encode::serialize_hex(&tx))
}

/// Constructs a refund transaction using the provided parameters.
///
/// # Parameters
///
/// * `refund_input` - The previous transaction output (`OutPoint`) that is being spent in this refund transaction.
/// * `participant` - A reference to a `Participant` object containing the participant's information, such as their
///                   public key and the amount they have locked in the transaction.
/// * `locktime` - A `u32` value representing the pre-computed locktime, usually derived from the
///                `refund_locktime()` function. This determines when the transaction can be mined.
/// * `fee` - A `u64` value specifying the transaction fee in satoshis to be deducted from the output amount.
///
/// # Returns
///
/// Returns a `Transaction` object representing the refund transaction. The refund transaction includes:
/// - A single input referencing the provided `refund_input`.
/// - A single output sending the refunded amount (participant's locked amount minus the fee) to the taproot address
///   derived from the participant's public key.
///
/// # Panics
///
/// This function will panic if:
/// - The participant's secp256k1 public key cannot be parsed or converted to an x-only public key.
/// - The derived taproot `Address` creation fails.
/// - The provided `locktime` is out of valid range for use in `LockTime::from_height`.
///
pub fn build_refund_tx(
    refund_input: OutPoint,
    participant: &Participant,
    locktime: u32, // pre-computed staggered locktime from refund_locktime()
    fee: u64,
) -> Transaction {
    let pk: musig2::secp256k1::PublicKey = participant.secp256k1_public_key.parse().unwrap();
    let xonly = XOnlyPublicKey::from_slice(&pk.serialize()[1..]).unwrap();
    let secp = bitcoin::secp256k1::Secp256k1::new();
    let recipient = Address::p2tr(&secp, xonly, None, Network::Bitcoin);

    Transaction {
        version: Version::TWO,
        lock_time: LockTime::from_height(locktime).unwrap(),
        input: vec![TxIn {
            previous_output: refund_input,
            script_sig: ScriptBuf::default(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::default(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(participant.amount_locking - fee),
            script_pubkey: recipient.script_pubkey(),
        }],
    }
}

/// Constructs a Bitcoin spending transaction with Taproot output.
///
/// # Parameters
///
/// * `spend_input` - The `OutPoint` representing the UTXO being spent in this transaction.
/// * `participant` - A reference to a `Participant` that includes information about the participant's
///    public key and the amount they are claiming.
/// * `fee` - The transaction fee in satoshis to apply to the output.
///
/// # Returns
///
/// A `Transaction` object representing the finalized spending transaction.
///
/// # Panics
///
/// This function will panic if:
/// - The `secp256k1_public_key` provided in the `participant` cannot be parsed
///   into a valid secp256k1 public key.
/// - The derived Taproot `XOnlyPublicKey` from the participant's public key is invalid.
///
pub fn build_spend_tx(spend_input: OutPoint, participant: &Participant, fee: u64) -> Transaction {
    let pk: musig2::secp256k1::PublicKey = participant.secp256k1_public_key.parse().unwrap();
    let xonly = XOnlyPublicKey::from_slice(&pk.serialize()[1..]).unwrap();
    let secp = bitcoin::secp256k1::Secp256k1::new();
    let recipient = Address::p2tr(&secp, xonly, None, Network::Bitcoin);

    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: spend_input,
            script_sig: ScriptBuf::default(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::default(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(participant.amount_claiming - fee),
            script_pubkey: recipient.script_pubkey(),
        }],
    }
}

/// Submits a Bitcoin transaction to a specified endpoint as a raw hexadecimal string.
///
/// # Parameters
///
/// * `tx_hex` - A string slice that holds the raw hexadecimal representation of the Bitcoin transaction.
/// * `base_url` - A string slice that represents the base URL of the endpoint where the transaction will be submitted.
///
/// # Returns
///
/// Returns `true` if the transaction submission was successful (HTTP status code indicates success),
/// or `false` if the submission failed due to a client error, server error, or network-related issue.
///
/// # Errors
///
/// - Logs an error and returns `false` if there is a network connectivity issue,
///   or if the server responds with a failure status or unexpected body content.
/// - Uses `resp.text().await.unwrap_or_default()` to handle cases where the response body might be inaccessible.
///
pub async fn submit_bitcoin_tx(tx_hex: &str, base_url: &str) -> bool {
    let url = format!("{}/tx", base_url);
    let client = reqwest::Client::new();
    match client.post(url).body(tx_hex.to_string()).send().await {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() {
                info!("bitcoin tx submitted: {status}");
                true
            } else {
                let body = resp.text().await.unwrap_or_default();
                error!("bitcoin tx submission failed: {status} — {body}");
                false
            }
        }
        Err(e) => {
            error!("bitcoin tx submission failed: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, str::FromStr};

    use crate::{
        blockchains::bitcoin_utils::{
            self, aggregate_address, bitcoin_pubkeys, compute_sighash, sign_bitcoin_lock_tx,
        },
        protocol::lock_funds::build_lock_txs,
        test_utils::{make_participant, make_swap_keys},
        types::{DaemonConfig, SwapSession},
    };
    use bitcoin::hashes::Hash;
    use bitcoin::sighash::{Prevouts, SighashCache, TapSighashType};
    use wiremock::matchers::{method, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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
    fn bitcoin_lock_tx_output_matches_aggregate_address() {
        let mut participants = BTreeMap::new();
        participants.insert(1, make_participant(1, true));
        participants.insert(2, make_participant(2, false));

        let all_pubkeys = bitcoin_pubkeys(&participants);
        let aggregate_addr = aggregate_address(all_pubkeys.clone(), bitcoin::Network::Signet);

        let mut session = SwapSession::new(1, participants, 0, 0, 5_000, 2_000_000);
        build_lock_txs(&mut session, &DaemonConfig::testnet("127.0.0.1:9000".to_string()));

        let lock_tx_hex = session.lock_txs.get(&1).unwrap();
        let lock_tx: bitcoin::Transaction =
            bitcoin::consensus::encode::deserialize_hex(lock_tx_hex).unwrap();

        // verify output script matches the aggregate address
        assert_eq!(
            lock_tx.output[0].script_pubkey,
            aggregate_addr.script_pubkey(),
            "output should go to aggregate taproot address"
        );
    }

    #[test]
    fn compute_sighash_matches_manual_calculation() {
        use bitcoin::sighash::{Prevouts, SighashCache, TapSighashType};

        let mut participants = BTreeMap::new();
        participants.insert(1, make_participant(1, true));
        participants.insert(2, make_participant(2, false));

        let all_pubkeys = bitcoin_pubkeys(&participants);
        let aggregate_addr = aggregate_address(all_pubkeys, bitcoin::Network::Signet);

        let funding_utxo = bitcoin::OutPoint {
            txid: bitcoin::Txid::from_str(
                "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2",
            )
            .unwrap(),
            vout: 0,
        };

        let lock_tx = bitcoin_utils::build_lock_tx(funding_utxo, 100_000, aggregate_addr.clone());
        let lock_tx_output = lock_tx.output[0].clone();

        // build spend tx
        let spend_input = bitcoin::OutPoint {
            txid: lock_tx.compute_txid(),
            vout: 0,
        };
        let participant = make_participant(1, true);
        let spend_tx = bitcoin_utils::build_spend_tx(spend_input, &participant, 10);

        // compute sighash using our function
        let our_sighash = compute_sighash(&spend_tx, &lock_tx_output);

        // compute sighash manually
        let mut sighash_cache = SighashCache::new(&spend_tx);
        let expected_sighash = sighash_cache
            .taproot_key_spend_signature_hash(
                0,
                &Prevouts::All(&[lock_tx_output]),
                TapSighashType::Default,
            )
            .unwrap();

        assert_eq!(
            our_sighash,
            expected_sighash.to_byte_array(),
            "sighash should match manual calculation"
        );
    }

    #[test]
    fn bitcoin_lock_tx_has_correct_structure() {
        let mut participants = BTreeMap::new();
        participants.insert(1, make_participant(1, true));
        participants.insert(2, make_participant(2, false));

        let all_pubkeys = bitcoin_pubkeys(&participants);
        let aggregate_addr = aggregate_address(all_pubkeys, bitcoin::Network::Signet);

        let funding_utxo = bitcoin::OutPoint {
            txid: bitcoin::Txid::from_str(
                "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2",
            )
            .unwrap(),
            vout: 0,
        };

        let lock_tx = bitcoin_utils::build_lock_tx(funding_utxo, 100_000, aggregate_addr.clone());

        assert_eq!(lock_tx.output.len(), 1);
        assert_eq!(lock_tx.output[0].value.to_sat(), 100_000);
        assert_eq!(
            lock_tx.output[0].script_pubkey,
            aggregate_addr.script_pubkey()
        );
        assert_eq!(lock_tx.input[0].previous_output, funding_utxo);
    }

    #[tokio::test]
    async fn bitcoin_lock_tx_signature_verifies() {
        let keys = make_swap_keys();

        // derive funding script from keys — this is what the participant's UTXO is locked to
        let xonly_bytes = keys.public_key.x_only_public_key().0.serialize();
        let xonly = bitcoin::secp256k1::XOnlyPublicKey::from_slice(&xonly_bytes).unwrap();
        let secp = bitcoin::secp256k1::Secp256k1::new();
        let funding_script =
            bitcoin::Address::p2tr(&secp, xonly, None, bitcoin::Network::Signet).script_pubkey();
        let funding_script_hex = hex::encode(funding_script.as_bytes());

        let participant = make_participant(1, true);

        let mut participants = BTreeMap::new();
        participants.insert(1, participant.clone());
        participants.insert(2, make_participant(2, false));

        let all_pubkeys = bitcoin_pubkeys(&participants);
        let aggregate_addr = aggregate_address(all_pubkeys, bitcoin::Network::Signet);

        let funding_utxo = bitcoin::OutPoint {
            txid: bitcoin::Txid::from_str(
                "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2",
            )
            .unwrap(),
            vout: 0,
        };

        let lock_tx = bitcoin_utils::build_lock_tx(funding_utxo, 100_000, aggregate_addr);
        let unsigned_tx_hex = bitcoin::consensus::encode::serialize_hex(&lock_tx);

        // mock the mempool GET /tx/{txid} endpoint so sign_bitcoin_lock_tx can fetch the prevout
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path_regex("/tx/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "vout": [{ "scriptpubkey": funding_script_hex, "value": 100_000u64 }]
            })))
            .mount(&mock_server)
            .await;

        let signed_tx_hex =
            sign_bitcoin_lock_tx(&unsigned_tx_hex, &participant, &keys, &mock_server.uri())
                .await
                .expect("sign should succeed with mocked prevout");

        let signed_tx: bitcoin::Transaction =
            bitcoin::consensus::encode::deserialize_hex(&signed_tx_hex).unwrap();

        assert!(
            !signed_tx.input[0].witness.is_empty(),
            "witness should not be empty"
        );

        let prevout = bitcoin::TxOut {
            value: bitcoin::Amount::from_sat(100_000),
            script_pubkey: funding_script,
        };

        let mut verify_cache = SighashCache::new(&signed_tx);
        let verify_sighash = verify_cache
            .taproot_key_spend_signature_hash(
                0,
                &Prevouts::All(&[prevout.clone()]),
                TapSighashType::Default,
            )
            .unwrap();

        // verify against tweaked xonly key extracted from script_pubkey
        let tweaked_xonly =
            bitcoin::secp256k1::XOnlyPublicKey::from_slice(&prevout.script_pubkey.as_bytes()[2..])
                .unwrap();

        let bitcoin_sig = bitcoin::secp256k1::schnorr::Signature::from_slice(
            signed_tx.input[0].witness.iter().next().unwrap(),
        )
        .unwrap();

        let msg = bitcoin::secp256k1::Message::from_digest(verify_sighash.to_byte_array());
        bitcoin::secp256k1::Secp256k1::new()
            .verify_schnorr(&bitcoin_sig, &msg, &tweaked_xonly)
            .expect("signature should verify");
    } // //This test has to be manually updated with the relevant utxo.
      // #[tokio::test]
      // #[ignore]
      // async fn submit_bitcoin_tx_appears_in_testnet_mempool() {
      //     use rand::rngs::OsRng;
      //     use bitcoin::{
      //     secp256k1:: {Secp256k1, SecretKey},
      //         Network, PrivateKey,
      //     };

    //     // pre-funded testnet4 key — fund this address once from the faucet
    //     // https://mempool.space/testnet4/faucet

    //     // // Run the below to genertae WIF and utxo
    //     // let secp = Secp256k1::new();
    //     // let secret_key = SecretKey::new(&mut OsRng);
    //     // let private_key = PrivateKey::new(secret_key, Network::Testnet);
    //     // let public_key = private_key.public_key(&secp);
    //     // let xonly = XOnlyPublicKey::from(public_key.inner);
    //     // let address = bitcoin::Address::p2tr(&secp, xonly, None, Network::Testnet);
    //     // println!("WIF: {}", private_key.to_wif());
    //     // println!("Taproot address: {}", address);

    //     const WIF_KEY: &str = "cUUysEnbToAThyhSEJdZg9vo7bzD9E81Ay6t1ghikpES1n2FaZTi";
    //     const ADDRE: &str = "tb1pu77v7a3khe5ggdak6jmegguy4dnszd09ypfen3c8fglnhht75n4sxmndzu";
    //     const FUNDED_UTXO_TXID: &str = "";
    //     const FUNDED_UTXO_VOUT: u32 = 0;
    //     const FUNDED_UTXO_AMOUNT_SATS: u64 = 10_000;
    //     const BASE_URL: &str = "https://mempool.space/testnet4";

    //     let secp = Secp256k1::new();
    //     let private_key = PrivateKey::from_wif(WIF_KEY).unwrap();
    //     let public_key = private_key.public_key(&secp);
    //     let address = bitcoin::Address::p2wpkh(&public_key, Network::Testnet).unwrap();

    //     // build tx
    //     let tx = Transaction {
    //         version: Version::TWO,
    //         lock_time: LockTime::ZERO,
    //         input: vec![TxIn {
    //             previous_output: OutPoint {
    //                 txid: bitcoin::Txid::from_str(FUNDED_UTXO_TXID).unwrap(),
    //                 vout: FUNDED_UTXO_VOUT,
    //             },
    //             script_sig: ScriptBuf::default(),
    //             sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
    //             witness: Witness::default(),
    //         }],
    //         output: vec![TxOut {
    //             value: Amount::from_sat(FUNDED_UTXO_AMOUNT_SATS - 1000),
    //             script_pubkey: address.script_pubkey(),
    //         }],
    //     };

    //     // sign the tx
    //     use bitcoin::sighash::{SighashCache, EcdsaSighashType};
    //     let mut sighash_cache = SighashCache::new(&tx);
    //     let sighash = sighash_cache.p2wpkh_signature_hash(
    //         0,
    //         &address.script_pubkey(),
    //         Amount::from_sat(FUNDED_UTXO_AMOUNT_SATS),
    //         EcdsaSighashType::All,
    //     ).unwrap();

    //     let msg = bitcoin::secp256k1::Message::from_digest(sighash.to_byte_array());
    //     let sig = secp.sign_ecdsa(&msg, &private_key.inner);
    //     let mut signed_tx = tx.clone();
    //     signed_tx.input[0].witness = Witness::from_slice(&[
    //         &[sig.serialize_der().as_ref(), &[EcdsaSighashType::All as u8]].concat(),
    //         &public_key.to_bytes(),
    //     ]);

    //     let tx_hex = serialize_hex(&signed_tx);
    //     println!("submitting tx: {}", signed_tx.compute_txid());

    //     // submit
    //     submit_bitcoin_tx(&tx_hex, BASE_URL).await;

    //     // wait for propagation
    //     tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    //     // check in mempool
    //     let txid = signed_tx.compute_txid();
    //     let url = format!("{}/api/tx/{}", BASE_URL, txid);
    //     let resp = reqwest::get(&url).await.unwrap();
    //     assert!(resp.status().is_success(), "tx {} should be in mempool", txid);
    //     println!("tx {} found in mempool", txid);
    // }
}
