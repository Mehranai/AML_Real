use crate::{
    Network,
    contract::{
        Contract, analytics_contract, baseline_contract, column_contract, core_contract,
        current_contract, identifier,
    },
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
    fn complete(&self, current: u32) -> bool {
        (1..=current).all(|v| self.versions.contains(&v)) && self.identity_version == Some(current)
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
    let columns = column_contract()?;
    let core = core_contract()?;
    let analytics = analytics_contract()?;
    let known = [&baseline, &columns, &core, &analytics, contract];
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
            ledger.complete(contract.version),
            "shared warehouse requires current schema version; run the chain schema job"
        );
    }
    if let Some(max) = ledger.versions.iter().max() {
        ensure!(
            (1..=*max).all(|v| ledger.versions.contains(&v)),
            "warehouse migration predecessor missing"
        );
    }
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
    let core = crate::core_schema::statements(database, &crate::core_schema::sql_template()?)?;
    let analytics =
        crate::analytics_schema::statements(database, &crate::analytics_schema::sql_template()?)?;
    let visibility =
        crate::visibility_schema::statements(database, &crate::visibility_schema::sql_template()?)?;
    initialize_statements(
        db,
        database,
        network,
        vec![statements, upgrade, core, analytics, visibility],
    )
    .await
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
    initialize_from_history(
        db,
        database,
        network,
        baseline_sql,
        upgrade_sql,
        &crate::core_schema::sql_template()?,
    )
    .await
}
pub async fn initialize_from_history(
    db: &impl Connection,
    database: &str,
    network: Network,
    baseline_sql: &str,
    column_sql: &str,
    core_sql: &str,
) -> Result<SchemaSummary> {
    initialize_from_all(
        db,
        database,
        network,
        [
            baseline_sql,
            column_sql,
            core_sql,
            &crate::analytics_schema::sql_template()?,
        ],
    )
    .await
}
pub async fn initialize_from_all(
    db: &impl Connection,
    database: &str,
    network: Network,
    sql: [&str; 4],
) -> Result<SchemaSummary> {
    let [baseline_sql, column_sql, core_sql, analytics_sql] = sql;
    initialize_from_chain(
        db,
        database,
        network,
        [
            baseline_sql,
            column_sql,
            core_sql,
            analytics_sql,
            &crate::visibility_schema::sql_template()?,
        ],
    )
    .await
}
pub async fn initialize_from_chain(
    db: &impl Connection,
    database: &str,
    network: Network,
    sql: [&str; 5],
) -> Result<SchemaSummary> {
    let [
        baseline_sql,
        column_sql,
        core_sql,
        analytics_sql,
        visibility_sql,
    ] = sql;
    let statements = baseline_contract()?.statements_from_template(database, baseline_sql)?;
    let upgrade = crate::semantics::upgrade_statements(database, column_sql)?;
    let core = crate::core_schema::statements(database, core_sql)?;
    let analytics = crate::analytics_schema::statements(database, analytics_sql)?;
    let visibility = crate::visibility_schema::statements(database, visibility_sql)?;
    initialize_statements(
        db,
        database,
        network,
        vec![statements, upgrade, core, analytics, visibility],
    )
    .await
}

async fn initialize_statements(
    db: &impl Connection,
    database: &str,
    network: Network,
    phases: Vec<Vec<String>>,
) -> Result<SchemaSummary> {
    identifier(database)?;
    let contract = current_contract()?;
    db.execute(&format!("CREATE DATABASE IF NOT EXISTS {database}"))
        .await?;
    let stored = objects(db, database).await?;
    let ledger = check_ledger(db, database, network, &contract, &stored, false).await?;
    let contracts = [
        baseline_contract()?,
        column_contract()?,
        core_contract()?,
        analytics_contract()?,
        contract.clone(),
    ];
    let mut applied = ledger.versions.iter().copied().max().unwrap_or(0);
    let identity_writes = u64::from(contract.version - applied)
        .max(u64::from(ledger.identity_version != Some(contract.version)));
    ensure!(
        ledger
            .identity_revision
            .checked_add(identity_writes)
            .is_some(),
        "warehouse identity revision exhausted; upgrade stopped before DDL"
    );
    let mut revision = ledger.identity_revision;
    let mut identity = ledger.identity_version;
    if applied == contract.version {
        validate_objects(db, database, &contract, &stored, true).await?;
        validate_constraints(db, database, &contract, &stored, true).await?;
    }
    while applied < contract.version {
        let next = &contracts[applied as usize];
        let previous = &contracts[applied.saturating_sub(1) as usize];
        let stored = objects(db, database).await?;
        let resume = resume_contract(db, database, previous, next, &stored, applied > 0).await?;
        validate_objects(db, database, &resume, &stored, applied > 0).await?;
        validate_constraints(db, database, &resume, &stored, applied > 0).await?;
        preflight(db, database, next, &stored).await?;
        for statement in &phases[applied as usize] {
            db.execute(statement).await.with_context(|| {
                format!("failed to apply shared schema version {}", next.version)
            })?;
        }
        let installed = objects(db, database).await?;
        validate_objects(db, database, next, &installed, true).await?;
        validate_constraints(db, database, next, &installed, true).await?;
        record_migration(db, database, network, next).await?;
        revision = record_identity(db, database, network, next, revision).await?;
        identity = Some(next.version);
        applied = next.version;
    }
    if identity != Some(contract.version) {
        record_identity(db, database, network, &contract, revision).await?;
    }
    validate(db, database, network).await?;
    summary(
        database,
        network,
        &contract,
        !ledger.complete(contract.version),
    )
}

