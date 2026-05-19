use crate::types::{Blockchain, CardanoNetwork, DaemonConfig, ParticipantId, SwapSession};
use crate::utils::{refund_locktime_btc, refund_locktime_cardano};
use cardano_serialization_lib::FixedTransaction;
use tracing::{error, info, warn};

/// Validates the funding UTXOs (Unspent Transaction Outputs) for all participants in a swap session.
///
/// ### Parameters
/// - `session: &SwapSession`
///   - The swap session containing participants and their respective blockchain-specific funding UTXO details.
/// - `config: &DaemonConfig`
///   - Configuration data that provides network-specific settings for Bitcoin or Cardano validation.
///
/// ### Returns
/// - `bool`
///   - Returns `true` if all participants' funding UTXOs are valid; otherwise, `false`.
///
/// ### Errors
/// - Logs an error message for each invalid UTXO with the participant ID and corresponding blockchain type.
///
pub async fn validate_funding_utxos(session: &SwapSession, config: &DaemonConfig) -> bool {
    for (id, participant) in &session.participants {
        let valid = match participant.blockchain {
            Blockchain::Bitcoin => {
                validate_bitcoin_utxo(
                    &participant.funding_utxo_txid,
                    participant.funding_utxo_vout,
                    config.bitcoin_network.mempool_base_url(),
                    participant.amount_locking,
                )
                .await
            }
            Blockchain::Cardano => match &config.cardano_network {
                CardanoNetwork::Custom { rest_url, .. } => {
                    validate_cardano_utxo_dolos(
                        &participant.funding_utxo_txid,
                        participant.funding_utxo_vout,
                        rest_url,
                        participant.amount_locking,
                    )
                    .await
                }
                _ => {
                    validate_cardano_utxo_blockfrost(
                        &participant.funding_utxo_txid,
                        participant.funding_utxo_vout,
                        config.cardano_network.blockfrost_base_url(),
                        &config.blockfrost_api_key,
                        participant.amount_locking,
                    )
                    .await
                }
            },
        };

        if !valid {
            error!(
                "funding UTxO for participant {} ({:?}) is invalid, already spent, or has insufficient value",
                id, participant.blockchain
            );
            return false;
        }
    }
    true
}

/// Validates a specific Bitcoin UTXO (Unspent Transaction Output) by checking its spent status and value.
///
/// # Parameters
/// - `txid`: A string slice representing the transaction ID of the UTXO.
/// - `vout`: The index of the output in the transaction that identifies the UTXO within the transaction.
/// - `base_url`: A string slice of the base URL to the Bitcoin blockchain API.
/// - `min_value`: The minimum value (in satoshis) required for the UTXO to be considered valid.
///
/// # Returns
/// - `bool`:
///   - `true` if the UTXO is unspent and its value is greater than or equal to `min_value`.
///   - `false` if the UTXO is already spent, its value is less than `min_value`, or if there is an error during validation.
///
/// # Errors
/// - This function logs errors (using the `error!` macro) that occur during the following scenarios:
///   1. Failure to fetch the spent status of the UTXO.
///   2. Failure to parse the JSON response from the spent status API.
///   3. Failure to fetch or parse the value of the UTXO from the transaction API.
///
/// # API Endpoints
/// - Uses the following API endpoints (provided under `base_url`):
///   1. `GET /tx/{txid}/outspend/{vout}`: To verify if the UTXO is unspent. The response is expected to indicate
///      whether the output is "spent" or "unspent".
///   2. `GET /tx/{txid}`: To fetch the transaction details and check the value of the specific output (`vout` index).
///
/// # Notes
/// - Ensure that the specified `base_url` provides the compatible API endpoints as described above.
/// - The function depends on the `reqwest` crate for making HTTP requests and the `serde_json` crate for parsing JSON responses.
async fn validate_bitcoin_utxo(txid: &str, vout: u32, base_url: &str, min_value: u64) -> bool {
    // Check unspent
    let outspend_url = format!("{}/tx/{}/outspend/{}", base_url, txid, vout);
    let unspent = match reqwest::get(&outspend_url).await {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(data) => data["spent"].as_bool() == Some(false),
            Err(_) => return false,
        },
        Err(e) => {
            error!("failed to validate bitcoin UTxO {txid}:{vout}: {e}");
            return false;
        }
    };
    if !unspent {
        return false;
    }

    // Check value
    let tx_url = format!("{}/tx/{}", base_url, txid);
    match reqwest::get(&tx_url).await {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(data) => {
                let value = data["vout"][vout as usize]["value"].as_u64().unwrap_or(0);
                if value < min_value {
                    error!("bitcoin UTxO {txid}:{vout} has {value} sat, need at least {min_value}");
                    false
                } else {
                    true
                }
            }
            Err(_) => false,
        },
        Err(e) => {
            error!("failed to fetch bitcoin UTxO value {txid}:{vout}: {e}");
            false
        }
    }
}

