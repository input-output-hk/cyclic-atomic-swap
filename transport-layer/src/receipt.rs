use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    id: String,
    size: usize,
    ack: HashSet<String>,
    nak: HashSet<String>,
}

impl Receipt {
    pub fn new(id: String, size: usize) -> Self {
        Self {
            id,
            size,
            ack: HashSet::with_capacity(size),
            nak: HashSet::new(),
        }
    }
    
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn ack(&self) -> &HashSet<String> {
        &self.ack
    }

    pub fn nak(&self) -> &HashSet<String> {
        &self.nak
    }

    pub fn add_ack(&mut self, address: String) {
        self.ack.insert(address);
    }

    pub fn add_nak(&mut self, address: String) {
        self.nak.insert(address);
    }

    pub fn is_complete(&self) -> bool {
        self.ack.len() + self.nak.len() >= self.size
    }
}
