use super::{Kind, Rule, Target, watch_id};
use crate::{App, load_evidence, refresh_source_validity, risk};
use aml_chain_api::{Network, kyt};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeSet, time::Duration};

pub(super) async fn check(app: &App, target: &Target) -> Result<Value> {
    let network = Network::parse(&target.network_id)?;
    match target.kind {
        Kind::Address => check_address(app, network, target).await,
        Kind::Transaction => check_transaction(app, network, target).await,
    }
}

async fn read_receipt(app: &App, network: Network, path: &str) -> Result<Value> {
    let upstream = app
        .upstreams
        .get(network.name())
        .context("missing chain upstream")?;
    let mut response = app
        .http
        .get(format!("{upstream}{path}"))
        .header("x-aml-service-key", &app.key)
        .timeout(Duration::from_secs(5))
        .send()
        .await?
        .error_for_status()?;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            body.len() + chunk.len() <= 65_536,
            "KYT receipt exceeded response limit"
        );
        body.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&body)?)
}

async fn wallet(app: &App, network: Network, address: &str) -> Result<Value> {
    let uri = format!(
        "/api/{}/wallet/{address}/investigation?limit=20",
        network.name()
    )
    .parse()?;
    let mut evidence = load_evidence(app, network.name(), address, None, &uri)
        .await
        .map_err(|_| anyhow::anyhow!("wallet evidence unavailable"))?;
    refresh_source_validity(app, &mut evidence, network.name()).await;
    ensure!(
        evidence["source_validity"]["status"] == "current",
        "wallet evidence changed or is unverifiable"
    );
    Ok(evidence)
}

fn assessment(evidence: &Value, target: &Target) -> Option<Value> {
    if evidence["source_validity"]["current_risk_usable"] != true {
        return None;
    }
    let score = evidence["risk_engine"]["risk_score"].as_f64()?;
    let signals = evidence["risk_engine"]["signals"].as_array()?;
    let rule_score = match target.rule {
        Rule::EvidenceScore => score,
        Rule::ApprovedLabel => signals
            .iter()
            .find(|signal| signal["signal_type"] == "APPROVED_RISK_LABEL")
            .and_then(|signal| signal["contribution"].as_f64())
            .unwrap_or(0.0),
    };
    Some(
        json!({"address":evidence["address"],"score":score,"rule_score":rule_score,
        "matched":rule_score>=f64::from(target.threshold),"policy_version":evidence["risk_engine"]["policy_version"],
        "policy_hash":evidence["risk_policy_hash"],
        "signals":signals.iter().map(|signal| json!({"type":signal["signal_type"],
            "contribution":signal["contribution"]})).collect::<Vec<_>>()}),
    )
}

async fn check_address(app: &App, network: Network, target: &Target) -> Result<Value> {
    let evidence = wallet(app, network, &target.value).await?;
    let result = assessment(&evidence, target);
    Ok(json!({"status":outcome(result.iter(), result.is_some()),
        "source_snapshot_id":evidence["source_snapshot"]["snapshot_id"],
        "reason":if result.is_none(){Some("insufficient_wallet_evidence")}else{None},
        "assessment":result}))
}

fn outcome<'a>(mut assessments: impl Iterator<Item = &'a Value>, complete: bool) -> &'static str {
    if assessments.any(|assessment| assessment["matched"] == true) {
        "matched"
    } else if complete {
        "no_match"
    } else {
        "unknown"
    }
}

pub(crate) async fn read_transaction(
    app: &App,
    network: Network,
    hash: &str,
) -> Result<kyt::TransactionObservation> {
    ensure!(
        kyt::transaction_hash(network, hash)? == hash,
        "noncanonical transaction hash"
    );
    let response =
        read_receipt(app, network, &format!("/internal/kyt/transactions/{hash}")).await?;
    let observation: kyt::TransactionObservation = serde_json::from_value(response)?;
    let source = &observation.source_snapshot;
    ensure!(
        observation.network_id == network.id()
            && observation.tx_hash == hash
            && source["network_id"] == network.id()
            && source["contract"] == "canonical_v1"
            && watch_id(risk::text(&source["snapshot_id"])),
        "transaction receipt identity mismatch"
    );
    if let Some(transaction) = &observation.occurrence {
        ensure!(
            !observation.ambiguous
                && source["initialized"] == true
                && source["as_of_block"]
                    .as_u64()
                    .is_some_and(|tip| transaction.block_number <= tip)
                && kyt::transaction_hash(network, &transaction.block_hash)?
                    == transaction.block_hash
                && transaction.participants.len() <= kyt::PARTICIPANT_LIMIT
                && transaction
                    .participants
                    .iter()
                    .collect::<BTreeSet<_>>()
                    .len()
                    == transaction.participants.len(),
            "invalid transaction occurrence"
        );
        for address in &transaction.participants {
            ensure!(
                kyt::wallet_address(network, address)? == *address,
                "invalid participant address"
            );
        }
    }
    Ok(observation)
}

async fn check_transaction(app: &App, network: Network, target: &Target) -> Result<Value> {
    let observation = read_transaction(app, network, &target.value).await?;
    let source = &observation.source_snapshot;
    let source_id = risk::text(&source["snapshot_id"]);
    if observation.ambiguous {
        return Ok(
            json!({"status":"unknown","reason":"ambiguous_transaction_occurrence",
            "source_snapshot_id":source_id}),
        );
    }
    let Some(transaction) = observation.occurrence else {
        let initialized = source["initialized"] == true;
        return Ok(
            json!({"status":if initialized{"not_indexed"}else{"unknown"},
            "reason":if initialized{"absent_from_indexed_canonical_history"}else{"index_uninitialized"},
            "source_snapshot_id":source_id}),
        );
    };
    let mut candidates = Vec::new();
    for address in &transaction.participants {
        if let Ok(Ok(evidence)) =
            tokio::time::timeout(Duration::from_secs(8), wallet(app, network, address)).await
            && evidence["source_snapshot"]["snapshot_id"] == source_id
            && let Some(assessed) = assessment(&evidence, target)
        {
            candidates.push((
                evidence["operational_snapshot"]["snapshot_id"].clone(),
                assessed,
            ));
        }
    }
    let status = read_receipt(app, network, "/status").await?;
    ensure!(
        status["network_id"] == network.id()
            && status["source_snapshot"]["snapshot_id"] == source_id,
        "transaction source changed during screening"
    );
    let assessments: Vec<_> = candidates
        .into_iter()
        .filter(|(operational, _)| {
            network == Network::Bitcoin
                || operational == &status["operational_snapshot"]["snapshot_id"]
        })
        .map(|(_, assessment)| assessment)
        .collect();
    let complete = !transaction.participants_truncated
        && !transaction.participants.is_empty()
        && assessments.len() == transaction.participants.len();
    let status = outcome(assessments.iter(), complete);
    Ok(
        json!({"status":status,"reason":if status=="unknown"{Some("incomplete_participant_evidence")}else{None},
        "source_snapshot_id":source_id,"block_number":transaction.block_number,"block_hash":transaction.block_hash,
        "participants":transaction.participants,"participants_truncated":transaction.participants_truncated,
        "assessments":assessments}),
    )
}
