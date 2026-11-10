use crate::{ApiError, Network};
use aml_warehouse::{Connection, publication::literal, relationships::Subject, values, visibility};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub const TRANSACTION_ROUTE: &str = "/internal/kyt/transactions/{hash}";
pub const PARTICIPANT_LIMIT: usize = 4;
const BUDGET: &str = " SETTINGS max_threads=2,max_memory_usage=268435456,max_execution_time=8";

#[derive(Debug, Deserialize, Serialize)]
pub struct TransactionObservation {
    pub network_id: String,
    pub tx_hash: String,
    pub source_snapshot: Value,
    pub occurrence: Option<TransactionOccurrence>,
    pub ambiguous: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TransactionOccurrence {
    pub block_number: u64,
    pub block_hash: String,
    pub participants: Vec<String>,
    pub participants_truncated: bool,
}

pub fn transaction_hash(network: Network, value: &str) -> anyhow::Result<String> {
    values::normalize_hash(network, value)
}

pub fn wallet_address(network: Network, value: &str) -> anyhow::Result<String> {
    let value = if network == Network::Bitcoin && value.to_ascii_lowercase().starts_with("bc1") {
        anyhow::ensure!(
            value == value.to_ascii_lowercase() || value == value.to_ascii_uppercase(),
            "mixed-case Bitcoin address"
        );
        value.to_ascii_lowercase()
    } else {
        value.to_owned()
    };
    Ok(Subject::address(network, &value)?.id().to_owned())
}

pub async fn transaction(
    db: &impl Connection,
    database: &str,
    network: Network,
    hash: &str,
) -> Result<TransactionObservation, ApiError> {
    let hash = transaction_hash(network, hash)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "Invalid transaction hash"))?;
    observe(db, database, network, hash).await.map_err(|error| {
        eprintln!("KYT canonical transaction lookup failed: {error:#}");
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Canonical transaction evidence unavailable",
        )
    })
}

async fn observe(
    db: &impl Connection,
    database: &str,
    network: Network,
    hash: String,
) -> anyhow::Result<TransactionObservation> {
    let snapshot = visibility::capture(db, database, network).await?;
    let mut observation = TransactionObservation {
        network_id: network.id().into(),
        tx_hash: hash,
        source_snapshot: snapshot.value(),
        occurrence: None,
        ambiguous: false,
    };
    if snapshot.state().initialized == 0 {
        return Ok(observation);
    }
    let scope = format!(
        "network_id={} AND tx_hash={}",
        literal(network.id()),
        literal(&observation.tx_hash)
    );
    let transactions = db.query_bounded(&format!(
        "SELECT toJSONString(tuple(toString(block_number),block_hash,commit_id,toString(revision),from_address,to_address))
        FROM {database}.aml_transactions_canonical WHERE {scope} LIMIT 2{BUDGET}"
    )).await?;
    observation.ambiguous = transactions.len() > 1;
    if let [row] = transactions.as_slice() {
        let (number, block, commit, revision, from, to): (
            String,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
        ) = serde_json::from_str(row)?;
        let number: u64 = number.parse()?;
        let revision: u64 = revision.parse()?;
        let participants = db.query_bounded(&format!(
            "SELECT toJSONString(subject) FROM (
                SELECT tupleElement(arrayJoin(arrayFilter(endpoint -> endpoint.1 = 'address',
                    [(from_subject_type,from_subject_id),(to_subject_type,to_subject_id)])),2) AS subject
                FROM {database}.aml_address_relationships_canonical WHERE {scope}
                  AND block_number={number} AND block_hash={} AND commit_id={} AND revision={revision}
            ) GROUP BY subject ORDER BY subject LIMIT {}{BUDGET}",
            literal(&block), literal(&commit), PARTICIPANT_LIMIT + 1
        )).await?;
        let related = participants
            .into_iter()
            .map(|address| serde_json::from_str::<String>(&address));
        let mut seen = BTreeSet::new();
        let mut addresses = Vec::new();
        for address in [from, to].into_iter().flatten().map(Ok).chain(related) {
            let address = wallet_address(network, &address?)?;
            if seen.insert(address.clone()) {
                addresses.push(address);
            }
        }
        observation.occurrence = Some(TransactionOccurrence {
            block_number: number,
            block_hash: block,
            participants_truncated: addresses.len() > PARTICIPANT_LIMIT,
            participants: addresses.into_iter().take(PARTICIPANT_LIMIT).collect(),
        });
    }
    visibility::unchanged(db, database, &snapshot).await?;
    Ok(observation)
}
