//! DB-07 migration. Earlier schema definitions and generated SQL remain frozen.
use crate::{
    Contract,
    contract::{Column, Constraint, Table, View},
    semantics::Rule,
};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::{collections::BTreeMap, sync::LazyLock};
pub const JSON: &str = include_str!("../schema/0005_visibility.json");
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
        .map_err(|_| anyhow::anyhow!("invalid visibility migration"))
}
pub fn apply(contract: &mut Contract) -> Result<()> {
    let m = migration()?;
    ensure!(
        m.version == 5 && contract.version == 4 && contract.checksum()? == m.previous_checksum,
        "visibility predecessor checksum mismatch"
    );
    for (name, columns) in &m.columns {
        contract
            .tables
            .iter_mut()
            .find(|t| &t.name == name)
            .context("visibility extension missing")?
            .columns
            .extend(columns.iter().cloned());
    }
    contract.tables.extend(m.tables.iter().cloned());
    for table in &mut contract.tables {
        let rules = m
            .rules
            .iter()
            .filter(|r| r.table == table.name)
            .map(|r| crate::semantics::rule_expression(r, &table.columns))
            .collect::<Result<Vec<_>>>()?;
        if !rules.is_empty() {
            table.constraints.push(Constraint {
                name: "aml_visibility_v5".into(),
                expression: rules.join(" AND "),
            });
        }
    }
    for view in &m.views {
        if let Some(old) = contract.views.iter_mut().find(|v| v.name == view.name) {
            *old = view.clone();
        } else {
            contract.views.push(view.clone());
        }
    }
    contract.version = m.version;
    contract.migration_id.clone_from(&m.migration_id);
    contract.description.clone_from(&m.description);
    contract.validate()
}
pub fn sql_template() -> Result<String> {
    let contract = crate::current_contract()?;
    let m = migration()?;
    let mut sql = format!(
        "-- Generated visibility upgrade; do not edit.\n-- Migration: {}\n-- Checksum: {}\n\n",
        contract.migration_id,
        contract.checksum()?
    );
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
        if let Some(columns) = m.columns.get(&table.name) {
            for c in columns {
                sql.push_str(&format!("ALTER TABLE {{{{database}}}}.{} ADD COLUMN IF NOT EXISTS `{}` {} DEFAULT {};\n\n",table.name,c.name,c.data_type,c.default.as_deref().context("visibility default missing")?));
            }
            for c in table
                .constraints
                .iter()
                .filter(|c| c.name == "aml_visibility_v5")
            {
                sql.push_str(&format!("ALTER TABLE {{{{database}}}}.{} ADD CONSTRAINT IF NOT EXISTS `{}` CHECK {};\n\n",table.name,c.name,c.expression));
            }
        }
    }
    for view in &m.views {
        sql.push_str(&format!(
            "CREATE OR REPLACE VIEW {{{{database}}}}.{} AS {};\n\n",
            view.name, view.select
        ));
    }
    Ok(sql)
}
pub fn defaults(table: &str) -> Result<BTreeMap<String, serde_json::Value>> {
    Ok(migration()?
        .columns
        .get(table)
        .into_iter()
        .flatten()
        .map(|c| (c.name.clone(), serde_json::Value::Null))
        .collect())
}
pub fn statements(database: &str, sql: &str) -> Result<Vec<String>> {
    crate::contract::identifier(database)?;
    ensure!(
        sql.replace("\r\n", "\n") == sql_template()?,
        "visibility SQL differs from shared contract"
    );
    sql.lines()
        .filter(|l| !l.is_empty() && !l.starts_with("--"))
        .map(|l| {
            Ok(l.strip_suffix(';')
                .context("unterminated visibility SQL")?
                .replace("{{database}}", database))
        })
        .collect()
}
