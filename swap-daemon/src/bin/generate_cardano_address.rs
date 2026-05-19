use cardano_serialization_lib::{Credential, EnterpriseAddress, PublicKey};
use tracing::info;
use swap_daemon::{blockchains::cardano_utils::CARDANO_TESTNET, test_utils::make_swap_keys};

/// Entry point of the application that generates a Cardano wallet address
/// and logs the associated private and public keys. This code is primarily
/// designed for debugging and testing purposes on the Cardano testnet.
///
/// # Notes
/// - Ensure `make_swap_keys` is correctly implemented and accessible.
/// - This implementation is built for the Cardano testnet
///   (configuration specified via `CARDANO_TESTNET`).
///
fn main() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();

    let keys = make_swap_keys();

    let pubkey = PublicKey::from_bytes(&keys.cardano_wallet_public_key).unwrap();
    let credential = Credential::from_keyhash(&pubkey.hash());
    let address = EnterpriseAddress::new(CARDANO_TESTNET, &credential)
        .to_address()
        .to_bech32(None)
        .unwrap();

    info!("wallet address: {}", address);
    info!(
        "secret_key: {}",
        hex::encode(&keys.cardano_wallet_secret_key)
    );
    info!(
        "public_key: {}",
        hex::encode(&keys.cardano_wallet_public_key)
    );
}
