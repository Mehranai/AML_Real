use aml_warehouse::{Network, current_contract, semantics, values::normalize_hash};
use anyhow::Result;
use serde_json::{Value, json};

pub fn fixture(table_name: &str, network: Network) -> Result<Value> {
    let contract = current_contract()?;
    let table = contract
        .tables
        .iter()
        .find(|t| t.name == table_name)
        .unwrap();
    let mut row = serde_json::Map::new();
    for c in &table.columns {
        let value = if c.data_type.starts_with("Nullable(") {
            Value::Null
        } else {
            match c.data_type.as_str() {
                "String" | "LowCardinality(String)" => json!("fixture"),
                "Array(String)" | "Array(UInt32)" => json!([]),
                "UInt256" => json!("1"),
                "Float64" => json!(0.5),
                _ => json!(1),
            }
        };
        row.insert(c.name.clone(), value);
    }
    let mut row = Value::Object(row);
    for (key, value) in aml_warehouse::core_schema::defaults(table_name)? {
        row[&key] = value;
    }
    for (key, value) in aml_warehouse::analytics_schema::defaults(table_name)? {
        row[&key] = value;
    }
    for (key, value) in aml_warehouse::visibility_schema::defaults(table_name)? {
        row[&key] = value;
    }
    row["network_id"] = json!(network.id());
    for c in &table.columns {
        if matches!(
            c.name.as_str(),
            "block_hash"
                | "parent_hash"
                | "last_block_hash"
                | "tx_hash"
                | "prev_tx_hash"
                | "script_hash"
                | "code_hash"
                | "last_tx_hash"
        ) && !row[&c.name].is_null()
        {
            row[&c.name] = json!(normalize_hash(network, &"a".repeat(64))?);
        }
        if c.name.ends_with("_json") {
            row[&c.name] = json!("{}");
        }
        if c.name == "asset_id" {
            row[&c.name] = json!(network.native_asset());
        }
        if c.name == "decimals" {
            row[&c.name] = json!(network.native_decimals());
        }
        if c.name == "script_hex" {
            row[&c.name] = json!("6a01ff");
        }
    }
    for rule in semantics::migration()?
        .rules
        .iter()
        .chain(aml_warehouse::analytics_schema::migration()?.rules.iter())
        .chain(aml_warehouse::visibility_schema::migration()?.rules.iter())
        .filter(|r| r.table == table_name && r.kind == "enum")
    {
        if rule.column != "network_id" {
            row[&rule.column] = json!(&rule.values[0]);
        }
    }
    if table_name == "aml_ingested_blocks" {
        row["prevout_coverage"] = if network == Network::Bitcoin {
            json!(1)
        } else {
            Value::Null
        };
        row["receipt_coverage"] = if network != Network::Bitcoin {
            json!(1)
        } else {
            Value::Null
        };
        row["trace_coverage"] = Value::Null;
    }
    if table_name == "aml_transactions" && network == Network::Bitcoin {
        row["from_address"] = Value::Null;
        row["to_address"] = Value::Null;
        row["amount_raw"] = Value::Null;
        row["input_count"] = json!(1);
        row["output_count"] = json!(1);
    }
    if table_name == "aml_raw_transactions" && network == Network::Bitcoin {
        row["receipt_json"] = Value::Null;
    }
    Ok(row)
}

#[allow(dead_code)] // Used by migration tests sharing this module with value tests.
pub fn historical_fixture(table_name: &str, network: Network) -> Result<Value> {
    let mut row = fixture(table_name, network)?;
    let contract = aml_warehouse::baseline_contract()?;
    let columns = &contract
        .tables
        .iter()
        .find(|t| t.name == table_name)
        .unwrap()
        .columns;
    row.as_object_mut()
        .unwrap()
        .retain(|key, _| columns.iter().any(|c| &c.name == key));
    Ok(row)
}
