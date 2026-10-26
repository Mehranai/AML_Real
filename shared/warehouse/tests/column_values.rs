use aml_warehouse::{
    Connection, Network, RawAmount, current_contract, semantics,
    values::{AssetId, ReviewStatus, normalize_hash},
};
use anyhow::Result;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

mod support;
use support::fixture;

#[test]
fn every_shared_table_has_a_valid_value_fixture_for_each_applicable_network() -> Result<()> {
    let contract = current_contract()?;
    assert_eq!(
        aml_warehouse::baseline_contract()?.checksum()?,
        "147ccdadef5b7c195af6dff3cc1d576fbc882a9f6912d99862e3d6919c1f6224"
    );
    assert_eq!(contract.version, 2);
    for network in Network::ALL {
        for table in &contract.tables {
            let applicable = match table.name.as_str() {
                "aml_bitcoin_inputs" | "aml_bitcoin_outputs" => network == Network::Bitcoin,
                "aml_evm_logs" => matches!(network, Network::Ethereum | Network::Bsc),
                "aml_tron_contracts" => network == Network::Tron,
                _ => true,
            };
            let result =
                semantics::validate_row(&contract, &table.name, &fixture(&table.name, network)?);
            assert_eq!(
                result.is_ok(),
                applicable,
                "{} {}: {result:?}",
                network.name(),
                table.name
            );
        }
    }
    Ok(())
}
#[test]
fn exact_decimal_native_amounts_and_uint256_have_no_float_round_trip() -> Result<()> {
    assert_eq!(RawAmount::from_decimal("0.00000001", 8)?.as_str(), "1");
    assert_eq!(
        RawAmount::from_decimal("1.230000000", 8)?.as_str(),
        "123000000"
    );
    assert_eq!(
        RawAmount::from_decimal("1", 18)?.decimal(18),
        "1.000000000000000000"
    );
    assert!(RawAmount::from_decimal("0.000000001", 8).is_err());
    for invalid in ["1e8", "NaN", "-1", "1.", ".1", "1.2.3"] {
        assert!(RawAmount::from_decimal(invalid, 8).is_err());
    }
    assert!(RawAmount::from_decimal(RawAmount::MAX, 1).is_err());
    let contract = current_contract()?;
    let mut row = fixture("aml_transactions", Network::Ethereum)?;
    row["amount_raw"] = json!(RawAmount::MAX);
    semantics::validate_row(&contract, "aml_transactions", &row)?;
    for invalid in [
        json!(1.5),
        json!(9007199254740993u64),
        json!("0001"),
        json!("1e18"),
        json!(format!("{}0", RawAmount::MAX)),
    ] {
        row["amount_raw"] = invalid;
        assert!(semantics::validate_row(&contract, "aml_transactions", &row).is_err());
    }
    Ok(())
}
#[test]
fn canonical_asset_keys_preserve_network_standard_and_nft_token_identity() -> Result<()> {
    for network in Network::ALL {
        assert_eq!(
            AssetId::parse_legacy(network, &network.native_asset().to_ascii_lowercase())?.as_str(),
            network.native_asset()
        );
    }
    let token = format!("eip155:56/erc721:0X{}/00042", "A".repeat(40));
    assert_eq!(
        AssetId::parse_legacy(Network::Bsc, &token)?.as_str(),
        format!("erc721:0x{}/42", "a".repeat(40))
    );
    assert!(AssetId::parse_legacy(Network::Ethereum, &token).is_err());
    assert!(
        AssetId::parse_legacy(
            Network::Bitcoin,
            "erc20:0x0000000000000000000000000000000000000000"
        )
        .is_err()
    );
    assert_eq!(
        AssetId::parse_legacy(Network::Tron, "trc20:T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb")?.as_str(),
        "trc20:410000000000000000000000000000000000000000"
    );
    assert!(
        AssetId::parse_legacy(Network::Tron, "trc20:T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwa").is_err()
    );
    let mut relation = fixture("aml_address_relationships", Network::Ethereum)?;
    relation["asset_id"] = json!(format!("erc721:0x{}", "a".repeat(40)));
    assert!(
        semantics::validate_row(&current_contract()?, "aml_address_relationships", &relation)
            .is_err()
    );
    relation["asset_id"] = json!(format!("erc721:0x{}/42", "a".repeat(40)));
    semantics::validate_row(&current_contract()?, "aml_address_relationships", &relation)?;
    Ok(())
}
#[test]
fn enum_confidence_nullable_and_hash_domains_cannot_be_silently_coerced() -> Result<()> {
    let contract = current_contract()?;
    let row = fixture("aml_intelligence_claims", Network::Tron)?;
    for (column, value) in [
        ("confidence", json!(1.01)),
        ("confidence", json!(-0.1)),
        ("review_status", json!("APPROVED")),
        ("review_status", json!("verified")),
        ("revision", json!(0)),
        ("is_exposure_seed", json!(2)),
    ] {
        let mut invalid = row.clone();
        invalid[column] = value;
        assert!(semantics::validate_row(&contract, "aml_intelligence_claims", &invalid).is_err());
    }
    let mut approved = row.clone();
    approved["review_status"] = json!("approved");
    approved["reviewed_by"] = json!("");
    assert!(semantics::validate_row(&contract, "aml_intelligence_claims", &approved).is_err());
    assert_eq!(
        ReviewStatus::parse_legacy("REVOKED")?,
        ReviewStatus::Revoked
    );
    assert!(ReviewStatus::parse_legacy("verified").is_err());
    let mut block = fixture("aml_ingested_blocks", Network::Bitcoin)?;
    for (column, value) in [
        ("block_hash", json!("A".repeat(64))),
        ("receipt_coverage", json!(0)),
        ("prevout_coverage", json!(2)),
    ] {
        let original = block.clone();
        block[column] = value;
        assert!(semantics::validate_row(&contract, "aml_ingested_blocks", &block).is_err());
        block = original;
    }
    Ok(())
}
#[test]
fn bitcoin_transaction_amount_and_inapplicable_fields_remain_unknown() -> Result<()> {
    let contract = current_contract()?;
    let row = fixture("aml_transactions", Network::Bitcoin)?;
    for (column, value) in [
        ("from_address", json!("invented-sender")),
        ("amount_raw", json!("1")),
        ("decimals", json!(18)),
        ("fee_raw", json!("2100000000000001")),
    ] {
        let mut invalid = row.clone();
        invalid[column] = value;
        assert!(semantics::validate_row(&contract, "aml_transactions", &invalid).is_err());
    }
    Ok(())
}
#[test]
fn evidence_refs_require_namespace_and_exact_outpoint_identity() -> Result<()> {
    for network in Network::ALL {
        let hash = normalize_hash(network, &"a".repeat(64))?;
        aml_warehouse::values::evidence_ref(network, &format!("tx:{hash}"))?;
        assert!(aml_warehouse::values::evidence_ref(network, "unqualified").is_err());
        assert!(aml_warehouse::values::evidence_ref(network, "source:unsafe\nrecord").is_err());
    }
    let base = format!("outpoint:{}:", "a".repeat(64));
    for invalid in [
        format!("{base}00"),
        format!("{base}4294967296"),
        format!("{base}-1"),
    ] {
        assert!(aml_warehouse::values::evidence_ref(Network::Bitcoin, &invalid).is_err());
    }
    Ok(())
}
struct NoIo {
    calls: AtomicUsize,
}
impl Connection for NoIo {
    async fn execute(&self, _: &str) -> Result<()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        anyhow::bail!("unexpected I/O")
    }
    async fn query(&self, _: &str) -> Result<Vec<String>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        anyhow::bail!("unexpected I/O")
    }
}
#[tokio::test]
async fn invalid_row_at_end_of_multiple_batches_is_rejected_before_any_database_io() -> Result<()> {
    let db = NoIo {
        calls: AtomicUsize::new(0),
    };
    let row = fixture("aml_transactions", Network::Ethereum)?;
    let mut rows = vec![row; 1001];
    rows[1000]["fee_raw"] = json!(1.5);
    assert!(
        aml_warehouse::publication::insert(&db, "fixture", "aml_transactions", &rows)
            .await
            .is_err()
    );
    assert_eq!(db.calls.load(Ordering::SeqCst), 0);
    Ok(())
}