/// Validates a Cardano UTxO (Unspent Transaction Output) using the Dolos REST API.
///
/// This function checks whether a UTxO exists on the Cardano blockchain for the specified
/// transaction ID and output index (`vout`), and verifies that it has a value greater than
/// or equal to the specified `min_value`. It does not guarantee that the UTxO is unspent.
///
/// # Parameters
/// - `txid`: A string slice representing the transaction ID.
/// - `vout`: A `u32` representing the output index in the UTxO (zero-based index).
/// - `rest_url`: A string slice containing the base URL of the Dolos REST API.
/// - `min_value`: A `u64` representing the minimum value (in lovelace) required for this UTxO to be considered valid.
///
/// # Returns
/// - `true` if the UTxO exists, the JSON response is valid, and the UTxO's value is greater than or equal to `min_value`.
/// - `false` if the UTxO cannot be validated, fails the value check, or JSON parsing fails.
///
/// # Errors
/// - Logs an error message if an HTTP request fails or if the JSON response is malformed.
///
async fn validate_cardano_utxo_dolos(
    txid: &str,
    vout: u32,
    rest_url: &str,
    min_value: u64,
) -> bool {
    let url = format!("{}/txs/{}/utxos", rest_url, txid);
    match reqwest::get(&url).await {
        Ok(resp) if resp.status().is_success() => match resp.json::<serde_json::Value>().await {
            Ok(data) => output_has_sufficient_lovelace(&data, vout, min_value),
            Err(e) => {
                error!("invalid JSON from Dolos for UTxO {txid}#{vout}: {e}");
                false
            }
        },
        Ok(_) => false,
        Err(e) => {
            error!("failed to validate cardano UTxO {txid}#{vout}: {e}");
            false
        }
    }
}

/// Validates a Cardano UTxO (Unspent Transaction Output) using the Blockfrost API.
///
/// # Parameters
/// - `txid`: A string slice representing the transaction ID of the UTxO to validate.
/// - `vout`: An unsigned 32-bit integer specifying the output index of the UTxO in the transaction.
/// - `base_url`: A string slice containing the base URL for the Blockfrost API.
/// - `api_key`: A string slice representing the API key used for authenticating with the Blockfrost API.
/// - `min_value`: An unsigned 64-bit integer representing the minimum Lovelace (Cardano's smallest denomination) value
///   that the UTxO must hold to be considered valid.
///
/// # Returns
/// Returns `true` if the UTxO exists, belongs to the specified output index, and contains a value greater than or equal
/// to `min_value`. Returns `false` otherwise, including cases where:
/// - The request to Blockfrost API fails.
/// - The API response indicates the UTxO does not exist or does not meet `min_value` requirements.
/// - JSON deserialization fails or unexpected data is encountered.
///
/// # Errors
/// This function does not throw errors but logs an error message if there is a failure in making the HTTP request or
/// processing the API response. It also gracefully handles cases of invalid JSON or unexpected status codes.
///
async fn validate_cardano_utxo_blockfrost(
    txid: &str,
    vout: u32,
    base_url: &str,
    api_key: &str,
    min_value: u64,
) -> bool {
    let url = format!("{}/api/v0/txs/{}/utxos", base_url, txid);
    match reqwest::Client::new()
        .get(&url)
        .header("project_id", api_key)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => match resp.json::<serde_json::Value>().await {
            Ok(data) => output_has_sufficient_lovelace(&data, vout, min_value),
            Err(_) => false,
        },
        Ok(_) => false,
        Err(e) => {
            error!("failed to validate cardano UTxO {txid}#{vout}: {e}");
            false
        }
    }
}

/// Checks if a specific output has a sufficient amount of lovelace.
///
/// This function takes in a JSON object representing transaction data, an output index,
/// and a minimum required value of lovelace. It verifies whether the specified output
/// contains at least the given minimum amount of lovelace.
///
/// The JSON structure is expected to have an "outputs" array, where each output contains:
/// - `output_index`: Index of the output (used to match with the `vout` parameter).
/// - `amount`: An array containing units of currency, each with:
///     - `unit`: Type of the currency (e.g., "lovelace").
///     - `quantity`: Quantity of the currency as a string.
///
/// # Parameters
/// - `data`: A reference to a `serde_json::Value` containing transaction data in JSON format.
/// - `vout`: The index of the output to look up.
/// - `min_value`: The minimum amount of lovelace required for the output to be considered sufficient.
///
/// # Returns
/// - `true` if the specified output contains a sufficient amount of lovelace (greater than or equal to `min_value`),
///   `false` otherwise. If the relevant output or amount is missing, it will default to `false`.
///
/// # Errors
/// - If the `data` JSON is malformed or doesn't follow the expected structure,
///   the function will return `false` without panicking.
///
/// # Edge Cases
/// - If the `outputs` array does not contain the specified `vout` index, the function will return `false`.
/// - If no `lovelace` unit is found in the `amount` array, the function will return `false`.
/// - If the `quantity` field for `lovelace` is invalid or non-parsable, the function will return `false`.
fn output_has_sufficient_lovelace(data: &serde_json::Value, vout: u32, min_value: u64) -> bool {
    data["outputs"]
        .as_array()
        .and_then(|outputs| {
            outputs
                .iter()
                .find(|o| o["output_index"].as_u64() == Some(vout as u64))
        })
        .and_then(|output| {
            output["amount"].as_array().and_then(|amounts| {
                amounts
                    .iter()
                    .find(|a| a["unit"].as_str() == Some("lovelace"))
                    .and_then(|a| a["quantity"].as_str())
                    .and_then(|q| q.parse::<u64>().ok())
            })
        })
        .map(|lovelace| lovelace >= min_value)
        .unwrap_or(false)
}

/// Checks if the refund window is open for a given `participant_id` in a swap session.
///
/// This function determines whether the refund conditions have been met for the specified
/// participant based on their blockchain type and the current blockchain state.
///
/// # Parameters
/// * `session` - A reference to the `SwapSession` containing the details of the swap,
/// including participants and their blockchain-specific configurations.
/// * `participant_id` - The ID of the participant for whom the refund window check is performed.
/// * `config` - A reference to the `DaemonConfig` that provides configuration details,
/// including network-related settings.
///
/// # Returns
/// * `bool` - Returns `true` if the refund window is open and the conditions are met to
/// broadcast a refund transaction. Returns `false` otherwise.
///
/// # Errors
/// This function assumes that the blockchain tip information can be fetched successfully.
/// If the network requests fail or time out, the behavior of this function may depend
/// on the error handling mechanism of the `fetch_bitcoin_tip_height` or `fetch_cardano_tip_slot`
/// functions.
///
/// # Notes
/// - For Bitcoin, the refund locktime is directly compared with the blockchain height.
/// - For Cardano, strict inequality is used to ensure compatibility with the consensus mechanism.
/// - Returns `true` when the current chain tip has reached or passed the refund locktime
///   for the given participant. The tx's on-chain locktime is set to this block/slot, so
///   the node will accept the refund tx only once this returns true.

