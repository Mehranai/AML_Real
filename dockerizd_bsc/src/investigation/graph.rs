use super::ZERO_ADDRESS;
use crate::{BSC_NETWORK_ID, db::warehouse::Warehouse};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, VecDeque};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edge {
    pub id: String,
    pub network_id: String,
    pub tx_hash: String,
    pub block_number: u64,
    pub block_timestamp_unix_ms: u64,
    pub transaction_index: u32,
    pub event_index: u32,
    pub from_address: String,
    pub to_address: String,
    pub asset_id: String,
    pub token_id: String,
    pub amount: String,
    pub transfer_type: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SearchOptions {
    pub limit: usize,
    pub depth: usize,
    pub from_ms: u64,
    pub to_ms: u64,
    pub asset_id: String,
}
impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            limit: 750,
            depth: 1,
            from_ms: 0,
            to_ms: u64::MAX,
            asset_id: String::new(),
        }
    }
}
impl SearchOptions {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=5000).contains(&self.limit) && (1..=3).contains(&self.depth),
            "limit must be 1..5000 and depth 1..3"
        );
        ensure!(
            self.from_ms <= self.to_ms && self.asset_id.len() <= 200,
            "invalid time or asset filter"
        );
        ensure!(
            self.asset_id.is_empty() || self.asset_id.starts_with("eip155:56/"),
            "asset must belong to BSC"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PathOptions {
    pub max_hops: usize,
    pub max_paths: usize,
    pub per_address_limit: usize,
    #[serde(deserialize_with = "read_direction")]
    pub direction: String,
    pub from_ms: u64,
    pub to_ms: u64,
    pub asset_id: String,
}
fn read_direction<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    match value.as_str() {
        "outbound" | "outgoing" => Ok("outgoing".into()),
        "inbound" | "incoming" => Ok("incoming".into()),
        "both" => Ok(value),
        _ => Err(serde::de::Error::custom(
            "direction must be outgoing, incoming or both",
        )),
    }
}
impl Default for PathOptions {
    fn default() -> Self {
        Self {
            max_hops: 10,
            max_paths: 25,
            per_address_limit: 200,
            direction: "outgoing".into(),
            from_ms: 0,
            to_ms: u64::MAX,
            asset_id: String::new(),
        }
    }
}
impl PathOptions {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=10).contains(&self.max_hops)
                && (1..=100).contains(&self.max_paths)
                && (1..=500).contains(&self.per_address_limit),
            "path limits out of range"
        );
        ensure!(
            matches!(self.direction.as_str(), "outgoing" | "incoming" | "both"),
            "invalid direction"
        );
        SearchOptions {
            from_ms: self.from_ms,
            to_ms: self.to_ms,
            asset_id: self.asset_id.clone(),
            ..Default::default()
        }
        .validate()
    }
}

pub async fn edges(
    db: &Warehouse,
    address: &str,
    options: &SearchOptions,
    max_block: u64,
    limit: usize,
) -> Result<Vec<Edge>> {
    let rows = db.rows(
        "SELECT relationship_id AS id, network_id, tx_hash, block_number, block_timestamp_unix_ms,
         transaction_index, event_index, from_address, to_address, asset_id, token_id, toString(amount) AS amount, transfer_type
         FROM address_relationships_canonical WHERE network_id = 'eip155:56'
         AND (from_address = {address:String} OR to_address = {address:String})
         AND block_number <= {height:UInt64} AND block_timestamp_unix_ms BETWEEN {start:UInt64} AND {end:UInt64}
         AND ({asset:String} = '' OR asset_id = {asset:String})
         ORDER BY block_number DESC, transaction_index DESC, event_index DESC, relationship_id LIMIT {limit:UInt64}",
        &[("address", address), ("height", &max_block.to_string()), ("start", &options.from_ms.to_string()),
          ("end", &options.to_ms.to_string()), ("asset", &options.asset_id), ("limit", &limit.to_string())]).await?;
    rows.into_iter()
        .map(|v| serde_json::from_value(v).map_err(Into::into))
        .collect()
}

pub fn graph(edges: &[Edge], focus: &[&str], truncated: bool) -> Value {
    let mut nodes = BTreeMap::<String, Value>::new();
    for address in focus {
        nodes.insert((*address).into(), json!({"id":address,"address":address,"is_focus":true,"inbound_edges":0,"outbound_edges":0}));
    }
    for edge in edges {
        for (address, field) in [
            (&edge.from_address, "outbound_edges"),
            (&edge.to_address, "inbound_edges"),
        ] {
            let node = nodes.entry(address.clone()).or_insert_with(|| json!({"id":address,"address":address,"is_focus":false,"inbound_edges":0,"outbound_edges":0}));
            node[field] = json!(node[field].as_u64().unwrap_or(0) + 1);
        }
    }
    json!({"nodes":nodes.into_values().collect::<Vec<_>>(),"edges":edges,"truncated":truncated})
}

pub async fn wallet_graph(
    db: &Warehouse,
    address: &str,
    options: &SearchOptions,
    height: u64,
) -> Result<Value> {
    let mut found = BTreeMap::new();
    let mut queue = VecDeque::from([(address.to_owned(), 0)]);
    let mut visited = std::collections::HashSet::new();
    let mut truncated = false;
    while let Some((current, depth)) = queue.pop_front() {
        if !visited.insert(current.clone()) || current == ZERO_ADDRESS {
            continue;
        }
        if visited.len() > 100 {
            truncated = true;
            break;
        }
        let batch = edges(db, &current, options, height, options.limit + 1).await?;
        for edge in batch {
            if found.len() >= options.limit && !found.contains_key(&edge.id) {
                truncated = true;
                break;
            }
            if depth + 1 < options.depth {
                queue.push_back((edge.from_address.clone(), depth + 1));
                queue.push_back((edge.to_address.clone(), depth + 1));
            }
            found.insert(edge.id.clone(), edge);
        }
        if truncated {
            break;
        }
    }
    Ok(graph(
        &found.into_values().collect::<Vec<_>>(),
        &[address],
        truncated,
    ))
}

