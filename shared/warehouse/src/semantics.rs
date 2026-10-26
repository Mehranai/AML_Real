//! Immutable schema-v2 value rules generate both database constraints and Rust
//! insert validation. Physical columns remain the v1 contract.
use crate::{Contract, Network, RawAmount, contract::Constraint, publication::literal, values};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::Value;
use std::sync::LazyLock;

pub const JSON: &str = include_str!("../schema/0002_column_contract.json");
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Migration {
    pub version: u32,
    pub migration_id: String,
    pub description: String,
    pub baseline_checksum: String,
    pub rules: Vec<Rule>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub table: String,
    pub column: String,
    pub kind: String,
    #[serde(default)]
    pub values: Vec<String>,
}
static MIGRATION: LazyLock<Result<Migration>> = LazyLock::new(|| Ok(serde_json::from_str(JSON)?));
pub fn migration() -> Result<&'static Migration> {
    MIGRATION
        .as_ref()
        .map_err(|_| anyhow::anyhow!("invalid embedded column migration"))
}
pub fn upgrade_contract(contract: &mut Contract) -> Result<()> {
    let migration = migration()?;
    ensure!(
        migration.version == 2,
        "unsupported column migration version"
    );
    ensure!(
        contract.version == 1 && contract.checksum()? == migration.baseline_checksum,
        "column migration baseline checksum mismatch"
    );
    let mut seen = std::collections::HashSet::new();
    for rule in &migration.rules {
        let table = contract
            .tables
            .iter()
            .find(|t| t.name == rule.table)
            .context("rule table missing")?;
        ensure!(
            table.columns.iter().any(|c| c.name == rule.column),
            "rule column missing"
        );
        ensure!(
            seen.insert((&rule.table, &rule.column, &rule.kind)),
            "duplicate column rule"
        );
    }
    for table in &mut contract.tables {
        let expressions = migration
            .rules
            .iter()
            .filter(|r| r.table == table.name)
            .map(|r| {
                let nullable = table
                    .columns
                    .iter()
                    .find(|c| c.name == r.column)
                    .unwrap()
                    .data_type
                    .starts_with("Nullable(");
                let expr = expression(r)?;
                Ok(
                    if nullable && !matches!(r.kind.as_str(), "native_decimals" | "capability") {
                        format!("(isNull(`{}`) OR ({expr}))", r.column)
                    } else {
                        format!("({expr})")
                    },
                )
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(!expressions.is_empty(), "table has no column rules");
        table.constraints.push(Constraint {
            name: "aml_columns_v2".into(),
            expression: expressions.join(" AND "),
        });
    }
    contract.version = migration.version;
    contract.migration_id.clone_from(&migration.migration_id);
    contract.description.clone_from(&migration.description);
    Ok(())
}

fn network_match(column: &str, expression: impl Fn(Network) -> String) -> String {
    Network::ALL
        .into_iter()
        .map(|n| {
            format!(
                "(network_id={} AND {})",
                literal(n.id()),
                expression(n).replace("$value", column)
            )
        })
        .collect::<Vec<_>>()
        .join(" OR ")
}
fn hash_expression(column: &str) -> String {
    network_match(column, |n| {
        format!(
            "match($value,{})",
            literal(if matches!(n, Network::Ethereum | Network::Bsc) {
                "^0x[0-9a-f]{64}$"
            } else {
                "^[0-9a-f]{64}$"
            })
        )
    })
}
fn asset_expression(column: &str) -> String {
    let uint = "(?:0|[1-9][0-9]*)";
    let patterns = network_match(column, |n| {
        let tokens = match n {
            Network::Bitcoin => String::new(),
            Network::Ethereum | Network::Bsc => {
                format!("|erc20:0x[0-9a-f]{{40}}|(?:erc721|erc1155):0x[0-9a-f]{{40}}(?:/{uint})?")
            }
            Network::Tron => {
                format!("|trc10:{uint}|trc20:41[0-9a-f]{{40}}|trc721:41[0-9a-f]{{40}}(?:/{uint})?")
            }
        };
        format!(
            "match($value,{})",
            literal(&format!("^(?:{}{tokens})$", n.native_asset()))
        )
    });
    let number = format!("extract({column},'([0-9]+)$')");
    format!(
        "({patterns}) AND (length({number})<78 OR (length({number})=78 AND {number}<={}))",
        literal(RawAmount::MAX)
    )
}
fn evidence_expression(column: &str) -> String {
    let hash = hash_expression("substring(x,position(x,':')+1)");
    format!(
        "arrayAll(x -> match(x,'^[a-z][a-z0-9_]*:[^[:cntrl:]]+$') AND (NOT startsWith(x,'tx:') AND NOT startsWith(x,'block:') OR ({hash})) AND (NOT startsWith(x,'outpoint:') OR (network_id={} AND match(x,'^outpoint:[0-9a-f]{{64}}:(0|[1-9][0-9]*)$') AND toUInt64OrNull(extract(x,'([0-9]+)$'))<=4294967295)), {column})",
        literal(Network::Bitcoin.id())
    )
}
fn expression(rule: &Rule) -> Result<String> {
    let c = format!("`{}`", rule.column);
    Ok(match rule.kind.as_str() {
        "network" => format!(
            "{c} IN ({})",
            Network::ALL
                .into_iter()
                .map(|n| literal(n.id()))
                .collect::<Vec<_>>()
                .join(",")
        ),
        "positive" => format!("{c}>0"),
        "nonempty" => format!("length({c})>0"),
        "hash" => hash_expression(&c),
        "parent_hash" => format!("(block_number=0 AND {c}='') OR ({})", hash_expression(&c)),
        "state_hash" => format!("(initialized=0 AND {c}='') OR ({})", hash_expression(&c)),
        "boolean" => format!("{c} IN (0,1)"),
        "ratio" => format!("isFinite({c}) AND {c}>=0 AND {c}<=1"),
        "score" => format!("isFinite({c}) AND {c}>=0 AND {c}<=100"),
        "enum" => format!(
            "{c} IN ({})",
            rule.values
                .iter()
                .map(|v| literal(v))
                .collect::<Vec<_>>()
                .join(",")
        ),
        "asset" => asset_expression(&c),
        "transfer_asset" => format!(
            "({}) AND NOT match({c},'^(erc721|erc1155|trc721):[^/]+$')",
            asset_expression(&c)
        ),
        "native_decimals" => format!(
            "NOT startsWith(asset_id,'native:') OR ({})",
            network_match(&c, |n| format!(
                "isNotNull($value) AND $value={}",
                n.native_decimals()
            ))
        ),
        "bitcoin_amount" => format!(
            "network_id!={} OR {c}<=2100000000000000",
            literal(Network::Bitcoin.id())
        ),
        "satoshis" => format!(
            "network_id={} AND {c}<=2100000000000000",
            literal(Network::Bitcoin.id())
        ),
        "json" => format!("isValidJSON({c})"),
        "hex_bytes" => format!("match({c},'^(?:[0-9a-f]{{2}})*$')"),
        "evidence" => evidence_expression(&c),
        "review_actor" => format!("review_status='pending' OR length({c})>0"),
        "bitcoin_transaction" => format!(
            "network_id!={} OR (isNull(from_address) AND isNull(to_address) AND isNull(amount_raw) AND decimals=8 AND asset_id='native:BTC' AND isNotNull(input_count) AND isNotNull(output_count))",
            literal(Network::Bitcoin.id())
        ),
        "capability" => {
            if rule.column == "prevout_coverage" {
                format!(
                    "network_id={} OR isNull({c})",
                    literal(Network::Bitcoin.id())
                )
            } else {
                format!(
                    "network_id!={} OR isNull({c})",
                    literal(Network::Bitcoin.id())
                )
            }
        }
        _ => anyhow::bail!("unknown column rule kind"),
    })
}

pub fn upgrade_sql(database: &str) -> Result<String> {
    crate::contract::identifier(database)?;
    let contract = crate::current_contract()?;
    let mut sql = format!(
        "-- Generated column-contract upgrade; do not edit.\n-- Migration: {}\n-- Checksum: {}\n\n",
        contract.migration_id,
        contract.checksum()?
    );
    for table in &contract.tables {
        for constraint in &table.constraints {
            sql.push_str(&format!(
                "ALTER TABLE {database}.{} ADD CONSTRAINT IF NOT EXISTS `{}` CHECK {};\n\n",
                table.name, constraint.name, constraint.expression
            ));
        }
    }
    Ok(sql)
}
pub fn upgrade_template() -> Result<String> {
    Ok(upgrade_sql("warehouse_database")?.replace("warehouse_database.", "{{database}}."))
}
pub fn upgrade_statements(database: &str, sql: &str) -> Result<Vec<String>> {
    crate::contract::identifier(database)?;
    ensure!(
        sql.replace("\r\n", "\n") == upgrade_template()?,
        "column migration SQL differs from shared contract"
    );
    Ok(sql
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with("--"))
        .map(|line| line.trim_end_matches(';').replace("{{database}}", database))
        .collect())
}

pub fn validate_row(contract: &Contract, table_name: &str, row: &Value) -> Result<()> {
    let table = contract
        .tables
        .iter()
        .find(|t| t.name == table_name)
        .context("unknown fact table")?;
    let fields = row.as_object().context("warehouse row must be an object")?;
    ensure!(
        fields.len() == table.columns.len(),
        "warehouse row column count mismatch: {table_name}"
    );
    for column in &table.columns {
        let value = fields
            .get(&column.name)
            .context("warehouse column missing")?;
        validate_type(value, &column.data_type)
            .with_context(|| format!("invalid value type: {table_name}.{}", column.name))?;
    }
    let network = Network::parse(
        row["network_id"]
            .as_str()
            .context("network id must be string")?,
    )?;
    for rule in migration()?.rules.iter().filter(|r| r.table == table_name) {
        let value = &row[&rule.column];
        // Cross-field rules must run even when the anchored field is NULL.
        if value.is_null() && !matches!(rule.kind.as_str(), "native_decimals" | "capability") {
            continue;
        }
        validate_rule(network, rule, value, row).with_context(|| {
            format!(
                "column contract rejected {table_name}.{} ({})",
                rule.column, rule.kind
            )
        })?;
    }
    Ok(())
}
fn validate_type(value: &Value, data_type: &str) -> Result<()> {
    if let Some(inner) = data_type
        .strip_prefix("Nullable(")
        .and_then(|s| s.strip_suffix(')'))
    {
        return if value.is_null() {
            Ok(())
        } else {
            validate_type(value, inner)
        };
    }
    match data_type {
        "String" | "LowCardinality(String)" => ensure!(value.is_string(), "string required"),
        "UInt256" => {
            let raw = value
                .as_str()
                .context("UInt256 requires an exact decimal string")?;
            ensure!(
                RawAmount::parse(raw)?.as_str() == raw,
                "noncanonical raw integer"
            );
        }
        "UInt8" | "UInt32" | "UInt64" => {
            let raw = value.as_u64().context("unsigned JSON integer required")?;
            let max = match data_type {
                "UInt8" => u8::MAX as u64,
                "UInt32" => u32::MAX as u64,
                _ => u64::MAX,
            };
            ensure!(raw <= max, "unsigned integer overflow");
        }
        "Float64" => ensure!(
            value.as_f64().is_some_and(f64::is_finite),
            "finite numeric value required"
        ),
        "Array(String)" => {
            let array = value.as_array().context("string array required")?;
            ensure!(
                array.iter().all(Value::is_string),
                "array element must be string"
            );
        }
        _ => anyhow::bail!("unsupported contract type"),
    }
    Ok(())
}
fn validate_rule(network: Network, rule: &Rule, value: &Value, row: &Value) -> Result<()> {
    let text = || value.as_str().context("string required");
    let number = || value.as_u64().context("unsigned integer required");
    let hash = || -> Result<()> {
        ensure!(
            values::normalize_hash(network, text()?)? == text()?,
            "hash is not canonical"
        );
        Ok(())
    };
    match rule.kind.as_str() {
        "network" => {
            Network::parse(text()?)?;
        }
        "positive" => ensure!(number()? > 0, "revision must be positive"),
        "nonempty" => ensure!(!text()?.is_empty(), "identifier must not be empty"),
        "hash" => hash()?,
        "parent_hash" => {
            if !(row["block_number"] == 0 && text()?.is_empty()) {
                hash()?;
            }
        }
        "state_hash" => {
            if !(row["initialized"] == 0 && text()?.is_empty()) {
                hash()?;
            }
        }
        "boolean" => ensure!(number()? <= 1, "boolean out of range"),
        "ratio" | "score" => {
            let n = value.as_f64().context("number required")?;
            ensure!(
                n.is_finite() && n >= 0.0 && n <= if rule.kind == "ratio" { 1.0 } else { 100.0 },
                "score/ratio out of range"
            );
        }
        "enum" => ensure!(rule.values.iter().any(|v| value == v), "unknown enum state"),
        "asset" | "transfer_asset" => {
            let asset = values::AssetId::parse(network, text()?)?;
            ensure!(
                rule.kind != "transfer_asset" || !asset.collection,
                "NFT transfer must identify a token"
            );
        }
        "native_decimals" => {
            if row["asset_id"]
                .as_str()
                .is_some_and(|v| v.starts_with("native:"))
            {
                ensure!(
                    value.as_u64() == Some(network.native_decimals() as u64),
                    "wrong native decimals"
                );
            }
        }
        "bitcoin_amount" | "satoshis" => {
            if rule.kind == "satoshis" {
                ensure!(network == Network::Bitcoin, "UTXO data requires Bitcoin");
            }
            if network == Network::Bitcoin {
                let amount = if let Some(text) = value.as_str() {
                    text.parse::<u64>()?
                } else {
                    number()?
                };
                RawAmount::from_satoshis(amount)?;
            }
        }
        "json" => {
            serde_json::from_str::<Value>(text()?)?;
        }
        "hex_bytes" => ensure!(
            text()?.len() % 2 == 0
                && text()?
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "noncanonical hex bytes"
        ),
        "evidence" => {
            for item in value.as_array().context("array required")? {
                values::evidence_ref(network, item.as_str().context("reference string required")?)?;
            }
        }
        "review_actor" => ensure!(
            row["review_status"] == "pending" || !text()?.is_empty(),
            "review actor missing"
        ),
        "bitcoin_transaction" => {
            if network == Network::Bitcoin {
                ensure!(
                    row["from_address"].is_null()
                        && row["to_address"].is_null()
                        && row["amount_raw"].is_null()
                        && row["asset_id"] == "native:BTC"
                        && row["decimals"] == 8
                        && !row["input_count"].is_null()
                        && !row["output_count"].is_null(),
                    "Bitcoin transaction is not a single addressed transfer"
                );
            }
        }
        "capability" => {
            let applicable = if rule.column == "prevout_coverage" {
                network == Network::Bitcoin
            } else {
                network != Network::Bitcoin
            };
            ensure!(
                applicable || value.is_null(),
                "inapplicable coverage must be NULL"
            );
        }
        _ => anyhow::bail!("unknown column rule"),
    }
    Ok(())
}
