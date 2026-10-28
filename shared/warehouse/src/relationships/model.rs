use crate::{Network, values};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Address,
    Transaction,
    Script,
}
impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Address => "address",
            Self::Transaction => "transaction",
            Self::Script => "script",
        }
    }
    fn parse(value: &str) -> Result<Self> {
        Ok(match value {
            "address" => Self::Address,
            "transaction" => Self::Transaction,
            "script" => Self::Script,
            _ => anyhow::bail!("unsupported subject type"),
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Subject {
    network_id: String,
    subject_id: String,
    subject_type: Kind,
}
impl Subject {
    pub fn new(network: Network, kind: Kind, id: &str) -> Result<Self> {
        match kind {
            Kind::Address => {
                ensure!(
                    !id.is_empty()
                        && id.len() <= 128
                        && id.bytes().all(|c| c.is_ascii_alphanumeric()),
                    "invalid address subject syntax"
                );
                match network {
                    Network::Ethereum | Network::Bsc => ensure!(
                        id.starts_with("0x")
                            && values::hex(&id[2..], 40)
                            && id == id.to_ascii_lowercase(),
                        "noncanonical EVM address subject"
                    ),
                    Network::Tron => ensure!(
                        values::tron_wallet(id)? == id,
                        "canonical TRON wallet subject required"
                    ),
                    Network::Bitcoin => {
                        let bech = id.starts_with("bc1")
                            && (14..=90).contains(&id.len())
                            && id == id.to_ascii_lowercase();
                        let base = (id.starts_with('1') || id.starts_with('3'))
                            && (26..=35).contains(&id.len())
                            && id.bytes().all(|c| {
                                b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
                                    .contains(&c)
                            });
                        ensure!(bech || base, "Bitcoin mainnet subject syntax required");
                    }
                }
            }
            Kind::Transaction => {
                let (block, tx) = id
                    .strip_prefix("tx:")
                    .and_then(|v| v.split_once(':'))
                    .context("transaction occurrence subject required")?;
                ensure!(
                    values::normalize_hash(network, block)? == block
                        && values::normalize_hash(network, tx)? == tx,
                    "noncanonical transaction occurrence"
                );
            }
            Kind::Script => {
                ensure!(
                    network == Network::Bitcoin,
                    "script subjects require Bitcoin"
                );
                let digest = id
                    .strip_prefix("script:")
                    .context("script subject prefix missing")?;
                ensure!(
                    values::hex(digest, 64) && digest == digest.to_ascii_lowercase(),
                    "noncanonical script identity"
                );
            }
        }
        Ok(Self {
            network_id: network.id().into(),
            subject_id: id.into(),
            subject_type: kind,
        })
    }
    pub fn address(network: Network, id: &str) -> Result<Self> {
        let canonical = match network {
            Network::Tron => values::tron_wallet(id)?,
            Network::Ethereum | Network::Bsc => id.to_ascii_lowercase(),
            Network::Bitcoin => id.to_owned(),
        };
        Self::new(network, Kind::Address, &canonical)
    }
    pub fn id(&self) -> &str {
        &self.subject_id
    }
    pub fn kind(&self) -> Kind {
        self.subject_type
    }
    pub fn network_id(&self) -> &str {
        &self.network_id
    }
    pub fn key(&self) -> String {
        format!(
            "{}/{}/{}",
            self.network_id,
            self.subject_type.as_str(),
            self.subject_id
        )
    }
    pub fn node(&self) -> Value {
        json!({"id":self.subject_id,"subject_id":self.subject_id,"subject_key":self.key(),"subject_type":self.subject_type,"node_type":self.subject_type,"network_id":self.network_id,"address":if self.subject_type==Kind::Address{Some(&self.subject_id)}else{None},"label":self.subject_id})
    }
}
/// Retains the deployed v1 Bitcoin script identity: SHA256 of canonical hex text,
/// not a consensus P2WSH hash. The script's original bytes remain in its fact.
pub fn script_subject_id(hex: &str) -> Result<String> {
    ensure!(
        hex.len().is_multiple_of(2)
            && hex
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
        "canonical script hex required"
    );
    Ok(format!("script:{:x}", Sha256::digest(hex.as_bytes())))
}
pub fn transaction_subject_id(network: Network, block: &str, tx: &str) -> Result<String> {
    let id = format!("tx:{block}:{tx}");
    Subject::new(network, Kind::Transaction, &id)?;
    Ok(id)
}
fn text<'a>(row: &'a Value, key: &str) -> Result<&'a str> {
    row[key]
        .as_str()
        .with_context(|| format!("relationship string missing: {key}"))
}
fn number(row: &Value, key: &str) -> Result<u64> {
    row[key]
        .as_u64()
        .with_context(|| format!("relationship integer missing: {key}"))
}
fn null(row: &Value, key: &str) -> Result<()> {
    ensure!(row[key].is_null(), "inapplicable relationship field: {key}");
    Ok(())
}
fn endpoint(row: &Value, network: Network, side: &str) -> Result<Subject> {
    let kind = Kind::parse(text(row, &format!("{side}_subject_type"))?)?;
    let subject = Subject::new(network, kind, text(row, &format!("{side}_subject_id"))?)?;
    let address = &row[format!("{side}_address")];
    ensure!(
        if kind == Kind::Address {
            address == subject.id()
        } else {
            address.is_null()
        },
        "address and typed subject disagree"
    );
    Ok(subject)
}
/// Deliberately separate from v4 value validation: v4 history remains immutable.
/// Call this at interpreted graph/write boundaries; old nonconforming rows are
/// retained in storage and must be converted explicitly in DB-08.
pub fn validate(row: &Value) -> Result<()> {
    validate_with_contract(&crate::current_contract()?, row)
}
pub fn validate_with_contract(contract: &crate::Contract, row: &Value) -> Result<()> {
    crate::semantics::validate_row(contract, "aml_address_relationships", row)?;
    interpret(row)?;
    Ok(())
}
fn interpret(row: &Value) -> Result<(Subject, Subject)> {
    let network = Network::parse(text(row, "network_id")?)?;
    let from = endpoint(row, network, "from")?;
    let to = endpoint(row, network, "to")?;
    if network == Network::Bitcoin {
        ensure!(
            row["event_sub_index"] == 0
                && row["trace_address"] == json!([])
                && row["token_id"].is_null(),
            "Bitcoin leg cannot carry EVM/NFT positions"
        );
        ensure!(
            row["confidence"] == 1.0,
            "observed UTXO evidence requires confidence 1"
        );
        let tx = transaction_subject_id(network, text(row, "block_hash")?, text(row, "tx_hash")?)?;
        let index = number(row, "event_index")?;
        let (direction, outpoint) = match text(row, "transfer_type")? {
            "utxo_input" => {
                ensure!(
                    matches!(from.kind(), Kind::Address | Kind::Script)
                        && to.kind() == Kind::Transaction
                        && to.id() == tx,
                    "Bitcoin input must connect its prevout subject to its transaction"
                );
                null(row, "output_index")?;
                ensure!(
                    row["attribution_method"] == "observed_prevout",
                    "unknown Bitcoin input attribution"
                );
                let previous = text(row, "prev_tx_hash")?;
                let output = number(row, "prev_output_index")?;
                ("in", format!("outpoint:{previous}:{output}"))
            }
            "utxo_output" => {
                ensure!(
                    from.kind() == Kind::Transaction
                        && from.id() == tx
                        && matches!(to.kind(), Kind::Address | Kind::Script),
                    "Bitcoin output must connect its transaction to its output subject"
                );
                null(row, "prev_tx_hash")?;
                null(row, "prev_output_index")?;
                ensure!(
                    number(row, "output_index")? == index
                        && row["attribution_method"] == "observed_output",
                    "Bitcoin output index/attribution mismatch"
                );
                ("out", format!("outpoint:{}:{index}", text(row, "tx_hash")?))
            }
            _ => anyhow::bail!("Bitcoin does not establish pairwise address transfers"),
        };
        ensure!(
            text(row, "relationship_id")?
                == format!(
                    "{}:{}:{direction}:{index}",
                    text(row, "block_hash")?,
                    text(row, "tx_hash")?
                ),
            "Bitcoin leg identity/index mismatch"
        );
        ensure!(
            row["evidence_refs"]
                .as_array()
                .context("evidence refs missing")?
                .iter()
                .any(|r| r == &outpoint),
            "UTXO outpoint evidence missing"
        );
    } else {
        ensure!(
            from.kind() == Kind::Address && to.kind() == Kind::Address,
            "account transfer endpoints must be observed addresses"
        );
        ensure!(
            !text(row, "transfer_type")?.starts_with("utxo_"),
            "UTXO semantics are not applicable to an account network"
        );
        null(row, "prev_tx_hash")?;
        null(row, "prev_output_index")?;
        null(row, "output_index")?;
    }
    Ok((from, to))
}
#[derive(Clone, Debug)]
pub struct Edge {
    from: Subject,
    to: Subject,
    row: Value,
}
impl Edge {
    pub fn from_row(row: Value) -> Result<Self> {
        crate::semantics::validate_row(
            &crate::current_contract()?,
            "aml_address_relationships",
            &row,
        )?;
        Self::from_validated(row)
    }
    pub(crate) fn from_validated(row: Value) -> Result<Self> {
        let (from, to) = interpret(&row)?;
        Ok(Self { from, to, row })
    }
    pub fn evidence(&self) -> &Value {
        &self.row
    }
    pub fn key(&self) -> String {
        format!(
            "{}/{}/{}/{}/{}",
            self.from.network_id(),
            self.row["block_hash"].as_str().unwrap(),
            self.row["commit_id"].as_str().unwrap(),
            self.row["revision"],
            self.row["relationship_id"].as_str().unwrap()
        )
    }
    pub fn value(&self) -> Value {
        let mut value = self.row.clone();
        let object = value.as_object_mut().unwrap();
        object.remove("raw_evidence_json");
        let bitcoin = self.from.network_id() == Network::Bitcoin.id();
        object.extend(json!({"id":self.key(),"relationship_id":self.row["relationship_id"],"from":self.from.id(),"to":self.to.id(),"amount":self.row["amount_raw"],"fund_continuity_claimed":false,"ownership_claimed":false,"amount_allocation_claimed":false,"edge_semantics":if bitcoin{self.row["transfer_type"].as_str().unwrap()}else{"observed_transfer"},"input_index":if bitcoin && self.row["transfer_type"]=="utxo_input"{self.row["event_index"].clone()}else{Value::Null},"prev_txid":self.row["prev_tx_hash"],"prev_vout":self.row["prev_output_index"]}).as_object().unwrap().clone());
        value
    }
}
#[derive(Clone, Debug)]
pub struct Graph {
    pub nodes: BTreeMap<String, Value>,
    pub edges: BTreeMap<String, Value>,
    pub truncated: bool,
    network_id: String,
}
impl Graph {
    pub fn new(network: Network) -> Self {
        Self {
            nodes: BTreeMap::new(),
            edges: BTreeMap::new(),
            truncated: false,
            network_id: network.id().into(),
        }
    }
    pub fn node(&mut self, subject: Subject) -> Result<()> {
        ensure!(
            subject.network_id() == self.network_id,
            "mixed-network graph"
        );
        self.nodes.insert(subject.id().into(), subject.node());
        Ok(())
    }
    pub fn edge(&mut self, edge: Edge) -> Result<()> {
        self.node(edge.from.clone())?;
        self.node(edge.to.clone())?;
        self.edges.insert(edge.key(), edge.value());
        Ok(())
    }
    pub fn merge(&mut self, other: Self, max_nodes: usize) -> Result<()> {
        ensure!(
            self.network_id == other.network_id,
            "mixed-network graph merge"
        );
        self.truncated |= other.truncated;
        for (id, node) in other.nodes {
            if self.nodes.len() < max_nodes || self.nodes.contains_key(&id) {
                self.nodes.insert(id, node);
            } else {
                self.truncated = true;
            }
        }
        for (id, edge) in other.edges {
            if self
                .nodes
                .contains_key(edge["from_subject_id"].as_str().unwrap_or(""))
                && self
                    .nodes
                    .contains_key(edge["to_subject_id"].as_str().unwrap_or(""))
                && self.edges.len() < 2000
            {
                self.edges.insert(id, edge);
            } else {
                self.truncated = true;
            }
        }
        Ok(())
    }
    pub fn value(&self) -> Value {
        json!({"network_id":self.network_id,"interpretation_version":super::INTERPRETATION_VERSION,"nodes":self.nodes.values().collect::<Vec<_>>(),"edges":self.edges.values().collect::<Vec<_>>(),"truncated":self.truncated,"model":if self.network_id==Network::Bitcoin.id(){"address_transaction_utxo"}else{"typed_observed_transfers"},"ownership_claimed":false,"fund_continuity_claimed":false,"amount_allocation_claimed":false})
    }
}