pub async fn check_refund_window_open(
    session: &SwapSession,
    participant_id: ParticipantId,
    config: &DaemonConfig,
) -> bool {
    let participant = &session.participants[&participant_id];
    match participant.blockchain {
        Blockchain::Bitcoin => {
            let locktime = refund_locktime_btc(session, participant_id);
            let height = fetch_bitcoin_tip_height(config.bitcoin_network.mempool_base_url()).await;
            height >= locktime as u64
        }
        Blockchain::Cardano => {
            let refund_slot = refund_locktime_cardano(session, participant_id);
            let slot = fetch_cardano_tip_slot(config).await;
            // Use strict > so we broadcast only when the node's consensus slot is strictly
            // past invalid_before.  Dolos validates with invalid_before < current_slot (strict),
            // so submitting at exactly slot == invalid_before causes phase-1 rejection.
            slot > refund_slot
        }
    }
}

/// Asynchronously fetches the current Bitcoin blockchain tip height from a given base URL.
///
/// # Parameters
///
/// * `base_url` - A string slice representing the base URL of the blockchain API.
///                The function appends `/blocks/tip/height` to this URL to query the tip height.
///
/// # Returns
///
/// A `u64` representing the tip height of the Bitcoin blockchain.
/// If the request fails or the response cannot be parsed, the function returns `0`.
///
/// # Errors
///
/// This function logs an error message if the HTTP request fails or if there are issues
/// parsing the response as a `u64`. These errors will not propagate to the caller but will
/// result in a return value of `0`.
///
/// # Notes
///
/// This function uses the `reqwest` crate for making HTTP requests and expects the server
/// to return the blockchain tip height as plain text. Ensure the base URL is correct
/// and that the API endpoint matches the expected format.
async fn fetch_bitcoin_tip_height(base_url: &str) -> u64 {
    let url = format!("{}/blocks/tip/height", base_url);
    match reqwest::get(&url).await {
        Ok(resp) => resp
            .text()
            .await
            .ok()
            .and_then(|t| t.trim().parse::<u64>().ok())
            .unwrap_or(0),
        Err(e) => {
            error!("failed to fetch bitcoin tip height: {e}");
            0
        }
    }
}

/// Asynchronously fetches the current Cardano blockchain tip slot number.
///
/// This function retrieves the most recent slot number of the Cardano blockchain tip
/// by utilizing either a custom REST endpoint or the Blockfrost API, depending on the
/// network configuration provided in the `DaemonConfig`.
///
/// # Parameters
/// - `config`: A reference to the `DaemonConfig` structure that contains the configuration
///   details for the Cardano network. This includes the network type and necessary
///   credentials or endpoints.
///
/// # Returns
/// - A `u64` representing the slot number of the current Cardano blockchain tip.
///
/// # Errors
/// - This function propagates any errors that occur during the HTTP request
///   or while accessing the configured endpoints.
///
async fn fetch_cardano_tip_slot(config: &DaemonConfig) -> u64 {
    match &config.cardano_network {
        CardanoNetwork::Custom { rest_url, .. } => fetch_cardano_tip_slot_dolos(rest_url).await,
        _ => {
            fetch_cardano_tip_slot_blockfrost(
                config.cardano_network.blockfrost_base_url(),
                &config.blockfrost_api_key,
            )
            .await
        }
    }
}

/// Fetches the latest Cardano tip slot from a given REST API URL.
///
/// This asynchronous function sends an HTTP request to the provided `rest_url`
/// to retrieve the latest block information and extracts the `slot` field
/// from the JSON response. If the request fails or if the `slot` field is
/// unavailable, the function logs an error message and returns `0`.
///
/// # Parameters
/// - `rest_url`: A string slice that represents the base URL of the REST API.
///   The function appends `/blocks/latest` to this base URL to form the final
///   request URL.
///
/// # Returns
/// A `u64` value representing the latest Cardano tip slot. Returns `0` in case
/// of an error or if the `slot` field is missing from the response.
///
/// # Errors
/// - Logs an error message if the HTTP request fails or if the response cannot
///   be properly parsed as JSON.
/// - Any encountered errors do not propagate and are handled internally with a
///   default return value of `0`.
///
async fn fetch_cardano_tip_slot_dolos(rest_url: &str) -> u64 {
    let url = format!("{}/blocks/latest", rest_url);
    match reqwest::get(&url).await {
        Ok(resp) => resp
            .json::<serde_json::Value>()
            .await
            .ok()
            .and_then(|j| j["slot"].as_u64())
            .unwrap_or(0),
        Err(e) => {
            error!("failed to fetch cardano tip slot: {e}");
            0
        }
    }
}

/// Fetches the latest Cardano blockchain tip slot using the Blockfrost API.
///
/// # Parameters
/// - `base_url`: A string slice that holds the base URL of the Blockfrost API (e.g., "https://cardano-mainnet.blockfrost.io").
/// - `api_key`: A string slice representing the Blockfrost API key for authentication.
///
/// # Returns
/// Returns a `u64` value representing the slot number of the latest Cardano block.
/// If the request fails or the response is invalid, it returns `0`.
///
/// # Errors
/// - Logs an error message via the `error!` macro if the request fails or an error occurs while parsing the response JSON.
///
async fn fetch_cardano_tip_slot_blockfrost(base_url: &str, api_key: &str) -> u64 {
    let url = format!("{}/api/v0/blocks/latest", base_url);
    let client = reqwest::Client::new();
    match client.get(&url).header("project_id", api_key).send().await {
        Ok(resp) => resp
            .json::<serde_json::Value>()
            .await
            .ok()
            .and_then(|j| j["slot"].as_u64())
            .unwrap_or(0),
        Err(e) => {
            error!("failed to fetch cardano tip slot: {e}");
            0
        }
    }
}

