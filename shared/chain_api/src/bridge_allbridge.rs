use super::{Log, Network, digits, values};

// Allbridge Core's native messenger emits both events without indexed fields.
pub const SEND: &str = "9cd6008e8d4ebd34fd9d022278fec7f95d133780ecc1a0dea459fae3e9675390";
pub const RECEIVE: &str = "e9d840d27ab4032a839c20760fb995af8e3ad1980b9428980ca1c7e072acd87a";
pub const MESSENGER: u8 = 1;

pub fn emitter(network: Network) -> Option<&'static str> {
    match network {
        Network::Bsc => Some("0x3c4fa639c8d7e65c603145adad8bd12f2358312f"),
        Network::Tron => Some("TAuErcuAtU6BPt6YwL51JZ4RpDCPQASCU2"),
        _ => None,
    }
}

pub fn emitter_word(network: Network) -> Option<[u8; 32]> {
    word(match network {
        Network::Bsc => "0000000000000000000000003c4fa639c8d7e65c603145adad8bd12f2358312f",
        Network::Tron => "0000000000000000000000000a38028ed6146aa29c687c052b233131468b6635",
        _ => return None,
    })
}

pub fn network_id(network: Network) -> Option<u8> {
    match network {
        Network::Bsc => Some(2),
        Network::Tron => Some(3),
        _ => None,
    }
}

pub fn peer(network: Network) -> Option<Network> {
    match network {
        Network::Bsc => Some(Network::Tron),
        Network::Tron => Some(Network::Bsc),
        _ => None,
    }
}

pub fn destination_usdt(network: Network) -> Option<([u8; 32], &'static str, u8)> {
    let (padded, address, decimals) = match network {
        Network::Bsc => (
            "00000000000000000000000055d398326f99059ff775485246999027b3197955",
            "0x55d398326f99059ff775485246999027b3197955",
            18,
        ),
        Network::Tron => (
            "000000000000000000000000a614f803b6fd780986a42c78ec9c7f77e6ded13c",
            "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t",
            6,
        ),
        _ => return None,
    };
    Some((word(padded)?, address, decimals))
}

#[derive(Debug)]
pub struct Leg {
    pub amount: [u8; 32],
    pub recipient: [u8; 32],
    pub nonce: [u8; 32],
    pub direction: Direction,
}

#[derive(Debug)]
pub enum Direction {
    Send { destination: u8, token: [u8; 32] },
    Receive { message: [u8; 32] },
}

fn word(text: &str) -> Option<[u8; 32]> {
    if !values::hex(text, 64) {
        return None;
    }
    let mut result = [0; 32];
    for (i, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(result)
}

pub fn decode(network: Network, log: &Log) -> Option<Leg> {
    if Some(log.address.as_str()) != emitter(network)
        || values::normalize_hash(network, &log.tx_hash).ok()? != log.tx_hash
        || values::normalize_hash(network, &log.block_hash).ok()? != log.block_hash
    {
        return None;
    }
    let [topic] = log.topics.as_slice() else {
        return None;
    };
    let send = match digits(topic) {
        SEND => true,
        RECEIVE => false,
        _ => return None,
    };
    let data = digits(&log.data);
    if !values::hex(data, if send { 384 } else { 320 }) {
        return None;
    }
    let read = |i: usize| word(&data[i * 64..(i + 1) * 64]);
    let uint8 = |i| {
        let value = read(i)?;
        (value[..31] == [0; 31]).then_some(value[31])
    };
    if uint8(if send { 5 } else { 3 })? != MESSENGER {
        return None;
    }
    let amount = read(0)?;
    let recipient = read(1)?;
    if amount == [0; 32] || recipient[..12] != [0; 12] || recipient == [0; 32] {
        return None;
    }
    let (nonce, direction) = if send {
        (
            read(4)?,
            Direction::Send {
                destination: uint8(2)?,
                token: read(3)?,
            },
        )
    } else {
        (read(2)?, Direction::Receive { message: read(4)? })
    };
    Some(Leg {
        amount,
        recipient,
        nonce,
        direction,
    })
}
