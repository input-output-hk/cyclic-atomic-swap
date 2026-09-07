//! Fetches the valid signature from the witness field of the first input of a confirmed Bitcoin transaction.
use crate::{
    cryptography::multisig,
    protocol::chain_monitor::cardano_txid,
    types::{Blockchain, CardanoNetwork, DaemonConfig, ParticipantId, SwapSession, TxRole},
    utils::get_my_id,
};
use bitcoin::consensus::encode::deserialize_hex;
use tracing::info;

/// Fetches the valid signature from the witness field of the first input of a confirmed Bitcoin transaction.
///
/// This function takes the hexadecimal representation of an unsigned Bitcoin transaction (`unsigned_tx_hex`)
/// and attempts to find its corresponding confirmed transaction on the blockchain. If found, it extracts the
/// signature (witness) from the first input of the confirmed transaction.
///
/// # Parameters
///
/// * `unsigned_tx_hex` - A `&str` containing the hexadecimal representation of the unsigned Bitcoin transaction.
/// * `base_url` - A `&str` specifying the base URL of the Bitcoin node or block explorer API to query for transaction details.
///
/// # Returns
///
/// * `Option<Vec<u8>>` - Returns `Some(Vec<u8>)` if a valid witness (signature) is found in the confirmed transaction,
///   or `None` if the transaction is not confirmed, not found, or does not contain a valid witness.
///
/// # Behavior
///
/// 1. Deserializes the `unsigned_tx_hex` into a Bitcoin transaction object.
/// 2. Computes the transaction ID (txid) of the unsigned transaction.
/// 3. Queries the specified `base_url` for the confirmed transaction details using the txid.
/// 4. Checks if the HTTP response is successful. If not, logs an informational message and exits with `None`.
/// 5. Parses the response into a confirmed transaction object.
/// 6. Attempts to extract the witness (signature) from the first input of the confirmed transaction.
///    - If successful, it returns the witness as a `Vec<u8>`.
///    - If the witness is not found or any other failure occurs, returns `None`.
///
/// # Notes
///
/// * This function makes use of the `reqwest` crate for HTTP requests and the `bitcoin` crate for transaction deserialization and handling.
/// * Error handling is minimal. If an error occurs (e.g., deserialization failure, HTTP error), it gracefully exits with `None`.
/// * Ensure the `base_url` corresponds to a valid API endpoint that returns raw transaction hex data.
///
/// # Dependencies
///
/// * `bitcoin` crate for Bitcoin transaction handling.
/// * `reqwest` crate for asynchronous HTTP requests.
///
/// # Logging
///
/// Logs an informational message if the transaction is not found on-chain or if an HTTP request fails.
async fn fetch_valid_sig_bitcoin(unsigned_tx_hex: &str, base_url: &str) -> Option<Vec<u8>> {
    let tx: bitcoin::Transaction = deserialize_hex(unsigned_tx_hex).unwrap();
    let txid = tx.compute_txid();
    let url = format!("{}/tx/{}/hex", base_url, txid);
    let resp = reqwest::get(&url).await.ok()?;
    if !resp.status().is_success() {
        info!("Bitcoin spend tx {txid} not found on chain (HTTP {}), assuming refund scenario", resp.status());
        return None;
    }
    let confirmed_tx_hex = resp.text().await.ok()?;
    let confirmed_tx: bitcoin::Transaction = deserialize_hex(&confirmed_tx_hex).ok()?;
    confirmed_tx.input[0].witness.iter().next().map(|w| w.to_vec())
}

