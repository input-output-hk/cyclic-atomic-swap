use std::str::FromStr;

use bitcoin::{consensus::encode::serialize_hex, Network, OutPoint, Txid};
use tracing::error;
use crate::{
    blockchains::{
        bitcoin_utils::{
            self, aggregate_address, aggregate_pubkey, bitcoin_pubkeys, sign_bitcoin_lock_tx,
            submit_bitcoin_tx,
        },
        cardano_utils::{self, sign_cardano_lock_tx, submit_cardano_tx, CARDANO_MAINNET, CARDANO_TESTNET},
    },
    networking::broadcast,
    transport::tcp::{TcpTransport, TcpConnector},
    types::{BitcoinNetwork, Blockchain, CardanoNetwork, DaemonConfig, Envelope, SwapKeys, SwapSession, WireMessage},
    utils::{get_my_id, get_other_addresses, refund_locktime_cardano},
};

/// Builds lock transactions for all participants in a swap session.
///
/// # Parameters
/// - `session`: A mutable reference to the current `SwapSession`. Contains information about participants and will store the resulting lock transactions.
/// - `config`: A reference to the `DaemonConfig`, providing network configurations for Bitcoin and Cardano.
///
/// # Notes
/// - Bitcoin transactions make use of the `aggregate_address` derived from the participants' public keys and serialized using Bitcoin-specific utilities.
/// - Cardano transactions include fee calculations and are serialized differently using Cardano-specific utilities.
/// - Hex encoding is consistently applied to Cardano transactions for storage.
///
/// # Errors
/// - If `Txid::from_str` fails to parse the `funding_utxo_txid` for Bitcoin participants, the function will panic.
/// - Ensure all required fields (e.g., UTXO txids, amounts) are properly set for each participant to avoid runtime errors.
///
pub fn build_lock_txs(session: &mut SwapSession, config: &DaemonConfig) {
    let all_pubkeys = bitcoin_pubkeys(&session.participants);

    let bitcoin_network = match &config.bitcoin_network {
        BitcoinNetwork::Mainnet => Network::Bitcoin,
        BitcoinNetwork::Testnet4 => Network::Testnet,
        BitcoinNetwork::Custom(_) => Network::Regtest,
    };

    let cardano_network_byte = match &config.cardano_network {
        CardanoNetwork::Mainnet => CARDANO_MAINNET,
        CardanoNetwork::Preprod | CardanoNetwork::Preview | CardanoNetwork::Custom { .. } => CARDANO_TESTNET,
    };

    // bitcoin specific — taproot address
    let aggregate_addr = aggregate_address(all_pubkeys.clone(), bitcoin_network);

    // chain agnostic — just the pubkey, works for both
    let agg_pubkey = aggregate_pubkey(&session.participants);

    for (participant_id, participant) in &session.participants {
        match participant.blockchain {
            Blockchain::Bitcoin => {
                let funding_utxo = OutPoint {
                    txid: Txid::from_str(&participant.funding_utxo_txid).unwrap(),
                    vout: participant.funding_utxo_vout,
                };
                let lock_tx = bitcoin_utils::build_lock_tx(
                    funding_utxo,
                    participant.amount_locking,
                    aggregate_addr.clone(),
                );
                session
                    .lock_txs
                    .insert(*participant_id, serialize_hex(&lock_tx));
            }
            Blockchain::Cardano => {
                let refund_slot = refund_locktime_cardano(session, *participant_id);
                let lock_tx = cardano_utils::build_lock_tx(
                    &participant.funding_utxo_txid,
                    participant.funding_utxo_vout,
                    participant.amount_locking,
                    &agg_pubkey,
                    session.cardano_fee,
                    cardano_network_byte,
                    refund_slot,
                );
                // serialize cardano tx differently
                session
                    .lock_txs
                    .insert(*participant_id, hex::encode(lock_tx.to_bytes()));
            }
        }
    }
}

