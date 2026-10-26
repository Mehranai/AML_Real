use crate::{
    Network,
    contract::{Contract, baseline_contract, current_contract, identifier},
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashSet},
    future::Future,
    time::{SystemTime, UNIX_EPOCH},
};

/// Both native clickhouse clients and the Bitcoin HTTP client implement this.
/// Query results are single String columns; metadata queries encode each row as
/// JSON. EXPLAIN SYNTAX returns ordinary strings without an extra JSON wrapper.
pub trait Connection: Sync {
    fn execute(&self, sql: &str) -> impl Future<Output = Result<()>> + Send;
    fn query(&self, sql: &str) -> impl Future<Output = Result<Vec<String>>> + Send;
    /// Migration preflight SELECT with query-local resource limits. Native
    /// 0.14.1 defaults fetches to readonly=1 (which forbids SETTINGS); its
    /// adapter uses readonly=2 for this explicit read-only operation only.
    fn query_bounded(&self, sql: &str) -> impl Future<Output = Result<Vec<String>>> + Send {
        self.query(sql)
    }
    fn insert_json(&self, table: &str, body: &str) -> impl Future<Output = Result<()>> + Send {
        async move {
            self.execute(&format!("INSERT INTO {table} FORMAT JSONEachRow\n{body}"))
                .await
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SchemaSummary {
    pub database: String,
    pub network_id: String,
    pub schema_version: u32,
    pub checksum: String,
    pub table_count: usize,
    pub view_count: usize,
    pub newly_applied: bool,
}

async fn json_rows(db: &impl Connection, sql: &str) -> Result<Vec<Value>> {
    db.query(sql)
        .await?
        .into_iter()
        .map(|row| serde_json::from_str(&row).context("invalid warehouse metadata row"))
        .collect()
}
fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}
fn summary(
    database: &str,
    network: Network,
    contract: &Contract,
    newly_applied: bool,
) -> Result<SchemaSummary> {
    Ok(SchemaSummary {
        database: database.into(),
        network_id: network.id().into(),
        schema_version: contract.version,
        checksum: contract.checksum()?,
        table_count: contract.tables.len(),
        view_count: contract.views.len(),
        newly_applied,
    })
}

async fn objects(db: &impl Connection, database: &str) -> Result<BTreeMap<String, Value>> {
    Ok(json_rows(db,&format!("SELECT toJSONString(map('name',name,'engine',engine,'engine_full',engine_full,'partition_key',partition_key,'sorting_key',sorting_key,'primary_key',primary_key,'as_select',as_select,'create_table_query',create_table_query)) AS row FROM system.tables WHERE database={} AND startsWith(name,'aml_') ORDER BY name",literal(database))).await?
        .into_iter().map(|r|(r["name"].as_str().unwrap_or("").to_owned(),r)).collect())
}

#[derive(Default)]
struct Ledger {
    versions: HashSet<u32>,
    identity_version: Option<u32>,
    identity_revision: u64,
}
impl Ledger {
    fn complete(&self) -> bool {
        self.versions.contains(&1) && self.versions.contains(&2) && self.identity_version == Some(2)
    }
}
async fn check_ledger(
    db: &impl Connection,
    database: &str,
    network: Network,
    contract: &Contract,
    stored: &BTreeMap<String, Value>,
    require_complete: bool,
) -> Result<Ledger> {
    let baseline = baseline_contract()?;
    let known = [&baseline, contract];
    let mut ledger = Ledger::default();
    if stored.contains_key("aml_schema_migrations") {
        let rows=json_rows(db,&format!("SELECT toJSONString(map('network_id',network_id,'migration_id',migration_id,'version',toString(schema_version),'checksum',checksum)) AS row FROM {database}.aml_schema_migrations FINAL")).await?;
        for row in rows {
            ensure!(
                row["network_id"] == network.id(),
                "shared warehouse database is bound to another network"
            );
            let expected = known
                .iter()
                .find(|known| {
                    row["migration_id"] == known.migration_id
                        && row["version"].as_str() == Some(known.version.to_string().as_str())
                })
                .context("unknown shared warehouse migration; use the matching adapter release")?;
            ensure!(
                row["checksum"] == expected.checksum()?,
                "shared warehouse migration checksum changed; do not edit an applied contract"
            );
            ledger.versions.insert(expected.version);
        }
    }
    if stored.contains_key("aml_warehouse_identity") {
        let rows=json_rows(db,&format!("SELECT toJSONString(map('network_id',network_id,'version',toString(schema_version),'checksum',checksum,'revision',toString(revision))) AS row FROM {database}.aml_warehouse_identity FINAL")).await?;
        ensure!(
            rows.len() <= 1,
            "multiple network identities in one warehouse database"
        );
        for row in rows {
            ensure!(
                row["network_id"] == network.id(),
                "shared warehouse identity mismatch"
            );
            let expected = known
                .iter()
                .find(|known| {
                    row["version"].as_str() == Some(known.version.to_string().as_str())
                        && row["checksum"] == known.checksum().unwrap_or_default()
                })
                .context("shared warehouse identity version/checksum mismatch")?;
            ensure!(
                ledger.versions.contains(&expected.version),
                "warehouse identity is ahead of its migration ledger"
            );
            ledger.identity_version = Some(expected.version);
            ledger.identity_revision = row["revision"]
                .as_str()
                .context("identity revision missing")?
                .parse()?;
        }
    }
    if require_complete {
        ensure!(
            ledger.complete(),
            "shared warehouse requires schema version 2; run the chain schema job"
        );
    }
    ensure!(
        !ledger.versions.contains(&2) || ledger.versions.contains(&1),
        "column migration is missing its baseline history"
    );
    Ok(ledger)
}

pub async fn initialize(
    db: &impl Connection,
    database: &str,
    network: Network,
) -> Result<SchemaSummary> {
    let statements = baseline_contract()?.statements(database)?;
    let upgrade =
        crate::semantics::upgrade_statements(database, &crate::semantics::upgrade_template()?)?;
    initialize_statements(db, database, network, statements, upgrade).await
}

/// An adapter may embed the generated SQL for review and packaging. Validation
/// happens before any DDL; independent or hand-edited schema copies are rejected.
pub async fn initialize_from_sql(
    db: &impl Connection,
    database: &str,
    network: Network,
    sql: &str,
) -> Result<SchemaSummary> {
    initialize_from_migrations(
        db,
        database,
        network,
        sql,
        &crate::semantics::upgrade_template()?,
    )
    .await
}
pub async fn initialize_from_migrations(
    db: &impl Connection,
    database: &str,
    network: Network,
    baseline_sql: &str,
    upgrade_sql: &str,
) -> Result<SchemaSummary> {
    let statements = baseline_contract()?.statements_from_template(database, baseline_sql)?;
    let upgrade = crate::semantics::upgrade_statements(database, upgrade_sql)?;
    initialize_statements(db, database, network, statements, upgrade).await
}

async fn initialize_statements(
    db: &impl Connection,
    database: &str,
    network: Network,
    statements: Vec<String>,
    upgrade: Vec<String>,
) -> Result<SchemaSummary> {
    identifier(database)?;
    let contract = current_contract()?;
    db.execute(&format!("CREATE DATABASE IF NOT EXISTS {database}"))
        .await?;
    let stored = objects(db, database).await?;
    let ledger = check_ledger(db, database, network, &contract, &stored, false).await?;
    ensure!(
        ledger.identity_version == Some(2) || ledger.identity_revision < u64::MAX,
        "warehouse identity revision exhausted; upgrade stopped before DDL"
    );
    // Validate overlapping objects before adding anything. CREATE IF NOT EXISTS
    // must not silently bless drift, even if an earlier attempt stopped midway.
    validate_objects(
        db,
        database,
        &contract,
        &stored,
        ledger.versions.contains(&1),
    )
    .await?;
    validate_constraints(
        db,
        database,
        &contract,
        &stored,
        ledger.versions.contains(&2),
    )
    .await?;
    if ledger.complete() {
        return summary(database, network, &contract, false);
    }
    if !ledger.versions.contains(&2) {
        // Before any DDL or ledger write, refuse incompatible historical rows.
        // No rewriting, defaults, deletion or automatic approval is performed.
        for table in &contract.tables {
            if stored.contains_key(&table.name) {
                for constraint in &table.constraints {
                    let violations=db.query_bounded(&format!("SELECT 'invalid' FROM {database}.{} WHERE NOT ifNull(({}),0) LIMIT 1 SETTINGS max_execution_time=60,max_threads=2",table.name,constraint.expression)).await?;
                    ensure!(
                        violations.is_empty(),
                        "existing rows violate schema v2: {}; preserve and migrate the original evidence before upgrading",
                        table.name
                    );
                }
            }
        }
    }
    if !ledger.versions.contains(&1) {
        for statement in statements {
            db.execute(&statement)
                .await
                .context("failed to apply shared warehouse baseline")?;
        }
    }
    validate_objects(db, database, &contract, &objects(db, database).await?, true).await?;
    let mut revision = ledger.identity_revision;
    let baseline = baseline_contract()?;
    if !ledger.versions.contains(&1) {
        record_migration(db, database, network, &baseline).await?;
    }
    if ledger.identity_version.is_none() {
        revision = record_identity(db, database, network, &baseline, revision).await?;
    }
    if !ledger.versions.contains(&2) {
        for statement in upgrade {
            db.execute(&statement)
                .await
                .context("failed to apply column-contract migration")?;
        }
    }
    validate_constraints(db, database, &contract, &objects(db, database).await?, true).await?;
    if !ledger.versions.contains(&2) {
        record_migration(db, database, network, &contract).await?;
    }
    if ledger.identity_version != Some(2) {
        record_identity(db, database, network, &contract, revision).await?;
    }
    validate(db, database, network).await?;
    summary(database, network, &contract, !ledger.complete())
}

async fn record_migration(
    db: &impl Connection,
    database: &str,
    network: Network,
    contract: &Contract,
) -> Result<()> {
    let now: u64 = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_millis()
        .try_into()?;
    db.execute(&format!("INSERT INTO {database}.aml_schema_migrations (network_id,migration_id,schema_version,checksum,revision,created_at_unix_ms) VALUES ({},{},{},{},{},{})",
        literal(network.id()),literal(&contract.migration_id),contract.version,literal(&contract.checksum()?),now,now)).await
}
async fn record_identity(
    db: &impl Connection,
    database: &str,
    network: Network,
    contract: &Contract,
    previous: u64,
) -> Result<u64> {
    let now: u64 = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_millis()
        .try_into()?;
    let revision = now.max(
        previous
            .checked_add(1)
            .context("identity revision exhausted")?,
    );
    db.execute(&format!("INSERT INTO {database}.aml_warehouse_identity (network_id,schema_version,checksum,revision,created_at_unix_ms) VALUES ({},{},{},{},{})",
        literal(network.id()),contract.version,literal(&contract.checksum()?),revision,now)).await?;
    Ok(revision)
}

/// Pure read operation: used by startup validation and explicit schema --check.
pub async fn validate(
    db: &impl Connection,
    database: &str,
    network: Network,
) -> Result<SchemaSummary> {
    identifier(database)?;
    let contract = current_contract()?;
    let stored = objects(db, database).await?;
    check_ledger(db, database, network, &contract, &stored, true).await?;
    validate_objects(db, database, &contract, &stored, true).await?;
    validate_constraints(db, database, &contract, &stored, true).await?;
    summary(database, network, &contract, false)
}

/// Lightweight liveness dependency check. Full drift validation remains part of
/// every schema job and explicit check, rather than running 40 EXPLAINs per probe.
pub async fn validate_identity(
    db: &impl Connection,
    database: &str,
    network: Network,
) -> Result<()> {
    identifier(database)?;
    let contract = current_contract()?;
    check_ledger(
        db,
        database,
        network,
        &contract,
        &objects(db, database).await?,
        true,
    )
    .await?;
    Ok(())
}

async fn validate_objects(
    db: &impl Connection,
    database: &str,
    contract: &Contract,
    stored: &BTreeMap<String, Value>,
    require_all: bool,
) -> Result<()> {
    let expected: HashSet<_> = contract
        .tables
        .iter()
        .map(|t| t.name.as_str())
        .chain(contract.views.iter().map(|v| v.name.as_str()))
        .collect();
    ensure!(
        stored.keys().all(|n| expected.contains(n.as_str())),
        "unexpected object in shared warehouse namespace"
    );
    if require_all {
        ensure!(
            stored.len() == expected.len(),
            "shared warehouse table/view missing"
        );
    }
    let mut columns = BTreeMap::<String, Vec<Value>>::new();
    for row in json_rows(db,&format!("SELECT toJSONString(map('table',table,'name',name,'type',type,'default_kind',default_kind,'default_expression',default_expression)) AS row FROM system.columns WHERE database={} AND startsWith(table,'aml_') ORDER BY table,position",literal(database))).await? {
        columns.entry(row["table"].as_str().unwrap_or("").into()).or_default().push(row);
    }
    let mut indexes = BTreeMap::<String, Vec<Value>>::new();
    for row in json_rows(db,&format!("SELECT toJSONString(map('table',table,'name',name,'type',type_full,'expression',expr,'granularity',toString(granularity))) AS row FROM system.data_skipping_indices WHERE database={} AND startsWith(table,'aml_') ORDER BY table,name",literal(database))).await? {
        indexes.entry(row["table"].as_str().unwrap_or("").into()).or_default().push(row);
    }
    for table in &contract.tables {
        let Some(actual) = stored.get(&table.name) else {
            continue;
        };
        let actual_columns = columns
            .get(&table.name)
            .context("shared table has no columns")?;
        ensure!(
            actual_columns.len() == table.columns.len(),
            "shared warehouse column count drift: {}",
            table.name
        );
        for (actual, column) in actual_columns.iter().zip(&table.columns) {
            ensure!(
                actual["name"] == column.name
                    && normalized(actual["type"].as_str().unwrap_or(""))
                        == normalized(&column.data_type),
                "shared warehouse column/type drift: {}.{}",
                table.name,
                column.name
            );
            ensure!(
                actual["default_kind"]
                    == if column.default.is_some() {
                        "DEFAULT"
                    } else {
                        ""
                    },
                "shared column default kind drift: {}.{}",
                table.name,
                column.name
            );
            ensure!(
                normalized(actual["default_expression"].as_str().unwrap_or(""))
                    == normalized(column.default.as_deref().unwrap_or("")),
                "shared column default drift: {}.{}",
                table.name,
                column.name
            );
        }
        let full_engine = actual["engine_full"].as_str().unwrap_or("");
        let engine = full_engine
            .split(" PARTITION BY ")
            .next()
            .unwrap_or("")
            .split(" ORDER BY ")
            .next()
            .unwrap_or("");
        ensure!(
            normalized(engine) == normalized(&table.engine),
            "shared warehouse engine drift: {}",
            table.name
        );
        ensure!(
            !full_engine.contains(" TTL ")
                && !full_engine.contains(" SAMPLE BY ")
                && full_engine
                    .split(" SETTINGS ")
                    .nth(1)
                    .is_some_and(|s| normalized(s) == "index_granularity=8192"),
            "shared warehouse retention or settings drift: {}",
            table.name
        );
        let order = table.order_by.join(",");
        ensure!(
            key(actual["sorting_key"].as_str().unwrap_or("")) == key(&order)
                && key(actual["primary_key"].as_str().unwrap_or("")) == key(&order),
            "shared warehouse key drift: {}",
            table.name
        );
        ensure!(
            normalized(actual["partition_key"].as_str().unwrap_or(""))
                == normalized(table.partition_by.as_deref().unwrap_or("")),
            "shared warehouse partition drift: {}",
            table.name
        );
        let actual_indexes = indexes
            .get(&table.name)
            .map(Vec::as_slice)
            .unwrap_or_default();
        ensure!(
            actual_indexes.len() == table.indexes.len(),
            "shared warehouse index count drift: {}",
            table.name
        );
        for index in &table.indexes {
            let actual = actual_indexes
                .iter()
                .find(|v| v["name"] == index.name)
                .context("missing shared skipping index")?;
            ensure!(
                normalized(actual["type"].as_str().unwrap_or("")) == normalized(&index.index_type)
                    && normalized(actual["expression"].as_str().unwrap_or(""))
                        == normalized(&index.expression)
                    && actual["granularity"]
                        .as_str()
                        .and_then(|v| v.parse::<u32>().ok())
                        == Some(index.granularity),
                "shared warehouse skipping index drift: {}.{}",
                table.name,
                index.name
            );
        }
    }
    for view in &contract.views {
        let Some(actual) = stored.get(&view.name) else {
            continue;
        };
        ensure!(
            actual["engine"] == "View",
            "shared warehouse view replaced with another engine: {}",
            view.name
        );
        let expected_select = view.select.replace("{{database}}", database);
        let actual_select = actual["as_select"]
            .as_str()
            .context("missing shared view definition")?;
        // Ask this ClickHouse version to normalize both expressions. This catches
        // predicate drift without treating formatter or alias layout as a change.
        let expected = db
            .query(&format!("EXPLAIN SYNTAX {expected_select}"))
            .await?
            .join("\n");
        let actual = db
            .query(&format!("EXPLAIN SYNTAX {actual_select}"))
            .await?
            .join("\n");
        ensure!(
            normalized(&actual) == normalized(&expected),
            "shared warehouse view definition drift: {}",
            view.name
        );
    }
    Ok(())
}

/// Split only top-level CREATE definition items; literals and nested SQL keep
/// their commas/parentheses. Unknown constraints are schema drift as well.
fn constraints(sql: &str) -> Result<BTreeMap<String, String>> {
    let start = sql.find('(').context("missing CREATE column list")?;
    let mut depth = 0i32;
    let mut bracket = 0i32;
    let mut quote = None;
    let mut escaped = false;
    let mut begin = start + 1;
    let mut items = Vec::new();
    for (offset, c) in sql[start..].char_indices() {
        let index = start + offset;
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' | '`' => quote = Some(c),
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    items.push(&sql[begin..index]);
                    break;
                }
            }
            '[' => bracket += 1,
            ']' => bracket -= 1,
            ',' if depth == 1 && bracket == 0 => {
                items.push(&sql[begin..index]);
                begin = index + 1;
            }
            _ => {}
        }
    }
    ensure!(
        depth == 0 && quote.is_none(),
        "incomplete CREATE definition"
    );
    let mut result = BTreeMap::new();
    for item in items {
        if let Some(rest) = item.trim().strip_prefix("CONSTRAINT ") {
            let (name, expression) = rest
                .split_once(" CHECK ")
                .context("invalid stored constraint")?;
            let name = name.trim().trim_matches('`').to_owned();
            ensure!(
                result.insert(name, expression.trim().into()).is_none(),
                "duplicate stored constraint"
            );
        }
    }
    Ok(result)
}
async fn validate_constraints(
    db: &impl Connection,
    database: &str,
    contract: &Contract,
    stored: &BTreeMap<String, Value>,
    require_all: bool,
) -> Result<()> {
    for table in &contract.tables {
        let Some(object) = stored.get(&table.name) else {
            continue;
        };
        let actual = constraints(
            object["create_table_query"]
                .as_str()
                .context("stored table definition missing")?,
        )?;
        ensure!(
            actual
                .keys()
                .all(|name| table.constraints.iter().any(|c| &c.name == name)),
            "unknown column constraint: {}",
            table.name
        );
        if require_all {
            ensure!(
                actual.len() == table.constraints.len(),
                "column constraint missing: {}",
                table.name
            );
        }
        for expected in &table.constraints {
            let Some(actual) = actual.get(&expected.name) else {
                continue;
            };
            let normalize = async |expression: &str| -> Result<String> {
                Ok(normalized(
                    &db.query(&format!(
                        "EXPLAIN SYNTAX SELECT ({expression}) FROM {database}.{}",
                        table.name
                    ))
                    .await?
                    .join("\n"),
                ))
            };
            ensure!(
                normalize(actual).await? == normalize(&expected.expression).await?,
                "column constraint drift: {}.{}",
                table.name,
                expected.name
            );
        }
    }
    Ok(())
}

fn normalized(sql: &str) -> String {
    let mut result = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for c in sql.chars() {
        if quoted {
            result.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '\'' {
                quoted = false;
            }
        } else if c == '\'' {
            quoted = true;
            result.push(c);
        } else if !c.is_whitespace() && c != '`' {
            result.push(c);
        }
    }
    result
}
fn key(sql: &str) -> String {
    normalized(sql)
        .trim_start_matches('(')
        .trim_end_matches(')')
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn constraint_parser_preserves_quoted_commas_nested_functions_and_arrays() {
        let sql = "CREATE TABLE db.t (`x` String DEFAULT 'a, ) b', `a` Array(String) DEFAULT ['a','b'], CONSTRAINT `aml_columns_v2` CHECK x IN ('a, ) b','c') AND length(x)>0) ENGINE=MergeTree ORDER BY x";
        let result = constraints(sql).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            result["aml_columns_v2"],
            "x IN ('a, ) b','c') AND length(x)>0"
        );
        assert!(constraints("CREATE TABLE db.t (x String, CONSTRAINT duplicate CHECK 1, CONSTRAINT duplicate CHECK 0) ENGINE=MergeTree ORDER BY x").is_err());
    }
    #[test]
    fn sql_normalization_preserves_literal_spaces_and_predicate_parentheses() {
        assert_eq!(
            normalized("SELECT `x` FROM t WHERE y='a b'"),
            "SELECTxFROMtWHEREy='a b'"
        );
        assert_ne!(
            normalized("WHERE a AND (b OR c)"),
            normalized("WHERE (a AND b) OR c")
        );
        assert_ne!(normalized("'a b'"), normalized("'ab'"));
    }
}
