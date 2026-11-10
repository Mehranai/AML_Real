use aml_warehouse::values;
use serde::Serialize;
use std::fmt::Write;

pub const ADDRESS: &str = "0x0000000000000068f116a894984e2db1123eb395";
pub const TOPIC: &str = "9d9af8e38d66c62e2c12f0225249fd9d721c54b83f48d9352c97c6cacdcb6f31";
pub const ZERO: &str = "0x0000000000000000000000000000000000000000";
const MAX_ITEMS: usize = 16;
pub const MAX_HEX: usize = 64 * (6 + MAX_ITEMS * (4 + 5));

#[derive(Serialize)]
pub struct Order {
    pub order_hash: String,
    pub offerer: String,
    pub zone: String,
    pub recipient: String,
    pub offer: Vec<Item>,
    pub consideration: Vec<Item>,
}

#[derive(Serialize)]
pub struct Item {
    pub item_type: &'static str,
    pub token: String,
    pub identifier: String,
    pub amount_raw: String,
    pub recipient: Option<String>,
}

impl Item {
    pub fn nft_asset(&self) -> Option<String> {
        matches!(self.item_type, "erc721" | "erc1155")
            .then(|| format!("{}:{}/{}", self.item_type, self.token, self.identifier))
    }
}

pub fn decode(topics: &[String], data: &str) -> Option<Order> {
    let [signature, offerer, zone] = topics else {
        return None;
    };
    if digits(signature) != TOPIC
        || !values::hex(digits(offerer), 64)
        || !values::hex(digits(zone), 64)
    {
        return None;
    }
    let data = digits(data);
    if data.len() > MAX_HEX || data.len() % 64 != 0 || !values::hex(data, data.len()) {
        return None;
    }
    let word = |index: usize| data.get(index * 64..(index + 1) * 64);
    let order_hash = word(0)?;
    let recipient = address(word(1)?)?;
    if !nonzero(order_hash) || offset(word(2)?)? != 128 {
        return None;
    }
    let offer_count = count(word(4)?)?;
    let consideration_start = 5 + offer_count * 4;
    if offset(word(3)?)? != consideration_start * 32 {
        return None;
    }
    let consideration_count = count(word(consideration_start)?)?;
    if data.len() / 64 != consideration_start + 1 + consideration_count * 5 {
        return None;
    }
    let item = |start: usize, received: bool| {
        let token = address(word(start + 1)?)?;
        let identifier = word(start + 2)?;
        let amount = word(start + 3)?;
        if !nonzero(amount) {
            return None;
        }
        let item_type = match offset(word(start)?)? {
            0 if token == ZERO && !nonzero(identifier) => "native",
            1 if token != ZERO && !nonzero(identifier) => "erc20",
            2 if token != ZERO && offset(amount) == Some(1) => "erc721",
            3 if token != ZERO => "erc1155",
            _ => return None,
        };
        Some(Item {
            item_type,
            token,
            identifier: decimal(identifier)?,
            amount_raw: decimal(amount)?,
            recipient: if received {
                Some(address(word(start + 4)?)?)
            } else {
                None
            },
        })
    };
    let offer = (0..offer_count)
        .map(|i| item(5 + i * 4, false))
        .collect::<Option<_>>()?;
    let consideration = (0..consideration_count)
        .map(|i| item(consideration_start + 1 + i * 5, true))
        .collect::<Option<_>>()?;
    let offerer = address(digits(offerer))?;
    (offerer != ZERO).then_some(Order {
        order_hash: format!("0x{}", order_hash.to_ascii_lowercase()),
        offerer,
        zone: address(digits(zone))?,
        recipient,
        offer,
        consideration,
    })
}

fn digits(value: &str) -> &str {
    value.strip_prefix("0x").unwrap_or(value)
}

fn offset(value: &str) -> Option<usize> {
    usize::from_str_radix(value, 16).ok()
}

fn count(value: &str) -> Option<usize> {
    offset(value).filter(|count| *count <= MAX_ITEMS)
}

fn nonzero(value: &str) -> bool {
    value.bytes().any(|byte| byte != b'0')
}

fn address(value: &str) -> Option<String> {
    let (padding, address) = value.split_at(24);
    (!nonzero(padding)).then(|| format!("0x{}", address.to_ascii_lowercase()))
}

fn decimal(value: &str) -> Option<String> {
    const BASE: u64 = 1_000_000_000;
    let mut limbs = [0; 9];
    for digit in value.chars() {
        let mut carry = u64::from(digit.to_digit(16)?);
        for limb in &mut limbs {
            let value = *limb * 16 + carry;
            *limb = value % BASE;
            carry = value / BASE;
        }
    }
    let last = limbs.iter().rposition(|limb| *limb != 0).unwrap_or(0);
    let mut decimal = limbs[last].to_string();
    for limb in limbs[..last].iter().rev() {
        write!(decimal, "{limb:09}").ok()?;
    }
    Some(decimal)
}
