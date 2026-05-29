use crate::swap_keys::SwapKeys;

pub struct PartyConfidential {
    pub address: String,
    pub swap_keys: SwapKeys,
}

impl PartyConfidential {
    pub fn new(address: String, swap_keys: SwapKeys) -> Self {
        Self { address, swap_keys }
    }
}
