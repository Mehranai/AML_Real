//! Bounded keyset pages of observed legs/transfers over one unchanged publication.
use super::{Edge, Kind, Subject};
use crate::publication::literal;
use crate::{Connection, Network, publication, visibility};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const VERSION: &str = "transfer_page_v1";
const ORDER: &str = "block_number DESC,transaction_index DESC,event_index DESC,event_sub_index DESC,relationship_id DESC";
const BUDGET: &str = " SETTINGS max_threads=2,max_memory_usage=268435456,max_execution_time=15";

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TransferDirection {
    Incoming,
    Outgoing,
    #[default]
    Both,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct PageQuery {
    pub limit: usize,
    pub from_block: Option<u64>,
    pub to_block: Option<u64>,
    pub from_unix_ms: Option<u64>,
    pub to_unix_ms: Option<u64>,
    pub asset_id: Option<String>,
    pub direction: TransferDirection,
    pub cursor: Option<String>,
}
impl Default for PageQuery {
    fn default() -> Self {
        Self {
            limit: 50,
            from_block: None,
            to_block: None,
            from_unix_ms: None,
            to_unix_ms: None,
            asset_id: None,
            direction: TransferDirection::Both,
            cursor: None,
        }
    }
}

#[derive(Debug)]
pub enum PageError {
    Invalid(anyhow::Error),
    SourceChanged,
    Unavailable(anyhow::Error),
}
impl std::fmt::Display for PageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(error) | Self::Unavailable(error) => write!(f, "{error}"),
            Self::SourceChanged => {
                f.write_str("source snapshot changed; restart pagination without a cursor")
            }
        }
    }
}
impl std::error::Error for PageError {}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Position {
    block_number: u64,
    transaction_index: u32,
    event_index: u32,
    event_sub_index: u32,
    relationship_id: String,
}
impl Position {
    fn from_edge(edge: &Edge) -> Result<Self> {
        let position: Self = serde_json::from_value(json!({
            "block_number":edge.evidence()["block_number"],
            "transaction_index":edge.evidence()["transaction_index"],
            "event_index":edge.evidence()["event_index"],
            "event_sub_index":edge.evidence()["event_sub_index"],
            "relationship_id":edge.evidence()["relationship_id"]
        }))?;
        ensure!(
            !position.relationship_id.is_empty() && position.relationship_id.len() <= 256,
            "invalid page relationship identity"
        );
        Ok(position)
    }
    fn sql(&self) -> String {
        format!(
            "({},{},{},{},{})",
            self.block_number,
            self.transaction_index,
            self.event_index,
            self.event_sub_index,
            literal(&self.relationship_id)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct NoIo(AtomicUsize);
    impl Connection for NoIo {
        async fn execute(&self, _: &str) -> Result<()> {
            unreachable!()
        }
        async fn query(&self, _: &str) -> Result<Vec<String>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            anyhow::bail!("unexpected IO")
        }
    }
    fn subject() -> Subject {
        Subject::address(Network::Ethereum, &format!("0x{}", "1".repeat(40))).unwrap()
    }
    fn continuation(query: &PageQuery) -> Result<String> {
        Cursor {
            contract: VERSION.into(),
            scope: query
                .prepare("fixture", Network::Ethereum, &subject())?
                .scope,
            snapshot_id: "a".repeat(64),
            after: Position {
                block_number: 7,
                transaction_index: 0,
                event_index: 1,
                event_sub_index: 2,
                relationship_id: "event:'quoted'?".into(),
            },
        }
        .encode()
    }
    #[tokio::test]
    async fn invalid_filters_and_mismatched_cursors_fail_before_io() -> Result<()> {
        let db = NoIo(AtomicUsize::new(0));
        let good = PageQuery::default();
        let cursor = continuation(&good)?;
        for query in [
            PageQuery {
                limit: 0,
                ..good.clone()
            },
            PageQuery {
                limit: 101,
                ..good.clone()
            },
            PageQuery {
                from_block: Some(8),
                to_block: Some(7),
                ..good.clone()
            },
            PageQuery {
                from_unix_ms: Some(2),
                to_unix_ms: Some(1),
                ..good.clone()
            },
            PageQuery {
                asset_id: Some("eip155:56/native:BNB".into()),
                ..good.clone()
            },
            PageQuery {
                asset_id: Some("native:ETH' OR 1=1".into()),
                ..good.clone()
            },
            PageQuery {
                cursor: Some(String::new()),
                ..good.clone()
            },
            PageQuery {
                cursor: Some("00".repeat(2049)),
                ..good.clone()
            },
            PageQuery {
                cursor: Some("zz".into()),
                ..good.clone()
            },
            PageQuery {
                cursor: Some(cursor.clone()),
                limit: 1,
                ..good.clone()
            },
            PageQuery {
                cursor: Some(cursor.clone()),
                direction: TransferDirection::Incoming,
                ..good.clone()
            },
        ] {
            assert!(matches!(
                read_page(&db, "fixture", Network::Ethereum, &subject(), &query).await,
                Err(PageError::Invalid(_))
            ));
        }
        let query = PageQuery {
            cursor: Some(cursor),
            ..good
        };
        let other = Subject::address(Network::Ethereum, &format!("0x{}", "2".repeat(40)))?;
        assert!(matches!(
            read_page(&db, "fixture", Network::Ethereum, &other, &query).await,
            Err(PageError::Invalid(_))
        ));
        assert!(matches!(
            read_page(&db, "other", Network::Ethereum, &subject(), &query).await,
            Err(PageError::Invalid(_))
        ));
        assert_eq!(db.0.load(Ordering::SeqCst), 0);
        Ok(())
    }
    #[test]
    fn filters_and_continuation_use_exact_canonical_values() -> Result<()> {
        let query = PageQuery {
            from_block: Some(0),
            to_block: Some(7),
            from_unix_ms: Some(1),
            to_unix_ms: Some(2),
            asset_id: Some(format!("eip155:1/erc1155:0x{}", "a".repeat(40))),
            direction: TransferDirection::Incoming,
            ..Default::default()
        };
        let prepared = query.prepare("fixture", Network::Ethereum, &subject())?;
        let predicate = query.predicate(&subject(), &prepared);
        assert!(predicate.contains("block_number BETWEEN 0 AND 7"));
        assert!(predicate.contains("block_timestamp_unix_ms BETWEEN 1 AND 2"));
        assert!(predicate.contains("to_subject_id="));
        assert!(!predicate.contains("from_subject_id="));
        assert!(predicate.contains(&format!(
            "startsWith(asset_id,'erc1155:0x{}/')",
            "a".repeat(40)
        )));
        let token = continuation(&query)?;
        let parsed = Cursor::decode(&token)?;
        assert_eq!(parsed.after.event_sub_index, 2);
        let next = PageQuery {
            cursor: Some(token),
            ..query
        };
        let predicate = next.predicate(
            &subject(),
            &next.prepare("fixture", Network::Ethereum, &subject())?,
        );
        assert!(predicate.contains(
            "(block_number,transaction_index,event_index,event_sub_index,relationship_id)<(7,0,1,2,"
        ));
        assert!(predicate.contains("\\'quoted"));
        assert!(!predicate.contains("OFFSET"));
        Ok(())
    }
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    contract: String,
    scope: String,
    snapshot_id: String,
    after: Position,
}
impl Cursor {
    fn decode(raw: &str) -> Result<Self> {
        ensure!(
            !raw.is_empty() && raw.len() <= 4096 && raw.len().is_multiple_of(2),
            "invalid cursor length"
        );
        ensure!(
            raw.bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
            "invalid cursor encoding"
        );
        let bytes = raw
            .as_bytes()
            .chunks_exact(2)
            .map(|v| u8::from_str_radix(std::str::from_utf8(v)?, 16).map_err(Into::into))
            .collect::<Result<Vec<_>>>()?;
        let cursor: Self = serde_json::from_slice(&bytes).context("invalid cursor contents")?;
        ensure!(
            cursor.contract == VERSION
                && crate::values::hex(&cursor.scope, 64)
                && crate::values::hex(&cursor.snapshot_id, 64)
                && !cursor.after.relationship_id.is_empty()
                && cursor.after.relationship_id.len() <= 256,
            "invalid cursor contract or position"
        );
        Ok(cursor)
    }
    fn encode(&self) -> Result<String> {
        let raw = serde_json::to_vec(self)?
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        ensure!(raw.len() <= 4096, "cursor exceeds bounded encoding");
        Ok(raw)
    }
}

struct Prepared {
    scope: String,
    asset: Option<crate::values::AssetId>,
    cursor: Option<Cursor>,
}
impl PageQuery {
    fn prepare(&self, database: &str, network: Network, subject: &Subject) -> Result<Prepared> {
        crate::contract::identifier(database)?;
        ensure!(
            subject.network_id() == network.id(),
            "cross-network transfer query"
        );
        ensure!(
            subject.kind() == Kind::Address,
            "transfer pages require an address subject"
        );
        ensure!((1..=100).contains(&self.limit), "limit must be 1..100");
        ensure!(
            self.from_block
                .zip(self.to_block)
                .is_none_or(|(a, b)| a <= b),
            "from_block must not exceed to_block"
        );
        ensure!(
            self.from_unix_ms
                .zip(self.to_unix_ms)
                .is_none_or(|(a, b)| a <= b),
            "from_unix_ms must not exceed to_unix_ms"
        );
        let asset = self
            .asset_id
            .as_deref()
            .map(|value| {
                ensure!(
                    !value.is_empty() && value.len() <= 256,
                    "invalid asset filter length"
                );
                crate::values::AssetId::parse_legacy(network, value)
            })
            .transpose()?;
        let scope_value = json!({"contract":VERSION,"database":database,"subject":subject,
            "limit":self.limit,"from_block":self.from_block,"to_block":self.to_block,
            "from_unix_ms":self.from_unix_ms,"to_unix_ms":self.to_unix_ms,
            "asset_id":asset.as_ref().map(|a|a.as_str()),"direction":self.direction});
        let scope = format!("{:x}", Sha256::digest(serde_json::to_vec(&scope_value)?));
        let cursor = self.cursor.as_deref().map(Cursor::decode).transpose()?;
        ensure!(
            cursor.as_ref().is_none_or(|c| c.scope == scope),
            "cursor belongs to a different network, address, database or query"
        );
        Ok(Prepared {
            scope,
            asset,
            cursor,
        })
    }

