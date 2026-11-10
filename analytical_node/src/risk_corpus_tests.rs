use super::{assess_fixture, labelled_wallet};
use aml_chain_api::{Network, kyt::wallet_address};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    str::FromStr,
};

const CONTRACT: &str = "aml_risk_evaluation_v1";
const MAX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum CorpusKind {
    Regression,
    Independent,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Corpus {
    contract: String,
    id: String,
    kind: CorpusKind,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    evidence_ref: String,
    label_ref: String,
    #[serde(default)]
    expected_review: Option<bool>,
    #[serde(default)]
    expected_signals: BTreeMap<Signal, bool>,
    source_status: SourceStatus,
    evidence: Value,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SourceStatus {
    Current,
    Stale,
    Unverified,
}

impl SourceStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Stale => "stale",
            Self::Unverified => "unverified",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Ord, PartialOrd, Eq, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Signal {
    ApprovedRiskLabel,
    IllicitSeedExposure,
    MixerInteraction,
    RapidSameAssetForwarding,
}

impl Signal {
    fn code(self) -> &'static str {
        match self {
            Self::ApprovedRiskLabel => "APPROVED_RISK_LABEL",
            Self::IllicitSeedExposure => "ILLICIT_SEED_EXPOSURE",
            Self::MixerInteraction => "MIXER_INTERACTION",
            Self::RapidSameAssetForwarding => "RAPID_SAME_ASSET_FORWARDING",
        }
    }
}

#[derive(Default)]
pub(super) struct Counts {
    tp: usize,
    tn: usize,
    fp: usize,
    fn_: usize,
    unknown_positive: usize,
    unknown_negative: usize,
}

impl Counts {
    pub(super) fn record(&mut self, expected: bool, actual: Option<bool>) {
        *match (expected, actual) {
            (true, Some(true)) => &mut self.tp,
            (false, Some(false)) => &mut self.tn,
            (false, Some(true)) => &mut self.fp,
            (true, Some(false)) => &mut self.fn_,
            (true, None) => &mut self.unknown_positive,
            (false, None) => &mut self.unknown_negative,
        } += 1;
    }

    pub(super) fn summary(&self) -> Value {
        let assessed = self.tp + self.tn + self.fp + self.fn_;
        let labels = assessed + self.unknown_positive + self.unknown_negative;
        let ratio = |a, b| (b > 0).then(|| a as f64 / b as f64);
        json!({
            "labels":labels, "assessed":assessed, "coverage":ratio(assessed, labels),
            "true_positive":self.tp,"true_negative":self.tn,"false_positive":self.fp,"false_negative":self.fn_,
            "unknown_positive":self.unknown_positive,"unknown_negative":self.unknown_negative,
            "precision_on_assessed":ratio(self.tp, self.tp + self.fp),
            "recall_on_assessed":ratio(self.tp, self.tp + self.fn_),
        })
    }
}

#[derive(Default)]
struct Cohort {
    cases: usize,
    source_status: BTreeMap<&'static str, usize>,
    review: Counts,
    signals: BTreeMap<Signal, Counts>,
}

impl Cohort {
    fn add(&mut self, case: &Case, assessment: &Value) {
        self.cases += 1;
        *self
            .source_status
            .entry(case.source_status.as_str())
            .or_default() += 1;
        let score = assessment["risk_score"].as_f64();
        if let Some(expected) = case.expected_review {
            self.review.record(
                expected,
                score.and_then(|_| assessment["requires_analyst_review"].as_bool()),
            );
        }
        for (&signal, &expected) in &case.expected_signals {
            let detected = score.map(|_| {
                assessment["signals"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|row| row["signal_type"] == signal.code())
            });
            self.signals
                .entry(signal)
                .or_default()
                .record(expected, detected);
        }
    }

    fn summary(self) -> Value {
        let signals = self
            .signals
            .into_iter()
            .map(|(signal, counts)| (signal.code().into(), counts.summary()))
            .collect::<serde_json::Map<String, Value>>();
        json!({"cases":self.cases,"source_status":self.source_status,"review":self.review.summary(),"signals":signals})
    }
}

fn evaluate(bytes: &[u8]) -> Result<Value> {
    ensure!(bytes.len() <= MAX_BYTES, "evaluation corpus exceeds 64 MiB");
    let corpus: Corpus = serde_json::from_slice(bytes)?;
    ensure!(
        corpus.contract == CONTRACT && !corpus.id.trim().is_empty(),
        "invalid corpus identity"
    );
    ensure!(!corpus.cases.is_empty(), "evaluation corpus has no cases");
    let mut ids = BTreeSet::new();
    let mut observations = BTreeSet::new();
    let mut cohorts = BTreeMap::<String, Cohort>::new();
    for case in &corpus.cases {
        ensure!(
            !case.id.trim().is_empty() && ids.insert(case.id.as_str()),
            "missing or repeated case ID"
        );
        ensure!(
            !case.evidence_ref.trim().is_empty()
                && !case.label_ref.trim().is_empty()
                && case.evidence_ref.trim() != case.label_ref.trim(),
            "case {} needs separate evidence and label references",
            case.id
        );
        ensure!(
            case.expected_review.is_some() || !case.expected_signals.is_empty(),
            "case {} has no target label",
            case.id
        );
        let network = Network::parse(case.evidence["network_id"].as_str().unwrap_or_default())
            .with_context(|| format!("invalid network in case {}", case.id))?;
        let snapshot = &case.evidence["source_snapshot"];
        if corpus.kind == CorpusKind::Independent {
            let address = case.evidence["address"]
                .as_str()
                .context("independent case needs a wallet address")?;
            let address = if network == Network::Bitcoin {
                bitcoin::Address::from_str(address)?
                    .require_network(bitcoin::Network::Bitcoin)?
                    .to_string()
            } else {
                wallet_address(network, address).context("invalid independent wallet address")?
            };
            let snapshot_id = snapshot["snapshot_id"]
                .as_str()
                .filter(|id| id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()))
                .context("independent case needs a source snapshot ID")?;
            ensure!(
                snapshot["network_id"] == network.id() && snapshot["contract"] == "canonical_v1",
                "independent case has an invalid snapshot identity"
            );
            ensure!(
                observations.insert((network.id(), address, snapshot_id.to_ascii_lowercase())),
                "repeated wallet observation in independent corpus"
            );
        }
        let history = match snapshot["history_complete"].as_bool() {
            Some(true) => "complete",
            Some(false) => "partial",
            None => "unknown",
        };
        let mut evidence = case.evidence.clone();
        // Benchmark the recorded inputs with the running policy. Saved scores
        // and retained historical scores must not decide the measured result.
        if let Some(fields) = evidence.as_object_mut() {
            fields.remove("historical_risk_engine");
        }
        let assessment = assess_fixture(network.name(), evidence, case.source_status.as_str());
        for group in [
            "all".into(),
            network.name().into(),
            format!("{}/{history}", network.name()),
        ] {
            cohorts
                .entry(group)
                .or_default()
                .add(case, &assessment["risk_engine"]);
        }
    }
    let groups = cohorts
        .into_iter()
        .map(|(key, group)| (key, group.summary()))
        .collect::<serde_json::Map<String, Value>>();
    let missing_networks: Vec<_> = Network::ALL
        .into_iter()
        .filter(|network| !groups.contains_key(network.name()))
        .map(Network::name)
        .collect();
    use sha2::{Digest, Sha256};
    Ok(json!({
        "contract":CONTRACT,"corpus_id":corpus.id,"corpus_kind":corpus.kind,
        "corpus_sha256":format!("{:x}",Sha256::digest(bytes)),
        "policy":aml_risk_policy::POLICY,"policy_sha256":aml_risk_policy::digest(&aml_risk_policy::definition()),
        "review_threshold":aml_risk_policy::MEDIUM,"missing_networks":missing_networks,"groups":groups
    }))
}