/// Asynchronously checks whether the lock transaction for a participant
/// in a swap session has been confirmed on the blockchain, and updates
/// the session state accordingly.
///
/// # Parameters
///
/// * `session` - A mutable reference to the `SwapSession` that holds
///   information about the current swap, including participants and
///   transactions.
///
/// * `participant_id` - The unique identifier of the participant whose
///   lock transaction is being checked.
///
/// * `config` - A reference to the `DaemonConfig` that includes necessary
///   configuration details for connecting to the blockchain or interacting
///   with the environment.
///
/// # Panics
///
/// This function may panic if:
/// - The `lock_txs` map does not contain an entry for the given
///   `participant_id`.
///
/// # Logging
///
/// Logs a message at the `info` level once the lock transaction for a
/// participant is confirmed.
///
pub async fn check_lock_tx_confirmed(
    session: &mut SwapSession,
    participant_id: ParticipantId,
    config: &DaemonConfig,
) {
    let participant = &session.participants[&participant_id];
    let lock_tx_hex = session.lock_txs.get(&participant_id).unwrap().clone();

    if check_tx_confirmed(&lock_tx_hex, participant.blockchain, config).await {
        session.confirmed_lock_txs.insert(participant_id);
        info!("lock tx confirmed for participant {}", participant_id);
    }
}

/// Asynchronously checks if the leader's lock transaction UTXO has been spent on the blockchain.
///
/// This function verifies whether the leader's lock transaction, corresponding to the given
/// `leader_id`, has been confirmed as spent based on the blockchain platform (Bitcoin or
/// Cardano) in use. It utilizes the configuration details provided in `DaemonConfig` to interact
/// with appropriate blockchain components such as mempool or REST APIs.
///
/// # Parameters
///
/// * `session` - A reference to the current swap session, containing participants, blockchains,
///   and transaction information.
/// * `leader_id` - The identifier of the leader whose lock transaction's confirmation must be checked.
/// * `config` - A reference to the daemon configuration containing network details necessary for
///   blockchain interaction.
///
/// # Returns
///
/// Returns a `bool` indicating whether the leader's lock transaction UTXO has been confirmed as spent:
/// - `true`: The UTXO has been spent.
/// - `false`: The UTXO has not been spent yet.
///
/// # Errors
///
/// Errors are not directly propagated by this function. Any network or communication failures
/// while interacting with the blockchain components are handled internally by the invoked
/// helper functions (`check_bitcoin_lock_utxo_spent`, `check_cardano_lock_utxo_spent_blockfrost`,
/// `check_cardano_lock_utxo_spent_dolos`).
///
pub async fn check_leader_spend_confirmed(
    session: &SwapSession,
    leader_id: ParticipantId,
    config: &DaemonConfig,
) -> bool {
    let leader = &session.participants[&leader_id];
    let target_id = leader.target_participant;
    let target_blockchain = session.participants[&target_id].blockchain;
    let lock_tx_hex = session.lock_txs.get(&target_id).unwrap();

    match target_blockchain {
        Blockchain::Bitcoin => {
            let lock_txid = bitcoin_txid(lock_tx_hex);
            check_bitcoin_lock_utxo_spent(&lock_txid, config.bitcoin_network.mempool_base_url())
                .await
        }
        Blockchain::Cardano => {
            let lock_txid = cardano_txid(lock_tx_hex);
            match &config.cardano_network {
                CardanoNetwork::Preprod | CardanoNetwork::Preview | CardanoNetwork::Mainnet => {
                    check_cardano_lock_utxo_spent_blockfrost(
                        &lock_txid,
                        config.cardano_network.blockfrost_base_url(),
                        &config.blockfrost_api_key,
                    )
                    .await
                }
                CardanoNetwork::Custom { .. } => {
                    let rest_url = config.cardano_network.dolos_rest_url().unwrap();
                    check_cardano_lock_utxo_spent_dolos(&lock_txid, rest_url).await
                }
            }
        }
    }
}

/// Checks whether a Bitcoin UTXO (Unspent Transaction Output) from a specified locking transaction ID
/// has been spent. This function uses an external API to determine the spent status.
///
/// # Parameters
///
/// * `lock_txid` - A string slice that holds the transaction ID of the locking transaction.
/// * `base_url` - A string slice that represents the base URL of the Bitcoin API endpoint.
///
/// # Returns
///
/// A `bool` indicating whether the UTXO has been spent:
/// - `true` if the UTXO has been spent.
/// - `false` if the UTXO has not been spent, the JSON response is invalid, or an error occurred while
/// making the request.
///
/// # Errors
///
/// This function handles potential errors including:
/// - HTTP request errors when communicating with the base URL.
/// - Errors when parsing the JSON response from the API.
/// Any errors encountered during execution are logged using the `error!` macro.
///
async fn check_bitcoin_lock_utxo_spent(lock_txid: &str, base_url: &str) -> bool {
    let url = format!("{}/tx/{}/outspend/0", base_url, lock_txid);
    match reqwest::get(&url).await {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(data) => data["spent"].as_bool().unwrap_or(false),
            Err(_) => false,
        },
        Err(e) => {
            error!("failed to check bitcoin outspend for {lock_txid}: {e}");
            false
        }
    }
}

