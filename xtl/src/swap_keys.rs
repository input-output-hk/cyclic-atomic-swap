use musig2::secp256k1::{PublicKey, SecretKey};
use rand::{rngs::OsRng, RngCore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwapKeys {
    pub secret_key: SecretKey,              // secp256k1 — MuSig2 signing
    pub public_key: PublicKey,              // secp256k1
    pub cardano_wallet_secret_key: Vec<u8>, // Ed25519 — signs Cardano lock tx input
    pub cardano_wallet_public_key: Vec<u8>, // Ed25519
}

impl SwapKeys {
    pub fn new(
        secret_key: SecretKey,
        public_key: PublicKey,
        cardano_wallet_secret_key: Vec<u8>,
        cardano_wallet_public_key: Vec<u8>,
    ) -> Self {
        Self {
            secret_key,
            public_key,
            cardano_wallet_secret_key,
            cardano_wallet_public_key,
        }
    }

    pub fn make_new() -> SwapKeys {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        let secret_key = musig2::secp256k1::SecretKey::from_byte_array(bytes).unwrap();
        let public_key = musig2::secp256k1::PublicKey::from_secret_key(
            &musig2::secp256k1::Secp256k1::new(),
            &secret_key,
        );

        // generate Ed25519 keys for Cardano lock tx signing
        let mut ed25519_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut ed25519_bytes);
        let ed25519_signing_key = ed25519_dalek::SigningKey::from_bytes(&ed25519_bytes);
        let ed25519_verifying_key = ed25519_signing_key.verifying_key();

        SwapKeys {
            secret_key,
            public_key,
            cardano_wallet_secret_key: ed25519_signing_key.to_bytes().to_vec(),
            cardano_wallet_public_key: ed25519_verifying_key.to_bytes().to_vec(),
        }
    }
}