/// Paths use strictly ordered transactions of the same asset. They establish connectivity,
/// not UTXO-style provenance or ownership; "both" is explicitly topology-only.
pub fn continues(previous: &Edge, next: &Edge, incoming: bool) -> bool {
    let a = (previous.block_number, previous.transaction_index);
    let b = (next.block_number, next.transaction_index);
    previous.asset_id == next.asset_id
        && previous.token_id == next.token_id
        && previous.tx_hash != next.tx_hash
        && if incoming { b < a } else { b > a }
}

pub async fn paths(
    db: &Warehouse,
    source: &str,
    target: &str,
    options: &PathOptions,
    height: u64,
) -> Result<Value> {
    options.validate()?;
    ensure!(
        source != target && source != ZERO_ADDRESS && target != ZERO_ADDRESS,
        "source and target must be distinct nonzero wallets"
    );
    let filters = SearchOptions {
        from_ms: options.from_ms,
        to_ms: options.to_ms,
        asset_id: options.asset_id.clone(),
        ..Default::default()
    };
    let mut cache: HashMap<String, Vec<Edge>> = HashMap::new();
    let mut queue = VecDeque::from([(vec![source.to_owned()], Vec::<Edge>::new())]);
    let mut results = Vec::new();
    let mut used = BTreeMap::new();
    let mut states = 0;
    let mut truncated = false;
    while let Some((addresses, path)) = queue.pop_front() {
        states += 1;
        if states > 10000 || cache.len() >= 100 || results.len() >= options.max_paths {
            truncated = true;
            break;
        }
        if path.len() >= options.max_hops {
            continue;
        }
        let current = addresses.last().unwrap();
        if !cache.contains_key(current) {
            let mut fetched =
                edges(db, current, &filters, height, options.per_address_limit + 1).await?;
            if fetched.len() > options.per_address_limit {
                truncated = true;
                fetched.truncate(options.per_address_limit);
            }
            cache.insert(current.clone(), fetched);
        }
        for edge in &cache[current] {
            let next = match options.direction.as_str() {
                "outgoing" if edge.from_address == *current => &edge.to_address,
                "incoming" if edge.to_address == *current => &edge.from_address,
                "both" => {
                    if edge.from_address == *current {
                        &edge.to_address
                    } else {
                        &edge.from_address
                    }
                }
                _ => continue,
            };
            if next == ZERO_ADDRESS || addresses.contains(next) {
                continue;
            }
            if options.direction != "both"
                && path
                    .last()
                    .is_some_and(|last| !continues(last, edge, options.direction == "incoming"))
            {
                continue;
            }
            let mut new_addresses = addresses.clone();
            new_addresses.push(next.clone());
            let mut new_path = path.clone();
            new_path.push(edge.clone());
            if next == target {
                for item in &new_path {
                    used.insert(item.id.clone(), item.clone());
                }
                results.push(json!({"hop_count":new_path.len(),"addresses":new_addresses,"edge_ids":new_path.iter().map(|e|&e.id).collect::<Vec<_>>()}));
                if results.len() >= options.max_paths {
                    truncated = true;
                    break;
                }
            } else if new_path.len() < options.max_hops {
                if queue.len() >= 10000 {
                    truncated = true;
                } else {
                    queue.push_back((new_addresses, new_path));
                }
            }
        }
    }
    let mut data = graph(
        &used.into_values().collect::<Vec<_>>(),
        &[source, target],
        truncated,
    );
    data["network_id"] = json!(BSC_NETWORK_ID);
    data["source"] = json!(source);
    data["target"] = json!(target);
    data["direction"] = json!(options.direction);
    data["max_hops"] = json!(options.max_hops);
    data["paths"] = json!(results);
    data["expanded_addresses"] = json!(cache.len());
    data["as_of_block"] = json!(height);
    data["continuity_type"] = json!(if options.direction == "both" {
        "topology_only"
    } else {
        "same_asset_ordered_transactions"
    });
    data["limitations"] = json!([
        "Bounded search, not exhaustive. No cross-asset swap/bridge continuity is inferred.",
        "Account-based connectivity does not prove that the same funds moved along a path."
    ]);
    data["neo4j_projection"] = json!({"projected":false,"owner":"main_vm","node_count":data["nodes"].as_array().unwrap().len(),"edge_count":data["edges"].as_array().unwrap().len()});
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounds_are_rejected_not_silently_clamped() {
        let alias: PathOptions =
            serde_json::from_value(serde_json::json!({"direction":"outbound"})).unwrap();
        assert_eq!(alias.direction, "outgoing");
        assert!(
            PathOptions {
                max_hops: 11,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            SearchOptions {
                depth: 4,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn chronology_asset_and_transaction_boundaries() {
        let edge = Edge {
            id: "a".into(),
            network_id: BSC_NETWORK_ID.into(),
            tx_hash: "a".into(),
            block_number: 1,
            block_timestamp_unix_ms: 1,
            transaction_index: 0,
            event_index: 0,
            from_address: "a".into(),
            to_address: "b".into(),
            asset_id: "bnb".into(),
            token_id: String::new(),
            amount: "1".into(),
            transfer_type: "native".into(),
        };
        let mut next = edge.clone();
        next.tx_hash = "b".into();
        next.block_number = 2;
        assert!(continues(&edge, &next, false));
        assert!(!continues(&edge, &next, true));
        next.asset_id = "usdt".into();
        assert!(!continues(&edge, &next, false));
    }
}