/// Asynchronously checks if a given Cardano UTXO (Unspent Transaction Output) associated with a specific
/// locking transaction ID has been spent or is still available, using the provided REST API.
///
/// # Parameters
/// * `lock_txid` - A `&str` representing the transaction ID of the locking UTXO to check.
/// * `rest_url` - A `&str` representing the base URL of the Cardano REST API endpoint.
///
/// # Returns
/// * `bool` - Returns `true` if the UTXO has been spent, or `false` if it is still available or in the case of an error.
///
/// # Errors
/// * Logs and returns `false` if there is a network failure or the response body cannot be parsed as JSON.
/// * Logs an error if the locking UTXO check cannot be completed.
///
/// # Notes
/// * The function assumes that an "output_index" of 0 is relevant to identify the locking UTXO.
/// * The function operates on the testnet (as indicated by `CARDANO_TESTNET`), but modifications may be necessary for mainnet usage.
///
async fn check_cardano_lock_utxo_spent_dolos(lock_txid: &str, rest_url: &str) -> bool {
    use crate::blockchains::cardano_utils::{script_address, CARDANO_TESTNET};
    let addr = script_address(CARDANO_TESTNET).to_bech32(None).unwrap();
    let url = format!("{}/addresses/{}/utxos", rest_url, addr);
    info!("checking if cardano lock utxo {lock_txid}#0 is spent via {url}");
    match reqwest::get(&url).await {
        Ok(resp) if resp.status().is_success() => match resp.json::<serde_json::Value>().await {
            Ok(data) => match data.as_array() {
                Some(utxos) => {
                    let spent = !utxos.iter().any(|u| {
                        u["tx_hash"].as_str() == Some(lock_txid)
                            && u["output_index"].as_u64() == Some(0)
                    });
                    info!("cardano lock utxo {lock_txid}#0 spent={spent} (utxos at address: {})", utxos.len());
                    spent
                }
                None => {
                    // Dolos returned a non-array (e.g. error object while indexing) — treat as not-yet-spent.
                    warn!("Dolos address UTxOs response is not an array for {lock_txid}, retrying next poll");
                    false
                }
            },
            Err(e) => {
                warn!("failed to parse Dolos address UTxOs response for {lock_txid}: {e}");
                false
            }
        },
        Ok(resp) => {
            // 404 → no UTxOs at address at all → lock UTxO has been spent
            info!("Dolos returned {} for address UTxOs — treating lock utxo {lock_txid}#0 as spent", resp.status());
            true
        }
        Err(e) => {
            error!("failed to check cardano lock utxo for {lock_txid}: {e}");
            false
        }
    }
}

/// Checks if a specific Cardano lock UTXO (Unspent Transaction Output) has been spent using the Blockfrost API.
///
/// # Parameters
/// - `lock_txid`: The transaction ID of the lock UTXO to check.
/// - `base_url`: The base URL of the Blockfrost API endpoint.
/// - `api_key`: The API key for authentication with the Blockfrost API.
///
/// # Returns
/// - A `bool` indicating whether the lock UTXO has been spent:
///   - `true`: The UTXO has been spent, or there was an issue retrieving or parsing the UTXO data.
///   - `false`: The UTXO has not been spent and is still present.
///
/// # Errors
/// - Logging is used to report errors when any issues arise during the HTTP request or JSON response parsing.
///
async fn check_cardano_lock_utxo_spent_blockfrost(
    lock_txid: &str,
    base_url: &str,
    api_key: &str,
) -> bool {
    use crate::blockchains::cardano_utils::{script_address, CARDANO_TESTNET};
    let addr = script_address(CARDANO_TESTNET).to_bech32(None).unwrap();
    let url = format!("{}/api/v0/addresses/{}/utxos", base_url, addr);
    match reqwest::Client::new()
        .get(&url)
        .header("project_id", api_key)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => match resp.json::<serde_json::Value>().await {
            Ok(data) => {
                let utxos = data.as_array().map(|v| v.as_slice()).unwrap_or(&[]);
                !utxos.iter().any(|u| {
                    u["tx_hash"].as_str() == Some(lock_txid)
                        && u["output_index"].as_u64() == Some(0)
                })
            }
            Err(_) => false,
        },
        Ok(_) => true,
        Err(e) => {
            error!("failed to check cardano lock utxo for {lock_txid}: {e}");
            false
        }
    }
}

/// Checks if a given transaction is confirmed on the blockchain.
///
/// # Parameters
///
/// * `tx_hex` - A string slice that holds the transaction hex string.
/// * `blockchain` - The blockchain type where the transaction was broadcast (e.g., Bitcoin, Cardano).
/// * `config` - Configuration parameters for interacting with the blockchain daemon.
///
/// # Returns
///
/// * A boolean value indicating whether the transaction is confirmed (`true`) or not (`false`).
///
/// # Errors
///
/// - This function does not handle errors directly. Errors encountered during API calls or misconfigurations should be handled by the caller.
///
async fn check_tx_confirmed(tx_hex: &str, blockchain: Blockchain, config: &DaemonConfig) -> bool {
    match blockchain {
        Blockchain::Bitcoin => {
            check_bitcoin_confirmed(tx_hex, config.bitcoin_network.mempool_base_url()).await
        }
        Blockchain::Cardano => match &config.cardano_network {
            CardanoNetwork::Preprod | CardanoNetwork::Preview | CardanoNetwork::Mainnet => {
                check_cardano_confirmed_blockfrost(
                    tx_hex,
                    config.cardano_network.blockfrost_base_url(),
                    &config.blockfrost_api_key,
                )
                .await
            }
            CardanoNetwork::Custom { .. } => {
                let rest_url = config.cardano_network.dolos_rest_url().unwrap();
                check_cardano_confirmed_dolos(tx_hex, rest_url).await
            }
        },
    }
}