/// Broadcasts the lock transaction (lock_tx) for the current participant in the swap session.
///
/// # Parameters
/// * `session` - A mutable reference to the `SwapSession` containing the swap details and state.
/// * `keys` - Reference to the `SwapKeys` used for signing transactions.
/// * `config` - Reference to the `DaemonConfig` containing configuration for blockchain networks and connections.
///
/// # Returns
/// Returns `true` if the lock transaction was signed, submitted, and broadcast successfully.
/// Returns `false` if any of these steps fail.
///
/// # Errors
/// If the transaction signing or submission fails for either blockchain, the error is logged and `false` is returned.
/// Additionally, if the notification broadcast to other participants fails, the error is logged but does not affect the return value.
///
/// # Side Effects
/// - Updates the `lock_txs_broadcast` set in the session to include the participant's ID upon successful transaction submission.
/// - Attempts to notify other participants via a `LockTxBroadcast` message using the session's connection pool.
///
/// # Logging
/// Logs errors for:
/// - Failed signing of the lock transaction.
/// - Failed submission of the lock transaction.
/// - Failed broadcast of the lock transaction notification.
///
pub async fn broadcast_my_lock_tx(
    session: &mut SwapSession,
    keys: &SwapKeys,
    config: &DaemonConfig,
) -> bool {
    let my_id = *get_my_id(&session.participants);
    let my_participant = &session.participants[&my_id];
    let my_lock_tx_hex = session.lock_txs.get(&my_id).unwrap().clone();

    let ok = match my_participant.blockchain {
        Blockchain::Bitcoin => {
            match sign_bitcoin_lock_tx(&my_lock_tx_hex, my_participant, keys, config.bitcoin_network.mempool_base_url()).await {
                Some(signed_tx_hex) => submit_bitcoin_tx(&signed_tx_hex, config.bitcoin_network.mempool_base_url()).await,
                None => {
                    error!("failed to sign bitcoin lock tx for participant {my_id}: could not fetch prevout");
                    false
                }
            }
        }
        Blockchain::Cardano => {
            let signed_tx_hex = sign_cardano_lock_tx(&my_lock_tx_hex, keys).await;
            submit_cardano_tx(&signed_tx_hex, config).await
        }
    };

    if !ok {
        error!("failed to submit lock tx for participant {my_id}");
        return false;
    }

    session.lock_txs_broadcast.insert(my_id);

    let addresses = get_other_addresses(&session.participants);
    let envelope = Envelope::new(session.id, my_id, WireMessage::LockTxBroadcast);
    if let Err(e) = broadcast::<TcpTransport, TcpConnector>(&addresses, &envelope, &session.connection_pool).await {
        error!("broadcast lock tx notification failed: {e}");
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::make_swap_keys;
    use crate::types::{
        BitcoinNetwork, Blockchain, CardanoNetwork, DaemonConfig, Participant, SwapSession,
    };
    use std::collections::BTreeMap;

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

    fn make_participant(
        id: u8,
        is_me: bool,
        public_key: String,
        target: u8,
        blockchain: Blockchain,
        utxo_txid: String,
        utxo_vout: u32,
        amount: u64,
    ) -> Participant {
        Participant {
            id,
            blockchain,
            tcp_address: format!("127.0.0.1:91{id:02}"),
            target_participant: target,
            amount_locking: amount,
            amount_claiming: amount,
            is_me,
            secp256k1_public_key: public_key.clone(),
            cardano_wallet_public_key: vec![],
            funding_utxo_txid: utxo_txid,
            funding_utxo_vout: utxo_vout,
        }
    }

    fn make_signet_config() -> DaemonConfig {
        DaemonConfig {
            tcp_address: "127.0.0.1:9000".to_string(),
            bitcoin_network: BitcoinNetwork::Testnet4,
            cardano_network: CardanoNetwork::Preprod,
            blockfrost_api_key: "preprodKBMK3jjlnByABL4ErKXN0NRszeAeffvj".to_string(),
            validate_utxos: false,
        }
    }

    // ── unit tests (no network) ──────────────────────────────────────────────

    #[test]
    fn build_lock_txs_creates_entries_for_all_participants() {
        let keys: Vec<_> = (0..2).map(|_| make_swap_keys()).collect();
        let mut participants = BTreeMap::new();
        participants.insert(
            1,
            make_participant(
                1,
                true,
                keys[0].public_key.to_string(),
                2,
                Blockchain::Bitcoin,
                "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2".to_string(),
                0,
                10_000,
            ),
        );
        participants.insert(
            2,
            make_participant(
                2,
                false,
                keys[1].public_key.to_string(),
                1,
                Blockchain::Bitcoin,
                "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2".to_string(),
                0,
                10_000,
            ),
        );

        let mut session = SwapSession::new(1, participants, 0, 0, 5_000, 2_000_000);
        build_lock_txs(&mut session, &make_signet_config());

        assert_eq!(session.lock_txs.len(), 2);
        for id in [1u8, 2] {
            assert!(
                session.lock_txs.contains_key(&id),
                "missing lock tx for participant {}",
                id
            );
            assert!(!session.lock_txs.get(&id).unwrap().is_empty());
        }
    }

    #[test]
    fn build_lock_txs_bitcoin_output_goes_to_aggregate_address() {
        let keys: Vec<_> = (0..2).map(|_| make_swap_keys()).collect();
        let mut participants = BTreeMap::new();
        participants.insert(
            1,
            make_participant(
                1,
                true,
                keys[0].public_key.to_string(),
                2,
                Blockchain::Bitcoin,
                "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2".to_string(),
                0,
                10_000,
            ),
        );
        participants.insert(
            2,
            make_participant(
                2,
                false,
                keys[1].public_key.to_string(),
                1,
                Blockchain::Bitcoin,
                "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2".to_string(),
                0,
                10_000,
            ),
        );

        let mut session = SwapSession::new(1, participants, 0, 0, 5_000, 2_000_000);
        build_lock_txs(&mut session, &make_signet_config());

        // deserialize and check output goes to aggregate address
        let lock_tx_hex = session.lock_txs.get(&1).unwrap();
        let lock_tx: bitcoin::Transaction =
            bitcoin::consensus::encode::deserialize_hex(lock_tx_hex).unwrap();

        assert_eq!(lock_tx.output.len(), 1);
        assert_eq!(lock_tx.output[0].value.to_sat(), 10_000);

        // output should be P2TR (starts with OP_1)
        let script = &lock_tx.output[0].script_pubkey;
        assert!(script.is_p2tr(), "output should be taproot");
    }

    #[test]
    fn build_lock_txs_amount_matches_participant() {
        let keys: Vec<_> = (0..2).map(|_| make_swap_keys()).collect();
        let mut participants = BTreeMap::new();
        participants.insert(
            1,
            make_participant(
                1,
                true,
                keys[0].public_key.to_string(),
                2,
                Blockchain::Bitcoin,
                "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2".to_string(),
                0,
                50_000,
            ),
        );
        participants.insert(
            2,
            make_participant(
                2,
                false,
                keys[1].public_key.to_string(),
                1,
                Blockchain::Bitcoin,
                "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2".to_string(),
                0,
                75_000,
            ),
        );

        let mut session = SwapSession::new(1, participants, 0, 0, 5_000, 2_000_000);
        build_lock_txs(&mut session, &make_signet_config());

        let tx1: bitcoin::Transaction =
            bitcoin::consensus::encode::deserialize_hex(session.lock_txs.get(&1).unwrap()).unwrap();
        let tx2: bitcoin::Transaction =
            bitcoin::consensus::encode::deserialize_hex(session.lock_txs.get(&2).unwrap()).unwrap();

        assert_eq!(tx1.output[0].value.to_sat(), 50_000);
        assert_eq!(tx2.output[0].value.to_sat(), 75_000);
    }

}
