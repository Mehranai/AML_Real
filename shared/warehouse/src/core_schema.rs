//! Immutable additive v3 core extensions; v1 and v2 definitions stay unchanged.
use crate::{
    Contract,
    contract::{Column, Constraint, View},
    semantics::Rule,
};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::Value;
use std::{collections::BTreeMap, sync::LazyLock};
pub const JSON: &str = include_str!("../schema/0003_core_evidence.json");
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Migration {
    pub version: u32,
    pub migration_id: String,
    pub description: String,
    pub previous_checksum: String,
    pub columns: BTreeMap<String, Vec<Column>>,
    pub rules: Vec<Rule>,
}
static MIGRATION: LazyLock<Result<Migration>> = LazyLock::new(|| Ok(serde_json::from_str(JSON)?));
pub fn migration() -> Result<&'static Migration> {
    MIGRATION
        .as_ref()
        .map_err(|_| anyhow::anyhow!("invalid core migration"))
}
pub fn apply(contract: &mut Contract) -> Result<()> {
    let m = migration()?;
    ensure!(
        m.version == 3 && contract.version == 2 && contract.checksum()? == m.previous_checksum,
        "core migration predecessor checksum mismatch"
    );
    for (name, columns) in &m.columns {
        let table = contract
            .tables
            .iter_mut()
            .find(|t| &t.name == name)
            .context("core extension table missing")?;
        for column in columns {
            ensure!(
                !table.columns.iter().any(|c| c.name == column.name),
                "core column already exists"
            );
            table.columns.push(column.clone());
        }
        let expressions = m
            .rules
            .iter()
            .filter(|r| &r.table == name)
            .map(|r| crate::semantics::rule_expression(r, &table.columns))
            .collect::<Result<Vec<_>>>()?;
        ensure!(!expressions.is_empty(), "core extension rules missing");
        table.constraints.push(Constraint {
            name: "aml_core_v3".into(),
            expression: expressions.join(" AND "),
        });
    }
    // Rebuild explicit projections from the same physical column definitions.
    for table in &contract.tables {
        if !m.columns.contains_key(&table.name) {
            continue;
        }
        let view_name = format!("{}_canonical", table.name);
        if table.name == "aml_ingested_blocks" {
            let fields = table
                .columns
                .iter()
                .filter(|c| !matches!(c.name.as_str(), "network_id" | "block_number"))
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>();
            let projection = fields
                .iter()
                .enumerate()
                .map(|(i, c)| format!("data.{} AS {c}", i + 1))
                .collect::<Vec<_>>()
                .join(", ");
            let tuple = fields
                .iter()
                .map(|c| format!("b.{c}"))
                .collect::<Vec<_>>()
                .join(", ");
            contract
                .views
                .iter_mut()
                .find(|v| v.name == view_name)
                .unwrap()
                .select = format!(
                "SELECT network_id, block_number, {projection} FROM (SELECT b.network_id AS network_id, b.block_number AS block_number, argMax(tuple({tuple}), b.revision) AS data FROM {{{{database}}}}.aml_ingested_blocks AS b INNER JOIN {{{{database}}}}.aml_sync_state_current AS s ON b.network_id=s.network_id WHERE s.initialized=1 AND b.revision<=s.revision AND b.block_number<=s.last_block_number GROUP BY b.network_id,b.block_number) WHERE data.6=1 AND data.7='complete'"
            );
        } else if let Some(view) = contract.views.iter_mut().find(|v| v.name == view_name) {
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
    contract.views.push(View{name:"aml_ingestion_benchmarks_current".into(),purpose:"Latest benchmark record per network and benchmark identity, including incomplete/failed runs.".into(),select:"SELECT * FROM {{database}}.aml_ingestion_benchmarks ORDER BY revision DESC LIMIT 1 BY network_id,benchmark_id".into()});
    contract.version = 3;
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
            let raw = c
                .default
                .as_deref()
                .context("core extension default missing")?;
            let value = if raw == "NULL" {
                Value::Null
            } else if let Some(text) = raw.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
                Value::String(text.into())
            } else {
                serde_json::from_str(raw)?
            };
            Ok((c.name.clone(), value))
        })
        .collect()
}
/// Explicit compatibility serialization for internal v1/v2 DTOs. Public insert
/// validation still requires every current column, without implicit defaults.
pub fn extend(table: &str, mut row: Value) -> Result<Value> {
    let object = row.as_object_mut().context("core row must be object")?;
    for (key, value) in defaults(table)? {
        object.entry(key).or_insert(value);
    }
    Ok(row)
}
pub fn sql_template() -> Result<String> {
    let current = crate::core_contract()?;
    let m = migration()?;
    let mut sql = format!(
        "-- Generated core-evidence upgrade; do not edit.\n-- Migration: {}\n-- Checksum: {}\n\n",
        current.migration_id,
        current.checksum()?
    );
    for table in &current.tables {
        if let Some(columns) = m.columns.get(&table.name) {
            for c in columns {
                sql.push_str(&format!("ALTER TABLE {{{{database}}}}.{} ADD COLUMN IF NOT EXISTS `{}` {} DEFAULT {};\n\n",table.name,c.name,c.data_type,c.default.as_deref().unwrap()));
            }
        }
    }
    for table in &current.tables {
        for c in table.constraints.iter().filter(|c| c.name == "aml_core_v3") {
            sql.push_str(&format!(
                "ALTER TABLE {{{{database}}}}.{} ADD CONSTRAINT IF NOT EXISTS `{}` CHECK {};\n\n",
                table.name, c.name, c.expression
            ));
        }
    }
    for view in &current.views {
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
        "core migration SQL differs from shared contract"
    );
    Ok(sql
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with("--"))
        .map(|l| l.trim_end_matches(';').replace("{{database}}", database))
        .collect())
}
