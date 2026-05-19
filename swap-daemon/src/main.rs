use std::collections::BTreeMap;
use tracing::info;
use swap_daemon::{
    test_utils::make_swap_keys,
    types::{Blockchain, Daemon, DaemonConfig, Participant, SwapSession},
};

/// This is just rough code to see how to create a daemon and start a session.
/// This would actually be done using a cli to start the daemon and interact with it.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();
    info!("Starting config phase test.");

    let swap_keys = make_swap_keys();
    let mut daemon = Daemon::new(
        swap_keys.clone(),
        DaemonConfig::testnet("127.0.0.1:9000".to_string()),
    );

    let mut participants = BTreeMap::new();
    participants.insert(
        1,
        Participant {
            id: 1,
            blockchain: Blockchain::Bitcoin,
            tcp_address: "127.0.0.1:9001".to_string(),
            target_participant: 2,
            amount_locking: 100_000,
            amount_claiming: 100_000,
            is_me: true,
            secp256k1_public_key: swap_keys.public_key.to_string(),
                        cardano_wallet_public_key: vec![],
            funding_utxo_txid: "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"
                .to_string(),
            funding_utxo_vout: 0,
        },
    );
    participants.insert(
        2,
        Participant {
            id: 2,
            blockchain: Blockchain::Bitcoin,
            tcp_address: "127.0.0.1:9002".to_string(),
            target_participant: 1, // closes the cycle: 1->2->1
            amount_locking: 100_000,
            amount_claiming: 100_000,
            is_me: false,
            secp256k1_public_key: make_swap_keys().public_key.to_string(),
                        cardano_wallet_public_key: vec![],
            funding_utxo_txid: "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"
                .to_string(),
            funding_utxo_vout: 0,
        },
    );

    let session = SwapSession::new(1, participants, 0, 0, 5_000, 2_000_000);
    daemon.insert_session(session);

    info!("{:#?}", daemon.sessions.get(&1));

    daemon.run().await?;

    Ok(())
}