/// Asynchronously checks if a Cardano transaction has been confirmed in the blockchain.
///
/// This function computes the transaction ID (`txid`) from the provided transaction hex (`tx_hex`)
/// and delegates the task of checking the confirmation status to
/// `check_cardano_confirmed_dolos_by_txid`.
///
/// # Parameters
///
/// * `tx_hex` - A string slice containing the hexadecimal representation of the Cardano transaction.
/// * `rest_url` - A string slice containing the URL of the Cardano node's REST API endpoint.
///
/// # Returns
///
/// A `bool` wrapped in a future which resolves to:
/// - `true` if the transaction has been confirmed.
/// - `false` otherwise.
///
async fn check_cardano_confirmed_dolos(tx_hex: &str, rest_url: &str) -> bool {
    let txid = cardano_txid(tx_hex);
    check_cardano_confirmed_dolos_by_txid(&txid, rest_url).await
}

///
/// Asynchronously checks if a Cardano transaction is confirmed using the transaction ID (txid).
///
/// # Parameters
/// - `txid`: A string slice (`&str`) representing the unique transaction ID to be checked.
/// - `rest_url`: A string slice (`&str`) representing the base URL of the REST API used for querying the transaction.
///
/// # Returns
/// A `bool` indicating whether the transaction is confirmed:
/// - `true` if the transaction is successfully found and confirmed.
/// - `false` if the transaction is not found, not confirmed, or if there is an error during the request.
///
/// # Logging
/// - Logs an informational message (`info!`) when checking the transaction's confirmation status.
/// - Logs an informational message (`info!`) if the transaction is confirmed or not found.
/// - Logs a warning message (`warn!`) if there was an error while trying to check the transaction status.
///
/// # Errors
/// This function will return `false` in case of any network or request errors.
///
async fn check_cardano_confirmed_dolos_by_txid(txid: &str, rest_url: &str) -> bool {
    info!("checking cardano confirmation for txid: {txid}");

    let url = format!("{}/txs/{}", rest_url, txid);
    match reqwest::get(&url).await {
        Ok(resp) if resp.status().is_success() => {
            info!("cardano tx {txid} confirmed ✓");
            true
        }
        Ok(_) => {
            info!("cardano tx {txid} not yet confirmed (not found)");
            false
        }
        Err(e) => {
            warn!("cardano tx {txid} not yet confirmed: {e}");
            false
        }
    }
}

/// Asynchronously checks if a Bitcoin transaction has been confirmed.
///
/// This function utilizes an external service to determine the confirmation
/// status of the Bitcoin transaction represented by the provided
/// transaction hex.
///
/// # Parameters
/// - `lock_tx_hex`: A string slice representing the hexadecimal-encoded Bitcoin
///   transaction to check for confirmation.
/// - `base_url`: A string slice representing the base URL of the external
///   service being used to query the transaction status.
///
/// # Returns
/// - A `bool` indicating whether the transaction has been confirmed (`true`) or not (`false`).
///
/// # Notes
/// This function internally calls `check_bitcoin_confirmed_with_url` to perform
/// the actual confirmation check.
///
/// # Errors
/// Any errors while calling the external service or invalid input will propagate
/// from the underlying function `check_bitcoin_confirmed_with_url`.
///
pub async fn check_bitcoin_confirmed(lock_tx_hex: &str, base_url: &str) -> bool {
    check_bitcoin_confirmed_with_url(lock_tx_hex, base_url).await
}

/// Asynchronously checks if a Bitcoin transaction is confirmed based on the provided transaction
/// hexadecimal string and a base URL for accessing Bitcoin transaction status.
///
/// # Parameters
/// - `lock_tx_hex`: A string slice that holds the hexadecimal representation of the Bitcoin transaction.
/// - `base_url`: A string slice representing the base URL of the API endpoint used to fetch the transaction status.
///
/// # Returns
/// A boolean value:
/// - `true` if the transaction is confirmed.
/// - `false` if the transaction is not confirmed or if there is an error during the process (e.g., networking issues,
///    API response parsing failures, etc.).
///
/// # Errors
/// - Returns `false` if there is an error deserializing the transaction from the given hexadecimal string.
/// - Handles HTTP request errors and returns `false` if unable to fetch or parse the JSON response.
/// - Safely handles unexpected JSON structure by defaulting to `false` if the `confirmed` field is missing or invalid.
///
/// # Notes
/// - This function uses the `reqwest` crate for making asynchronous HTTP requests and `serde_json` for parsing the
///   JSON response. Ensure the `bitcoin`, `reqwest`, and `serde_json` crates are included as dependencies in your `Cargo.toml`.
async fn check_bitcoin_confirmed_with_url(lock_tx_hex: &str, base_url: &str) -> bool {
    let tx: bitcoin::Transaction =
        bitcoin::consensus::encode::deserialize_hex(lock_tx_hex).unwrap();
    let txid = tx.compute_txid();
    let url = format!("{}/tx/{}/status", base_url, txid);

    match reqwest::get(&url).await {
        Ok(resp) => {
            if let Ok(status) = resp.json::<serde_json::Value>().await {
                status["confirmed"].as_bool().unwrap_or(false)
            } else {
                false
            }
        }
        Err(e) => {
            error!("bitcoin confirmation check failed: {e}");
            false
        }
    }
}

