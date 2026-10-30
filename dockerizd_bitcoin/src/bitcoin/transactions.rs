//! Canonical Bitcoin transaction contexts and exact spent-outpoint navigation.
use crate::{NETWORK_ID, db::Database, model};
use aml_warehouse::{
    Network,
    relationships::{self, Kind, Subject},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
const SUPPLY: u64 = 2_100_000_000_000_000;
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Header {
    pub(crate) block_number: u64,
    pub(crate) block_hash: String,
    pub(crate) tx_hash: String,
    pub(crate) transaction_index: u32,
    pub(crate) block_timestamp_unix_ms: u64,
    pub(crate) commit_id: String,
    pub(crate) revision: u64,
    pub(crate) coinbase: u8,
    pub(crate) input_count: Option<u32>,
    pub(crate) output_count: Option<u32>,
    pub(crate) fee_raw: String,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Leg {
    pub(crate) index: u32,
    pub(crate) subject: String,
    pub(crate) value: u64,
    pub(crate) relationship_id: String,
    pub(crate) prev_tx_hash: Option<String>,
    pub(crate) prev_output_index: Option<u32>,
}
#[derive(Clone, Debug)]
pub(crate) struct Tx {
    pub(crate) h: Header,
    pub(crate) inputs: Vec<Leg>,
    pub(crate) outputs: Vec<Leg>,
    pub(crate) total: u64,
    pub(crate) fee: u64,
}
impl Tx {
    pub(crate) fn position(&self) -> (u64, u32) {
        (self.h.block_number, self.h.transaction_index)
    }
    pub(crate) fn key(&self) -> String {
        format!("tx:{}:{}", self.h.block_hash, self.h.tx_hash)
    }
}
fn text(row: &Value, key: &str) -> Result<String> {
    Ok(row[key]
        .as_str()
        .with_context(|| format!("missing exposure field {key}"))?
        .into())
}
fn number(row: &Value, key: &str) -> Result<u64> {
    match &row[key] {
        Value::String(s) => Ok(s.parse()?),
        v => v
            .as_u64()
            .with_context(|| format!("missing exposure integer {key}")),
    }
}
pub(crate) struct Reader<'a> {
    db: &'a Database,
    max_transactions: usize,
    max_legs: usize,
    max_edges: usize,
    pub(crate) cache: HashMap<String, Arc<Tx>>,
    legs: usize,
    pub(crate) truncated: bool,
    pub(crate) missing: usize,
}
impl<'a> Reader<'a> {
    pub(crate) fn new(
        db: &'a Database,
        max_transactions: usize,
        max_legs: usize,
        max_edges: usize,
    ) -> Self {
        Self {
            db,
            max_transactions,
            max_legs,
            max_edges,
            cache: HashMap::new(),
            legs: 0,
            truncated: false,
            missing: 0,
        }
    }
    pub(crate) async fn load(&mut self, block: &str, hash: &str) -> Result<Option<Arc<Tx>>> {
        let key = relationships::transaction_subject_id(Network::Bitcoin, block, hash)?;
        if let Some(tx) = self.cache.get(&key) {
            return Ok(Some(tx.clone()));
        }
        if self.cache.len() >= self.max_transactions {
            self.truncated = true;
            return Ok(None);
        }
        let params = [
            ("network", NETWORK_ID.into()),
            ("block", block.into()),
            ("tx", hash.into()),
        ];
        let rows:Vec<Header>=self.db.rows("SELECT block_number,block_hash,tx_hash,transaction_index,block_timestamp_unix_ms,commit_id,revision,coinbase,input_count,output_count,toString(fee_raw) AS fee_raw FROM aml_transactions_canonical WHERE network_id={network:String} AND block_hash={block:String} AND tx_hash={tx:String} LIMIT 2",&params).await?;
        ensure!(
            rows.len() == 1,
            "missing or ambiguous canonical exposure transaction"
        );
        let h = rows.into_iter().next().unwrap();
        if let (Some(inputs), Some(outputs)) = (h.input_count, h.output_count) {
            let legs =
                inputs.saturating_sub(u32::from(h.coinbase == 1)) as usize + outputs as usize;
            if legs > self.max_edges || self.legs + legs > self.max_legs {
                self.truncated = true;
                return Ok(None);
            }
        }
        let graph = relationships::read_neighborhood(
            &self.db.warehouse_connection(),
            self.db.database(),
            Network::Bitcoin,
            &Subject::new(Network::Bitcoin, Kind::Transaction, &key)?,
            relationships::Query {
                max_transactions: 1,
                max_edges: self.max_edges,
            },
        )
        .await?;
        if graph.graph.truncated || self.legs + graph.graph.edges.len() > self.max_legs {
            self.truncated = true;
            return Ok(None);
        }
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        for edge in graph.graph.edges.values() {
            ensure!(
                edge["block_hash"] == h.block_hash
                    && edge["tx_hash"] == h.tx_hash
                    && number(edge, "revision")? == h.revision
                    && edge["commit_id"] == h.commit_id,
                "exposure occurrence changed"
            );
            let incoming = edge["transfer_type"] == "utxo_input";
            let subject = text(
                edge,
                if incoming {
                    "from_subject_id"
                } else {
                    "to_subject_id"
                },
            )?;
            if !subject.starts_with("script:") {
                ensure!(
                    model::address(&subject)? == subject,
                    "noncanonical exposure address"
                );
            }
            let value = number(edge, "amount")?;
            ensure!(value <= SUPPLY, "exposure amount exceeds supply");
            let leg = Leg {
                index: u32::try_from(number(
                    edge,
                    if incoming {
                        "input_index"
                    } else {
                        "output_index"
                    },
                )?)?,
                subject,
                value,
                relationship_id: text(edge, "id")?,
                prev_tx_hash: if incoming {
                    Some(text(edge, "prev_txid")?)
                } else {
                    None
                },
                prev_output_index: if incoming {
                    Some(u32::try_from(number(edge, "prev_vout")?)?)
                } else {
                    None
                },
            };
            if incoming {
                inputs.push(leg);
            } else {
                outputs.push(leg);
            }
        }
        inputs.sort_by_key(|v| v.index);
        outputs.sort_by_key(|v| v.index);
        let mut outpoints = HashSet::new();
        ensure!(
            inputs
                .iter()
                .all(|i| outpoints.insert((i.prev_tx_hash.clone(), i.prev_output_index))),
            "a transaction repeats a spent outpoint"
        );
        ensure!(
            h.coinbase <= 1
                && h.output_count == Some(outputs.len() as u32)
                && h.input_count
                    == Some(if h.coinbase == 1 {
                        1
                    } else {
                        inputs.len() as u32
                    }),
            "incomplete exposure transaction legs"
        );
        ensure!(
            inputs
                .iter()
                .enumerate()
                .all(|(i, v)| i == v.index as usize)
                && outputs
                    .iter()
                    .enumerate()
                    .all(|(i, v)| i == v.index as usize),
            "duplicate or missing exposure leg positions"
        );
        let total = inputs.iter().try_fold(0u64, |n, v| {
            n.checked_add(v.value).context("exposure input overflow")
        })?;
        let out = outputs.iter().try_fold(0u64, |n, v| {
            n.checked_add(v.value).context("exposure output overflow")
        })?;
        let fee = h.fee_raw.parse::<u64>()?;
        ensure!(
            total <= SUPPLY
                && out <= SUPPLY
                && (if h.coinbase == 1 {
                    inputs.is_empty() && fee == 0
                } else {
                    out.checked_add(fee) == Some(total)
                }),
            "exposure conservation mismatch"
        );
        self.legs += inputs.len() + outputs.len();
        let tx = Arc::new(Tx {
            h,
            inputs,
            outputs,
            total,
            fee,
        });
        self.cache.insert(key, tx.clone());
        Ok(Some(tx))
    }
    pub(crate) async fn previous(
        &mut self,
        tx: &Tx,
        input: &Leg,
    ) -> Result<Option<(Arc<Tx>, usize)>> {
        let params = [
            ("network", NETWORK_ID.into()),
            ("tx", input.prev_tx_hash.clone().context("prevout missing")?),
            (
                "index",
                input
                    .prev_output_index
                    .context("prevout index missing")?
                    .to_string(),
            ),
            ("height", tx.h.block_number.to_string()),
            ("position", tx.h.transaction_index.to_string()),
        ];
        let rows:Vec<Value>=self.db.rows("SELECT block_hash,tx_hash FROM aml_bitcoin_outputs_canonical WHERE network_id={network:String} AND tx_hash={tx:String} AND output_index={index:UInt32} AND tuple(block_number,transaction_index)<tuple({height:UInt64},{position:UInt32}) ORDER BY block_number DESC,transaction_index DESC LIMIT 1",&params).await?;
        let Some(row) = rows.first() else {
            self.missing += 1;
            return Ok(None);
        };
        let Some(parent) = self
            .load(&text(row, "block_hash")?, &text(row, "tx_hash")?)
            .await?
        else {
            return Ok(None);
        };
        let index = input.prev_output_index.unwrap() as usize;
        let output = parent
            .outputs
            .get(index)
            .context("prevout index absent from canonical occurrence")?;
        ensure!(
            parent.position() < tx.position()
                && output.value == input.value
                && output.subject == input.subject,
            "prevout occurrence/value/subject mismatch"
        );
        Ok(Some((parent, index)))
    }
    pub(crate) async fn spender(
        &mut self,
        tx: &Tx,
        index: usize,
    ) -> Result<Option<(Arc<Tx>, usize)>> {
        let params = [
            ("network", NETWORK_ID.into()),
            ("tx", tx.h.tx_hash.clone()),
            ("index", index.to_string()),
            ("height", tx.h.block_number.to_string()),
            ("position", tx.h.transaction_index.to_string()),
        ];
        let rows:Vec<Value>=self.db.rows("SELECT block_hash,tx_hash,input_index FROM aml_bitcoin_inputs_canonical WHERE network_id={network:String} AND prev_tx_hash={tx:String} AND prev_output_index={index:UInt32} AND tuple(block_number,transaction_index)>tuple({height:UInt64},{position:UInt32}) ORDER BY block_number,transaction_index LIMIT 1",&params).await?;
        let Some(row) = rows.first() else {
            return Ok(None);
        };
        let Some(child) = self
            .load(&text(row, "block_hash")?, &text(row, "tx_hash")?)
            .await?
        else {
            return Ok(None);
        };
        let i = usize::try_from(number(row, "input_index")?)?;
        let Some((parent, parent_index)) = self
            .previous(
                &child,
                child.inputs.get(i).context("spending input missing")?,
            )
            .await?
        else {
            return Ok(None);
        };
        // BIP30: the hash/index alone must not connect two different occurrences.
        if parent.key() != tx.key() || parent_index != index {
            self.missing += 1;
            return Ok(None);
        }
        Ok(Some((child, i)))
    }
}
