use tracing::info;
use swap_daemon::test_utils::make_swap_keys;

/// Main function demonstrating the creation of a single-key Taproot address in Bitcoin.
fn main() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();

    let keys = make_swap_keys();

    // single key taproot address — just wrap in a 1-of-1 "aggregate"
    let secp = bitcoin::secp256k1::Secp256k1::new();
    let xonly_bytes = keys.public_key.serialize()[1..].to_vec();
    let xonly = bitcoin::XOnlyPublicKey::from_slice(&xonly_bytes).unwrap();
    let address = bitcoin::Address::p2tr(&secp, xonly, None, bitcoin::Network::Regtest);

    info!("address: {}", address);
    info!(
        "secret_key: {}",
        hex::encode(keys.secret_key.secret_bytes())
    );
    info!("public_key: {}", keys.public_key);
}
