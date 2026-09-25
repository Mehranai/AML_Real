use std::collections::BTreeMap;

use clickhouse::types::UInt256;

pub const ZERO_ADDRESS: &str = "T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb";

#[derive(Debug, Clone)]
pub struct SimpleTransfer {
    pub token: String,
    pub from: String,
    pub to: String,
    pub raw_amount: UInt256,
}

#[derive(Debug, Clone)]
pub enum AmlEvent {
    Swap {
        user: String,
        token_in: String,
        token_out: String,
    },

    BridgeIn {
        user: String,
        token: String,
    },

    BridgeOut {
        user: String,
        token: String,
    },

    Mint {
        user: String,
        token: String,
    },

    Burn {
        user: String,
        token: String,
    },

    LiquidityAdd {
        user: String,
        lp_token: String,
        sent_tokens: Vec<String>,
    },

    LiquidityRemove {
        user: String,
        lp_token: String,
        received_tokens: Vec<String>,
    },
}

pub type FlowMap = BTreeMap<String, BTreeMap<String, num_bigint::BigInt>>;