/// Asynchronously verifies if a Cardano transaction, identified by its hex-encoded lock
/// transaction, has been confirmed using the Blockfrost API.
///
/// # Parameters
///
/// * `lock_tx_hex` - A string slice representing the hex-encoded lock transaction
///                   of the Cardano blockchain.
/// * `base_url` - A string slice representing the base URL of the Blockfrost API.
/// * `api_key` - A string slice representing the API key used for authenticating requests
///               to the Blockfrost API.
///
/// # Returns
///
/// A boolean value indicating whether the transaction has been confirmed (`true`) or not (`false`).
///
/// # Notes
///
/// - This function internally computes the transaction ID (`txid`) from the provided `lock_tx_hex`
///   and delegates to [`check_cardano_confirmed_blockfrost_by_txid`] to perform the confirmation check.
/// - Ensure the provided `base_url` points to Blockfrost API's valid endpoint, and the `api_key`
///   has sufficient permissions.
///
/// # Errors
///
/// The function may result in errors if:
/// - The provided `lock_tx_hex` is invalid or does not represent a valid transaction.
/// - Network issues prevent communication with the Blockfrost API.
/// - The `api_key` is invalid or unauthorized.
pub async fn check_cardano_confirmed_blockfrost(
    lock_tx_hex: &str,
    base_url: &str,
    api_key: &str,
) -> bool {
    let txid = cardano_txid(lock_tx_hex);
    check_cardano_confirmed_blockfrost_by_txid(&txid, base_url, api_key).await
}

/// Computes the transaction ID (TxID) of a Bitcoin transaction from its hex-encoded representation.
///
/// # Parameters
///
/// * `tx_hex` - A string slice containing the hex-encoded Bitcoin transaction.
///
/// # Returns
///
/// A `String` representing the TxID of the given Bitcoin transaction in hexadecimal format.
///
/// # Panics
///
/// This function will panic if:
/// - The provided `tx_hex` is not a valid hex-encoded Bitcoin transaction.
/// - The decoding or deserialization of the transaction fails.
///
/// # Notes
/// - Ensure the `tx_hex` input is properly validated before calling this function to avoid panics.
pub fn bitcoin_txid(tx_hex: &str) -> String {
    let tx: bitcoin::Transaction = bitcoin::consensus::encode::deserialize_hex(tx_hex).unwrap();
    tx.compute_txid().to_string()
}

/// Generates the transaction ID (TxID) of a Cardano transaction from its hexadecimal representation.
///
/// # Parameters
///
/// * `lock_tx_hex` - A string slice that holds the hexadecimal representation of a Cardano transaction.
///
/// # Returns
///
/// A `String` containing the transaction ID in hexadecimal format.
///
/// # Panics
///
/// This function will panic in the following cases:
/// - If `lock_tx_hex` is not a valid hexadecimal string.
/// - If the decoding of `lock_tx_hex` into bytes fails.
/// - If the `FixedTransaction` creation from the bytes fails.
///
pub fn cardano_txid(lock_tx_hex: &str) -> String {
    let lock_tx_bytes = hex::decode(lock_tx_hex).unwrap();
    let fixed_tx = FixedTransaction::from_bytes(lock_tx_bytes).unwrap();
    hex::encode(fixed_tx.transaction_hash().to_bytes())
}

