use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Party {
    address: String,
}

impl Party {
    pub fn new(address: String) -> Self {
        Self { address }
    }

    pub fn address(&self) -> &String {
        &self.address
    }
}
