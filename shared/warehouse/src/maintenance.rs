//! Bounded operator commands over the existing canonical and failure contracts.
//! Adapters retain their RPC extraction, writer locks and publication semantics.
use crate::{Connection, Network, core, publication, visibility};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeSet, ops::AsyncFn};

pub const MAX_BLOCKS: u64 = 1000;
pub fn failure_limit(name: &str) -> Result<u64> {
    let value = std::env::var(name)
        .unwrap_or_else(|_| "10".into())
        .parse::<u64>()?;
    ensure!(
        (1..=1000).contains(&value),
        "failure budget must be 1..1000"
    );
    Ok(value)
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub action: String,
    pub from: Option<u64>,
    pub to: Option<u64>,
    pub maximum: Option<u64>,
    pub status: Option<String>,
    pub limit: usize,
    pub cursor: Option<(u64, String)>,
    pub include_dead: bool,
}
impl Command {
    pub fn parse(args: &[String]) -> Result<Self> {
        let mut result = Self {
            action: args.first().context("maintenance action required")?.clone(),
            from: None,
            to: None,
            maximum: None,
            status: None,
            limit: 50,
            cursor: None,
            include_dead: false,
        };
        ensure!(
            ["probe", "failures", "scan-gaps", "replay", "repair"]
                .contains(&result.action.as_str()),
            "unknown maintenance action"
        );
        let mut seen = BTreeSet::new();
        let mut i = 1;
        while i < args.len() {
            let flag = args[i].as_str();
            ensure!(seen.insert(flag), "duplicate maintenance option");
            if flag == "--include-dead" {
                ensure!(
                    result.action == "repair",
                    "include-dead is only valid for repair"
                );
                result.include_dead = true;
                i += 1;
                continue;
            }
            let value = args
                .get(i + 1)
                .context("maintenance option requires a value")?;
            match flag {
                "--from-block" => result.from = Some(value.parse()?),
                "--to-block" => result.to = Some(value.parse()?),
                "--max-blocks" => result.maximum = Some(value.parse()?),
                "--status" => result.status = Some(value.clone()),
                "--limit" => result.limit = value.parse()?,
                "--cursor" => {
                    let (height, id) = value.split_once('~').context("invalid failure cursor")?;
                    ensure!(
                        !id.is_empty()
                            && id.len() <= 256
                            && id
                                .bytes()
                                .all(|c| c.is_ascii_alphanumeric() || b":_-./".contains(&c)),
                        "invalid failure cursor ID"
                    );
                    result.cursor = Some((height.parse()?, id.into()));
                }
                _ => anyhow::bail!("unknown maintenance option"),
            }
            let allowed = match result.action.as_str() {
                "failures" => [
                    "--from-block",
                    "--to-block",
                    "--status",
                    "--limit",
                    "--cursor",
                ]
                .contains(&flag),
                "probe" => false,
                _ => ["--from-block", "--to-block", "--max-blocks"].contains(&flag),
            };
            ensure!(
                allowed,
                "option is not supported by this maintenance action"
            );
            i += 2;
        }
        ensure!(
            (1..=200).contains(&result.limit),
            "failure limit must be 1..200"
        );
        if let Some(status) = &result.status {
            ensure!(
                ["retry", "dead_letter", "repairing", "gap", "resolved"].contains(&status.as_str()),
                "invalid failure status"
            );
        }
        if let (Some(from), Some(to)) = (result.from, result.to) {
            ensure!(from <= to, "inverted maintenance range");
        }
        if !["probe", "failures"].contains(&result.action.as_str()) {
            result.range()?;
        }
        Ok(result)
    }
    pub fn range(&self) -> Result<(u64, u64)> {
        let from = self.from.context("from-block is required")?;
        let to = self.to.context("to-block is required")?;
        let maximum = self.maximum.context("max-blocks is required")?;
        ensure!(
            (1..=MAX_BLOCKS).contains(&maximum)
                && to.checked_sub(from).is_some_and(|n| n < maximum),
            "range exceeds max-blocks (1..1000)"
        );
        Ok((from, to))
    }
    pub fn mutates(&self) -> bool {
        matches!(self.action.as_str(), "replay" | "repair")
    }
    pub fn help() -> &'static str {
        "maintenance probe | failures [--status STATUS] [--limit 1..200] [--cursor CURSOR] [--from-block N --to-block M] | scan-gaps/replay/repair --from-block N --to-block M --max-blocks 1..1000 [repair: --include-dead]"
    }
}
fn number(v: &Value) -> Result<u64> {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .context("invalid maintenance row count")
}
async fn rows(
    db: &impl Connection,
    database: &str,
    network: Network,
    filter: &str,
    limit: usize,
) -> Result<Vec<Value>> {
    crate::contract::identifier(database)?;
    let contract = crate::current_contract()?;
    let schema = contract
        .tables
        .iter()
        .find(|t| t.name == "aml_ingestion_failures")
        .context("failure schema missing")?;
    core::decode_rows("aml_ingestion_failures",network,db.query_bounded(&format!("SELECT toJSONString(tuple({})) FROM {database}.aml_ingestion_failures_current WHERE network_id={} AND ({filter}) ORDER BY block_number,failure_id LIMIT {limit}",core::projection(&schema.columns),publication::literal(network.id()))).await?)
}
pub async fn failures(
    db: &impl Connection,
    database: &str,
    network: Network,
    query: &Command,
) -> Result<Value> {
    ensure!(
        (1..=200).contains(&query.limit),
        "failure limit must be 1..200"
    );
    let before = visibility::capture(db, database, network).await?;
    let mut filters = vec!["1".to_owned()];
    if let Some(h) = query.from {
        filters.push(format!("block_number>={h}"));
    }
    if let Some(h) = query.to {
        filters.push(format!("block_number<={h}"));
    }
    if let Some(status) = &query.status {
        filters.push(format!("status={}", publication::literal(status)));
    }
    if let Some((h, id)) = &query.cursor {
        filters.push(format!(
            "(block_number,failure_id)>({h},{})",
            publication::literal(id)
        ));
    }
    let mut items = rows(
        db,
        database,
        network,
        &filters.join(" AND "),
        query.limit + 1,
    )
    .await?;
    let more = items.len() > query.limit;
    items.truncate(query.limit);
    let cursor = if more {
        items.last().map(|r| {
            format!(
                "{}~{}",
                r["block_number"],
                r["failure_id"].as_str().unwrap_or_default()
            )
        })
    } else {
        None
    };
    visibility::unchanged(db, database, &before).await?;
    Ok(
        json!({"contract":"chain_maintenance_v1","network_id":network.id(),"source_snapshot":before.value(),"items":items,
        "pagination":{"has_more":more,"next_cursor":cursor,"limit":query.limit},"scope":"current_failure_records_not_an_immutable_audit_page"}),
    )
}
pub async fn scan(
    db: &impl Connection,
    database: &str,
    network: Network,
    from: u64,
    to: u64,
    maximum: u64,
    require_trace: bool,
) -> Result<Value> {
    ensure!(
        (1..=MAX_BLOCKS).contains(&maximum) && to.checked_sub(from).is_some_and(|n| n < maximum),
        "invalid bounded gap range"
    );
    let snapshot = visibility::capture(db, database, network).await?;
    let state = snapshot.state();
    ensure!(
        state.initialized == 1 && from >= state.start_block && to <= state.last_block_number,
        "gap scan is limited to published history"
    );
    let id = publication::literal(network.id());
    let count = to - from + 1;
    let sql = format!(
        "SELECT toJSONString(tuple(h.number,b.block_hash,b.parent_hash,b.transaction_count,t.n,b.receipt_coverage,b.trace_coverage,b.prevout_coverage)) FROM numbers({from},{count}) AS h LEFT JOIN (SELECT block_number,block_hash,parent_hash,transaction_count,receipt_coverage,trace_coverage,prevout_coverage FROM {database}.aml_ingested_blocks_canonical WHERE network_id={id} AND block_number BETWEEN {from} AND {to}) b ON h.number=b.block_number LEFT JOIN (SELECT block_number,count() AS n FROM {database}.aml_transactions_canonical WHERE network_id={id} AND block_number BETWEEN {from} AND {to} GROUP BY block_number) t ON h.number=t.block_number ORDER BY h.number SETTINGS join_use_nulls=1"
    );
    let records = db.query_bounded(&sql).await?;
    ensure!(records.len() == count as usize, "incomplete gap scan");
    let mut previous = if from > state.start_block {
        db.query_bounded(&format!("SELECT block_hash FROM {database}.aml_ingested_blocks_canonical WHERE network_id={id} AND block_number={}",from-1)).await?.into_iter().next()
    } else {
        None
    };
    let mut items = vec![];
    let mut gaps = 0;
    for (index, raw) in records.iter().enumerate() {
        let row: Vec<Value> = serde_json::from_str(raw)?;
        ensure!(row.len() == 8, "invalid gap scan row");
        let h = number(&row[0])?;
        ensure!(h == from + index as u64, "gap scan ordering mismatch");
        let mut reasons = vec![];
        if row[1].is_null() {
            reasons.push("missing_block");
        } else {
            if number(&row[3])?
                != if row[4].is_null() {
                    0
                } else {
                    number(&row[4])?
                }
            {
                reasons.push("transaction_count_mismatch");
            }
            if h > state.start_block && previous.as_deref() != row[2].as_str() {
                reasons.push("parent_not_verified");
            }
            if network == Network::Bitcoin {
                if row[7].is_null() || number(&row[7])? != 1 {
                    reasons.push("prevout_coverage_missing");
                }
            } else {
                if row[5].is_null() || number(&row[5])? != 1 {
                    reasons.push("receipt_coverage_missing");
                }
                if require_trace && (row[6].is_null() || number(&row[6])? != 1) {
                    reasons.push("trace_coverage_missing");
                }
            }
        }
        if !reasons.is_empty() {
            gaps += 1;
        }
        items.push(json!({"block_number":h,"block_hash":row[1],"expected_transactions":row[3],"stored_transactions":row[4],"reasons":reasons,
            "receipt_coverage":row[5],"trace_coverage":row[6],"prevout_coverage":row[7]}));
        previous = row[1].as_str().map(str::to_owned);
    }
    visibility::unchanged(db, database, &snapshot).await?;
    Ok(
        json!({"contract":"chain_maintenance_v1","network_id":network.id(),"source_snapshot":snapshot.value(),"from_block":from,"to_block":to,"gaps":gaps,"items":items,
        "scope":"published_block_counts_continuity_and_required_coverage","full_history_verified":false,"mutates":false}),
    )
}
pub async fn blocked(
    db: &impl Connection,
    database: &str,
    network: Network,
    height: u64,
) -> Result<bool> {
    Ok(!rows(
        db,
        database,
        network,
        &format!("block_number={height} AND status='dead_letter'"),
        1,
    )
    .await?
    .is_empty())
}
pub async fn read(
    db: &impl Connection,
    database: &str,
    network: Network,
    command: &Command,
    require_trace: bool,
) -> Result<Value> {
    match command.action.as_str() {
        "failures" => failures(db, database, network, command).await,
        "scan-gaps" => {
            let (from, to) = command.range()?;
            scan(
                db,
                database,
                network,
                from,
                to,
                command.maximum.unwrap(),
                require_trace,
            )
            .await
        }
        _ => anyhow::bail!("not a read-only maintenance command"),
    }
}
pub async fn set_failure_status(
    db: &impl Connection,
    database: &str,
    network: Network,
    height: u64,
    status: &str,
) -> Result<()> {
    ensure!(
        ["retry", "resolved", "dead_letter"].contains(&status),
        "invalid maintenance failure transition"
    );
    let mut records = rows(
        db,
        database,
        network,
        &format!("block_number={height} AND status!='resolved'"),
        1001,
    )
    .await?;
    ensure!(
        records.len() <= 1000,
        "too many failure rows for one repair"
    );
    if records.is_empty() {
        return Ok(());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as u64;
    let version=db.query(&format!("SELECT toString(max(revision)) FROM {database}.aml_ingestion_failures WHERE network_id={}",publication::literal(network.id()))).await?;
    let mut revision = version
        .first()
        .context("missing failure revision")?
        .parse::<u64>()?
        .checked_add(1)
        .context("revision overflow")?
        .max(now);
    for record in &mut records {
        record["status"] = json!(status);
        record["next_attempt_unix_ms"] = json!(0);
        record["revision"] = json!(revision);
        record["retryable"] = json!(u8::from(status == "retry"));
        record["resolved_at_unix_ms"] = if status == "resolved" {
            json!(now)
        } else {
            Value::Null
        };
        revision = revision.checked_add(1).context("revision overflow")?;
    }
    publication::insert(db, database, "aml_ingestion_failures", &records).await
}
pub async fn run<C: Connection, F: AsyncFn(u64, bool) -> Result<()>>(
    db: &C,
    database: &str,
    network: Network,
    command: &Command,
    require_trace: bool,
    configured_start: u64,
    write: F,
) -> Result<Value> {
    if command.action == "failures" {
        return failures(db, database, network, command).await;
    }
    let (from, to) = command.range()?;
    if command.action == "scan-gaps" {
        return scan(
            db,
            database,
            network,
            from,
            to,
            command.maximum.unwrap(),
            require_trace,
        )
        .await;
    }
    ensure!(
        command.mutates(),
        "RPC probe must be handled by the adapter"
    );
    let before = visibility::capture(db, database, network).await?;
    if command.action == "replay" {
        ensure!(
            before.state().initialized == 1
                && from >= before.state().start_block
                && to <= before.state().last_block_number,
            "replay requires an already published range"
        );
    }
    let mut results = vec![];
    for height in from..=to {
        let current = visibility::capture(db, database, network).await?;
        let state = current.state();
        let historical = state.initialized == 1 && height <= state.last_block_number;
        let issues = rows(
            db,
            database,
            network,
            &format!("block_number={height} AND status!='resolved'"),
            1001,
        )
        .await?;
        ensure!(issues.len() <= 1000, "too many failure rows for one repair");
        if command.action != "repair" || !command.include_dead {
            ensure!(
                issues.iter().all(|r| r["status"] != "dead_letter"),
                "dead letter requires repair --include-dead"
            );
        }
        if historical {
            let checked = scan(db, database, network, height, height, 1, require_trace).await?;
            ensure!(
                !checked["items"][0]["block_hash"].is_null(),
                "missing historical block marker: restore or rebuild an empty database from RPC; cursor is not rewound automatically"
            );
            if command.action == "repair" && issues.is_empty() && checked["gaps"] == 0 {
                results.push(json!({"block_number":height,"status":"no_detected_gap"}));
                continue;
            }
        } else {
            let next = if state.initialized == 1 {
                state
                    .last_block_number
                    .checked_add(1)
                    .context("height overflow")?
            } else {
                configured_start
            };
            ensure!(
                height == next && command.action == "repair" && !issues.is_empty(),
                "repair cannot skip the published frontier or create unrelated history; use normal ingestion"
            );
        }
        visibility::unchanged(db, database, &current).await?;
        set_failure_status(db, database, network, height, "retry").await?;
        let verified: Result<()> = async {
            write(height, historical).await?;
            let checked = scan(db, database, network, height, height, 1, require_trace).await?;
            ensure!(
                checked["gaps"] == 0,
                "repair did not restore required canonical facts/coverage"
            );
            Ok(())
        }
        .await;
        if let Err(error) = verified {
            if issues.iter().any(|r| r["status"] == "dead_letter") {
                set_failure_status(db, database, network, height, "dead_letter").await?;
            }
            return Err(error);
        }
        set_failure_status(db, database, network, height, "resolved").await?;
        results.push(json!({"block_number":height,"status":"verified"}));
    }
    let after = visibility::capture(db, database, network).await?;
    Ok(
        json!({"contract":"chain_maintenance_v1","network_id":network.id(),"action":command.action,"status":"complete","items":results,
        "source_before":before.value(),"source_snapshot":after.value(),"full_history_verified":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_owned).collect()
    }
    #[test]
    fn strict_ranges_and_cursor_selection() {
        assert!(
            Command::parse(&args(
                "repair --from-block 1 --to-block 2 --max-blocks 2 --include-dead"
            ))
            .is_ok()
        );
        for s in [
            "repair",
            "repair --from-block 2 --to-block 1 --max-blocks 2",
            "replay --from-block 0 --to-block 1000 --max-blocks 1000",
            "probe --limit 3",
            "failures --status unknown",
            "failures --limit 0",
            "scan-gaps --from-block 0 --to-block 1 --max-blocks 2 --include-dead",
            "repair --from-block 1 --from-block 2 --to-block 2 --max-blocks 2",
            "failures --cursor 2~bad'ID",
        ] {
            assert!(Command::parse(&args(s)).is_err(), "{s}");
        }
        assert_eq!(
            Command::parse(&args("failures --cursor 7~network:stage:id"))
                .unwrap()
                .cursor,
            Some((7, "network:stage:id".into()))
        );
    }
}
