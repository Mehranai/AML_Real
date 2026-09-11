CREATE TABLE IF NOT EXISTS {{database}}.intelligence_claims
(
    network_id LowCardinality(String),
    claim_id String,
    address String,
    claim_kind LowCardinality(String),
    entity_id String,
    entity_name String,
    entity_type LowCardinality(String),
    address_role LowCardinality(String),
    confidence Float64,
    risk_level UInt8,
    is_exposure_seed Bool,
    seed_category LowCardinality(String),
    source_id String,
    source_reference String,
    evidence_refs Array(String),
    review_status LowCardinality(String),
    reviewed_by String,
    review_note String,
    revision UInt64,
    created_at_unix_ms UInt64,
    inserted_at DateTime64(3) DEFAULT now64(3)
)
ENGINE = MergeTree
ORDER BY (network_id, address, claim_id, revision);

CREATE VIEW IF NOT EXISTS {{database}}.intelligence_claims_current AS
SELECT * FROM {{database}}.intelligence_claims ORDER BY revision DESC LIMIT 1 BY network_id, claim_id;

CREATE VIEW IF NOT EXISTS {{database}}.intelligence_active AS
SELECT * FROM {{database}}.intelligence_claims_current
WHERE review_status='approved' AND reviewed_by!='' AND source_id!=''
  AND source_reference!='' AND notEmpty(evidence_refs);

ALTER TABLE {{database}}.token_metadata ADD COLUMN IF NOT EXISTS reviewed_by String DEFAULT '';
ALTER TABLE {{database}}.token_metadata ADD COLUMN IF NOT EXISTS source_reference String DEFAULT '';
DROP VIEW IF EXISTS {{database}}.token_metadata_current;
CREATE VIEW {{database}}.token_metadata_current AS
SELECT * FROM {{database}}.token_metadata
ORDER BY is_verified DESC, created_at_unix_ms DESC, inserted_at DESC
LIMIT 1 BY network_id, token_address;
