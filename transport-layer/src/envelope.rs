use crate::message::Message;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub enum Envelope {
    Ask {
        id: String,
        from: String,
        to: Vec<String>,
        message: Message,
    },
    Ack {
        id: String,
        from: String,
        to: String,
        size: usize,
    },
    Nak {
        id: String,
        from: String,
        to: String,
        size: usize,
    },
}

impl Envelope {
    pub fn from_json<'a>(json: &'a str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}
