# Shared evidence risk policy

This is the existing central `aml_evidence_v2_verified_inputs` policy, extracted
without changing its scoring rules. Both the Analytical Node and Bitcoin adapter
depend on the same Rust source. This crate has no database, HTTP server or worker.
Other network adapters continue to supply evidence to the central consumer.

`assess` evaluates evidence, `definition` exports the compiled weights/rules and
thresholds, and `bitcoin_inputs` defines the exact evidence bound to a persisted
Bitcoin assessment. `bitcoin_binding_matches` lets the central consumer check
that the persisted assessment matches its own policy and returned evidence.

The definition hash includes a normalized source digest of this implementation.
Changes to this source, including non-scoring edits, require a new policy version
before writing into a database that already contains that version. Stored policy
versions are immutable; do not edit existing rows to bless a changed implementation.
Historical assessments remain readable and are checked against the running policy.

The 15 existing policy regression tests moved with the implementation. Full-width
amount arithmetic and TRON/EVM behavior remain covered. Bitcoin persistence,
freshness and real ClickHouse fixtures are tested in the Bitcoin adapter.

See [Bitcoin persistence](../../dockerizd_bitcoin/RISK.md). Docker builds must supply
the `risk_policy_source` named context, as configured in the main and Bitcoin
Compose files; this does not introduce another runtime service.