#[test]
#[ignore = "requires AML_RISK_EVALUATION_INPUT containing archived wallet evidence and separate labels"]
fn evaluate_archived_risk_corpus() -> Result<()> {
    let path =
        std::env::var_os("AML_RISK_EVALUATION_INPUT").context("set AML_RISK_EVALUATION_INPUT")?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    println!("{}", serde_json::to_string(&evaluate(&bytes)?)?);
    Ok(())
}

#[test]
fn corpus_reports_outcomes_by_network_and_history_without_saved_scores() -> Result<()> {
    let mut cases = Vec::new();
    for network in Network::ALL {
        let mut evidence = labelled_wallet(network.name(), "approved");
        evidence["network_id"] = json!(network.id());
        evidence["source_snapshot"] = json!({"history_complete":false});
        evidence["risk_engine"] = json!({"risk_score":0});
        evidence["historical_risk_engine"] = json!({"risk_score":0});
        for (i, source, expected) in [
            (0, "current", true),
            (1, "current", false),
            (2, "stale", true),
        ] {
            cases.push(
                json!({"id":format!("{}/{i}",network.name()),"evidence_ref":"fixture:wallet",
                "label_ref":"fixture:review","source_status":source,"expected_review":expected,
                "expected_signals":{"APPROVED_RISK_LABEL":expected},"evidence":evidence}),
            );
        }
    }
    let mut corpus =
        json!({"contract":CONTRACT,"id":"matrix-test","kind":"regression","cases":cases});
    let report = evaluate(&serde_json::to_vec(&corpus)?)?;
    assert_eq!(report["missing_networks"], json!([]));
    for network in Network::ALL {
        let group = &report["groups"][network.name()];
        assert_eq!(
            group,
            &report["groups"][format!("{}/partial", network.name())]
        );
        for target in [&group["review"], &group["signals"]["APPROVED_RISK_LABEL"]] {
            assert_eq!(target["true_positive"], 1);
            assert_eq!(target["false_positive"], 1);
            assert_eq!(target["unknown_positive"], 1);
            assert_eq!(target["coverage"], 2.0 / 3.0);
            assert_eq!(target["precision_on_assessed"], 0.5);
        }
    }
    corpus["cases"][1]["id"] = corpus["cases"][0]["id"].clone();
    assert!(evaluate(&serde_json::to_vec(&corpus)?).is_err());
    Ok(())
}