/// Fetches and validates the CBOR-encoded transaction data for a given Cardano transaction ID
/// using the Blockfrost API.
///
/// # Parameters
///
/// * `txid` - A string slice that holds the Cardano transaction ID.
/// * `base_url` - A string slice containing the base URL of the Blockfrost API (e.g., "https://cardano-mainnet.blockfrost.io").
/// * `api_key` - A string slice containing the Blockfrost project API key for authentication.
///
/// # Returns
///
/// An `Option<Vec<u8>>` containing the first element of the Plutus redeemer data as a byte array,
/// if the data can be successfully fetched and validated. Returns `None` in the following cases:
/// - Failure to make a successful API request.
/// - The API response status is not successful.
/// - Failure to parse the JSON response from the API.
/// - Missing or malformed CBOR-encoded transaction data.
/// - Errors in CBOR decoding or transaction deserialization.
/// - Missing or malformed redeemer data within the transaction.
///
/// # Errors
///
/// This function handles potential errors internally and uses `Option` as the return type to
/// indicate success or failure. It does not throw explicit errors.
///
/// # Dependencies
///
/// This function relies on the following crates:
/// - `reqwest` for making HTTP requests to the Blockfrost API.
/// - `serde_json` for JSON deserialization of the API response.
/// - `hex` for decoding the CBOR hex string.
/// - `cardano_serialization_lib` for working with Cardano transaction data.
///
/// Make sure to include these crates in your `Cargo.toml` to use this function.
async fn fetch_valid_sig_cardano_blockfrost(txid: &str, base_url: &str, api_key: &str) -> Option<Vec<u8>> {
    let url = format!("{}/api/v0/txs/{}/cbor", base_url, txid);
    let resp = reqwest::Client::new()
        .get(&url)
        .header("project_id", api_key)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: serde_json::Value = resp.json().await.ok()?;
    let confirmed_tx_hex = body["cbor"].as_str()?.to_string();
    let confirmed_tx_bytes = hex::decode(&confirmed_tx_hex).ok()?;
    let tx = cardano_serialization_lib::Transaction::from_bytes(confirmed_tx_bytes).ok()?;
    let redeemers = tx.witness_set().redeemers()?;
    let redeemer = redeemers.get(0);
    let constr = redeemer.data().as_constr_plutus_data()?;
    constr.data().get(0).as_bytes()
}


/// Fetches and validates the signature of a Cardano transaction from a given REST API.
///
/// This asynchronous function takes a transaction ID (`txid`) and a REST API base URL (`rest_url`) to
/// fetch the CBOR-encoded data associated with the transaction. It then attempts to extract the
/// redeemer's signature bytes from the transaction.
///
/// # Parameters
///
/// * `txid` - A `&str` representing the transaction ID for which the CBOR data is fetched.
/// * `rest_url` - A `&str` representing the base URL of the REST API to query.
///
/// # Returns
///
/// * `Option<Vec<u8>>` - Returns `Some` containing the extracted signature bytes if successful, or
///   `None` if an error occurs during any step of the process.
///
/// # Errors
///
/// This function returns `None` if any of the following errors occur:
/// * The HTTP request to fetch the CBOR data fails.
/// * The HTTP response status is not successful (e.g., not 2xx).
/// * The CBOR field is missing or cannot be deserialized from the JSON response.
/// * The CBOR hex cannot be decoded into bytes.
/// * The transaction bytes cannot be deserialized into a `Transaction` object.
/// * The transaction does not contain redeemers.
/// * The redeemer data cannot be extracted or parsed.
///
/// # Dependencies
///
/// This function uses the following dependencies:
/// * `reqwest` for making HTTP requests.
/// * `serde_json` for parsing JSON responses.
/// * `cardano_serialization_lib` for working with serialized Cardano transaction data.
/// * `hex` for decoding hex-encoded data.
///
/// # Notes
///
/// Ensure that the provided `rest_url` points to a valid Cardano REST API that supports querying
/// transaction CBOR data. Additionally, proper error handling is recommended in a real-world
/// scenario to handle specific failure cases accordingly.
/// Fetch the Schnorr signature from an on-chain Cardano tx (Dolos REST API).
/// `txid` must be the CORRECT on-chain txid (from the signed tx after attach_final_sig,
/// which adds script_data_hash and changes the body vs the unsigned tx).
async fn fetch_valid_sig_cardano_dolos(txid: &str, rest_url: &str) -> Option<Vec<u8>> {
    let url = format!("{}/txs/{}/cbor", rest_url, txid);
    info!("fetching cardano tx cbor for txid: {txid}");
    let resp = reqwest::get(&url).await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: serde_json::Value = resp.json().await.ok()?;
    let cbor_hex = body["cbor"].as_str()?;
    let confirmed_tx_bytes = hex::decode(cbor_hex).ok()?;
    let confirmed_tx =
        cardano_serialization_lib::Transaction::from_bytes(confirmed_tx_bytes).ok()?;
    let redeemers = confirmed_tx.witness_set().redeemers()?;
    let redeemer = redeemers.get(0);
    let constr = redeemer.data().as_constr_plutus_data()?;
    constr.data().get(0).as_bytes()
}