    fn predicate(&self, subject: &Subject, prepared: &Prepared) -> String {
        let endpoint = |side: &str| {
            format!(
                "({side}_subject_id={} AND {side}_subject_type={})",
                literal(subject.id()),
                literal(subject.kind().as_str())
            )
        };
        let direction = match self.direction {
            TransferDirection::Incoming => endpoint("to"),
            TransferDirection::Outgoing => endpoint("from"),
            TransferDirection::Both => format!("({} OR {})", endpoint("from"), endpoint("to")),
        };
        let mut predicate = format!(
            "{direction} AND block_number BETWEEN {} AND {} AND block_timestamp_unix_ms BETWEEN {} AND {}",
            self.from_block.unwrap_or(0),
            self.to_block.unwrap_or(u64::MAX),
            self.from_unix_ms.unwrap_or(0),
            self.to_unix_ms.unwrap_or(u64::MAX)
        );
        if let Some(asset) = &prepared.asset {
            predicate.push_str(&if asset.collection {
                format!(
                    " AND startsWith(asset_id,{})",
                    literal(&format!("{}/", asset.as_str()))
                )
            } else {
                format!(" AND asset_id={}", literal(asset.as_str()))
            });
        }
        if let Some(cursor) = &prepared.cursor {
            predicate.push_str(&format!(" AND (block_number,transaction_index,event_index,event_sub_index,relationship_id)<{}",cursor.after.sql()));
        }
        predicate
    }
}

/// Stateless cursor: a continuation marker, never an authentication credential.
/// Conservatively rejects a new publication/reorg/coverage refresh with 409.
pub async fn read_page(
    db: &impl Connection,
    database: &str,
    network: Network,
    subject: &Subject,
    query: &PageQuery,
) -> std::result::Result<Value, PageError> {
    // Decode and bind cursor before any IO, including identity/checkpoint reads.
    let prepared = query
        .prepare(database, network, subject)
        .map_err(PageError::Invalid)?;
    let snapshot = visibility::capture(db, database, network)
        .await
        .map_err(PageError::Unavailable)?;
    if prepared
        .cursor
        .as_ref()
        .is_some_and(|c| c.snapshot_id != snapshot.id())
    {
        return Err(PageError::SourceChanged);
    }
    let data = select_page(
        db,
        database,
        network,
        &query.predicate(subject, &prepared),
        query.limit + 1,
    )
    .await;
    let after = publication::state(db, database, network)
        .await
        .map_err(PageError::Unavailable)?;
    if &after != snapshot.state() {
        return Err(PageError::SourceChanged);
    }
    let mut edges = data.map_err(PageError::Unavailable)?;
    let has_more = edges.len() > query.limit;
    edges.truncate(query.limit);
    let next_cursor = if has_more {
        let position = Position::from_edge(edges.last().expect("nonzero page limit"))
            .map_err(PageError::Unavailable)?;
        Some(
            Cursor {
                contract: VERSION.into(),
                scope: prepared.scope,
                snapshot_id: snapshot.id(),
                after: position,
            }
            .encode()
            .map_err(PageError::Unavailable)?,
        )
    } else {
        None
    };
    Ok(
        json!({"api_version":"1.0","contract":VERSION,"network_id":network.id(),"address":subject.id(),
        "source_snapshot":snapshot.value(),"items":edges.iter().map(Edge::value).collect::<Vec<_>>(),
        "query_scope":{"from_block":query.from_block,"to_block":query.to_block,
            "from_unix_ms":query.from_unix_ms,"to_unix_ms":query.to_unix_ms,
            "asset_id":prepared.asset.as_ref().map(|a|a.as_str()),"direction":query.direction},
        "pagination":{"limit":query.limit,"has_more":has_more,"next_cursor":next_cursor,
            "order":"block_desc_tx_desc_event_desc_subindex_desc_id_desc",
            "snapshot_policy":"restart_on_source_change"},
        "model":if network==Network::Bitcoin{"address_transaction_utxo"}else{"observed_transfers"},
        "history_complete":snapshot.state().initialized==1 && snapshot.state().start_block==0,
        "fund_continuity_claimed":false,"ownership_claimed":false,"amount_allocation_claimed":false}),
    )
}

async fn select_page(
    db: &impl Connection,
    database: &str,
    network: Network,
    predicate: &str,
    limit: usize,
) -> Result<Vec<Edge>> {
    let keys=db.query_bounded(&format!("SELECT toJSONString(tuple(toString(block_number),block_hash,commit_id,toString(revision),tx_hash,transaction_index,relationship_id,event_index,event_sub_index)) FROM {database}.aml_address_relationships_canonical WHERE network_id={} AND ({predicate}) ORDER BY {ORDER} LIMIT {limit}{BUDGET}",literal(network.id()))).await?;
    ensure!(keys.len() <= limit, "page key bound exceeded");
    if keys.is_empty() {
        return Ok(vec![]);
    }
    let mut row_keys = Vec::new();
    let mut witnesses = Vec::new();
    let mut expected = Vec::new();
    for key in keys {
        let row: Vec<Value> = serde_json::from_str(&key)?;
        ensure!(row.len() == 9, "invalid page key columns");
        let text = |i: usize| row[i].as_str().context("exact page key String required");
        let height: u64 = text(0)?.parse()?;
        let revision: u64 = text(3)?.parse()?;
        ensure!(
            crate::values::normalize_hash(network, text(1)?)? == text(1)?
                && crate::values::normalize_hash(network, text(4)?)? == text(4)?,
            "invalid page key hashes"
        );
        let tx_index: u32 = serde_json::from_value(row[5].clone())?;
        let position = Position {
            block_number: height,
            transaction_index: tx_index,
            relationship_id: text(6)?.into(),
            event_index: serde_json::from_value(row[7].clone())?,
            event_sub_index: serde_json::from_value(row[8].clone())?,
        };
        row_keys.push(format!(
            "({height},{},{},{revision},{})",
            literal(text(1)?),
            literal(text(2)?),
            literal(text(6)?)
        ));
        witnesses.push(format!(
            "({height},{},{},{revision},{},{tx_index})",
            literal(text(1)?),
            literal(text(2)?),
            literal(text(4)?)
        ));
        expected.push(position);
    }
    let selector = format!(
        "(r.block_number,r.block_hash,r.commit_id,r.revision,r.relationship_id) IN ({})",
        row_keys.join(",")
    );
    let witness_selector = format!(
        "(block_number,block_hash,commit_id,revision,tx_hash,transaction_index) IN ({})",
        witnesses.join(",")
    );
    let edges = super::query::read_context(
        db,
        database,
        network,
        &selector,
        &witness_selector,
        ORDER,
        limit,
    )
    .await?;
    let actual = edges
        .iter()
        .map(Position::from_edge)
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        actual == expected,
        "page evidence differs from selected canonical keys"
    );
    Ok(edges)
}
