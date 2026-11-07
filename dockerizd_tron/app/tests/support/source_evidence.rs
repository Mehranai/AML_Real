//! Deliberately synthetic RPC records with genuine, independently encoded
//! protobuf transaction bytes. Never mainnet or ownership evidence.
#![allow(dead_code)]
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const HEIGHT: u64 = 100;
pub const TIME: u64 = 1_730_000_000_123;
pub const OWNER: &str = "411111111111111111111111111111111111111111";
pub const TARGET: &str = "412222222222222222222222222222222222222222";

pub fn integer(number: u32, value: u64) -> Vec<u8> {
    let mut out = vint(u64::from(number) << 3);
    out.extend(vint(value));
    out
}
pub fn bytes(number: u32, value: &[u8]) -> Vec<u8> {
    let mut out = vint((u64::from(number) << 3) | 2);
    out.extend(vint(value.len() as u64));
    out.extend(value);
    out
}
fn vint(mut value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let part = (value & 127) as u8;
        value >>= 7;
        out.push(part | if value == 0 { 0 } else { 128 });
        if value == 0 {
            return out;
        }
    }
}
pub fn contract(kind: u32, name: &str, value: &[u8], json_value: Value) -> (Vec<u8>, Value) {
    let url = format!("type.googleapis.com/protocol.{name}");
    let any = [bytes(1, url.as_bytes()), bytes(2, value)].concat();
    (
        [integer(1, u64::from(kind)), bytes(2, &any)].concat(),
        json!({"type":name,"parameter":{"type_url":url,"value":json_value}}),
    )
}
pub fn transaction(contracts: Vec<(Vec<u8>, Value)>) -> Value {
    let mut raw = Vec::new();
    let mut json_contracts = Vec::new();
    for (proto, json) in contracts {
        raw.extend(bytes(11, &proto));
        json_contracts.push(json);
    }
    raw.extend(integer(14, TIME - 600));
    json!({"txID":format!("{:x}",Sha256::digest(&raw)),"raw_data_hex":hex::encode(raw),"raw_data":{"timestamp":TIME-600,"contract":json_contracts},"ret":[{"contractRet":"SUCCESS"}],"unknown_rpc_field":{"original":"preserved ? ' ","large":"9007199254740993"}})
}
pub fn transfer() -> (Vec<u8>, Value) {
    contract(
        1,
        "TransferContract",
        &[
            bytes(1, &hex::decode(OWNER).unwrap()),
            bytes(2, &hex::decode(TARGET).unwrap()),
            integer(3, 9_007_199_254_740_993),
        ]
        .concat(),
        json!({"owner_address":OWNER,"to_address":TARGET,"amount":9_007_199_254_740_993u64}),
    )
}
pub fn multi_transaction() -> Value {
    let trc10 = contract(
        2,
        "TransferAssetContract",
        &[
            bytes(1, b"1000001"),
            bytes(2, &hex::decode(OWNER).unwrap()),
            bytes(3, &hex::decode(TARGET).unwrap()),
            integer(4, 123),
        ]
        .concat(),
        json!({"asset_name":hex::encode("1000001"),"owner_address":OWNER,"to_address":TARGET,"amount":123}),
    );
    let trigger = contract(
        31,
        "TriggerSmartContract",
        &[
            bytes(1, &hex::decode(OWNER).unwrap()),
            bytes(2, &hex::decode(TARGET).unwrap()),
            integer(3, 100),
            bytes(4, &[0x12, 0x34, 0x56, 0x78]),
            integer(5, 20),
            integer(6, 1000001),
        ]
        .concat(),
        json!({"owner_address":OWNER,"contract_address":TARGET,"call_value":100,"data":"12345678","call_token_value":20,"token_id":1000001}),
    );
    transaction(vec![transfer(), trc10, trigger])
}
pub fn receipt(tx: &Value) -> Value {
    let transfer_topic = "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
    let owner_topic = format!("{}{}", "0".repeat(24), &OWNER[2..]);
    let target_topic = format!("{}{}", "0".repeat(24), &TARGET[2..]);
    json!({"id":tx["txID"],"blockNumber":HEIGHT,"blockTimeStamp":TIME,"fee":0,"receipt":{"energy_usage_total":123,"net_usage":0,"result":"SUCCESS"},"log":[
        {"address":&TARGET[2..],"topics":[],"data":"abcd","vendor_extension":true},
        {"address":&TARGET[2..],"topics":[transfer_topic,owner_topic,target_topic],"data":"f".repeat(64)},
        {"address":&TARGET[2..],"topics":[transfer_topic,owner_topic,target_topic,"f".repeat(64)],"data":""}
    ],"internal_transactions":[
        {"hash":"c".repeat(64),"caller_address":OWNER,"transferTo_address":TARGET,"note":"63616c6c"},
        {"hash":"d".repeat(64),"caller_address":OWNER,"transferTo_address":TARGET,"rejected":true,"callValueInfo":[{"callValue":7}]},
        {"hash":"e".repeat(64),"caller_address":OWNER,"transferTo_address":TARGET,"rejected":false,"callValueInfo":[{}, {"callValue":9_223_372_036_854_775_807u64,"tokenId":"1000001"}]}
    ]})
}