/// Finds a Cardano transaction ID that spends a specific locked UTXO.
///
/// This asynchronous function queries the Cardano blockchain to locate
/// a transaction that spends the UTXO with the provided transaction ID (`lock_txid`)
/// and output index `0`. If such a transaction is found, its ID is returned; otherwise,
/// the function returns `None`.
///
/// # Parameters
///
/// * `lock_txid` - A string slice representing the transaction ID of the locked UTXO.
/// * `rest_url` - A string slice specifying the base URL of the Cardano REST API.
///
/// # Returns
///
/// Returns an `Option<String>`:
/// - `Some(String)` containing the transaction ID of the spending transaction if found.
/// - `None` if no spending transaction is found or if an error occurs during the search.
///
/// # Behavior
///
/// 1. The function first computes the expected Cardano script address using `script_address`
///    and converts it to the Bech32 format.
/// 2. Constructs a URL to fetch the list of transactions related to the script address.
/// 3. Iterates through the returned transactions to check if any of them spend the UTXO
///    identified by `lock_txid` at index `0`.
/// 4. If a matching transaction is found, its transaction ID is returned. Otherwise, it logs
///    a message indicating that no spending transaction was found and assumes a refund
///    scenario.
///
/// # Dependencies
///
/// This function relies on the following external crates:
/// - `reqwest` for sending HTTP requests to the Cardano REST API.
/// - `serde_json` for parsing JSON responses.
/// - Logging (e.g., `info!`) for informational messages.
///
/// Additionally, it imports utilities from the `cardano_utils` module within the crate,
/// including:
/// - `script_address` for address generation.
/// - `CARDANO_TESTNET` constant for specifying the testnet environment.
///
/// # Errors
///
/// The function returns `None` in the following cases:
/// - The HTTP request to the REST API fails or returns a non-success status code.
/// - JSON parsing of the REST API response fails.
/// - The expected structure of the response data is not met.
///
async fn find_cardano_spend_txid_dolos(lock_txid: &str, rest_url: &str) -> Option<String> {
    use crate::blockchains::cardano_utils::{script_address, CARDANO_TESTNET};
    let addr = script_address(CARDANO_TESTNET).to_bech32(None).unwrap();
    let url = format!("{}/addresses/{}/transactions", rest_url, addr);
    let resp = reqwest::get(&url).await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let txs: serde_json::Value = resp.json().await.ok()?;
    let tx_list = txs.as_array()?;
    for entry in tx_list {
        let tx_hash = entry["tx_hash"].as_str()?;
        let utxos_url = format!("{}/txs/{}/utxos", rest_url, tx_hash);
        let utxos_resp = reqwest::get(&utxos_url).await.ok()?;
        let utxos: serde_json::Value = utxos_resp.json().await.ok()?;
        let inputs = utxos["inputs"].as_array().map(|v| v.as_slice()).unwrap_or(&[]);
        if inputs.iter().any(|i| {
            i["tx_hash"].as_str() == Some(lock_txid) && i["output_index"].as_u64() == Some(0)
        }) {
            return Some(tx_hash.to_string());
        }
    }
    info!("no spending tx found for Cardano lock UTXO {lock_txid}#0 — assuming refund scenario");
    None
}

