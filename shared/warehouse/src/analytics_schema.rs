//! Additive DB-05 contract. Earlier migrations and checksums remain immutable.
use crate::{
    Contract,
    contract::{Column, Constraint, Table, View},
    semantics::Rule,
};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::Value;
use std::{collections::BTreeMap, sync::LazyLock};
pub const JSON: &str = include_str!("../schema/0004_analytics_specialized.json");
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Migration {
    pub version: u32,
    pub migration_id: String,
    pub description: String,
    pub previous_checksum: String,
    pub columns: BTreeMap<String, Vec<Column>>,
    pub tables: Vec<Table>,
    pub views: Vec<View>,
    pub rules: Vec<Rule>,
}
static MIGRATION: LazyLock<Result<Migration>> = LazyLock::new(|| Ok(serde_json::from_str(JSON)?));
pub fn migration() -> Result<&'static Migration> {
    MIGRATION
        .as_ref()
        .map_err(|_| anyhow::anyhow!("invalid analytics migration"))
}
pub fn apply(contract: &mut Contract) -> Result<()> {
    let m = migration()?;
    ensure!(
        m.version == 4 && contract.version == 3 && contract.checksum()? == m.previous_checksum,
        "analytics predecessor checksum mismatch"
    );
    for (name, columns) in &m.columns {
        let table = contract
            .tables
            .iter_mut()
            .find(|t| &t.name == name)
            .context("analytics extension table missing")?;
        table.columns.extend(columns.iter().cloned());
    }
    contract.tables.extend(m.tables.iter().cloned());
    // Existing fact views have explicit projections too. Extending EVM logs
    // must expose its new columns through the same exact publication fence.
    for table in &contract.tables {
        if !m.columns.contains_key(&table.name)
            || !crate::core::FACTS.contains(&table.name.as_str())
        {
            continue;
        }
        if let Some(view) = contract
            .views
            .iter_mut()
            .find(|v| v.name == format!("{}_canonical", table.name))
        {
            let projection = table
                .columns
                .iter()
                .map(|c| format!("f.{} AS {}", c.name, c.name))
                .collect::<Vec<_>>()
                .join(", ");
            view.select = format!(
                "SELECT {projection} FROM {{{{database}}}}.{} AS f FINAL INNER JOIN {{{{database}}}}.aml_ingested_blocks_canonical AS b ON f.network_id=b.network_id AND f.block_number=b.block_number AND f.block_hash=b.block_hash AND f.commit_id=b.commit_id AND f.revision=b.revision",
                table.name
            );
        }
    }
    let mut seen = std::collections::HashSet::new();
    for r in &m.rules {
        ensure!(
            seen.insert((&r.table, &r.column, &r.kind)),
            "duplicate analytics rule"
        );
    }
    for table in &mut contract.tables {
        let expressions = m
            .rules
            .iter()
            .filter(|r| r.table == table.name)
            .map(|r| crate::semantics::rule_expression(r, &table.columns))
            .collect::<Result<Vec<_>>>()?;
        if !expressions.is_empty() {
            table.constraints.push(Constraint {
                name: "aml_analytics_v4".into(),
                expression: expressions.join(" AND "),
            });
        }
    }
    contract.views.extend(m.views.iter().cloned());
    contract.version = m.version;
    contract.migration_id.clone_from(&m.migration_id);
    contract.description.clone_from(&m.description);
    contract.validate()
}
pub fn defaults(table: &str) -> Result<BTreeMap<String, Value>> {
    migration()?
        .columns
        .get(table)
        .into_iter()
        .flatten()
        .map(|c| {
            let raw = c.default.as_deref().context("extension default missing")?;
            let v = if raw == "NULL" {
                Value::Null
            } else if let Some(text) = raw.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
                Value::String(text.into())
            } else {
                serde_json::from_str(raw)?
            };
            Ok((c.name.clone(), v))
        })
        .collect()
}
pub fn sql_template() -> Result<String> {
    let contract = crate::contract::analytics_contract()?;
    let m = migration()?;
    let mut sql = format!(
        "-- Generated analytics/specialized upgrade; do not edit.\n-- Migration: {}\n-- Checksum: {}\n\n",
        contract.migration_id,
        contract.checksum()?
    );
    for table in &contract.tables {
        if let Some(columns) = m.columns.get(&table.name) {
            for c in columns {
                sql.push_str(&format!("ALTER TABLE {{{{database}}}}.{} ADD COLUMN IF NOT EXISTS `{}` {} DEFAULT {};\n\n",table.name,c.name,c.data_type,c.default.as_deref().unwrap()));
            }
        }
    }
    let fresh = Contract {
        tables: contract
            .tables
            .iter()
            .filter(|t| m.tables.iter().any(|n| n.name == t.name))
            .cloned()
            .collect(),
        views: vec![],
        ..contract.clone()
    };
    for statement in fresh.statements("warehouse_database")? {
        sql.push_str(&statement.replace("warehouse_database.", "{{database}}."));
        sql.push_str(";\n\n");
    }
    for table in &contract.tables {
        if m.columns.contains_key(&table.name) {
            for c in table
                .constraints
                .iter()
                .filter(|c| c.name == "aml_analytics_v4")
            {
                sql.push_str(&format!("ALTER TABLE {{{{database}}}}.{} ADD CONSTRAINT IF NOT EXISTS `{}` CHECK {};\n\n",table.name,c.name,c.expression));
            }
        }
    }
    // Refresh SELECT * metadata for extended tables as well as installing new views.
    for view in &contract.views {
        sql.push_str(&format!(
            "CREATE OR REPLACE VIEW {{{{database}}}}.{} AS {};\n\n",
            view.name, view.select
        ));
    }
    Ok(sql)
}
pub fn statements(database: &str, sql: &str) -> Result<Vec<String>> {
    crate::contract::identifier(database)?;
    ensure!(
        sql.replace("\r\n", "\n") == sql_template()?,
        "analytics SQL differs from shared contract"
    );
    Ok(sql
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with("--"))
        .map(|l| l.trim_end_matches(';').replace("{{database}}", database))
        .collect())
}
