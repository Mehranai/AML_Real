use super::*;

const A: &str = "0x1111111111111111111111111111111111111111";
const B: &str = "0x2222222222222222222222222222222222222222";
const NFT: &str = "0x3333333333333333333333333333333333333333";

fn word(value: u64) -> String {
    format!("{value:064x}")
}

fn address_word(address: &str) -> String {
    format!("{:0>64}", &address[2..])
}

fn receipt() -> Receipt {
    let words = [
        word(1),
        address_word(B),
        word(128),
        word(288),
        word(1),
        word(2),
        address_word(NFT),
        "f".repeat(64),
        word(1),
        word(1),
        word(0),
        word(0),
        word(0),
        "f".repeat(64),
        address_word(A),
    ];
    Receipt {
        block_number: 7,
        block_hash: format!("0x{}", word(7)),
        tx_hash: format!("0x{}", word(77)),
        log_index: 2,
        emitter: seaport::ADDRESS.into(),
        topics: vec![
            format!("0x{}", seaport::TOPIC),
            format!("0x{}", address_word(A)),
            format!("0x{}", word(0)),
        ],
        data: words.concat(),
    }
}

fn wallet(receipt: &Receipt, from: &str, to: &str, token: &str, id: &str) -> Value {
    json!({"address":to,"source_snapshot":{"snapshot_id":"1".repeat(64),"as_of_block":receipt.block_number,"block_hash":receipt.block_hash},
        "graph":{"edges":[{"id":"edge","tx_hash":receipt.tx_hash,"block_number":receipt.block_number,
            "canonical_asset_id":format!("erc721:{token}/{id}"),"canonical_token_id":id,"amount_raw":"1",
            "from_subject_id":from,"to_subject_id":to}]}})
}

#[test]
fn market_requires_same_occurrence_participants_asset_and_receipt() -> Result<()> {
    let receipt = receipt();
    let max = aml_warehouse::RawAmount::MAX;
    let mut data = wallet(&receipt, A, B, NFT, max);
    let order = seaport::decode(&receipt.topics, &receipt.data).expect("valid ABI fixture");
    let item = project(&receipt, &order, &data).expect("matching transfer");
    assert_eq!(item["order"]["consideration"][0]["amount_raw"], max);
    assert_eq!(item["related_nft_transfers"], json!(["edge"]));
    data["nft_marketplace_evidence"] = envelope(&data, "available", 1, vec![item]);
    validate(Network::Ethereum, Mode::Wallet, &data)?;
    assert!(validate(Network::Bsc, Mode::Wallet, &data).is_err());
    assert!(validate(Network::Ethereum, Mode::Paths, &data).is_err());
    for (field, replacement) in [
        ("from_subject_id", json!(B)),
        ("amount_raw", json!("2")),
        ("canonical_asset_id", json!(format!("erc721:{NFT}/1"))),
        ("block_number", json!(6)),
        ("tx_hash", json!(format!("0x{}", word(78)))),
    ] {
        let mut altered = data.clone();
        altered["graph"]["edges"][0][field] = replacement;
        assert!(
            validate(Network::Ethereum, Mode::Wallet, &altered).is_err(),
            "{field}"
        );
    }
    data["nft_marketplace_evidence"]["items"][0]["receipt"]["emitter"] = json!(B);
    assert!(validate(Network::Ethereum, Mode::Wallet, &data).is_err());
    Ok(())
}

#[test]
fn erc1155_matches_the_full_raw_quantity() {
    let mut receipt = receipt();
    receipt.data.replace_range(5 * 64..6 * 64, &word(3));
    receipt.data.replace_range(8 * 64..9 * 64, &"f".repeat(64));
    let max = aml_warehouse::RawAmount::MAX;
    let mut data = wallet(&receipt, A, B, NFT, max);
    data["graph"]["edges"][0]["canonical_asset_id"] = json!(format!("erc1155:{NFT}/{max}"));
    data["graph"]["edges"][0]["amount_raw"] = json!(max);
    let order = seaport::decode(&receipt.topics, &receipt.data).unwrap();
    assert!(project(&receipt, &order, &data).is_some());
    data["graph"]["edges"][0]["amount_raw"] = json!("1");
    assert!(project(&receipt, &order, &data).is_none());
}

#[test]
fn rejects_truncated_overlapping_overlarge_or_trailing_arrays() {
    let receipt = receipt();
    for (index, value) in [(2, 0), (3, 128), (4, 17), (5, 4), (8, 2), (9, 17)] {
        let mut corrupt = receipt.data.clone();
        corrupt.replace_range(index * 64..(index + 1) * 64, &word(value));
        assert!(
            seaport::decode(&receipt.topics, &corrupt).is_none(),
            "word {index}"
        );
    }
    assert!(seaport::decode(&receipt.topics, &receipt.data[..receipt.data.len() - 64]).is_none());
    assert!(seaport::decode(&receipt.topics, &(receipt.data + &word(0))).is_none());
}

#[test]
#[ignore = "requires NFT_TEST_PUBLIC_RECEIPT containing the public receipt described in TESTING.md"]
fn public_seaport_receipt_matches_its_canonical_nft_transfer() -> Result<()> {
    let path = std::env::var_os("NFT_TEST_PUBLIC_RECEIPT").expect("set NFT_TEST_PUBLIC_RECEIPT");
    let sample: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    let logs = sample["receipt"]["logs"].as_array().unwrap();
    let read = |index| {
        let hex = format!("0x{index:x}");
        let log = logs.iter().find(|log| log["logIndex"] == hex).unwrap();
        serde_json::from_value::<Receipt>(json!({
            "tx_hash":log["transactionHash"],"block_number":sample["block_number"],"block_hash":sample["block_hash"],
            "log_index":index,"emitter":log["address"],"topics":log["topics"],"data":log["data"]
        }))
    };
    let receipt = read(702)?;
    let other = read(703)?;
    let transfer = logs.iter().find(|log| log["logIndex"] == "0x2c1").unwrap();
    let from = format!("0x{}", &transfer["topics"][1].as_str().unwrap()[26..]);
    let to = format!("0x{}", &transfer["topics"][2].as_str().unwrap()[26..]);
    let id = u64::from_str_radix(&transfer["topics"][3].as_str().unwrap()[2..], 16)?.to_string();
    let mut data = wallet(
        &receipt,
        &from,
        &to,
        transfer["address"].as_str().unwrap(),
        &id,
    );
    let order = seaport::decode(&receipt.topics, &receipt.data).expect("canonical Seaport ABI");
    let item = project(&receipt, &order, &data).expect("real consideration NFT matches transfer");
    assert_eq!(item["order"]["consideration"][0]["identifier"], "1183");
    assert_eq!(item["order"]["offer"][0]["item_type"], "erc20");
    let unrelated = seaport::decode(&other.topics, &other.data).expect("canonical Seaport ABI");
    assert!(project(&other, &unrelated, &data).is_none());
    data["nft_marketplace_evidence"] = envelope(&data, "available", 1, vec![item]);
    validate(Network::Ethereum, Mode::Wallet, &data)
}