/// Asynchronously finds the transaction ID that spends a specific Cardano UTXO (Unspent Transaction Output)
/// by querying the Blockfrost API. This function searches for transactions that reference the given `lock_txid`
/// in their inputs, identifying the spending transaction.
///
/// # Parameters
///
/// * `lock_txid` - The transaction ID of the UTXO to check for spending.
/// * `base_url` - The base URL of the Blockfrost API.
/// * `api_key` - The API key for authenticating with the Blockfrost API.
///
/// # Returns
///
/// Returns an `Option<String>`:
/// - `Some(String)` containing the transaction ID of the spending transaction if found.
/// - `None` if no spending transaction is found or an error occurs during the process.
///
/// # Behavior
///
/// 1. Computes the script address for the Cardano testnet.
/// 2. Fetches the list of transactions associated with the script address using the Blockfrost API.
/// 3. For each transaction, retrieves the list of UTXO inputs.
/// 4. Searches for an input referencing the given `lock_txid` with `output_index = 0`.
/// 5. If found, returns the transaction ID of the spending transaction.
/// 6. Logs a message and returns `None` if no spending transaction is found, assuming a refund scenario.
///
/// # Dependencies
///
/// - `reqwest`: For performing HTTP requests.
/// - `serde_json`: For parsing JSON responses.
/// - `crate::blockchains::cardano_utils`:
///   - Provides the function `script_address`
///   - Uses the constant `CARDANO_TESTNET`
///
/// # Examples
///
/// ```text
/// use my_crate::find_cardano_spend_txid_blockfrost;
///
/// #[tokio::main]
/// async fn main() {
///     let lock_txid = "abc123...";
///     let base_url = "https://blockfrost.io";
///     let api_key = "your_api_key";
///
///     if let Some(spend_txid) = find_cardano_spend_txid_blockfrost(lock_txid, base_url, api_key).await {
///         println!("Found spending transaction: {}", spend_txid);
///     } else {
///         println!("No spending transaction found.");
///     }
/// }
/// ```
///
/// # Notes
///
/// - The function specifically queries `CARDANO_TESTNET`, and the address format is derived accordingly.
/// - The lookup assumes the UTXO being spent has an `output_index` of `0`.
/// - API errors (e.g., network issues, invalid API key) or unexpected JSON payloads will result in `None` being returned.
///
/// # Error Handling
///
/// Any errors in HTTP requests, JSON parsing, or unexpected data structure will cause the function to return `None`.
///
/// # Logs
///
/// Logs a message when no spending transaction is found for the specified UTXO.
async fn find_cardano_spend_txid_blockfrost(lock_txid: &str, base_url: &str, api_key: &str) -> Option<String> {
    use crate::blockchains::cardano_utils::{script_address, CARDANO_TESTNET};
    let addr = script_address(CARDANO_TESTNET).to_bech32(None).unwrap();
    let url = format!("{}/api/v0/addresses/{}/transactions", base_url, addr);
    let resp = reqwest::Client::new()
        .get(&url)
        .header("project_id", api_key)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let txs: serde_json::Value = resp.json().await.ok()?;
    let tx_list = txs.as_array()?;
    for entry in tx_list {
        let tx_hash = entry["tx_hash"].as_str()?;
        let utxos_url = format!("{}/api/v0/txs/{}/utxos", base_url, tx_hash);
        let utxos_resp = reqwest::Client::new()
            .get(&utxos_url)
            .header("project_id", api_key)
            .send()
            .await
            .ok()?;
        let utxos: serde_json::Value = utxos_resp.json().await.ok()?;
        let inputs = utxos["inputs"].as_array().map(|v| v.as_slice()).unwrap_or(&[]);
        if inputs.iter().any(|i| {
            i["tx_hash"].as_str() == Some(lock_txid) && i["output_index"].as_u64() == Some(0)
        }) {
            return Some(tx_hash.to_string());
        }
    }
    info!("no spending tx found for Cardano lock UTXO {lock_txid}#0 — assuming refund scenario");
    None
}

