use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub const CONTRACT_JSON: &str = include_str!("../schema/0001_warehouse.json");

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    pub version: u32,
    pub migration_id: String,
    pub description: String,
    pub tables: Vec<Table>,
    pub views: Vec<View>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Table {
    pub name: String,
    pub purpose: String,
    pub columns: Vec<Column>,
    pub engine: String,
    pub order_by: Vec<String>,
    pub partition_by: Option<String>,
    #[serde(default)]
    pub indexes: Vec<Index>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<Constraint>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Constraint {
    pub name: String,
    pub expression: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Column {
    pub name: String,
    pub data_type: String,
    pub description: String,
    pub default: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Index {
    pub name: String,
    pub expression: String,
    pub index_type: String,
    pub granularity: u32,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    pub name: String,
    pub purpose: String,
    pub select: String,
}

pub fn identifier(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            && !value.as_bytes()[0].is_ascii_digit(),
        "unsafe SQL identifier"
    );
    Ok(())
}
pub fn current_contract() -> Result<Contract> {
    let mut contract = baseline_contract()?;
    crate::semantics::upgrade_contract(&mut contract)?;
    contract.validate()?;
    Ok(contract)
}
pub fn baseline_contract() -> Result<Contract> {
    let contract: Contract =
        serde_json::from_str(CONTRACT_JSON).context("invalid embedded warehouse contract")?;
    contract.validate()?;
    Ok(contract)
}
impl Contract {
    /// Reviewable SQL artifact generated from the single immutable definition.
    /// The placeholder is replaced only after validating the database identifier.
    pub fn sql_template(&self) -> Result<String> {
        let mut sql = format!(
            "-- Generated from shared/warehouse/schema/0001_warehouse.json; do not edit.\n-- Migration: {}\n-- Checksum: {}\n-- Database placeholder: {{{{database}}}}\n\n",
            self.migration_id,
            self.checksum()?
        );
        for statement in self.statements("warehouse_database")? {
            sql.push_str(&statement.replace("warehouse_database.", "{{database}}."));
            sql.push_str(";\n\n");
        }
        Ok(sql)
    }
    pub fn statements_from_template(&self, database: &str, sql: &str) -> Result<Vec<String>> {
        identifier(database)?;
        let normalized = sql.replace("\r\n", "\n");
        ensure!(
            normalized == self.sql_template()?,
            "generated migration SQL differs from the shared contract; regenerate it before running schema"
        );
        normalized
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with("--"))
            .map(|line| {
                line.strip_suffix(';')
                    .context("migration statement is not terminated")
                    .map(|s| s.replace("{{database}}", database))
            })
            .collect::<Result<Vec<_>>>()
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version > 0 && !self.migration_id.is_empty(),
            "missing migration identity"
        );
        let mut objects = HashSet::new();
        for table in &self.tables {
            identifier(&table.name)?;
            ensure!(
                table.name.starts_with("aml_") && objects.insert(&table.name),
                "duplicate or unscoped object"
            );
            let mut columns = HashSet::new();
            for column in &table.columns {
                identifier(&column.name)?;
                ensure!(columns.insert(column.name.as_str()), "duplicate column");
            }
            let mut constraints = HashSet::new();
            for constraint in &table.constraints {
                identifier(&constraint.name)?;
                ensure!(constraints.insert(&constraint.name), "duplicate constraint");
                ensure!(
                    !constraint.expression.contains(';'),
                    "invalid constraint expression"
                );
            }
            ensure!(
                columns.contains("network_id"),
                "table must be network-scoped"
            );
            ensure!(
                table.order_by.first().is_some_and(|v| v == "network_id"),
                "sort key must start with network_id"
            );
            ensure!(
                table.order_by.iter().all(|c| columns.contains(c.as_str())),
                "missing sort column"
            );
        }
        for view in &self.views {
            identifier(&view.name)?;
            ensure!(
                view.name.starts_with("aml_") && objects.insert(&view.name),
                "duplicate or unscoped view"
            );
            ensure!(
                !view.select.contains(';'),
                "view must be a single statement"
            );
        }
        Ok(())
    }
    pub fn checksum(&self) -> Result<String> {
        // Serialization removes host line-ending differences; definitions remain immutable.
        Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(self)?)))
    }
    pub fn statements(&self, database: &str) -> Result<Vec<String>> {
        identifier(database)?;
        let mut statements = Vec::new();
        for table in &self.tables {
            let mut columns: Vec<String> = table
                .columns
                .iter()
                .map(|c| {
                    format!(
                        "`{}` {}{}",
                        c.name,
                        c.data_type,
                        c.default
                            .as_ref()
                            .map(|d| format!(" DEFAULT {d}"))
                            .unwrap_or_default()
                    )
                })
                .collect();
            columns.extend(table.indexes.iter().map(|i| {
                format!(
                    "INDEX {} {} TYPE {} GRANULARITY {}",
                    i.name, i.expression, i.index_type, i.granularity
                )
            }));
            columns.extend(
                table
                    .constraints
                    .iter()
                    .map(|c| format!("CONSTRAINT `{}` CHECK {}", c.name, c.expression)),
            );
            statements.push(format!("CREATE TABLE IF NOT EXISTS {database}.{} ({}) ENGINE = {}{} ORDER BY ({}) SETTINGS index_granularity = 8192",
                table.name,columns.join(","),table.engine,table.partition_by.as_ref().map(|p|format!(" PARTITION BY {p}")).unwrap_or_default(),table.order_by.join(",")));
        }
        for view in &self.views {
            statements.push(format!(
                "CREATE VIEW IF NOT EXISTS {database}.{} AS {}",
                view.name,
                view.select.replace("{{database}}", database)
            ));
        }
        Ok(statements)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generated_sql_matches_every_network_and_rejects_independent_edits() {
        let contract = current_contract().unwrap();
        let sql = contract.sql_template().unwrap();
        for network in crate::Network::ALL {
            assert_eq!(
                contract
                    .statements_from_template(network.name(), &sql)
                    .unwrap(),
                contract.statements(network.name()).unwrap()
            );
        }
        assert!(
            contract
                .statements_from_template("bitcoin_aml", &sql.replace("UInt256", "UInt64"))
                .is_err()
        );
        assert_eq!(
            contract
                .statements_from_template("bitcoin_aml", &sql.replace('\n', "\r\n"))
                .unwrap(),
            contract.statements("bitcoin_aml").unwrap()
        );
    }
    #[test]
    fn all_networks_use_the_same_ddl_and_namespace() {
        let contract = current_contract().unwrap();
        let ddl = contract.statements("warehouse_test").unwrap();
        assert!(contract.tables.len() >= 25);
        for network in crate::Network::ALL {
            assert_eq!(
                contract
                    .statements(network.name())
                    .unwrap()
                    .iter()
                    .map(|s| s.replace(&format!("{}.", network.name()), "warehouse_test."))
                    .collect::<Vec<_>>(),
                ddl
            );
        }
        assert!(contract.statements("x; DROP DATABASE y").is_err());
    }
    #[test]
    fn checksums_are_line_ending_independent_and_capture_schema_changes() {
        let original = baseline_contract().unwrap();
        let windows: Contract = serde_json::from_str(&CONTRACT_JSON.replace('\n', "\r\n")).unwrap();
        assert_eq!(original.checksum().unwrap(), windows.checksum().unwrap());
        let mut changed = original.clone();
        changed.tables[0].columns[1].data_type = "UInt32".into();
        assert_ne!(original.checksum().unwrap(), changed.checksum().unwrap());
    }
}