async fn column_names(
    db: &impl Connection,
    database: &str,
) -> Result<BTreeMap<String, Vec<String>>> {
    let mut columns = BTreeMap::<String, Vec<String>>::new();
    for row in json_rows(db,&format!("SELECT toJSONString(map('table',table,'name',name)) FROM system.columns WHERE database={} AND startsWith(table,'aml_') ORDER BY table,position",literal(database))).await? {
        columns.entry(row["table"].as_str().context("column table missing")?.into()).or_default().push(row["name"].as_str().context("column name missing")?.into());
    }
    Ok(columns)
}
async fn normalized_select(db: &impl Connection, select: &str, database: &str) -> Result<String> {
    Ok(normalized(
        &db.query(&format!(
            "EXPLAIN SYNTAX {}",
            select.replace("{{database}}", database)
        ))
        .await?
        .join("\n"),
    ))
}
/// A partial migration may contain only the declared appended-column prefix,
/// declared constraints, and exactly either the old or new view definition.
/// Applied versions never use this relaxation.
async fn resume_contract(
    db: &impl Connection,
    database: &str,
    previous: &Contract,
    next: &Contract,
    stored: &BTreeMap<String, Value>,
    applied: bool,
) -> Result<Contract> {
    let columns = column_names(db, database).await?;
    let mut resume = previous.clone();
    for table in &mut resume.tables {
        let Some(object) = stored.get(&table.name) else {
            continue;
        };
        let future = next
            .tables
            .iter()
            .find(|t| t.name == table.name)
            .context("table retirement needs an explicit migration")?;
        let actual = columns.get(&table.name).context("stored columns missing")?;
        ensure!(
            actual.len() >= table.columns.len() && actual.len() <= future.columns.len(),
            "unexpected partial column count: {}",
            table.name
        );
        ensure!(
            actual
                .iter()
                .zip(&future.columns)
                .all(|(name, c)| name == &c.name),
            "unexpected partial column order: {}",
            table.name
        );
        table.columns = future.columns[..actual.len()].to_vec();
        let actual_constraints = constraints(
            object["create_table_query"]
                .as_str()
                .context("missing stored DDL")?,
        )?;
        if applied {
            ensure!(
                table
                    .constraints
                    .iter()
                    .all(|c| actual_constraints.contains_key(&c.name)),
                "applied constraint missing: {}",
                table.name
            );
        }
        for name in actual_constraints.keys() {
            let future_constraint = future
                .constraints
                .iter()
                .find(|c| &c.name == name)
                .context("unknown partial constraint")?;
            if !table.constraints.iter().any(|c| &c.name == name) {
                table.constraints.push(future_constraint.clone());
            }
        }
    }
    for view in &mut resume.views {
        let Some(object) = stored.get(&view.name) else {
            continue;
        };
        let actual = normalized_select(
            db,
            object["as_select"].as_str().context("missing view SQL")?,
            database,
        )
        .await?;
        if actual != normalized_select(db, &view.select, database).await? {
            let future = next
                .views
                .iter()
                .find(|v| v.name == view.name)
                .context("view retirement needs explicit migration")?;
            ensure!(
                actual == normalized_select(db, &future.select, database).await?,
                "unexpected partial view definition: {}",
                view.name
            );
            *view = future.clone();
        }
    }
    // A CREATE TABLE is atomic. A new table from an interrupted next migration
    // must match its entire declared shape, including defaults and constraints.
    for table in &next.tables {
        if stored.contains_key(&table.name) && !resume.tables.iter().any(|t| t.name == table.name) {
            resume.tables.push(table.clone());
        }
    }
    for view in &next.views {
        if stored.contains_key(&view.name) && !resume.views.iter().any(|v| v.name == view.name) {
            resume.views.push(view.clone());
        }
    }
    Ok(resume)
}
async fn preflight(
    db: &impl Connection,
    database: &str,
    next: &Contract,
    stored: &BTreeMap<String, Value>,
) -> Result<()> {
    let columns = column_names(db, database).await?;
    for table in &next.tables {
        if !stored.contains_key(&table.name) {
            continue;
        }
        let actual = columns.get(&table.name).context("table columns missing")?;
        let missing = table
            .columns
            .iter()
            .filter(|c| !actual.contains(&c.name))
            .map(|c| {
                Ok(format!(
                    "{} AS `{}`",
                    c.default
                        .as_deref()
                        .context("new column requires an explicit default")?,
                    c.name
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let source = if missing.is_empty() {
            format!("{database}.{}", table.name)
        } else {
            format!(
                "(SELECT *, {} FROM {database}.{})",
                missing.join(","),
                table.name
            )
        };
        for constraint in &table.constraints {
            let violations=db.query_bounded(&format!("SELECT 'invalid' FROM {source} WHERE NOT ifNull(({}),0) LIMIT 1 SETTINGS max_execution_time=60,max_threads=2",constraint.expression)).await?;
            ensure!(
                violations.is_empty(),
                "existing rows violate schema v{}: {}; preserve original evidence and migrate explicitly",
                next.version,
                table.name
            );
        }
    }
    Ok(())
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