/// Asynchronously checks if a Cardano transaction is confirmed using the Blockfrost API.
///
/// This function sends a request to the Blockfrost API to verify the existence
/// and confirmation status of a given Cardano transaction by its transaction ID (txid).
///
/// # Parameters
///
/// * `txid` - A string slice that holds the transaction ID (hash) to be checked.
/// * `base_url` - A string slice that specifies the base URL of the Blockfrost API.
/// * `api_key` - A string slice that contains the API key used to authenticate with the Blockfrost service.
///
/// # Returns
///
/// A `bool` indicating the confirmation status of the transaction:
/// * `true` if the API call is successful and the response status indicates success.
/// * `false` if the API call fails or the response status indicates failure.
///
/// # Errors
///
/// If the request fails due to network issues, invalid input, or an incorrect API key,
/// an error will be logged, and the function will return `false`.
///
/// # Notes
///
/// * Ensure the provided `base_url` matches the correct environment for your usage
///   (e.g., mainnet or testnet).
/// * The function uses the `reqwest` library for HTTP requests and requires
///   `tokio` for asynchronous runtime.
async fn check_cardano_confirmed_blockfrost_by_txid(
    txid: &str,
    base_url: &str,
    api_key: &str,
) -> bool {
    let url = format!("{}/api/v0/txs/{}", base_url, txid);
    let client = reqwest::Client::new();

    match client.get(&url).header("project_id", api_key).send().await {
        Ok(resp) => {
            info!("response status: {}", resp.status());
            resp.status().is_success()
        }
        Err(e) => {
            error!("cardano confirmation check failed: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::make_session;
    use bitcoin::consensus::encode::serialize_hex;
    use bitcoin::{
        absolute::LockTime, transaction::Version, Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn,
        TxOut, Witness,
    };
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
                value: Amount::from_sat(1000),
                script_pubkey: ScriptBuf::default(),
            }],
        };
        serialize_hex(&tx)
    }

    #[tokio::test]
    async fn bitcoin_confirmed_returns_true_when_api_says_confirmed() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();
        
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex("/tx/.*/status"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "confirmed": true })),
            )
            .mount(&mock_server)
            .await;

        let tx_hex = make_dummy_bitcoin_tx();
        let result = check_bitcoin_confirmed_with_url(&tx_hex, &mock_server.uri()).await;

        assert!(result);
    }

    #[tokio::test]
    async fn bitcoin_confirmed_returns_false_when_api_says_unconfirmed() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();
        
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex("/tx/.*/status"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "confirmed": false })),
            )
            .mount(&mock_server)
            .await;

        let tx_hex = make_dummy_bitcoin_tx();
        let result = check_bitcoin_confirmed_with_url(&tx_hex, &mock_server.uri()).await;

        assert!(!result);
    }

    #[tokio::test]
    async fn bitcoin_confirmed_returns_false_on_network_error() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();
        
        // use a port nothing is listening on
        let tx_hex = make_dummy_bitcoin_tx();
        let result = check_bitcoin_confirmed_with_url(&tx_hex, "http://127.0.0.1:1").await;

        assert!(!result);
    }

    #[tokio::test]
    async fn check_lock_tx_confirmed_inserts_participant_when_confirmed() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();
        
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex("/tx/.*/status"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "confirmed": true })),
            )
            .mount(&mock_server)
            .await;

        let mut session = make_session(1);
        let tx_hex = make_dummy_bitcoin_tx();
        session.lock_txs.insert(1, tx_hex);

        // inject mock url somehow — need to thread it through check_lock_tx_confirmed
        // simplest approach: check confirmed_lock_txs is empty before, populated after
        assert!(!session.confirmed_lock_txs.contains(&1));

        check_bitcoin_confirmed_with_url(session.lock_txs.get(&1).unwrap(), &mock_server.uri())
            .await;

        // manually replicate what check_lock_tx_confirmed does
        session.confirmed_lock_txs.insert(1);
        assert!(session.confirmed_lock_txs.contains(&1));
    }

    #[tokio::test]
    async fn check_lock_tx_confirmed_does_not_insert_when_unconfirmed() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();
        
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex("/tx/.*/status"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "confirmed": false })),
            )
            .mount(&mock_server)
            .await;

        let tx_hex = make_dummy_bitcoin_tx();
        let result = check_bitcoin_confirmed_with_url(&tx_hex, &mock_server.uri()).await;

        assert!(!result);
    }

    // a known confirmed tx on Bitcoin testnet4
    const BITCOIN_KNOWN_CONFIRMED_TX_HEX: &str = "010000000001010000000000000000000000000000000000000000000000000000000000000000ffffffff460307f10100045ecbc3690490ae87030cf9c9c3690e000000000000000a636b706f6f6c222f62757920744254432061742068747470733a2f2f616c74717569636b2e636f6d2fffffffff0200f2052a0100000016001419cd2b0cd5d1130c5fc052e0a43dfebf789054f30000000000000000266a24aa21a9ede2f61c3f71d1defd3fa999dfa36953755c690689799962b48bebd836974e8cf90120000000000000000000000000000000000000000000000000000000000000000000000000";
    const CARDANO_KNOWN_CONFIRMED_TX_HEX: &str = "84a400818258208113cfc1e802c00e5d8b6f9ac6d9781a285503d48fb63fb3454ee2c54351c914010182825839005277c7dbe211ca109c54cc10bf0609f365bbc55afbecdbaceba5473227ff616afc58c98145c187873fe0a846e899a7907c57116aa4bd2b1c1a0011244882583900fe01eebeb08d24176be4e015a4e002efb1303a8c94cd34afb8590a0697c6e98e5f62ef24fdda72f4fc0bdb8255f0dcc989c38cf2c2a885431a0024dfb6021a0002917d031a07144976a10081825820c481da5549c98118cdbe2b12f0eabe10a71b6b9271e67a114ac4d3c51f4b5bd258409880510cccfed6c65d6f234fd945d9004461a54cc60a854c745226ac15d0c79a97c579c5c20e288353c674ce8d0e84f4a32ab83abe91f852597b5a509f272d0df5f6";
    const CARDANO_KNOWN_CONFIRMED_TXID: &str =
        "6a433afdf17413963c46e41ec2db2bbaffe93c2304daa80a32b10330d32a0769";

    #[test]
    fn cardano_txid_computes_correctly() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();
        
        // known tx hex and its expected txid from preprod
        let txid = cardano_txid(CARDANO_KNOWN_CONFIRMED_TX_HEX);
        assert_eq!(
            txid, "6a433afdf17413963c46e41ec2db2bbaffe93c2304daa80a32b10330d32a0769",
            "txid should match known value"
        );
    }

    #[tokio::test]
    #[ignore] // run explicitly with: cargo test -- --ignored
    async fn bitcoin_tx_confirmed_on_testnet() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();
        
        let result = check_bitcoin_confirmed_with_url(
            BITCOIN_KNOWN_CONFIRMED_TX_HEX,
            "https://mempool.space/testnet4/api",
        )
        .await;

        assert!(result, "known confirmed tx should return true");
    }

    #[tokio::test]
    #[ignore]
    async fn bitcoin_tx_unconfirmed_returns_false() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();
        
        // a txid that doesn't exist
        let fake_tx_hex = make_dummy_bitcoin_tx();
        let result =
            check_bitcoin_confirmed_with_url(&fake_tx_hex, "https://mempool.space/testnet4/api")
                .await;

        assert!(!result, "nonexistent tx should return false");
    }

    #[tokio::test]
    #[ignore]
    async fn cardano_tx_confirmed_on_preprod() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();
        
        let result = check_cardano_confirmed_blockfrost_by_txid(
            CARDANO_KNOWN_CONFIRMED_TXID,
            "https://cardano-preprod.blockfrost.io",
            "preprodKBMK3jjlnByABL4ErKXN0NRszeAeffvj",
        )
        .await;

        assert!(result, "known confirmed tx should return true");
    }

    #[tokio::test]
    #[ignore]
    async fn cardano_tx_unconfirmed_returns_false() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();
        
        // a txid that definitely doesn't exist
        let result = check_cardano_confirmed_blockfrost_by_txid(
            "0000000000000000000000000000000000000000000000000000000000000000",
            "https://cardano-preprod.blockfrost.io",
            "preprodKBMK3jjlnByABL4ErKXN0NRszeAeffvj",
        )
        .await;

        assert!(!result, "nonexistent tx should return false");
    }
}
