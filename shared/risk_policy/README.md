# Shared evidence risk policy

The central `aml_evidence_v3_receipt_proof` policy requires a validated mixer ABI
and registry provenance before scoring a mixer signal. Both the Analytical Node and Bitcoin adapter
depend on the same Rust source. This crate has no database, HTTP server or worker.
Other network adapters continue to supply evidence to the central consumer.

`assess` evaluates evidence, `definition` exports the compiled weights/rules and
thresholds, and `bitcoin_inputs` defines the exact evidence bound to a persisted
Bitcoin assessment. `bitcoin_binding_matches` lets the central consumer check
that the persisted assessment matches its own policy and returned evidence.

The definition hash includes normalized source digests of the policy and the shared ABI verifier.
Changes to this source, including non-scoring edits, require a new policy version
before writing into a database that already contains that version. Stored policy
versions are immutable; do not edit existing rows to bless a changed implementation.
Historical assessments remain readable and are checked against the running policy.

The 15 existing policy regression tests moved with the implementation. Full-width
amount arithmetic and TRON/EVM behavior remain covered. Bitcoin persistence,
freshness and real ClickHouse fixtures are tested in the Bitcoin adapter.

See [Bitcoin persistence](../../dockerizd_bitcoin/RISK.md). Docker builds must supply
the `risk_policy_source` and `warehouse_source` named contexts configured in
the main and Bitcoin Compose files.