#[test]
fn independent_corpus_rejects_duplicate_observations_and_keeps_targets_separate() -> Result<()> {
    let source = Network::Ethereum.id();
    let address = "0x1111111111111111111111111111111111111111";
    let mut edges = Vec::new();
    for index in 0..3 {
        let time = 1_000_000 + index * 1_000_000;
        edges.push(json!({"tx_hash":format!("in-{index}"), "to_address":address,
            "from_address":"sender","asset_id":"native:ETH","amount":"100","block_timestamp_unix_ms":time}));
        edges.push(json!({"tx_hash":format!("out-{index}"),"from_address":address,
            "to_address":"recipient","asset_id":"native:ETH","amount":"98","block_timestamp_unix_ms":time+1_000}));
    }
    let case = json!({"id":"forwarding-only","evidence_ref":"fixture:transfer-events",
        "label_ref":"fixture:analyst-review","source_status":"current","expected_review":false,
        "expected_signals":{"RAPID_SAME_ASSET_FORWARDING":true,"MIXER_INTERACTION":false,
            "ILLICIT_SEED_EXPOSURE":false},
        "evidence":{"network_id":source,"address":address,"fingerprint":{"transfer_count":6},
            "source_snapshot":{"network_id":source,"contract":"canonical_v1","snapshot_id":"b".repeat(64)},
            "graph":{"edges":edges}}});
    let mut corpus = json!({"contract":CONTRACT,"id":"target-separation-test","kind":"independent",
        "cases":[case]});
    let report = evaluate(&serde_json::to_vec(&corpus)?)?;
    let group = &report["groups"]["ethereum/unknown"];
    assert_eq!(group["review"]["true_negative"], 1);
    assert_eq!(
        group["signals"]["RAPID_SAME_ASSET_FORWARDING"]["true_positive"],
        1
    );
    assert_eq!(group["signals"]["MIXER_INTERACTION"]["true_negative"], 1);
    assert_eq!(
        group["signals"]["ILLICIT_SEED_EXPOSURE"]["true_negative"],
        1
    );
    assert_eq!(
        report["missing_networks"],
        json!(["tron", "bsc", "bitcoin"])
    );

    let mut repeated = corpus["cases"][0].clone();
    repeated["id"] = json!("different-case-id");
    corpus["cases"].as_array_mut().unwrap().push(repeated);
    assert!(
        evaluate(&serde_json::to_vec(&corpus)?)
            .unwrap_err()
            .to_string()
            .contains("repeated wallet observation")
    );
    Ok(())
}
