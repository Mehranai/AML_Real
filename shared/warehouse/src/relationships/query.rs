use super::{Edge, Graph, Subject};
use crate::{
    Connection, Network,
    publication::{self, literal},
};
use anyhow::{Context, Result, ensure};
use serde_json::Value;

const SQL: &str = include_str!("query.sql");
pub fn query_template() -> &'static str {
    SQL
}
#[derive(Clone, Copy, Debug)]
pub struct Query {
    pub max_transactions: usize,
    pub max_edges: usize,
}
impl Default for Query {
    fn default() -> Self {
        Self {
            max_transactions: 40,
            max_edges: 500,
        }
    }
}
impl Query {
    fn validate(self) -> Result<()> {
        ensure!(
            (1..=100).contains(&self.max_transactions) && (1..=2000).contains(&self.max_edges),
            "relationship query limits exceeded"
        );
        Ok(())
    }
}
#[derive(Clone, Debug)]
struct Occurrence {
    height: u64,
    hash: String,
    commit: String,
    revision: u64,
    tx: String,
    index: u32,
}
impl Occurrence {
    fn parse(network: Network, line: &str) -> Result<Self> {
        let v: Vec<Value> = serde_json::from_str(line)?;
        ensure!(v.len() == 6, "invalid transaction occurrence columns");
        let text = |i: usize| v[i].as_str().context("exact occurrence String required");
        let hash = text(1)?.to_owned();
        let tx = text(4)?.to_owned();
        ensure!(
            crate::values::normalize_hash(network, &hash)? == hash
                && crate::values::normalize_hash(network, &tx)? == tx,
            "invalid occurrence hashes"
        );
        let commit = text(2)?.to_owned();
        let revision = text(3)?.parse()?;
        ensure!(
            !commit.is_empty() && revision > 0,
            "invalid occurrence commit/revision"
        );
        Ok(Self {
            height: text(0)?.parse()?,
            hash,
            commit,
            revision,
            tx,
            index: u32::try_from(v[5].as_u64().context("transaction index missing")?)?,
        })
    }
    fn tuple(&self) -> String {
        format!(
            "({},{},{},{},{},{})",
            self.height,
            literal(&self.hash),
            literal(&self.commit),
            self.revision,
            literal(&self.tx),
            self.index
        )
    }
}
#[derive(Clone, Debug)]
pub struct Neighborhood {
    pub graph: Graph,
    pub revision: u64,
    pub selected_transactions: usize,
}
fn tuple_columns(alias: &str) -> String {
    [
        "block_number",
        "block_hash",
        "commit_id",
        "revision",
        "tx_hash",
        "transaction_index",
    ]
    .into_iter()
    .map(|c| format!("{alias}{c}"))
    .collect::<Vec<_>>()
    .join(",")
}
fn qualified_projection() -> Result<String> {
    let contract = crate::current_contract()?;
    let table = contract
        .tables
        .iter()
        .find(|t| t.name == "aml_address_relationships")
        .context("relationship table missing")?;
    Ok(table
        .columns
        .iter()
        .map(|c| {
            let name = format!("r.`{}`", c.name);
            let typ = c
                .data_type
                .strip_prefix("Nullable(")
                .and_then(|v| v.strip_suffix(')'))
                .unwrap_or(&c.data_type);
            if matches!(typ, "UInt64" | "UInt256") {
                format!("toString({name})")
            } else {
                name
            }
        })
        .collect::<Vec<_>>()
        .join(","))
}
pub async fn read_neighborhood(
    db: &impl Connection,
    database: &str,
    network: Network,
    subject: &Subject,
    query: Query,
) -> Result<Neighborhood> {
    crate::contract::identifier(database)?;
    query.validate()?;
    ensure!(
        subject.network_id() == network.id(),
        "cross-network subject query"
    );
    crate::migration::validate_identity(db, database, network).await?;
    let before = publication::state(db, database, network).await?;
    let predicate = format!(
        "((from_subject_id={} AND from_subject_type={}) OR (to_subject_id={} AND to_subject_type={}))",
        literal(subject.id()),
        literal(subject.kind().as_str()),
        literal(subject.id()),
        literal(subject.kind().as_str())
    );
    let selected=db.query(&format!("SELECT toJSONString(tuple(toString(block_number),block_hash,commit_id,toString(revision),tx_hash,transaction_index)) FROM {database}.aml_address_relationships_canonical WHERE network_id={} AND {predicate} GROUP BY {} ORDER BY block_number DESC,transaction_index,block_hash,tx_hash LIMIT {}",literal(network.id()),tuple_columns(""),query.max_transactions+1)).await?;
    let mut graph = Graph::new(network);
    graph.node(subject.clone())?;
    graph.truncated = selected.len() > query.max_transactions;
    let occurrences = selected
        .iter()
        .take(query.max_transactions)
        .map(|v| Occurrence::parse(network, v))
        .collect::<Result<Vec<_>>>()?;
    if !occurrences.is_empty() {
        let keys = occurrences
            .iter()
            .map(Occurrence::tuple)
            .collect::<Vec<_>>()
            .join(",");
        let predicate = |alias: &str| format!("({}) IN ({keys})", tuple_columns(alias));
        let sql = SQL
            .lines()
            .filter(|l| !l.starts_with("--"))
            .collect::<Vec<_>>()
            .join("\n")
            .replace("{{database}}", database)
            .replace("{{projection}}", &qualified_projection()?)
            .replace("{{network}}", &literal(network.id()))
            .replace("{{selector}}", &predicate("r."))
            .replace("{{witness_selector}}", &predicate(""))
            .replace("{{limit}}", &(query.max_edges + 1).to_string());
        let lines = db.query(&sql).await?;
        graph.truncated |= lines.len() > query.max_edges;
        let mut encoded = Vec::with_capacity(lines.len());
        let mut witnesses = Vec::with_capacity(lines.len());
        for line in lines {
            let mut row: Vec<Value> = serde_json::from_str(&line)?;
            let witness = row.pop().context("missing UTXO witness flag")?;
            encoded.push(serde_json::to_string(&row)?);
            witnesses.push(witness);
        }
        let values = crate::core::decode_rows("aml_address_relationships", network, encoded)?;
        for (index, (value, witness)) in values.into_iter().zip(witnesses).enumerate() {
            let edge = Edge::from_validated(value)?;
            if network == Network::Bitcoin {
                ensure!(
                    witness.as_u64() == Some(1),
                    "canonical UTXO witness does not match relationship {}",
                    edge.evidence()["relationship_id"]
                );
            }
            if index < query.max_edges {
                graph.edge(edge)?;
            }
        }
    }
    ensure!(
        publication::state(db, database, network).await? == before,
        "checkpoint changed during relationship query; retry"
    );
    Ok(Neighborhood {
        graph,
        revision: before.revision,
        selected_transactions: occurrences.len(),
    })
}
