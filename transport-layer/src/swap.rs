use crate::party::Party;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Swap {
    id: String,
    parties: Vec<Party>,
}

impl Swap {
    pub fn new(id: String, parties: Vec<Party>) -> Self {
        Self { id, parties }
    }

    // pub fn id(&self) -> &str {
    //     &self.id
    // }
    //
    // pub fn parties(&self) -> &Vec<Party> {
    //     &self.parties
    // }

    // pub fn size(&self) -> usize {
    //     self.parties.len()
    // }
}