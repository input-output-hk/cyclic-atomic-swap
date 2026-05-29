use serde::{Deserialize, Serialize};
use crate::swap_description::SwapDescription;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Message {
    SwapDescription(SwapDescription),
    Text(String)
}