use crate::types::{BitcoinNetwork, CardanoNetwork, DaemonConfig};


impl DaemonConfig {
    /// Creates a new instance configured for the Mainnet environment.
    ///
    /// This function initializes a structure with settings for the Mainnet
    /// environment, specifically for Bitcoin and Cardano networks. It also
    /// enables UTXO validation by default.
    ///
    /// # Parameters
    ///
    /// - `tcp_address`: A `String` containing the TCP address for network communication.
    /// - `blockfrost_api_key`: A `String` containing the API key used to interact
    ///   with the Blockfrost service for Cardano network operations.
    ///
    /// # Returns
    ///
    /// - `Self`: An instance of the structure configured for Mainnet with the
    ///   provided TCP address and Blockfrost API key, along with pre-set network
    ///   configurations for Bitcoin and Cardano (Mainnet) and UTXO validation enabled.
    ///
    pub fn mainnet(tcp_address: String, blockfrost_api_key: String) -> Self {
        Self {
            tcp_address,
            bitcoin_network: BitcoinNetwork::Mainnet,
            cardano_network: CardanoNetwork::Mainnet,
            blockfrost_api_key,
            validate_utxos: true,
        }
    }

    /// Creates a new instance configured for the Testnet environment.
    ///
    /// # Parameters
    ///
    /// * `tcp_address` - A `String` representing the TCP address to connect to.
    ///
    /// # Returns
    ///
    /// Returns an instance of `Self` with the following configurations:
    /// - `tcp_address`: The provided TCP address.
    /// - `bitcoin_network`: Set to the `BitcoinNetwork::Testnet4`.
    /// - `cardano_network`: Set to the `CardanoNetwork::Preprod`.
    /// - `blockfrost_api_key`: The API key for accessing the Blockfrost API in the Preprod environment.
    /// - `validate_utxos`: Enabled (`true`) to validate UTXOs.
    ///
    pub fn testnet(tcp_address: String) -> Self {
        Self {
            tcp_address,
            bitcoin_network: BitcoinNetwork::Testnet4,
            cardano_network: CardanoNetwork::Preprod,
            blockfrost_api_key: std::env::var("BLOCKFROST_API_KEY").unwrap_or_default(),
            validate_utxos: true,
        }
    }
}

impl BitcoinNetwork {
    /// Returns the base URL of the mempool API for the specified Bitcoin network.
    ///
    /// This function provides the appropriate mempool API base URL based on the Bitcoin network
    /// variant. It supports Mainnet, Testnet4, and custom URLs.
    ///
    /// # Returns
    /// A string slice (`&str`) referring to the base URL of the mempool API for the selected network.
    ///
    pub fn mempool_base_url(&self) -> &str {
        match self {
            BitcoinNetwork::Mainnet => "https://mempool.space/api",
            BitcoinNetwork::Testnet4 => "https://mempool.space/testnet4/api",
            BitcoinNetwork::Custom(url) => url,
        }
    }
}

impl CardanoNetwork {
    /// Returns the base URL for the Blockfrost API corresponding to the current Cardano network.
    ///
    /// # Panics
    /// This method will panic if called on a `CardanoNetwork::Custom` as custom networks are expected
    /// to use the "Dolos" service instead of Blockfrost.
    ///
    /// # Returns
    /// A string slice (`&str`) representing the base URL for the respective Cardano network.
    ///
    pub fn blockfrost_base_url(&self) -> &str {
        match self {
            CardanoNetwork::Mainnet => "https://cardano-mainnet.blockfrost.io",
            CardanoNetwork::Preprod => "https://cardano-preprod.blockfrost.io",
            CardanoNetwork::Preview => "https://cardano-preview.blockfrost.io",
            CardanoNetwork::Custom { .. } => panic!("Custom network uses Dolos, not Blockfrost"),
        }
    }

    /// Returns the gRPC URL for the `CardanoNetwork` if it is a custom network.
    ///
    /// # Description
    /// This function checks if the `CardanoNetwork` is of type `Custom`. If it is,
    /// it returns a reference to the `grpc_url` associated with the custom network.
    /// If the network is not custom, the function returns `None`.
    ///
    /// # Returns
    /// - `Some(&str)` - A reference to the gRPC URL if the network is custom.
    /// - `None` - If the network is not custom.
    ///
    pub fn dolos_grpc_url(&self) -> Option<&str> {
        match self {
            CardanoNetwork::Custom { grpc_url, .. } => Some(grpc_url),
            _ => None,
        }
    }

    /// Retrieves the REST URL for the Cardano network configuration.
    ///
    /// # Returns
    /// 
    /// - `Some(&str)` containing the REST URL if the `CardanoNetwork` is of the `Custom` variant.
    /// - `None` if the `CardanoNetwork` is not a `Custom` variant.
    ///
    /// # Notes
    /// 
    /// - This method is useful when you need to fetch the custom REST endpoint
    ///   for a `CardanoNetwork` instance that has been configured with a custom URL.
    /// 
    pub fn dolos_rest_url(&self) -> Option<&str> {
        match self {
            CardanoNetwork::Custom { rest_url, .. } => Some(rest_url),
            _ => None,
        }
    }
}