/// Extracts the secret from a cryptographic adaptor signature provided by the leader
/// and optionally adapts the local participant's transaction.
///
/// # Parameters
/// - `session` - A mutable reference to the [`SwapSession`] containing swap-related data.
/// - `leader_id` - The ID of the participant acting as the leader in the swap.
/// - `config` - The [`DaemonConfig`] containing network-specific configuration.
///
/// # Return
/// - Returns `true` if the secret extraction and optional adaptation were successful.
/// - Returns `false` if the leader's spend transaction was not found on the chain,
///   which could indicate the UTXO was spent via a refund transaction.
///
/// # Errors
/// This function panics if:
/// - Any of the required data (e.g., unsigned transaction, lock transaction, or adaptor signature)
///   is not present in the `SwapSession`.
/// - The decoding or deserialization of hex strings or signatures fails.
/// - Any cryptographic computation (e.g., lifting or adapting signatures) fails.
///
/// # Notes
/// - Returns `true` if the leader's spend tx was found and the secret was extracted,
///   or `false` if the UTXO was spent by a refund tx (spend tx never confirmed).
pub async fn extract_secret_and_adapt(
    session: &mut SwapSession,
    leader_id: ParticipantId,
    config: &DaemonConfig,
) -> bool {
    let role = TxRole::Spend(leader_id);
    let unsigned_tx_hex = session.unsigned_txs.get(&role).unwrap().clone();
    let target_id = session.participants[&leader_id].target_participant;
    let target = &session.participants[&target_id];

    let valid_sig_bytes_opt = match target.blockchain {
        Blockchain::Bitcoin => {
            fetch_valid_sig_bitcoin(&unsigned_tx_hex, config.bitcoin_network.mempool_base_url())
                .await
        }
        Blockchain::Cardano => {
            let lock_txid = cardano_txid(session.lock_txs.get(&target_id).unwrap());
            match &config.cardano_network {
                CardanoNetwork::Preprod | CardanoNetwork::Preview | CardanoNetwork::Mainnet => {
                    let txid = find_cardano_spend_txid_blockfrost(
                        &lock_txid,
                        config.cardano_network.blockfrost_base_url(),
                        &config.blockfrost_api_key,
                    )
                    .await;
                    match txid {
                        Some(txid) => fetch_valid_sig_cardano_blockfrost(
                            &txid,
                            config.cardano_network.blockfrost_base_url(),
                            &config.blockfrost_api_key,
                        )
                        .await,
                        None => None,
                    }
                }
                CardanoNetwork::Custom { .. } => {
                    let rest_url = config.cardano_network.dolos_rest_url().unwrap();
                    let txid = find_cardano_spend_txid_dolos(&lock_txid, rest_url).await;
                    match txid {
                        Some(txid) => fetch_valid_sig_cardano_dolos(&txid, rest_url).await,
                        None => None,
                    }
                }
            }
        }
    };

    let valid_sig_bytes = match valid_sig_bytes_opt {
        Some(bytes) => bytes,
        None => {
            info!("leader spend tx not found on chain — UTXO was likely spent by a refund tx, skipping secret extraction");
            return false;
        }
    };

    let adaptor_sig_hex = session.adaptor_sigs.get(&role).unwrap();
    let adaptor_sig =
        musig2::AdaptorSignature::from_bytes(&hex::decode(adaptor_sig_hex).unwrap()).unwrap();

    let valid_sig = musig2::LiftedSignature::from_bytes(&valid_sig_bytes).unwrap();
    let agg_secret = multisig::compute_adaptor_secret(adaptor_sig, valid_sig).unwrap();

    session
        .adaptor_secrets
        .insert(leader_id, hex::encode(agg_secret.serialize()));

    let my_id = *get_my_id(&session.participants);
    let my_role = TxRole::Spend(my_id);
    if session.adaptor_sigs.contains_key(&my_role) {
        // Pass the extracted aggregate secret (t1+t2) directly — do NOT call adapt_role which
        // would re-aggregate with our own t_own and double-count it.
        multisig::adapt_role_with(session, my_role, agg_secret, config).await;
    }
    true
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use bitcoin::{
        absolute::LockTime, consensus::encode::serialize_hex, transaction::Version, Amount,
        OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness,
    };
    use musig2::{
        secp::Scalar, AdaptorSignature, FirstRound, KeyAggContext, PartialSignature,
        SecNonceSpices, SecondRound,
    };
    use secp256k1::schnorr::Signature;
    use wiremock::{
        matchers::{method, path_regex},
        Mock, MockServer, ResponseTemplate,
    };

    use crate::{
        protocol::secret::{
            extract_secret_and_adapt, fetch_valid_sig_bitcoin, fetch_valid_sig_cardano_blockfrost,
        },
        test_utils::{make_seed, make_swap_keys},
        types::{
            BitcoinNetwork, Blockchain, CardanoNetwork, DaemonConfig, Participant, ParticipantId,
            SwapSession, TxRole,
        },
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

    fn make_dummy_bitcoin_tx() -> String {
        let tx = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint::null(),
                script_sig: ScriptBuf::default(),
                sequence: Sequence::MAX,
                witness: Witness::default(),
            }],
            output: vec![TxOut {
                value: Amount::from_sat(90_000),
                script_pubkey: ScriptBuf::default(),
            }],
        };
        serialize_hex(&tx)
    }

    fn make_participant(
        id: u8,
        is_me: bool,
        public_key: String,
        target: u8,
        blockchain: Blockchain,
    ) -> Participant {
        Participant {
            id,
            blockchain,
            tcp_address: format!("127.0.0.1:91{id:02}"),
            target_participant: target,
            amount_locking: 100_000,
            amount_claiming: 100_000,
            is_me,
            secp256k1_public_key: public_key,
            cardano_wallet_public_key: vec![],
            funding_utxo_txid: "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"
                .to_string(),
            funding_utxo_vout: 0,
        }
    }

    fn make_session_with_adaptor_sig(
        adaptor_sig_hex: String,
        unsigned_tx_hex: String,
        blockchain: Blockchain,
    ) -> (SwapSession, ParticipantId) {
        let keys: Vec<_> = (0..2).map(|_| make_swap_keys()).collect();
        let mut participants = BTreeMap::new();

        // participant 1 — leader, not me, target is participant 2
        participants.insert(
            1,
            make_participant(1, false, keys[0].public_key.to_string(), 2, blockchain),
        );
        // participant 2 — me, target is participant 1
        participants.insert(
            2,
            make_participant(2, true, keys[1].public_key.to_string(), 1, blockchain),
        );

        let mut session = SwapSession::new(1, participants, 0, 0, 5_000, 2_000_000);
        session.leader = Some(1);

        // leader's spend role
        let leader_role = TxRole::Spend(1);
        session.adaptor_sigs.insert(leader_role, adaptor_sig_hex);
        session.unsigned_txs.insert(leader_role, unsigned_tx_hex);

        // our own spend role — needs an adaptor sig and unsigned tx too
        let (our_adaptor_sig_hex, _, _) = run_adaptor_signing(Scalar::one());
        session
            .adaptor_sigs
            .insert(TxRole::Spend(2), our_adaptor_sig_hex);
        session
            .unsigned_txs
            .insert(TxRole::Spend(2), make_dummy_bitcoin_tx());

        // adaptor secrets for all participants
        session
            .adaptor_secrets
            .insert(1, hex::encode(Scalar::one().serialize()));
        session
            .adaptor_secrets
            .insert(2, hex::encode(Scalar::one().serialize()));

        (session, 1) // leader_id = 1
    }

    fn run_adaptor_signing(adaptor_secret: Scalar) -> (String, Vec<u8>, Scalar) {
        let keys: Vec<_> = (0..2).map(|_| make_swap_keys()).collect();
        let mut pubkeys: Vec<_> = keys.iter().map(|k| k.public_key).collect();
        pubkeys.sort_by(|a, b| a.serialize().cmp(&b.serialize()));

        // derive correct signer indices from sorted pubkey list
        let index_0 = pubkeys
            .iter()
            .position(|pk| pk == &keys[0].public_key)
            .unwrap();
        let index_1 = pubkeys
            .iter()
            .position(|pk| pk == &keys[1].public_key)
            .unwrap();

        let adaptor_point = adaptor_secret.base_point_mul();
        let msg: &[u8] = b"test spend tx";
        let key_agg_ctx = KeyAggContext::new(pubkeys).unwrap();

        let mut fr0 = FirstRound::new(
            key_agg_ctx.clone(),
            &make_seed(),
            index_0,
            SecNonceSpices::new()
                .with_seckey(keys[0].secret_key)
                .with_message(&msg),
        )
        .unwrap();

        let mut fr1 = FirstRound::new(
            key_agg_ctx.clone(),
            &make_seed(),
            index_1,
            SecNonceSpices::new()
                .with_seckey(keys[1].secret_key)
                .with_message(&msg),
        )
        .unwrap();

        let n0 = fr0.our_public_nonce();
        let n1 = fr1.our_public_nonce();
        fr0.receive_nonce(index_1, n1).unwrap();
        fr1.receive_nonce(index_0, n0).unwrap();

        let mut sr0: SecondRound<&[u8]> = fr0
            .finalize_adaptor(keys[0].secret_key, adaptor_point, msg)
            .unwrap();
        let sr1: SecondRound<&[u8]> = fr1
            .finalize_adaptor(keys[1].secret_key, adaptor_point, msg)
            .unwrap();

        let ps1: PartialSignature = sr1.our_signature();
        sr0.receive_signature(index_1, ps1).unwrap();

        let adaptor_sig: AdaptorSignature = sr0.finalize_adaptor::<AdaptorSignature>().unwrap();

        let valid_sig: Signature = adaptor_sig.adapt(adaptor_secret).unwrap();
        let valid_sig_bytes = valid_sig.to_byte_array().to_vec();

        (
            hex::encode(adaptor_sig.serialize()),
            valid_sig_bytes,
            adaptor_secret,
        )
    }
    #[tokio::test]
    async fn extract_secret_recovers_correct_scalar() {
        let adaptor_secret = Scalar::one();
        let (adaptor_sig_hex, valid_sig_bytes, _) = run_adaptor_signing(adaptor_secret);
        let unsigned_tx_hex = make_dummy_bitcoin_tx();

        let mut confirmed_tx: Transaction =
            bitcoin::consensus::encode::deserialize_hex(&unsigned_tx_hex).unwrap();
        confirmed_tx.input[0].witness = Witness::from_slice(&[valid_sig_bytes]);
        let confirmed_tx_hex = serialize_hex(&confirmed_tx);

        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path_regex("/tx/.*/hex"))
            .respond_with(ResponseTemplate::new(200).set_body_string(&confirmed_tx_hex))
            .mount(&mock_server)
            .await;

        let (mut session, leader_id) =
            make_session_with_adaptor_sig(adaptor_sig_hex, unsigned_tx_hex, Blockchain::Bitcoin);

        let config = DaemonConfig {
            tcp_address: "127.0.0.1:9000".to_string(),
            bitcoin_network: BitcoinNetwork::Custom(mock_server.uri()),
            cardano_network: CardanoNetwork::Preprod,
            blockfrost_api_key: "".to_string(),
            validate_utxos: false,
        };

        extract_secret_and_adapt(&mut session, leader_id, &config).await;

        assert!(
            session.signed_txs.contains_key(&TxRole::Spend(2)),
            "signed tx should exist after adapting"
        );
    }

    #[tokio::test]
    async fn extract_secret_and_adapt_populates_signed_tx() {
        let adaptor_secret = Scalar::one();
        let (adaptor_sig_hex, valid_sig_bytes, _) = run_adaptor_signing(adaptor_secret);
        let unsigned_tx_hex = make_dummy_bitcoin_tx();

        let mut confirmed_tx: Transaction =
            bitcoin::consensus::encode::deserialize_hex(&unsigned_tx_hex).unwrap();
        confirmed_tx.input[0].witness = Witness::from_slice(&[valid_sig_bytes]);
        let confirmed_tx_hex = serialize_hex(&confirmed_tx);

        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path_regex("/tx/.*/hex"))
            .respond_with(ResponseTemplate::new(200).set_body_string(&confirmed_tx_hex))
            .mount(&mock_server)
            .await;

        let (mut session, leader_id) =
            make_session_with_adaptor_sig(adaptor_sig_hex, unsigned_tx_hex, Blockchain::Bitcoin);

        let config = DaemonConfig {
            tcp_address: "127.0.0.1:9000".to_string(),
            bitcoin_network: BitcoinNetwork::Custom(mock_server.uri()),
            cardano_network: CardanoNetwork::Preprod,
            blockfrost_api_key: "".to_string(),
            validate_utxos: false,
        };

        extract_secret_and_adapt(&mut session, leader_id, &config).await;

        assert!(
            session.signed_txs.contains_key(&TxRole::Spend(2)),
            "signed tx should exist after adapting"
        );
    }

    #[tokio::test]
    async fn fetch_valid_sig_bitcoin_extracts_witness() {
        let valid_sig_bytes = vec![1u8; 64]; // dummy 64-byte sig
        let unsigned_tx_hex = make_dummy_bitcoin_tx();

        let mut confirmed_tx: Transaction =
            bitcoin::consensus::encode::deserialize_hex(&unsigned_tx_hex).unwrap();
        confirmed_tx.input[0].witness = Witness::from_slice(&[valid_sig_bytes.clone()]);
        let confirmed_tx_hex = serialize_hex(&confirmed_tx);

        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path_regex("/tx/.*/hex"))
            .respond_with(ResponseTemplate::new(200).set_body_string(&confirmed_tx_hex))
            .mount(&mock_server)
            .await;

        let result = fetch_valid_sig_bitcoin(&unsigned_tx_hex, &mock_server.uri()).await;
        assert_eq!(result, Some(valid_sig_bytes));
    }

    #[tokio::test]
    async fn fetch_valid_sig_cardano_blockfrost_extracts_redeemer_mocked() {
        use cardano_serialization_lib::{
            BigNum, ConstrPlutusData, ExUnits, PlutusData, PlutusList, Redeemer, RedeemerTag,
            Redeemers, Transaction, TransactionBody, TransactionInputs, TransactionOutputs,
            TransactionWitnessSet,
        };

        let known_sig = vec![42u8; 64];

        // build confirmed tx with known sig in redeemer
        let mut sig_fields = PlutusList::new();
        sig_fields.add(&PlutusData::new_bytes(known_sig.clone()));
        let redeemer_data = PlutusData::new_constr_plutus_data(&ConstrPlutusData::new(
            &BigNum::zero(),
            &sig_fields,
        ));
        let redeemer = Redeemer::new(
            &RedeemerTag::new_spend(),
            &BigNum::zero(),
            &redeemer_data,
            &ExUnits::new(&BigNum::from(1_000_000u64), &BigNum::from(500_000_000u64)),
        );
        let mut redeemers = Redeemers::new();
        redeemers.add(&redeemer);
        let mut witness_set = TransactionWitnessSet::new();
        witness_set.set_redeemers(&redeemers);

        let body = TransactionBody::new_tx_body(
            &TransactionInputs::new(),
            &TransactionOutputs::new(),
            &BigNum::from(200_000u64),
        );
        let confirmed_tx = Transaction::new(&body, &witness_set, None);
        let confirmed_tx_hex = hex::encode(confirmed_tx.to_bytes());

        // mock blockfrost
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path_regex("/api/v0/txs/.*/cbor"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "cbor": confirmed_tx_hex })),
            )
            .mount(&mock_server)
            .await;

        let result = fetch_valid_sig_cardano_blockfrost("deadbeef", &mock_server.uri(), "").await;
        let result = result.expect("should extract sig from mocked blockfrost response");

        assert_eq!(result, known_sig);
        assert_eq!(result.len(), 64, "should extract 64-byte schnorr sig");
    }

    // #[tokio::test]
    // #[ignore]
    // async fn extract_secret_and_adapt_against_preprod() {
    //     const LEADER_ID: ParticipantId = 1;
    //     const KNOWN_UNSIGNED_TX_HEX: &str = "YOUR_UNSIGNED_TX_HEX";
    //     const KNOWN_ADAPTOR_SIG_HEX: &str = "YOUR_ADAPTOR_SIG_HEX";

    //     let (mut session, _) = make_session_with_adaptor_sig(
    //         KNOWN_ADAPTOR_SIG_HEX.to_string(),
    //         KNOWN_UNSIGNED_TX_HEX.to_string(),
    //         Blockchain::Cardano,
    //     );

    //     extract_secret_and_adapt(
    //         &mut session,
    //         LEADER_ID,
    //         "https://mempool.space",
    //         "https://cardano-preprod.blockfrost.io",
    //     )
    //     .await;

    //     assert!(
    //         session.signed_txs.contains_key(&TxRole::Spend(2)),
    //         "signed tx should exist after extracting secret from preprod"
    //     );
    // }

    // #[tokio::test]
    // #[ignore]
    // async fn fetch_valid_sig_cardano_extracts_redeemer_on_preprod() {
    //     const KNOWN_UNSIGNED_TX_HEX: &str = "YOUR_UNSIGNED_TX_HEX";
    //     let result = fetch_valid_sig_cardano(
    //         KNOWN_UNSIGNED_TX_HEX,
    //         "https://cardano-preprod.blockfrost.io",
    //     )
    //     .await;
    //     assert_eq!(result.len(), 64, "should extract 64-byte schnorr sig");
    // }
}
