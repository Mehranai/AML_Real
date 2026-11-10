//! Additive presentation contract. Native risk/holdings evidence stays immutable.
use aml_warehouse::{
    Network, RawAmount,
    relationships::{Kind, Subject},
    values::AssetId,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};
pub const VERSION: &str = "chain_response_v1";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Wallet,
    Paths,
}

fn text<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value[name]
        .as_str()
        .filter(|v| !v.is_empty())
        .with_context(|| format!("missing response {name}"))
}
fn integer(value: &Value, signed: bool) -> Result<Value> {
    if value.is_null() {
        return Ok(Value::Null);
    }
    let raw = value
        .as_str()
        .context("exact amount must be a decimal string")?;
    let digits = if signed {
        raw.strip_prefix('-').unwrap_or(raw)
    } else {
        raw
    };
    ensure!(
        !digits.is_empty()
            && digits.len() <= 128
            && digits.bytes().all(|b| b.is_ascii_digit())
            && (digits == "0" || !digits.starts_with('0'))
            && raw != "-0",
        "invalid exact integer"
    );
    Ok(json!(raw))
}
fn asset(network: Network, raw: &str, token: Option<&str>) -> Result<(String, Value)> {
    let parsed = AssetId::parse_legacy(network, raw)?;
    let token = token.filter(|v| !v.is_empty());
    if let Some(id) = token {
        RawAmount::parse(id)?;
    }
    let canonical = if parsed.collection {
        token
            .map(|id| format!("{}/{id}", parsed.as_str()))
            .unwrap_or_else(|| parsed.as_str().to_owned())
    } else {
        parsed.as_str().to_owned()
    };
    let checked = AssetId::parse(network, &canonical)?;
    if !checked.collection && canonical.contains('/') {
        ensure!(
            token.is_none_or(|id| canonical
                .rsplit_once('/')
                .is_some_and(|(_, actual)| actual == id)),
            "NFT token ID mismatch"
        );
    }
    let native = canonical == network.native_asset();
    let id = if native {
        Value::Null
    } else if let Some(id) = token {
        json!(id)
    } else if !checked.collection {
        canonical
            .rsplit_once('/')
            .map(|(_, id)| json!(id))
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    Ok((canonical, id))
}
fn source(network: Network, data: &Value) -> Result<()> {
    let value = &data["source_snapshot"];
    ensure!(
        value["contract"] == "canonical_v1" && value["network_id"] == network.id(),
        "source snapshot contract/network mismatch"
    );
    ensure!(
        aml_warehouse::values::hex(text(value, "snapshot_id")?, 64),
        "invalid source snapshot ID"
    );
    text(value, "source_revision")?.parse::<u64>()?;
    ensure!(
        value["as_of_block"].is_null() || value["as_of_block"].as_u64().is_some(),
        "invalid source height"
    );
    Ok(())
}
fn graph(network: Network, graph: &mut Value) -> Result<()> {
    let nodes = graph["nodes"]
        .as_array_mut()
        .context("missing graph nodes")?;
    ensure!(nodes.len() <= 20_000, "graph node cap exceeded");
    let mut subjects = BTreeMap::new();
    for node in nodes {
        let id = node
            .get("subject_id")
            .or_else(|| node.get("address"))
            .or_else(|| node.get("id"))
            .and_then(Value::as_str)
            .context("missing graph subject")?
            .to_owned();
        let kind = if network == Network::Bitcoin {
            match node["subject_type"].as_str().or(node["node_type"].as_str()) {
                Some("address") => Kind::Address,
                Some("transaction") => Kind::Transaction,
                Some("script") => Kind::Script,
                _ => anyhow::bail!("unknown Bitcoin graph subject"),
            }
        } else {
            Kind::Address
        };
        let subject = Subject::new(network, kind, &id)?;
        for alias in ["id", "address"] {
            ensure!(
                node[alias].as_str().is_none_or(|native| native == id),
                "graph subject aliases disagree"
            );
        }
        ensure!(
            subjects.insert(id.clone(), kind).is_none(),
            "duplicate graph subject"
        );
        node["subject_id"] = json!(id);
        node["subject_type"] = json!(kind.as_str());
        node["subject_key"] = json!(subject.key());
        node["address"] = if kind == Kind::Address {
            json!(id)
        } else {
            Value::Null
        };
        if node["id"].is_null() {
            node["id"] = json!(id);
        }
    }
    let edges = graph["edges"]
        .as_array_mut()
        .context("missing graph edges")?;
    ensure!(edges.len() <= 20_000, "graph edge cap exceeded");
    let mut ids = HashSet::new();
    for edge in edges {
        ensure!(
            ids.insert(text(edge, "id")?.to_owned()),
            "duplicate graph edge"
        );
        let endpoint = |name: &str| -> Result<String> {
            [
                format!("{name}_subject_id"),
                format!("{name}_address"),
                name.into(),
            ]
            .iter()
            .find_map(|key| edge[key].as_str())
            .map(str::to_owned)
            .context("edge endpoint missing")
        };
        let from = endpoint("from")?;
        let to = endpoint("to")?;
        for (name, id) in [("from", &from), ("to", &to)] {
            for alias in [name.to_owned(), format!("{name}_address")] {
                ensure!(
                    edge[alias].as_str().is_none_or(|native| native == id),
                    "graph endpoint aliases disagree"
                );
            }
        }
        let from_kind = *subjects.get(&from).context("graph edge source missing")?;
        let to_kind = *subjects.get(&to).context("graph edge target missing")?;
        if network == Network::Bitcoin {
            ensure!(
                match text(edge, "transfer_type")? {
                    "utxo_input" =>
                        matches!(from_kind, Kind::Address | Kind::Script)
                            && to_kind == Kind::Transaction,
                    "utxo_output" =>
                        from_kind == Kind::Transaction
                            && matches!(to_kind, Kind::Address | Kind::Script),
                    _ => false,
                },
                "Bitcoin graph cannot allocate pairwise address payments"
            );
        }
        let amount = integer(edge.get("amount").unwrap_or(&edge["amount_raw"]), false)?;
        ensure!(
            edge["amount_raw"].is_null() || edge["amount_raw"] == amount,
            "raw and native transfer amounts disagree"
        );
        ensure!(!amount.is_null(), "transfer amount unavailable");
        RawAmount::parse(amount.as_str().unwrap())?;
        let (canonical, token) =
            asset(network, text(edge, "asset_id")?, edge["token_id"].as_str())?;
        ensure!(
            !AssetId::parse(network, &canonical)?.collection,
            "NFT transfer requires a token ID"
        );
        edge["from_subject_id"] = json!(from);
        edge["to_subject_id"] = json!(to);
        edge["from_subject_type"] = json!(from_kind.as_str());
        edge["to_subject_type"] = json!(to_kind.as_str());
        edge["canonical_asset_id"] = json!(canonical);
        edge["canonical_token_id"] = token;
        edge["amount_raw"] = amount;
        edge["source_position"] = json!({"block_number":edge["block_number"],
            "transaction_index":edge["transaction_index"],"event_index":edge["event_index"],
            "event_sub_index":edge["event_sub_index"],"trace_address":edge["trace_address"],
            "block_timestamp_unix_ms":edge.get("block_timestamp_unix_ms").or_else(||edge.get("timestamp"))});
        edge["from_address"] = if from_kind == Kind::Address {
            json!(from)
        } else {
            Value::Null
        };
        edge["to_address"] = if to_kind == Kind::Address {
            json!(to)
        } else {
            Value::Null
        };
        for flag in [
            "ownership_claimed",
            "fund_continuity_claimed",
            "amount_allocation_claimed",
        ] {
            ensure!(edge[flag] != true, "unsupported graph assertion");
            edge[flag] = json!(false);
        }
    }
    graph["model"] = json!(if network == Network::Bitcoin {
        "address_transaction_utxo"
    } else {
        "observed_account_transfers"
    });
    Ok(())
}
fn activity(data: &Value, mode: Mode) -> Result<Value> {
    if mode == Mode::Paths {
        return Ok(json!({"status":"not_requested","items":[]}));
    }
    let rows = data["activity"]
        .as_array()
        .or_else(|| data["activity"]["trend"].as_array());
    let Some(rows) = rows else {
        return Ok(json!({"status":"unavailable","items":[]}));
    };
    let mut items = vec![];
    for row in rows {
        let get = |names: &[&str]| {
            names
                .iter()
                .find_map(|name| row.get(*name))
                .cloned()
                .unwrap_or(Value::Null)
        };
        items.push(
            json!({"bucket_start_unix_ms":get(&["bucket_start_unix_ms","day_unix_ms"]),
            "received_count":get(&["incoming_transfers","received"]),
            "sent_count":get(&["outgoing_transfers","spent","sent"]),
            "transaction_count":get(&["unique_transactions","transactions"])}),
        );
    }
    Ok(
        json!({"status":"available","items":items,"scope":data["query_scope"],
        "bucket_unit":data["activity"]["bucket_unit"].as_str().unwrap_or("day"),
        "complete_history_claimed":false}),
    )
}
fn holdings(network: Network, data: &Value, mode: Mode) -> Result<Value> {
    let model = match network {
        Network::Bitcoin => "indexed_utxo",
        Network::Tron => "indexed_netflow",
        _ => "finalized_rpc",
    };
    if mode == Mode::Paths {
        return Ok(json!({"status":"not_requested","model":model,"assets":[]}));
    }
    let raw = &data["holdings"];
    if !raw.is_object() {
        return Ok(json!({"status":"unavailable","model":model,"assets":[]}));
    }
    let mut assets = vec![];
    let status = if network == Network::Bitcoin {
        assets.push(json!({"asset_id":network.native_asset(),"token_id":null,"amount_raw":integer(&raw["balance_sats"],false)?,
            "decimals":8,"status":raw["status"],"is_complete_balance":raw["is_complete_balance"]}));
        raw["status"].as_str().unwrap_or("unavailable")
    } else {
        if let Some(rows) = raw["assets"].as_array() {
            for row in rows {
                let (id, token) = asset(network, text(row, "asset_id")?, row["token_id"].as_str())?;
                let amount = integer(
                    if network == Network::Tron {
                        &row["balance_raw"]
                    } else {
                        &row["amount"]
                    },
                    network == Network::Tron,
                )?;
                assets.push(json!({"asset_id":id,"token_id":token,"amount_raw":amount,
                "decimals":if row["decimals_known"]==false{Value::Null}else{row["decimals"].clone()},
                "status":if amount.is_null(){"unavailable"}else if network==Network::Tron{"partial"}else{"available"},
                "is_complete_balance":if network==Network::Tron{false}else{!amount.is_null()}}));
            }
        }
        if network == Network::Tron {
            "partial"
        } else {
            raw["status"].as_str().unwrap_or("unavailable")
        }
    };
    Ok(
        json!({"status":status,"model":model,"assets":assets,"source":raw["source"],
        "scope":raw["observation_scope"].as_str().or(raw["scope"].as_str()),
        "observation_snapshot":{"canonical":raw["source_snapshot"],"block_number":raw["block_number"],
            "block_hash":raw["block_hash"],"observed_at_unix_ms":raw["observed_at_unix_ms"]},
        "unavailable_is_not_zero":true}),
    )
}
fn attribution(data: &Value, mode: Mode) -> Value {
    if mode == Mode::Paths {
        return json!({"status":"not_requested","items":[],"ownership_claimed":false});
    }
    let entities = data["entities"]
        .as_array()
        .or_else(|| data["intelligence"]["entity_claims"].as_array());
    let items=entities.map(|rows|rows.iter().map(|row|json!({
        "claim_id":row.get("label_id").or_else(||row.get("source_label_id")).or_else(||row.get("claim_id")),
        "entity_id":row["entity_id"],"entity_name":row["entity_name"],"entity_type":row["entity_type"],
        "source_id":row["source_id"],"review_status":row["review_status"],"evidence_refs":row["evidence_refs"],
        "confidence":row["confidence"],"ownership_claimed":false
    })).collect::<Vec<_>>()).unwrap_or_default();
    json!({"status":if entities.is_some(){"available"}else{"unavailable"},"items":items,
        "automatic_approval":false,"ownership_claimed":false})
}
fn paths(data: &Value, mode: Mode) -> Result<Value> {
    if mode == Mode::Wallet {
        return Ok(json!({"status":"not_requested","items":[],"search_complete":false}));
    }
    let graph = &data["graph"];
    let nodes = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["subject_id"].as_str().unwrap())
        .collect::<HashSet<_>>();
    let edges = graph["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_str().unwrap())
        .collect::<HashSet<_>>();
    let mut items = vec![];
    for path in data["paths"].as_array().context("missing paths")? {
        let ids = path
            .get("node_ids")
            .or_else(|| path.get("addresses"))
            .and_then(Value::as_array)
            .context("missing path nodes")?;
        let edge_ids = path["edge_ids"].as_array().context("missing path edges")?;
        ensure!(
            ids.iter()
                .all(|id| id.as_str().is_some_and(|id| nodes.contains(id)))
                && edge_ids
                    .iter()
                    .all(|id| id.as_str().is_some_and(|id| edges.contains(id))),
            "path references missing graph evidence"
        );
        items.push(
            json!({"node_ids":ids,"edge_ids":edge_ids,"hop_count":path["hop_count"],
            "fund_continuity_claimed":false,"ownership_claimed":false}),
        );
    }
    Ok(
        json!({"status":"available","items":items,"search_complete":false,
        "direction":data["direction"],"max_hops":data.get("max_hops").or_else(||data.get("max_depth")),
        "meaning":"bounded_observed_connectivity"}),
    )
}
pub fn normalize(network: Network, mode: Mode, mut data: Value) -> Result<Value> {
    ensure!(
        data.is_object() && data["network_id"] == network.id(),
        "response network mismatch"
    );
    if mode == Mode::Paths {
        let from = data
            .get("source")
            .or_else(|| data.get("source_address"))
            .and_then(Value::as_str)
            .context("path source missing")?
            .to_owned();
        let to = data
            .get("target")
            .or_else(|| data.get("target_address"))
            .and_then(Value::as_str)
            .context("path target missing")?
            .to_owned();
        data["source"] = json!(from);
        data["target"] = json!(to);
        data["address"] = json!(from);
    }
    Subject::address(network, text(&data, "address")?)?;
    source(network, &data)?;
    if !data["graph"].is_object() {
        data["graph"] =
            json!({"nodes":data["nodes"],"edges":data["edges"],"truncated":data["truncated"]});
    }
    graph(network, &mut data["graph"])?;
    let activity = activity(&data, mode)?;
    let holdings = holdings(network, &data, mode)?;
    let paths = paths(&data, mode)?;
    let attribution = attribution(&data, mode);
    data["api_version"] = json!("1.0");
    data["response_contract"] = json!(VERSION);
    data["response_capabilities"] = json!({"transaction_model":if network==Network::Bitcoin{"utxo"}else{"account"},
        "receipt_logs":{"applicability":if network==Network::Bitcoin{"not_applicable"}else{"applicable"}},
        "execution_traces":{"applicability":if matches!(network,Network::Ethereum|Network::Bsc){"applicable"}else{"not_applicable"}},
        "tron_internal_records":{"applicability":if network==Network::Tron{"applicable"}else{"not_applicable"}},
        "prevouts":{"applicability":if network==Network::Bitcoin{"applicable"}else{"not_applicable"}},
        "holdings_model":holdings["model"],"transfer_pages":true,"max_path_hops":10,
        "coverage":data["data_coverage"]});
    data["evidence"] = json!({"contract":VERSION,"mode":if mode==Mode::Wallet{"wallet"}else{"paths"},
        "source_snapshot_id":data["source_snapshot"]["snapshot_id"],
        "graph":{"status":"available","field":"graph","model":data["graph"]["model"]},
        "activity":activity,"holdings":holdings,"paths":paths,"attribution":attribution});
    data["analysis_evidence"] = crate::analysis::project(network, mode, &data);
    data["semantic_evidence"] = crate::semantic_evidence::project(network, mode, &data);
    Ok(data)
}
pub fn validate(network: Network, mode: Mode, data: &Value) -> Result<()> {
    ensure!(
        data["response_contract"] == VERSION,
        "unsupported response contract"
    );
    let checked = normalize(network, mode, data.clone())?;
    ensure!(
        data["graph"] == checked["graph"]
            && data["evidence"] == checked["evidence"]
            && data["response_capabilities"] == checked["response_capabilities"]
            && data
                .get("analysis_evidence")
                .is_none_or(|value| value == &checked["analysis_evidence"])
            && data
                .get("semantic_evidence")
                .is_none_or(|value| value == &checked["semantic_evidence"]),
        "response presentation contradicts source evidence"
    );
    crate::marketplace::validate(network, mode, data)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wallet(network: Network) -> Value {
        let address = match network {
            Network::Bitcoin => "1BoatSLRHtKNngkdXEeobR76b53LETtpyT".into(),
            Network::Tron => {
                aml_warehouse::values::tron_wallet(&format!("41{}", "1".repeat(40))).unwrap()
            }
            _ => format!("0x{}", "1".repeat(40)),
        };
        let target = if network == Network::Bitcoin {
            format!("tx:{}:{}", "a".repeat(64), "b".repeat(64))
        } else {
            address.clone()
        };
        let mut data = json!({"network_id":network.id(),"address":address,
            "source_snapshot":{"contract":"canonical_v1","network_id":network.id(),"snapshot_id":"a".repeat(64),"source_revision":"1","as_of_block":7,"history_complete":false},
            "graph":{"nodes":[{"id":address,"address":address,"node_type":"address"}],"edges":[],"truncated":false},
            "activity":[],"entities":[],"holdings":{"status":"unavailable","source":"finalized_rpc","assets":[]}});
        if network == Network::Bitcoin {
            data["graph"]["nodes"].as_array_mut().unwrap().push(json!({"id":target,"subject_id":target,"node_type":"transaction","subject_type":"transaction","address":null}));
            data["holdings"] = json!({"status":"partial","balance_sats":"1","is_complete_balance":false,"holdings_snapshot":{"evidence_hash":"unchanged"}});
        }
        if network == Network::Tron {
            data["holdings"] = json!({"source":"canonical_indexed_netflow_not_rpc_balance","assets":[{"asset_id":"native:TRX","balance_raw":"-3","decimals":6}]});
        }
        data["graph"]["edges"] = json!([{"id":"one","from_address":address,"to_address":target,
            "from_subject_id":address,"to_subject_id":target,"asset_id":network.native_asset(),"amount":"1",
            "transfer_type":if network==Network::Bitcoin{"utxo_input"}else{"native_transfer"},
            "block_number":7,"transaction_index":0,"event_index":0,"event_sub_index":0}]);
        data
    }
    #[test]
    fn four_profiles_keep_native_evidence_and_explicit_balance_models() -> Result<()> {
        for network in Network::ALL {
            let raw = wallet(network);
            let value = normalize(network, Mode::Wallet, raw.clone())?;
            validate(network, Mode::Wallet, &value)?;
            let mut changed = value.clone();
            changed["semantic_evidence"]["coverage"]["exhaustive"] = json!(true);
            assert!(validate(network, Mode::Wallet, &changed).is_err());
            assert_eq!(value["holdings"], raw["holdings"]);
            assert_eq!(
                value["evidence"]["source_snapshot_id"],
                value["source_snapshot"]["snapshot_id"]
            );
            assert_eq!(normalize(network, Mode::Wallet, value.clone())?, value);
            assert_eq!(value["graph"]["edges"][0]["amount_raw"], "1");
            assert_eq!(value["evidence"]["paths"]["status"], "not_requested");
            if network == Network::Bitcoin {
                assert_eq!(
                    value["response_capabilities"]["receipt_logs"]["applicability"],
                    "not_applicable"
                );
                assert_eq!(value["evidence"]["holdings"]["model"], "indexed_utxo");
                assert!(value["graph"]["nodes"][1]["address"].is_null());
            } else if network == Network::Tron {
                assert_eq!(value["evidence"]["holdings"]["model"], "indexed_netflow");
                assert_eq!(
                    value["evidence"]["holdings"]["assets"][0]["amount_raw"],
                    "-3"
                );
                let mut unknown = raw.clone();
                unknown["holdings"]["assets"][0]["decimals_known"] = json!(false);
                let unknown = normalize(network, Mode::Wallet, unknown)?;
                assert!(unknown["evidence"]["holdings"]["assets"][0]["decimals"].is_null());
            } else {
                assert_eq!(value["evidence"]["holdings"]["status"], "unavailable");
                assert!(
                    value["evidence"]["holdings"]["assets"]
                        .as_array()
                        .unwrap()
                        .is_empty()
                );
            }
        }
        Ok(())
    }
    #[test]
    fn exact_nft_identity_and_amount_cannot_be_replaced_by_floats() -> Result<()> {
        let mut raw = wallet(Network::Ethereum);
        raw["graph"]["edges"][0]["asset_id"] =
            json!(format!("eip155:1/erc1155:0x{}", "2".repeat(40)));
        raw["graph"]["edges"][0]["token_id"] = json!(RawAmount::MAX);
        raw["graph"]["edges"][0]["amount"] = json!(RawAmount::MAX);
        let mut value = normalize(Network::Ethereum, Mode::Wallet, raw.clone())?;
        assert_eq!(
            value["graph"]["edges"][0]["canonical_token_id"],
            RawAmount::MAX
        );
        assert_eq!(value["graph"]["edges"][0]["amount_raw"], RawAmount::MAX);
        let mut inconsistent = value.clone();
        inconsistent["graph"]["edges"][0]["amount_raw"] = json!("1");
        assert!(validate(Network::Ethereum, Mode::Wallet, &inconsistent).is_err());
        let mut inconsistent = value.clone();
        inconsistent["graph"]["nodes"][0]["subject_id"] = json!(format!("0x{}", "3".repeat(40)));
        assert!(validate(Network::Ethereum, Mode::Wallet, &inconsistent).is_err());
        value["evidence"]["holdings"]["status"] = json!("available");
        assert!(validate(Network::Ethereum, Mode::Wallet, &value).is_err());
        raw["graph"]["edges"][0]["amount"] = json!(1.5);
        assert!(normalize(Network::Ethereum, Mode::Wallet, raw).is_err());
        Ok(())
    }
    #[test]
    fn attribution_keeps_review_state_and_references_without_approving_claims() -> Result<()> {
        for network in Network::ALL {
            let mut raw = wallet(network);
            raw["entities"] = json!([{"claim_id":"case:pending","entity_id":"entity:1","entity_name":"Fixture",
                "entity_type":"unknown","source_id":"review-source","review_status":"PENDING",
                "evidence_refs":["tx:fixture"],"confidence":0.5}]);
            let value = normalize(network, Mode::Wallet, raw)?;
            let claims = &value["evidence"]["attribution"];
            assert_eq!(claims["items"][0]["review_status"], "PENDING");
            assert_eq!(claims["items"][0]["claim_id"], "case:pending");
            assert_eq!(claims["items"][0]["evidence_refs"], json!(["tx:fixture"]));
            assert_eq!(claims["automatic_approval"], false);
            assert_eq!(claims["items"][0]["ownership_claimed"], false);
        }
        Ok(())
    }
    #[test]
    fn analysis_distinguishes_leads_memberships_and_directional_exposure() -> Result<()> {
        for network in Network::ALL {
            let mut raw = wallet(network);
            let seed = raw["address"].clone();
            let hash = if matches!(network, Network::Ethereum | Network::Bsc) {
                format!("0x{}", "a".repeat(64))
            } else {
                "a".repeat(64)
            };
            let (expected_cluster, expected_exposure) = match network {
                Network::Bitcoin => {
                    raw["clusters"] = json!([{"cluster_id":"lead","review_status":"approved"}]);
                    ("investigative_lead", "utxo_bounds_path")
                }
                Network::Ethereum => {
                    raw["clusters"] =
                        json!([{"cluster_id":"association","membership_type":"REVIEWED_ENTITY"}]);
                    ("reviewed_association", "observed_account_path")
                }
                Network::Bsc => {
                    raw["clusters"] =
                        json!([{"cluster_id":"association","review_status":"approved"}]);
                    raw["cluster_candidates"] =
                        json!([{"destination":seed,"review_status":"pending"}]);
                    ("reviewed_association", "observed_account_path")
                }
                Network::Tron => {
                    raw["intelligence"] = json!({
                        "active_clusters":[{"cluster_id":"association","source_claim_id":"active-claim"}],
                        "cluster_claims":[{"cluster_id":"candidate","review_status":"PENDING"}]
                    });
                    ("reviewed_association", "propagated_source_summary")
                }
            };
            if network == Network::Tron {
                raw["exposure"] = json!({"top_sources":[
                    {"source_address":seed,"hop_distance":1,"exposure_type":"DIRECTED_FUND_FLOW","last_tx_hash":hash,"service_mediated":true},
                    {"source_address":seed,"hop_distance":2,"exposure_type":"DIRECTED_FUND_FLOW","last_tx_hash":hash}
                ]});
            } else {
                raw["exposure_paths"] = json!([
                    {"seed_address":seed,"hop_count":1,"direction":"SENT_TO_SEED","tx_hashes":[hash],"service_mediated":true},
                    {"seed_address":seed,"hop_count":2,"direction":"RECEIVED_FROM_SEED","tx_hashes":[hash]}
                ]);
            }
            let value = normalize(network, Mode::Wallet, raw.clone())?;
            let analysis = &value["analysis_evidence"];
            let cluster = &analysis["clustering"]["items"][0];
            let exposure = &analysis["exposure"];
            assert_eq!(cluster["kind"], expected_cluster);
            assert_eq!(cluster["score_eligible"], false);
            assert_eq!(cluster["ownership_claimed"], false);
            assert_eq!(exposure["direct_count"], 1);
            assert_eq!(exposure["indirect_count"], 1);
            assert_eq!(exposure["items"][0]["evidence_model"], expected_exposure);
            assert_eq!(exposure["items"][0]["service_mediated"], true);
            assert_eq!(exposure["items"][1]["direction"], "received_from_seed");
            assert_eq!(exposure["items"][1]["fund_continuity_claimed"], false);
            assert!(
                raw.pointer(exposure["items"][1]["native_ref"].as_str().unwrap())
                    .is_some()
            );
            if matches!(network, Network::Bsc | Network::Tron) {
                assert_eq!(
                    analysis["clustering"]["items"][1]["kind"],
                    "investigative_lead"
                );
                assert_eq!(
                    analysis["clustering"]["items"][1]["review_status"],
                    "pending"
                );
            }
            let mut tampered = value.clone();
            tampered["analysis_evidence"]["exposure"]["items"][0]["ownership_claimed"] =
                json!(true);
            assert!(validate(network, Mode::Wallet, &tampered).is_err());
            tampered
                .as_object_mut()
                .unwrap()
                .remove("analysis_evidence");
            validate(network, Mode::Wallet, &tampered)?;
            assert_eq!(normalize(network, Mode::Wallet, tampered)?, value);
        }
        Ok(())
    }
    #[test]
    fn analysis_flags_missing_transaction_witness_and_unreviewed_membership() -> Result<()> {
        let network = Network::Ethereum;
        let mut raw = wallet(network);
        raw["clusters"] = json!([{"cluster_id":"unreviewed","membership_type":"heuristic"}]);
        raw["exposure_paths"] = json!([{"seed_address":raw["address"],"hop_count":2,
            "direction":"RECEIVED_FROM_SEED","tx_hashes":["invalid"]}]);
        let value = normalize(network, Mode::Wallet, raw)?;
        let analysis = &value["analysis_evidence"];
        assert_eq!(analysis["clustering"]["status"], "partial");
        assert_eq!(analysis["clustering"]["items"], json!([]));
        assert_eq!(analysis["exposure"]["status"], "partial");
        assert_eq!(analysis["exposure"]["direct_count"], 0);
        assert_eq!(analysis["exposure"]["indirect_count"], 0);
        Ok(())
    }
    #[test]
    fn missing_graph_paths_and_wrong_source_are_rejected() -> Result<()> {
        let mut raw = wallet(Network::Bitcoin);
        raw["graph"]["edges"][0]["to_subject_id"] = raw["address"].clone();
        assert!(normalize(Network::Bitcoin, Mode::Wallet, raw).is_err());
        let mut raw = wallet(Network::Bsc);
        raw["source"] = raw["address"].clone();
        raw["target"] = raw["address"].clone();
        raw["paths"] = json!([{"node_ids":[raw["address"]],"edge_ids":["missing"],"hop_count":1}]);
        assert!(normalize(Network::Bsc, Mode::Paths, raw).is_err());
        let mut raw = wallet(Network::Ethereum);
        raw["source_snapshot"]["network_id"] = json!("eip155:56");
        assert!(normalize(Network::Ethereum, Mode::Wallet, raw).is_err());
        Ok(())
    }
}
