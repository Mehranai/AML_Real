//! Narrow ABI checks for semantic events. A matching layout alone does not identify an emitter.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub fn definition_hash() -> String {
    format!(
        "{:x}",
        Sha256::digest(
            include_str!("receipt_abi.rs")
                .replace("\r\n", "\n")
                .as_bytes()
        )
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    V2Swap,
    V3Swap,
    CurveSwap,
    BalancerSwap,
    V2Mint,
    V2Burn,
    V3Mint,
    V3Burn,
    TornadoDeposit,
    TornadoWithdrawal,
}

fn hex(value: &str) -> &str {
    value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value)
}

impl Event {
    pub fn from_topic(topic: &str) -> Option<Self> {
        Some(match hex(topic).to_ascii_lowercase().as_str() {
            "d78ad95fa46c994b6551d0da85fc275fe613ce37657fb8d5e3d130840159d822" => Self::V2Swap,
            "c42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67" => Self::V3Swap,
            "8b3e96f2b889fa771c53c981b40daf005f63f637f1869f707052d15a3dd97140"
            | "d013ca23e77a65003c2c659c5442c00c805371b7fc1ebd4c206c41d1536bd90b" => Self::CurveSwap,
            "2170c741c41531aec20e7c107c24eecfdd15e69c9bb0a8dd37b1840b9e0b207b" => {
                Self::BalancerSwap
            }
            "4c209b5fc8ad50758f13e2e1088ba56a560dff690a1c6fef26394f4c03821c4f" => Self::V2Mint,
            "dccd412f0b1252819cb1fd330b93224ca42612892bb3f4f789976e6d81936496" => Self::V2Burn,
            "7a53080ba414158be7ec69b987b5fb7d07dee101fe85488f0853ae16239d0bde" => Self::V3Mint,
            "0c396cd989a39f4459b5fa1aed6a9a8dcdbc45908acfd67e028cd568da98982c" => Self::V3Burn,
            "a945e51eec50ab98c161376f0db4cf2aeba3ec92755fe2fcd388bdbbb80ff196" => {
                Self::TornadoDeposit
            }
            "e9e508bad6d4c3227e881ca19068f099da81b5164dd6d62b2eaf1e8bc6c34931" => {
                Self::TornadoWithdrawal
            }
            _ => return None,
        })
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::V2Swap => "amm_v2_swap",
            Self::V3Swap => "amm_v3_swap",
            Self::CurveSwap => "curve_int128_swap",
            Self::BalancerSwap => "balancer_v2_swap",
            Self::V2Mint => "amm_v2_mint",
            Self::V2Burn => "amm_v2_burn",
            Self::V3Mint => "amm_v3_mint",
            Self::V3Burn => "amm_v3_burn",
            Self::TornadoDeposit => "tornado_deposit",
            Self::TornadoWithdrawal => "tornado_withdrawal",
        }
    }

    pub fn operation(self) -> &'static str {
        match self {
            Self::V2Swap | Self::V3Swap | Self::CurveSwap | Self::BalancerSwap => "swap",
            Self::V2Mint | Self::V3Mint => "liquidity_add",
            Self::V2Burn | Self::V3Burn => "liquidity_remove",
            Self::TornadoDeposit => "mixer_deposit",
            Self::TornadoWithdrawal => "mixer_withdrawal",
        }
    }
}

fn padding(word: &str, digits: usize, signed: bool) -> bool {
    let split = 64 - digits;
    let leading = if signed
        && matches!(word.as_bytes()[split].to_ascii_lowercase(), b'8'..=b'9' | b'a'..=b'f')
    {
        b'f'
    } else {
        b'0'
    };
    word.as_bytes()[..split]
        .iter()
        .all(|b| b.to_ascii_lowercase() == leading)
}

pub fn proof(topics: &[&str], data: &str) -> Option<(Event, Value)> {
    let event = Event::from_topic(topics.first()?)?;
    let (topic_count, words) = match event {
        Event::V2Swap => (3, 4),
        Event::V3Swap => (3, 5),
        Event::CurveSwap => (2, 4),
        Event::BalancerSwap => (4, 2),
        Event::V2Mint => (2, 2),
        Event::V2Burn => (3, 2),
        Event::V3Mint => (4, 4),
        Event::V3Burn => (4, 3),
        Event::TornadoDeposit => (2, 2),
        Event::TornadoWithdrawal => (2, 3),
    };
    let data = hex(data);
    if topics.len() != topic_count
        || !crate::values::hex(data, words * 64)
        || !topics
            .iter()
            .all(|topic| crate::values::hex(hex(topic), 64))
    {
        return None;
    }
    let topic = |index: usize| hex(topics[index]);
    let word = |index: usize| &data[index * 64..(index + 1) * 64];
    let address = |index| padding(topic(index), 40, false);
    let valid = match event {
        Event::V2Swap | Event::V2Burn => address(1) && address(2),
        Event::V3Swap => {
            address(1)
                && address(2)
                && padding(word(2), 40, false)
                && padding(word(3), 32, false)
                && padding(word(4), 6, true)
        }
        Event::CurveSwap => address(1) && padding(word(0), 32, true) && padding(word(2), 32, true),
        Event::BalancerSwap => address(2) && address(3),
        Event::V2Mint => address(1),
        Event::V3Mint | Event::V3Burn => {
            address(1)
                && padding(topic(2), 6, true)
                && padding(topic(3), 6, true)
                && if event == Event::V3Mint {
                    padding(word(0), 40, false) && padding(word(1), 32, false)
                } else {
                    padding(word(0), 32, false)
                }
        }
        Event::TornadoDeposit => padding(word(0), 8, false),
        Event::TornadoWithdrawal => address(1) && padding(word(0), 40, false),
    };
    if !valid {
        return None;
    }
    let encode = |value: &str| format!("0x{}", value.to_ascii_lowercase());
    let mut proof = json!({"abi":event.id(),"topics":topics.iter().map(|topic|encode(hex(topic))).collect::<Vec<_>>(),
        "data":encode(data)});
    if event == Event::TornadoWithdrawal {
        proof["recipient"] = json!(encode(&word(0)[24..]));
        proof["relayer"] = json!(encode(&topic(1)[24..]));
        proof["nullifier_hash"] = json!(encode(word(1)));
    }
    Some((event, proof))
}

pub fn verify(value: &Value) -> Option<Event> {
    let topics = value["topics"].as_array()?;
    if topics.len() > 4 {
        return None;
    }
    let topics: Vec<_> = topics.iter().map(Value::as_str).collect::<Option<_>>()?;
    let (event, checked) = proof(&topics, value["data"].as_str()?)?;
    (checked == *value).then_some(event)
}
