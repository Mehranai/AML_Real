use aml_warehouse::{Network, relationships::Subject, values::normalize_hash};
use serde_json::{Value, json};

use crate::response::Mode;

const MAX_ITEMS: usize = 100;

fn cluster_inputs(network: Network) -> &'static [(&'static str, &'static str)] {
    match network {
        Network::Bitcoin => &[("/clusters", "investigative_lead")],
        Network::Ethereum => &[("/clusters", "reviewed_association")],
        Network::Bsc => &[
            ("/clusters", "reviewed_association"),
            ("/cluster_candidates", "investigative_lead"),
        ],
        Network::Tron => &[
            ("/intelligence/active_clusters", "reviewed_association"),
            ("/intelligence/cluster_claims", "investigative_lead"),
        ],
    }
}

fn clustering(network: Network, data: &Value) -> Value {
    let mut items = Vec::new();
    let mut available = false;
    let mut partial = data["cluster_coverage"]["status"] == "partial"
        || data["cluster_candidate_coverage"]["status"] == "unavailable";
    if data["cluster_coverage"]["status"] == "waiting_for_blocks" {
        return json!({"status":"unavailable","items":[],"search_complete":false});
    }
    for (path, kind) in cluster_inputs(network) {
        let Some(rows) = data.pointer(path).and_then(Value::as_array) else {
            continue;
        };
        available = true;
        for (index, row) in rows.iter().enumerate() {
            if items.len() == MAX_ITEMS {
                partial = true;
                break;
            }
            let id = row["cluster_id"]
                .as_str()
                .or_else(|| row["destination"].as_str());
            if id.is_none_or(str::is_empty) {
                partial = true;
                continue;
            }
            let review = row["review_status"].as_str();
            let reviewed = *kind == "reviewed_association";
            let approved = review.is_some_and(|status| status.eq_ignore_ascii_case("approved"));
            let membership = review.is_none()
                && match network {
                    Network::Ethereum => row["membership_type"] == "REVIEWED_ENTITY",
                    Network::Tron => row["source_claim_id"]
                        .as_str()
                        .is_some_and(|id| !id.is_empty()),
                    _ => false,
                };
            if reviewed && !(approved || membership) {
                partial = true;
                continue;
            }
            items.push(json!({
                "kind":kind, "cluster_id":row["cluster_id"], "destination":row["destination"],
                "review_status":review.map(str::to_ascii_lowercase)
                    .or_else(|| membership.then(|| "approved".into())),
                "native_ref":format!("{path}/{index}"),
                "ownership_claimed":false, "score_eligible":false
            }));
        }
    }
    let status = if !available {
        "unavailable"
    } else if partial {
        "partial"
    } else {
        "available"
    };
    json!({"status":status,"items":items,"search_complete":false})
}

fn exposure(network: Network, data: &Value) -> Value {
    let tron = network == Network::Tron;
    let field = if tron {
        "/exposure/top_sources"
    } else {
        "/exposure_paths"
    };
    let paths = data
        .pointer(field)
        .and_then(Value::as_array)
        .filter(|_| data["exposure_coverage"]["status"] != "waiting_for_blocks");
    let Some(paths) = paths else {
        return json!({"status":"unavailable","items":[],"search_complete":false,
            "direct_count":0,"indirect_count":0});
    };
    let mut items = Vec::new();
    let mut direct = 0;
    let mut partial = data["exposure_coverage"]["truncated"] == true
        || data["exposure_coverage"]["status"] == "partial"
        || paths.len() > MAX_ITEMS;
    for (index, path) in paths.iter().take(MAX_ITEMS).enumerate() {
        let seed = path[if tron {
            "source_address"
        } else {
            "seed_address"
        }]
        .as_str();
        let hops = path[if tron { "hop_distance" } else { "hop_count" }].as_u64();
        let flow = path[if tron { "exposure_type" } else { "direction" }].as_str();
        let direction = match flow {
            Some("RECEIVED_FROM_SEED" | "INCOMING" | "incoming" | "received") => {
                Some("received_from_seed")
            }
            Some("SENT_TO_SEED" | "OUTGOING" | "outgoing" | "sent") => Some("sent_to_seed"),
            Some("DIRECTED_FUND_FLOW") if tron => Some("received_from_seed"),
            _ => None,
        };
        let has_tx = |v: &Value| {
            v.as_str()
                .is_some_and(|hash| normalize_hash(network, hash).is_ok())
        };
        let has_witness = if tron {
            has_tx(&path["last_tx_hash"])
        } else {
            path["tx_hashes"]
                .as_array()
                .is_some_and(|hashes| !hashes.is_empty() && hashes.iter().all(has_tx))
        };
        let (Some(seed), Some(hops @ 1..=10), Some(direction)) = (seed, hops, direction) else {
            partial = true;
            continue;
        };
        if !has_witness || Subject::address(network, seed).is_err() {
            partial = true;
            continue;
        }
        direct += usize::from(hops == 1);
        items.push(json!({
            "kind":if hops == 1 {"direct"} else {"indirect"},
            "direction":direction,"hop_count":hops,"seed_address":seed,
            "native_ref":format!("{field}/{index}"),"service_mediated":path["service_mediated"].as_bool(),
            "evidence_model":match network {
                Network::Bitcoin => "utxo_bounds_path",
                Network::Tron => "propagated_source_summary",
                _ => "observed_account_path",
            },
            "ownership_claimed":false,"fund_continuity_claimed":false
        }));
    }
    json!({"status":if partial {"partial"} else {"available"},
        "direct_count":direct,"indirect_count":items.len()-direct,"items":items,"search_complete":false})
}

pub(crate) fn project(network: Network, mode: Mode, data: &Value) -> Value {
    if mode == Mode::Paths {
        return json!({"contract":"chain_analysis_evidence_v1","source_snapshot_id":data["source_snapshot"]["snapshot_id"],
            "clustering":{"status":"not_requested","items":[],"search_complete":false},
            "exposure":{"status":"not_requested","items":[],"search_complete":false,"direct_count":0,"indirect_count":0}});
    }
    json!({"contract":"chain_analysis_evidence_v1","source_snapshot_id":data["source_snapshot"]["snapshot_id"],
        "clustering":clustering(network, data),"exposure":exposure(network, data)})
}
