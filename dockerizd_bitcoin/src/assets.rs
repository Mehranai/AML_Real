//! Verified native protocol constants, not imported intelligence or an RPC supply estimate.
use crate::{
    NETWORK_ID,
    db::{self, Database},
};
use aml_warehouse::{Network, analytics};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
pub const VERSION: &str = "bitcoin_native_asset_v1";
pub const MAX_MONEY: u64 = 2_100_000_000_000_000;
pub const COINBASE_MATURITY: u64 = 100;
const SOURCE: &str = "bitcoin_core_v28_protocol_constants";
const REFERENCE: &str = "https://github.com/bitcoin/bitcoin/blob/v28.0/src/consensus/amount.h";
fn digest(v: &Value) -> String {
    aml_risk_policy::digest(v)
}
pub fn definition() -> Value {
    json!({"contract":VERSION,"network_id":NETWORK_ID,"asset_id":Network::Bitcoin.native_asset(),"symbol":"BTC","name":"Bitcoin","decimals":Network::Bitcoin.native_decimals(),"asset_standard":"native",
        "max_money_sats":MAX_MONEY.to_string(),"max_money_is_circulating_supply":false,"coinbase_maturity_blocks":COINBASE_MATURITY,
        "source_id":SOURCE,"source_reference":REFERENCE,"consensus_reference":"https://github.com/bitcoin/bitcoin/blob/v28.0/src/consensus/consensus.h",
        "verification_method":"compiled_protocol_definition","human_reviewed":false,"total_supply_raw":null,"code_hash":null})
}
fn snapshot() -> Value {
    let mut value = json!({"contract":VERSION,"network_id":NETWORK_ID,"asset_id":"native:BTC","definition_hash":digest(&definition())});
    value["snapshot_id"] = json!(digest(&value));
    value
}
fn row(at: u64) -> Result<Value> {
    let mut value = json!({"network_id":NETWORK_ID,"asset_id":"native:BTC","symbol":"BTC","name":"Bitcoin","decimals":8,"asset_standard":"native",
        "source_id":SOURCE,"source_reference":REFERENCE,"review_status":"approved","reviewed_by":"system:compiled_bitcoin_protocol_definition",
        "evidence_refs":["protocol:bitcoin_native_asset_v1","source:bitcoin_core_v28_protocol_constants"],"revision":at,"created_at_unix_ms":at,
        "is_verified":1,"metadata_status":"verified_protocol_constants","verified_at_unix_ms":at,"reviewed_at_unix_ms":at,"updated_at_unix_ms":at,"raw_evidence_json":definition().to_string()});
    let contract = aml_warehouse::current_contract()?;
    let schema = contract
        .tables
        .iter()
        .find(|t| t.name == "aml_asset_metadata")
        .context("asset table missing")?;
    for c in &schema.columns {
        if value.get(&c.name).is_none() {
            let default = c
                .default
                .as_deref()
                .context("native metadata column requires a value")?;
            ensure!(default == "NULL", "unexpected native metadata default");
            value[&c.name] = Value::Null;
        }
    }
    aml_warehouse::semantics::validate_row(&contract, "aml_asset_metadata", &value)?;
    Ok(value)
}
async fn rows(db: &Database, visible: bool) -> Result<Vec<Value>> {
    let view = if visible {
        "aml_asset_metadata_visible"
    } else {
        "aml_asset_metadata_current"
    };
    db.rows(&format!("SELECT * FROM {view} WHERE network_id={{network:String}} AND asset_id='native:BTC' LIMIT 2"),&[("network",NETWORK_ID.into())]).await
}
fn validate(value: &Value) -> Result<()> {
    let at = value["created_at_unix_ms"]
        .as_u64()
        .context("native metadata timestamp missing")?;
    ensure!(
        at > 0 && *value == row(at)?,
        "native BTC metadata conflicts with the compiled definition; inspect the registry before a deliberate version upgrade"
    );
    Ok(())
}
/// Schema initialization is idempotent; an existing conflicting row is never overwritten.
pub async fn initialize(db: &Database) -> Result<()> {
    let existing = rows(db, false).await?;
    ensure!(existing.len() <= 1, "ambiguous native metadata");
    if let Some(value) = existing.first() {
        validate(value)?;
    } else {
        analytics::insert(
            &db.warehouse_connection(),
            db.database(),
            "aml_asset_metadata",
            &[row(db::timestamp().max(1))?],
        )
        .await?;
    }
    read(db).await?;
    Ok(())
}
pub async fn read(db: &Database) -> Result<Value> {
    let values = rows(db, true).await?;
    ensure!(
        values.len() == 1,
        "native BTC metadata missing; rerun bitcoin_aml schema with the current binary"
    );
    validate(&values[0])?;
    let mut asset = definition();
    asset["metadata_snapshot"] = snapshot();
    asset["metadata_version"] = json!(VERSION);
    asset["storage_revision"] = json!(
        values[0]["revision"]
            .as_u64()
            .context("metadata revision missing")?
            .to_string()
    );
    asset["verification"] = json!({"status":"verified_protocol_constants","method":"compiled_protocol_definition","human_reviewed":false,"intelligence_approval":false});
    asset["persisted"] = json!(true);
    Ok(
        json!({"network_id":NETWORK_ID,"assets":[asset],"native_asset_snapshot":snapshot(),"capabilities":capabilities()}),
    )
}
pub fn capabilities() -> Value {
    json!({"native_btc":"supported","erc20":"not_applicable","erc721":"not_applicable","erc1155":"not_applicable","evm_contract_metadata":"not_applicable","evm_logs":"not_applicable","token_discovery":"not_applicable","scope":"native_bitcoin_utxo_only_not_other_asset_protocols"})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protocol_row_is_complete_verified_and_never_invents_supply_or_human_review() {
        let r = row(1).unwrap();
        validate(&r).unwrap();
        assert_eq!(
            r.as_object().unwrap().len(),
            aml_warehouse::current_contract()
                .unwrap()
                .tables
                .iter()
                .find(|t| t.name == "aml_asset_metadata")
                .unwrap()
                .columns
                .len()
        );
        for key in [
            "total_supply_raw",
            "code_hash",
            "source_revision",
            "observed_block",
            "observed_block_hash",
        ] {
            assert!(r[key].is_null());
        }
        assert_eq!(definition()["human_reviewed"], false);
        assert_eq!(definition()["max_money_is_circulating_supply"], false);
        let mut bad = r;
        bad["decimals"] = json!(18);
        assert!(validate(&bad).is_err());
    }
}
