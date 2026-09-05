use std::{
    collections::HashSet,
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(test)]
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

use clickhouse::{Client, Row, types::UInt256};
use serde::{Deserialize, Serialize};

use crate::{
    BSC_NETWORK_ID,
    config::ClickHouseConfig,
    semantic::{
        ProtocolContract, ProtocolImportResult, ProtocolRegistryInput, RegistryValidationError,
        SemanticBlockEvidence, SemanticEvent, TransactionFeature, ValidatedProtocolContract,
    },
};

use super::{
    model::CanonicalBlock,
    transfers::{CanonicalRelationship, ExtractedBlockEvidence, TokenMetadataDiscovery},
};

#[derive(Clone)]
pub struct ClickHouseIngestionStore {
    client: Client,
    database: String,
    #[cfg(test)]
    fail_point: Arc<AtomicU8>,
}

impl ClickHouseIngestionStore {
    pub fn new(config: &ClickHouseConfig) -> Self {
        let client = Client::default()
            .with_url(config.endpoint())
            .with_user(config.user())
            .with_password(config.password())
            .with_database(config.database());
        Self {
            client,
            database: config.database().to_string(),
            #[cfg(test)]
            fail_point: Arc::new(AtomicU8::new(0)),
        }
    }

    pub async fn protocol_contracts(&self) -> Result<Vec<ProtocolContract>, StoreError> {
        self.client
            .query(
                r#"
                SELECT
                    network_id, contract_address, protocol, protocol_type, contract_role,
                    decoder, remote_network_id, remote_contract_address, method_ids,
                    method_event_types, event_topics, event_types,
                    remote_receiver_topic_index, message_topic_index, source_id,
                    source_reference, review_status, evidence_confidence, enabled,
                    registry_revision, reviewed_by, review_note, created_at_unix_ms
                FROM protocol_contract_registry_active
                WHERE network_id = ?
                ORDER BY contract_address
                "#,
            )
            .bind(BSC_NETWORK_ID)
            .fetch_all::<ProtocolContract>()
            .await
            .map_err(|source| StoreError::ClickHouse {
                operation: "load active protocol registry",
                source,
            })
    }

    pub async fn import_protocol_contract(
        &self,
        input: ProtocolRegistryInput,
    ) -> Result<ProtocolImportResult, StoreError> {
        let record = input.validate()?;
        let previous_revision = self
            .client
            .query(
                "SELECT coalesce(max(registry_revision), 0) FROM protocol_contract_registry WHERE network_id = ? AND contract_address = ?",
            )
            .bind(BSC_NETWORK_ID)
            .bind(&record.contract_address)
            .fetch_one::<u64>()
            .await
            .map_err(|source| StoreError::ClickHouse {
                operation: "read protocol registry revision",
                source,
            })?;
        let registry_revision = previous_revision.checked_add(1).ok_or_else(|| {
            StoreError::RegistryRevisionOverflow {
                contract_address: record.contract_address.clone(),
            }
        })?;
        let created_at_unix_ms = now_unix_ms()?;
        let mut insert = self
            .client
            .insert::<ProtocolRegistryRow>("protocol_contract_registry")
            .await
            .map_err(|source| StoreError::ClickHouse {
                operation: "start protocol registry insert",
                source,
            })?;
        insert
            .write(&ProtocolRegistryRow::from_validated(
                &record,
                registry_revision,
                created_at_unix_ms,
            ))
            .await
            .map_err(|source| StoreError::ClickHouse {
                operation: "write protocol registry record",
                source,
            })?;
        insert
            .end()
            .await
            .map_err(|source| StoreError::ClickHouse {
                operation: "finish protocol registry insert",
                source,
            })?;
        Ok(ProtocolImportResult {
            contract_address: record.contract_address,
            registry_revision,
            review_status: record.review_status.clone(),
            active: record.review_status == "approved" && record.enabled == 1,
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(client: Client, database: impl Into<String>) -> Self {
        Self {
            client,
            database: database.into(),
            fail_point: Arc::new(AtomicU8::new(0)),
        }
    }

    #[cfg(test)]
    pub(crate) fn inject_failure_once(&self, point: TestFailPoint) {
        self.fail_point.store(point as u8, Ordering::SeqCst);
    }

    pub(crate) async fn commit_block(
        &self,
        block: &CanonicalBlock,
        evidence: &ExtractedBlockEvidence,
        semantic: &SemanticBlockEvidence,
        rpc_provider: &str,
        rpc_client_version: &str,
    ) -> Result<BlockCommit, StoreError> {
        let state_revision = self.next_state_revision(block.number).await?;
        let transaction_count =
            u32::try_from(block.transactions.len()).map_err(|_| StoreError::CountOverflow {
                field: "transaction_count",
            })?;
        let log_count = u32::try_from(block.logs.len())
            .map_err(|_| StoreError::CountOverflow { field: "log_count" })?;
        let relationship_count =
            u32::try_from(evidence.relationships.len()).map_err(|_| StoreError::CountOverflow {
                field: "relationship_count",
            })?;
        let feature_count = u32::try_from(semantic.transaction_features.len()).map_err(|_| {
            StoreError::CountOverflow {
                field: "feature_count",
            }
        })?;
        let semantic_event_count =
            u32::try_from(semantic.events.len()).map_err(|_| StoreError::CountOverflow {
                field: "semantic_event_count",
            })?;

        if !block.transactions.is_empty() {
            let mut insert = self
                .client
                .insert::<TransactionRow>("transactions")
                .await
                .map_err(|source| StoreError::ClickHouse {
                    operation: "start transaction evidence insert",
                    source,
                })?;
            for transaction in &block.transactions {
                insert
                    .write(&TransactionRow {
                        network_id: BSC_NETWORK_ID.to_string(),
                        tx_hash: transaction.hash.clone(),
                        block_hash: block.hash.clone(),
                        block_number: block.number,
                        block_timestamp_unix_ms: block.timestamp_unix_ms,
                        block_state_revision: state_revision,
                        transaction_index: transaction.transaction_index,
                        from_address: transaction.from_address.clone(),
                        to_address: transaction.to_address.clone(),
                        contract_address: transaction.contract_address.clone(),
                        nonce: transaction.nonce,
                        transaction_type: transaction.transaction_type,
                        value: transaction.value.as_clickhouse(),
                        input_selector: transaction.input_selector.clone(),
                        input_data: transaction.input_data.clone(),
                        status: transaction.status,
                        gas_limit: transaction.gas_limit,
                        gas_used: transaction.gas_used,
                        effective_gas_price: transaction.effective_gas_price.as_clickhouse(),
                        fee_paid: transaction.fee_paid.as_clickhouse(),
                    })
                    .await
                    .map_err(|source| StoreError::ClickHouse {
                        operation: "write transaction evidence",
                        source,
                    })?;
            }
            insert
                .end()
                .await
                .map_err(|source| StoreError::ClickHouse {
                    operation: "finish transaction evidence insert",
                    source,
                })?;
        }
        self.maybe_fail(TestFailPoint::Transactions)?;

        if !block.logs.is_empty() {
            let mut insert = self
                .client
                .insert::<LogRow>("evm_logs")
                .await
                .map_err(|source| StoreError::ClickHouse {
                    operation: "start log evidence insert",
                    source,
                })?;
            for log in &block.logs {
                insert
                    .write(&LogRow {
                        event_id: log.event_id.clone(),
                        network_id: BSC_NETWORK_ID.to_string(),
                        block_hash: block.hash.clone(),
                        block_number: block.number,
                        block_timestamp_unix_ms: block.timestamp_unix_ms,
                        block_state_revision: state_revision,
                        tx_hash: log.tx_hash.clone(),
                        transaction_index: log.transaction_index,
                        log_index: log.log_index,
                        contract_address: log.contract_address.clone(),
                        topic0: log.topic0.clone(),
                        topics: log.topics.clone(),
                        data: log.data.clone(),
                    })
                    .await
                    .map_err(|source| StoreError::ClickHouse {
                        operation: "write log evidence",
                        source,
                    })?;
            }
            insert
                .end()
                .await
                .map_err(|source| StoreError::ClickHouse {
                    operation: "finish log evidence insert",
                    source,
                })?;
        }
        self.maybe_fail(TestFailPoint::Logs)?;

        self.insert_relationships(block, state_revision, &evidence.relationships)
            .await?;
        self.maybe_fail(TestFailPoint::Relationships)?;
        self.insert_token_discoveries(block, state_revision, &evidence.token_discoveries)
            .await?;
        self.maybe_fail(TestFailPoint::TokenDiscoveries)?;
        self.insert_transaction_features(block, state_revision, &semantic.transaction_features)
            .await?;
        self.insert_semantic_events(block, state_revision, &semantic.events)
            .await?;
        self.maybe_fail(TestFailPoint::SemanticEvidence)?;

        let indexed_at_unix_ms = now_unix_ms()?;
        self.insert_block(&BlockRow {
            network_id: BSC_NETWORK_ID.to_string(),
            block_number: block.number,
            block_hash: block.hash.clone(),
            parent_hash: block.parent_hash.clone(),
            block_timestamp_unix_ms: block.timestamp_unix_ms,
            transaction_count,
            log_count,
            receipt_data_complete: 1,
            trace_data_complete: u8::from(evidence.trace_data_complete),
            canonical: 1,
            ingestion_status: "complete".to_string(),
            rpc_provider: rpc_provider.to_string(),
            rpc_client_version: rpc_client_version.to_string(),
            state_revision,
            indexed_at_unix_ms,
        })
        .await?;
        self.maybe_fail(TestFailPoint::CompleteMarker)?;

        Ok(BlockCommit {
            block_number: block.number,
            block_hash: block.hash.clone(),
            state_revision,
            transaction_count,
            log_count,
            relationship_count,
            feature_count,
            semantic_event_count,
            trace_data_complete: evidence.trace_data_complete,
        })
    }

    #[cfg(test)]
    fn maybe_fail(&self, point: TestFailPoint) -> Result<(), StoreError> {
        if self
            .fail_point
            .compare_exchange(point as u8, 0, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            return Err(StoreError::InjectedFailure {
                point: point.name(),
            });
        }
        Ok(())
    }

    #[cfg(not(test))]
    fn maybe_fail(&self, _point: TestFailPoint) -> Result<(), StoreError> {
        Ok(())
    }

    async fn insert_relationships(
        &self,
        block: &CanonicalBlock,
        state_revision: u64,
        relationships: &[CanonicalRelationship],
    ) -> Result<(), StoreError> {
        if relationships.is_empty() {
            return Ok(());
        }
        let operation = "insert canonical address relationships";
        let mut insert = self
            .client
            .insert::<RelationshipRow>("address_relationships")
            .await
            .map_err(|source| StoreError::ClickHouse { operation, source })?;
        for relationship in relationships {
            insert
                .write(&RelationshipRow {
                    relationship_id: relationship.relationship_id.clone(),
                    network_id: BSC_NETWORK_ID.to_string(),
                    block_hash: block.hash.clone(),
                    block_number: block.number,
                    block_timestamp_unix_ms: block.timestamp_unix_ms,
                    block_state_revision: state_revision,
                    tx_hash: relationship.tx_hash.clone(),
                    transaction_index: relationship.transaction_index,
                    event_index: relationship.event_index,
                    event_sub_index: relationship.event_sub_index,
                    trace_address: relationship.trace_address.clone(),
                    from_address: relationship.from_address.clone(),
                    to_address: relationship.to_address.clone(),
                    asset_id: relationship.asset_id.clone(),
                    token_id: relationship.token_id.clone(),
                    amount: relationship.amount.as_clickhouse(),
                    transfer_type: relationship.transfer_type.to_string(),
                })
                .await
                .map_err(|source| StoreError::ClickHouse { operation, source })?;
        }
        insert
            .end()
            .await
            .map_err(|source| StoreError::ClickHouse { operation, source })
    }

    async fn insert_token_discoveries(
        &self,
        block: &CanonicalBlock,
        state_revision: u64,
        discoveries: &[TokenMetadataDiscovery],
    ) -> Result<(), StoreError> {
        if discoveries.is_empty() {
            return Ok(());
        }
        let existing = self
            .existing_token_discoveries(block.number, discoveries)
            .await?;
        let pending = discoveries
            .iter()
            .filter(|discovery| {
                !existing.contains(&(
                    discovery.token_address.clone(),
                    discovery.standard_hint.to_string(),
                ))
            })
            .collect::<Vec<_>>();
        if pending.is_empty() {
            return Ok(());
        }
        let operation = "insert revision-gated token discoveries";
        let mut insert = self
            .client
            .insert::<TokenDiscoveryRow>("token_metadata_discoveries")
            .await
            .map_err(|source| StoreError::ClickHouse { operation, source })?;
        for discovery in pending {
            insert
                .write(&TokenDiscoveryRow {
                    discovery_id: discovery.discovery_id.clone(),
                    network_id: BSC_NETWORK_ID.to_string(),
                    token_address: discovery.token_address.clone(),
                    standard_hint: discovery.standard_hint.to_string(),
                    discovered_block: block.number,
                    block_hash: block.hash.clone(),
                    block_state_revision: state_revision,
                    tx_hash: discovery.tx_hash.clone(),
                    evidence_id: discovery.evidence_id.clone(),
                    created_at_unix_ms: block.timestamp_unix_ms,
                })
                .await
                .map_err(|source| StoreError::ClickHouse { operation, source })?;
        }
        insert
            .end()
            .await
            .map_err(|source| StoreError::ClickHouse { operation, source })
    }

    async fn insert_transaction_features(
        &self,
        block: &CanonicalBlock,
        state_revision: u64,
        features: &[TransactionFeature],
    ) -> Result<(), StoreError> {
        if features.is_empty() {
            return Ok(());
        }
        let operation = "insert semantic transaction features";
        let mut insert = self
            .client
            .insert::<TransactionFeatureRow>("transaction_features")
            .await
            .map_err(|source| StoreError::ClickHouse { operation, source })?;
        for feature in features {
            insert
                .write(&TransactionFeatureRow {
                    feature_id: feature.feature_id.clone(),
                    network_id: BSC_NETWORK_ID.to_string(),
                    block_hash: block.hash.clone(),
                    block_number: block.number,
                    block_timestamp_unix_ms: block.timestamp_unix_ms,
                    block_state_revision: state_revision,
                    tx_hash: feature.tx_hash.clone(),
                    transaction_type: feature.transaction_type.clone(),
                    transaction_subtype: feature.transaction_subtype.clone(),
                    protocol: feature.protocol.clone(),
                    method_id: feature.method_id.clone(),
                    is_swap: feature.is_swap,
                    is_bridge: feature.is_bridge,
                    is_mint: feature.is_mint,
                    is_burn: feature.is_burn,
                    is_liquidity_add: feature.is_liquidity_add,
                    is_liquidity_remove: feature.is_liquidity_remove,
                    is_contract_call: feature.is_contract_call,
                    unique_assets: feature.unique_assets,
                    participants: feature.participants,
                    classification_confidence: feature.classification_confidence,
                    classification_source: feature.classification_source.clone(),
                    detector: feature.detector.clone(),
                    detector_version: feature.detector_version.clone(),
                    evidence_refs: feature.evidence_refs.clone(),
                })
                .await
                .map_err(|source| StoreError::ClickHouse { operation, source })?;
        }
        insert
            .end()
            .await
            .map_err(|source| StoreError::ClickHouse { operation, source })
    }

    async fn insert_semantic_events(
        &self,
        block: &CanonicalBlock,
        state_revision: u64,
        events: &[SemanticEvent],
    ) -> Result<(), StoreError> {
        if events.is_empty() {
            return Ok(());
        }
        let operation = "insert semantic AML evidence";
        let mut insert = self
            .client
            .insert::<SemanticEventRow>("semantic_aml_events")
            .await
            .map_err(|source| StoreError::ClickHouse { operation, source })?;
        for event in events {
            insert
                .write(&SemanticEventRow {
                    event_id: event.event_id.clone(),
                    network_id: BSC_NETWORK_ID.to_string(),
                    block_hash: block.hash.clone(),
                    block_number: block.number,
                    block_timestamp_unix_ms: block.timestamp_unix_ms,
                    block_state_revision: state_revision,
                    tx_hash: event.tx_hash.clone(),
                    event_index: event.event_index,
                    event_type: event.event_type.clone(),
                    subject_address: event.subject_address.clone(),
                    protocol: event.protocol.clone(),
                    protocol_contract: event.protocol_contract.clone(),
                    counterparty_address: event.counterparty_address.clone(),
                    correlation_key: event.correlation_key.clone(),
                    asset_in: event.asset_in.clone(),
                    asset_out: event.asset_out.clone(),
                    remote_network_id: event.remote_network_id.clone(),
                    remote_asset: event.remote_asset.clone(),
                    bridge_direction: event.bridge_direction.clone(),
                    remote_receiver: event.remote_receiver.clone(),
                    bridge_message_id: event.bridge_message_id.clone(),
                    amount_in: event.amount_in.clone(),
                    amount_out: event.amount_out.clone(),
                    detector: event.detector.clone(),
                    detector_version: event.detector_version.clone(),
                    confidence: event.confidence,
                    evidence_refs: event.evidence_refs.clone(),
                    evidence_json: event.evidence_json.clone(),
                })
                .await
                .map_err(|source| StoreError::ClickHouse { operation, source })?;
        }
        insert
            .end()
            .await
            .map_err(|source| StoreError::ClickHouse { operation, source })
    }

    async fn existing_token_discoveries(
        &self,
        current_block: u64,
        discoveries: &[TokenMetadataDiscovery],
    ) -> Result<HashSet<(String, String)>, StoreError> {
        let predicates = std::iter::repeat_n(
            "(token_address = ? AND standard_hint = ?)",
            discoveries.len(),
        )
        .collect::<Vec<_>>()
        .join(" OR ");
        let sql = format!(
            "SELECT token_address, standard_hint FROM token_metadata_discoveries_canonical WHERE network_id = ? AND discovered_block != ? AND ({predicates})"
        );
        let mut query = self
            .client
            .query(&sql)
            .bind(BSC_NETWORK_ID)
            .bind(current_block);
        for discovery in discoveries {
            query = query
                .bind(&discovery.token_address)
                .bind(discovery.standard_hint);
        }
        let rows = query
            .fetch_all::<DiscoveredTokenRow>()
            .await
            .map_err(|source| StoreError::ClickHouse {
                operation: "read existing canonical token discoveries",
                source,
            })?;
        Ok(rows
            .into_iter()
            .map(|row| (row.token_address, row.standard_hint))
            .collect())
    }

    pub(crate) async fn record_failure(
        &self,
        failure: FailureInput<'_>,
        max_attempts: u32,
    ) -> Result<(), StoreError> {
        let failure_id = failure_id(failure.block_number);
        let existing = self.failure_state(&failure_id).await?;
        let now = now_unix_ms()?;
        let attempt_count = existing
            .as_ref()
            .map_or(1, |state| state.attempt_count.saturating_add(1));
        let remains_retryable = failure.retryable && attempt_count < max_attempts;
        let row = FailureRow {
            failure_id,
            network_id: BSC_NETWORK_ID.to_string(),
            block_number: failure.block_number,
            block_hash: failure.block_hash.unwrap_or_default().to_string(),
            stage: failure.stage.to_string(),
            error_class: failure.error_class.to_string(),
            error_summary: sanitize_summary(failure.error_summary),
            retryable: u8::from(remains_retryable),
            attempt_count,
            status: if remains_retryable { "open" } else { "dead" }.to_string(),
            created_at_unix_ms: existing
                .as_ref()
                .map_or(now, |state| state.created_at_unix_ms),
            updated_at_unix_ms: now,
            resolved_at_unix_ms: 0,
        };
        self.insert_failure(&row, "record ingestion failure").await
    }

    pub(crate) async fn resolve_failure(&self, block_number: u64) -> Result<(), StoreError> {
        let failure_id = failure_id(block_number);
        let Some(existing) = self.failure_state(&failure_id).await? else {
            return Ok(());
        };
        if existing.status == "resolved" {
            return Ok(());
        }
        let now = now_unix_ms()?;
        self.insert_failure(
            &FailureRow {
                failure_id,
                network_id: BSC_NETWORK_ID.to_string(),
                block_number,
                block_hash: existing.block_hash,
                stage: existing.stage,
                error_class: existing.error_class,
                error_summary: existing.error_summary,
                retryable: existing.retryable,
                attempt_count: existing.attempt_count,
                status: "resolved".to_string(),
                created_at_unix_ms: existing.created_at_unix_ms,
                updated_at_unix_ms: now,
                resolved_at_unix_ms: now,
            },
            "resolve ingestion failure",
        )
        .await
    }

    pub(crate) async fn requeue_failure(&self, block_number: u64) -> Result<(), StoreError> {
        let failure_id = failure_id(block_number);
        let Some(existing) = self.failure_state(&failure_id).await? else {
            return Ok(());
        };
        let now = now_unix_ms()?;
        self.insert_failure(
            &FailureRow {
                failure_id,
                network_id: BSC_NETWORK_ID.to_string(),
                block_number,
                block_hash: existing.block_hash,
                stage: existing.stage,
                error_class: existing.error_class,
                error_summary: existing.error_summary,
                retryable: 1,
                attempt_count: existing.attempt_count,
                status: "open".to_string(),
                created_at_unix_ms: existing.created_at_unix_ms,
                updated_at_unix_ms: now,
                resolved_at_unix_ms: 0,
            },
            "requeue dead ingestion failure",
        )
        .await
    }

    pub(crate) async fn canonical_block(
        &self,
        block_number: u64,
    ) -> Result<Option<StoredBlock>, StoreError> {
        let mut rows = self
            .client
            .query(
                r#"
                SELECT
                    block_number, block_hash, parent_hash, block_timestamp_unix_ms,
                    transaction_count, log_count, receipt_data_complete,
                    trace_data_complete, rpc_provider, rpc_client_version, current_revision
                FROM ingested_blocks_canonical
                WHERE network_id = ? AND block_number = ?
                LIMIT 1
                "#,
            )
            .bind(BSC_NETWORK_ID)
            .bind(block_number)
            .fetch_all::<StoredBlock>()
            .await
            .map_err(|source| StoreError::ClickHouse {
                operation: "read canonical block marker",
                source,
            })?;
        Ok(rows.pop())
    }

    pub(crate) async fn checkpoint(&self) -> Result<Option<SyncCheckpoint>, StoreError> {
        let mut rows = self
            .client
            .query(
                r#"
                SELECT next_block, last_finalized_block, last_finalized_block_hash, state_revision
                FROM sync_state_current
                WHERE network_id = ?
                LIMIT 1
                "#,
            )
            .bind(BSC_NETWORK_ID)
            .fetch_all::<SyncCheckpoint>()
            .await
            .map_err(|source| StoreError::ClickHouse {
                operation: "read sync checkpoint",
                source,
            })?;
        Ok(rows.pop())
    }

    pub(crate) async fn advance_checkpoint(
        &self,
        block_number: u64,
        block_hash: &str,
    ) -> Result<SyncCheckpoint, StoreError> {
        let current = self.checkpoint().await?;
        if let Some(current) = &current {
            if current.next_block > block_number {
                if current.last_finalized_block == block_number
                    && current.last_finalized_block_hash == block_hash
                {
                    return Ok(current.clone());
                }
                return Err(StoreError::CursorRegression {
                    current_next: current.next_block,
                    attempted_block: block_number,
                });
            }
            if current.next_block < block_number {
                return Err(StoreError::CursorGap {
                    expected_block: current.next_block,
                    attempted_block: block_number,
                });
            }
        }
        let state_revision = current
            .as_ref()
            .map_or(Some(1), |value| value.state_revision.checked_add(1))
            .ok_or(StoreError::SyncStateRevisionOverflow)?;
        self.write_checkpoint(
            block_number
                .checked_add(1)
                .ok_or(StoreError::BlockNumberOverflow)?,
            block_number,
            block_hash,
            state_revision,
        )
        .await
    }

    pub(crate) async fn rewind_checkpoint(
        &self,
        next_block: u64,
        ancestor: Option<&StoredBlock>,
    ) -> Result<SyncCheckpoint, StoreError> {
        let revision = self
            .checkpoint()
            .await?
            .map_or(Some(1), |value| value.state_revision.checked_add(1))
            .ok_or(StoreError::SyncStateRevisionOverflow)?;
        self.write_checkpoint(
            next_block,
            ancestor.map_or(0, |block| block.block_number),
            ancestor.map_or("", |block| block.block_hash.as_str()),
            revision,
        )
        .await
    }

    async fn write_checkpoint(
        &self,
        next_block: u64,
        last_finalized_block: u64,
        last_finalized_block_hash: &str,
        state_revision: u64,
    ) -> Result<SyncCheckpoint, StoreError> {
        let row = SyncStateRow {
            network_id: BSC_NETWORK_ID.to_string(),
            next_block,
            last_finalized_block,
            last_finalized_block_hash: last_finalized_block_hash.to_string(),
            state_revision,
            updated_at_unix_ms: now_unix_ms()?,
        };
        let mut insert = self
            .client
            .insert::<SyncStateRow>("sync_state")
            .await
            .map_err(|source| StoreError::ClickHouse {
                operation: "start sync checkpoint insert",
                source,
            })?;
        insert
            .write(&row)
            .await
            .map_err(|source| StoreError::ClickHouse {
                operation: "write sync checkpoint",
                source,
            })?;
        insert
            .end()
            .await
            .map_err(|source| StoreError::ClickHouse {
                operation: "finish sync checkpoint insert",
                source,
            })?;
        Ok(SyncCheckpoint {
            next_block,
            last_finalized_block,
            last_finalized_block_hash: last_finalized_block_hash.to_string(),
            state_revision,
        })
    }

    pub(crate) async fn invalidate_range(
        &self,
        start_block: u64,
        end_block: u64,
        rpc_provider: &str,
        rpc_client_version: &str,
    ) -> Result<u64, StoreError> {
        if start_block > end_block {
            return Ok(0);
        }
        let blocks = self.canonical_blocks(start_block, end_block).await?;
        for block in &blocks {
            self.insert_block(&BlockRow {
                network_id: BSC_NETWORK_ID.to_string(),
                block_number: block.block_number,
                block_hash: block.block_hash.clone(),
                parent_hash: block.parent_hash.clone(),
                block_timestamp_unix_ms: block.block_timestamp_unix_ms,
                transaction_count: block.transaction_count,
                log_count: block.log_count,
                receipt_data_complete: block.receipt_data_complete,
                trace_data_complete: block.trace_data_complete,
                canonical: 0,
                ingestion_status: "orphaned".to_string(),
                rpc_provider: rpc_provider.to_string(),
                rpc_client_version: rpc_client_version.to_string(),
                state_revision: block.current_revision.checked_add(1).ok_or(
                    StoreError::StateRevisionOverflow {
                        block_number: block.block_number,
                    },
                )?,
                indexed_at_unix_ms: now_unix_ms()?,
            })
            .await?;
        }
        Ok(blocks.len() as u64)
    }

    async fn canonical_blocks(
        &self,
        start_block: u64,
        end_block: u64,
    ) -> Result<Vec<StoredBlock>, StoreError> {
        self.client
            .query(
                r#"
                SELECT
                    block_number, block_hash, parent_hash, block_timestamp_unix_ms,
                    transaction_count, log_count, receipt_data_complete,
                    trace_data_complete, rpc_provider, rpc_client_version, current_revision
                FROM ingested_blocks_canonical
                WHERE network_id = ? AND block_number BETWEEN ? AND ?
                ORDER BY block_number
                "#,
            )
            .bind(BSC_NETWORK_ID)
            .bind(start_block)
            .bind(end_block)
            .fetch_all::<StoredBlock>()
            .await
            .map_err(|source| StoreError::ClickHouse {
                operation: "read canonical block range",
                source,
            })
    }

    pub(crate) async fn find_gaps(
        &self,
        start_block: u64,
        end_block: u64,
        require_trace: bool,
        limit: u64,
    ) -> Result<Vec<u64>, StoreError> {
        let blocks = self.canonical_blocks(start_block, end_block).await?;
        let complete = blocks
            .into_iter()
            .filter(|block| {
                block.receipt_data_complete == 1
                    && (!require_trace || block.trace_data_complete == 1)
            })
            .map(|block| block.block_number)
            .collect::<HashSet<_>>();
        Ok((start_block..=end_block)
            .filter(|number| !complete.contains(number))
            .take(usize::try_from(limit).unwrap_or(usize::MAX))
            .collect())
    }

    pub(crate) async fn pending_failure_blocks(
        &self,
        start_block: u64,
        end_block: u64,
        include_dead: bool,
        limit: u64,
    ) -> Result<Vec<u64>, StoreError> {
        let statuses = if include_dead {
            "('open', 'dead')"
        } else {
            "('open')"
        };
        let sql = format!(
            "SELECT block_number FROM ingestion_failures_current WHERE network_id = ? AND block_number BETWEEN ? AND ? AND status IN {statuses} ORDER BY block_number LIMIT ?"
        );
        let rows = self
            .client
            .query(&sql)
            .bind(BSC_NETWORK_ID)
            .bind(start_block)
            .bind(end_block)
            .bind(limit)
            .fetch_all::<FailureBlockRow>()
            .await
            .map_err(|source| StoreError::ClickHouse {
                operation: "read pending ingestion failures",
                source,
            })?;
        Ok(rows.into_iter().map(|row| row.block_number).collect())
    }

    pub(crate) async fn dead_failure_blocks(
        &self,
        start_block: u64,
        end_block: u64,
    ) -> Result<HashSet<u64>, StoreError> {
        let rows = self
            .client
            .query(
                "SELECT block_number FROM ingestion_failures_current WHERE network_id = ? AND block_number BETWEEN ? AND ? AND status = 'dead'",
            )
            .bind(BSC_NETWORK_ID)
            .bind(start_block)
            .bind(end_block)
            .fetch_all::<FailureBlockRow>()
            .await
            .map_err(|source| StoreError::ClickHouse {
                operation: "read dead ingestion failures",
                source,
            })?;
        Ok(rows.into_iter().map(|row| row.block_number).collect())
    }

    pub(crate) async fn storage_snapshot(&self) -> Result<StorageSnapshot, StoreError> {
        self.client
            .query(
                r#"
                SELECT
                    coalesce(sum(rows), 0) AS rows,
                    coalesce(sum(data_compressed_bytes), 0) AS compressed_bytes,
                    coalesce(sum(data_uncompressed_bytes), 0) AS uncompressed_bytes
                FROM system.parts
                WHERE active AND database = ? AND table IN
                    ('ingested_blocks', 'transactions', 'evm_logs', 'address_relationships', 'token_metadata_discoveries')
                "#,
            )
            .bind(&self.database)
            .fetch_one::<StorageSnapshot>()
            .await
            .map_err(|source| StoreError::ClickHouse {
                operation: "read ClickHouse storage metrics",
                source,
            })
    }

    pub(crate) async fn canonical_range_span(
        &self,
        start_block: u64,
        end_block: u64,
    ) -> Result<CanonicalRangeSpan, StoreError> {
        self.client
            .query(
                r#"
                SELECT
                    count() AS block_count,
                    coalesce(min(block_timestamp_unix_ms), 0) AS first_timestamp_unix_ms,
                    coalesce(max(block_timestamp_unix_ms), 0) AS last_timestamp_unix_ms
                FROM ingested_blocks_canonical
                WHERE network_id = ? AND block_number BETWEEN ? AND ?
                "#,
            )
            .bind(BSC_NETWORK_ID)
            .bind(start_block)
            .bind(end_block)
            .fetch_one::<CanonicalRangeSpan>()
            .await
            .map_err(|source| StoreError::ClickHouse {
                operation: "read canonical benchmark time span",
                source,
            })
    }

    pub(crate) async fn insert_benchmark(
        &self,
        benchmark: &BenchmarkRow,
    ) -> Result<(), StoreError> {
        let mut insert = self
            .client
            .insert::<BenchmarkRow>("ingestion_benchmarks")
            .await
            .map_err(|source| StoreError::ClickHouse {
                operation: "start ingestion benchmark insert",
                source,
            })?;
        insert
            .write(benchmark)
            .await
            .map_err(|source| StoreError::ClickHouse {
                operation: "write ingestion benchmark",
                source,
            })?;
        insert.end().await.map_err(|source| StoreError::ClickHouse {
            operation: "finish ingestion benchmark insert",
            source,
        })
    }

    async fn next_state_revision(&self, block_number: u64) -> Result<u64, StoreError> {
        let current = self
            .client
            .query(
                "SELECT coalesce(max(state_revision), 0) FROM ingested_blocks WHERE network_id = ? AND block_number = ?",
            )
            .bind(BSC_NETWORK_ID)
            .bind(block_number)
            .fetch_one::<u64>()
            .await
            .map_err(|source| StoreError::ClickHouse {
                operation: "read block state revision",
                source,
            })?;
        current
            .checked_add(1)
            .ok_or(StoreError::StateRevisionOverflow { block_number })
    }

    async fn failure_state(&self, failure_id: &str) -> Result<Option<FailureState>, StoreError> {
        let mut rows = self
            .client
            .query(
                r#"
                SELECT
                    block_hash, stage, error_class, error_summary, retryable,
                    attempt_count, status, created_at_unix_ms
                FROM ingestion_failures_current
                WHERE network_id = ? AND failure_id = ?
                LIMIT 1
                "#,
            )
            .bind(BSC_NETWORK_ID)
            .bind(failure_id)
            .fetch_all::<FailureState>()
            .await
            .map_err(|source| StoreError::ClickHouse {
                operation: "read ingestion failure state",
                source,
            })?;
        Ok(rows.pop())
    }

    async fn insert_block(&self, row: &BlockRow) -> Result<(), StoreError> {
        let operation = "commit complete block marker";
        let mut insert = self
            .client
            .insert::<BlockRow>("ingested_blocks")
            .await
            .map_err(|source| StoreError::ClickHouse { operation, source })?;
        insert
            .write(row)
            .await
            .map_err(|source| StoreError::ClickHouse { operation, source })?;
        insert
            .end()
            .await
            .map_err(|source| StoreError::ClickHouse { operation, source })
    }

    async fn insert_failure(
        &self,
        row: &FailureRow,
        operation: &'static str,
    ) -> Result<(), StoreError> {
        let mut insert = self
            .client
            .insert::<FailureRow>("ingestion_failures")
            .await
            .map_err(|source| StoreError::ClickHouse { operation, source })?;
        insert
            .write(row)
            .await
            .map_err(|source| StoreError::ClickHouse { operation, source })?;
        insert
            .end()
            .await
            .map_err(|source| StoreError::ClickHouse { operation, source })
    }
}

fn failure_id(block_number: u64) -> String {
    format!("{BSC_NETWORK_ID}:canonical_ingestion:{block_number}")
}

fn now_unix_ms() -> Result<u64, StoreError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(StoreError::Clock)?
        .as_millis();
    u64::try_from(millis).map_err(|_| StoreError::ClockOverflow)
}

fn sanitize_summary(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(320)
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum TestFailPoint {
    Transactions = 1,
    Logs = 2,
    Relationships = 3,
    TokenDiscoveries = 4,
    SemanticEvidence = 5,
    CompleteMarker = 6,
}

impl TestFailPoint {
    #[cfg(test)]
    fn name(self) -> &'static str {
        match self {
            Self::Transactions => "after_transactions",
            Self::Logs => "after_logs",
            Self::Relationships => "after_relationships",
            Self::TokenDiscoveries => "after_token_discoveries",
            Self::SemanticEvidence => "after_semantic_evidence",
            Self::CompleteMarker => "after_block_marker",
        }
    }
}

pub(crate) struct FailureInput<'a> {
    pub block_number: u64,
    pub block_hash: Option<&'a str>,
    pub stage: &'static str,
    pub error_class: &'static str,
    pub error_summary: &'a str,
    pub retryable: bool,
}

#[derive(Debug, Clone, Deserialize, Row, PartialEq, Eq)]
pub(crate) struct StoredBlock {
    pub block_number: u64,
    pub block_hash: String,
    pub parent_hash: String,
    pub block_timestamp_unix_ms: u64,
    pub transaction_count: u32,
    pub log_count: u32,
    pub receipt_data_complete: u8,
    pub trace_data_complete: u8,
    pub rpc_provider: String,
    pub rpc_client_version: String,
    pub current_revision: u64,
}

#[derive(Debug, Clone, Deserialize, Row, PartialEq, Eq)]
pub(crate) struct SyncCheckpoint {
    pub next_block: u64,
    pub last_finalized_block: u64,
    pub last_finalized_block_hash: String,
    pub state_revision: u64,
}

#[derive(Debug, Clone, Serialize, Row)]
struct SyncStateRow {
    network_id: String,
    next_block: u64,
    last_finalized_block: u64,
    last_finalized_block_hash: String,
    state_revision: u64,
    updated_at_unix_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Row)]
struct FailureBlockRow {
    block_number: u64,
}

#[derive(Debug, Clone, Deserialize, Row, PartialEq, Eq)]
pub(crate) struct StorageSnapshot {
    pub rows: u64,
    pub compressed_bytes: u64,
    pub uncompressed_bytes: u64,
}

#[derive(Debug, Clone, Deserialize, Row, PartialEq, Eq)]
pub(crate) struct CanonicalRangeSpan {
    pub block_count: u64,
    pub first_timestamp_unix_ms: u64,
    pub last_timestamp_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize, Row)]
pub(crate) struct BenchmarkRow {
    pub benchmark_id: String,
    pub network_id: String,
    pub start_block: u64,
    pub end_block: u64,
    pub completed_blocks: u64,
    pub transaction_count: u64,
    pub log_count: u64,
    pub relationship_count: u64,
    pub feature_count: u64,
    pub semantic_event_count: u64,
    pub elapsed_ms: u64,
    pub blocks_per_second: f64,
    pub observed_live_blocks_per_second: f64,
    pub live_rate_multiple: f64,
    pub rows_per_second: f64,
    pub compressed_bytes: u64,
    pub uncompressed_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Row)]
struct TransactionRow {
    network_id: String,
    tx_hash: String,
    block_hash: String,
    block_number: u64,
    block_timestamp_unix_ms: u64,
    block_state_revision: u64,
    transaction_index: u32,
    from_address: String,
    to_address: String,
    contract_address: String,
    nonce: u64,
    transaction_type: u8,
    value: UInt256,
    input_selector: String,
    input_data: String,
    status: u8,
    gas_limit: u64,
    gas_used: u64,
    effective_gas_price: UInt256,
    fee_paid: UInt256,
}

#[derive(Debug, Clone, Serialize, Row)]
struct LogRow {
    event_id: String,
    network_id: String,
    block_hash: String,
    block_number: u64,
    block_timestamp_unix_ms: u64,
    block_state_revision: u64,
    tx_hash: String,
    transaction_index: u32,
    log_index: u32,
    contract_address: String,
    topic0: String,
    topics: Vec<String>,
    data: String,
}

#[derive(Debug, Clone, Serialize, Row)]
struct RelationshipRow {
    relationship_id: String,
    network_id: String,
    block_hash: String,
    block_number: u64,
    block_timestamp_unix_ms: u64,
    block_state_revision: u64,
    tx_hash: String,
    transaction_index: u32,
    event_index: u32,
    event_sub_index: u32,
    trace_address: Vec<u32>,
    from_address: String,
    to_address: String,
    asset_id: String,
    token_id: String,
    amount: UInt256,
    transfer_type: String,
}

#[derive(Debug, Clone, Serialize, Row)]
struct TokenDiscoveryRow {
    discovery_id: String,
    network_id: String,
    token_address: String,
    standard_hint: String,
    discovered_block: u64,
    block_hash: String,
    block_state_revision: u64,
    tx_hash: String,
    evidence_id: String,
    created_at_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize, Row)]
struct TransactionFeatureRow {
    feature_id: String,
    network_id: String,
    block_hash: String,
    block_number: u64,
    block_timestamp_unix_ms: u64,
    block_state_revision: u64,
    tx_hash: String,
    transaction_type: String,
    transaction_subtype: String,
    protocol: String,
    method_id: String,
    is_swap: u8,
    is_bridge: u8,
    is_mint: u8,
    is_burn: u8,
    is_liquidity_add: u8,
    is_liquidity_remove: u8,
    is_contract_call: u8,
    unique_assets: u16,
    participants: u16,
    classification_confidence: f32,
    classification_source: String,
    detector: String,
    detector_version: String,
    evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Row)]
struct SemanticEventRow {
    event_id: String,
    network_id: String,
    block_hash: String,
    block_number: u64,
    block_timestamp_unix_ms: u64,
    block_state_revision: u64,
    tx_hash: String,
    event_index: u32,
    event_type: String,
    subject_address: String,
    protocol: String,
    protocol_contract: String,
    counterparty_address: String,
    correlation_key: String,
    asset_in: String,
    asset_out: String,
    remote_network_id: String,
    remote_asset: String,
    bridge_direction: String,
    remote_receiver: String,
    bridge_message_id: String,
    amount_in: String,
    amount_out: String,
    detector: String,
    detector_version: String,
    confidence: f32,
    evidence_refs: Vec<String>,
    evidence_json: String,
}

#[derive(Debug, Clone, Serialize, Row)]
struct ProtocolRegistryRow {
    network_id: String,
    contract_address: String,
    protocol: String,
    protocol_type: String,
    contract_role: String,
    decoder: String,
    remote_network_id: String,
    remote_contract_address: String,
    method_ids: Vec<String>,
    method_event_types: Vec<String>,
    event_topics: Vec<String>,
    event_types: Vec<String>,
    remote_receiver_topic_index: i8,
    message_topic_index: i8,
    source_id: String,
    source_reference: String,
    review_status: String,
    evidence_confidence: f32,
    enabled: u8,
    registry_revision: u64,
    reviewed_by: String,
    review_note: String,
    created_at_unix_ms: u64,
}

impl ProtocolRegistryRow {
    fn from_validated(
        record: &ValidatedProtocolContract,
        registry_revision: u64,
        created_at_unix_ms: u64,
    ) -> Self {
        Self {
            network_id: BSC_NETWORK_ID.to_string(),
            contract_address: record.contract_address.clone(),
            protocol: record.protocol.clone(),
            protocol_type: record.protocol_type.clone(),
            contract_role: record.contract_role.clone(),
            decoder: record.decoder.clone(),
            remote_network_id: record.remote_network_id.clone(),
            remote_contract_address: record.remote_contract_address.clone(),
            method_ids: record.method_ids.clone(),
            method_event_types: record.method_event_types.clone(),
            event_topics: record.event_topics.clone(),
            event_types: record.event_types.clone(),
            remote_receiver_topic_index: record.remote_receiver_topic_index,
            message_topic_index: record.message_topic_index,
            source_id: record.source_id.clone(),
            source_reference: record.source_reference.clone(),
            review_status: record.review_status.clone(),
            evidence_confidence: record.evidence_confidence,
            enabled: record.enabled,
            registry_revision,
            reviewed_by: record.reviewed_by.clone(),
            review_note: record.review_note.clone(),
            created_at_unix_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize, Row)]
struct BlockRow {
    network_id: String,
    block_number: u64,
    block_hash: String,
    parent_hash: String,
    block_timestamp_unix_ms: u64,
    transaction_count: u32,
    log_count: u32,
    receipt_data_complete: u8,
    trace_data_complete: u8,
    canonical: u8,
    ingestion_status: String,
    rpc_provider: String,
    rpc_client_version: String,
    state_revision: u64,
    indexed_at_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize, Row)]
struct FailureRow {
    failure_id: String,
    network_id: String,
    block_number: u64,
    block_hash: String,
    stage: String,
    error_class: String,
    error_summary: String,
    retryable: u8,
    attempt_count: u32,
    status: String,
    created_at_unix_ms: u64,
    updated_at_unix_ms: u64,
    resolved_at_unix_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Row)]
struct FailureState {
    block_hash: String,
    stage: String,
    error_class: String,
    error_summary: String,
    retryable: u8,
    attempt_count: u32,
    status: String,
    created_at_unix_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Row)]
struct DiscoveredTokenRow {
    token_address: String,
    standard_hint: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BlockCommit {
    pub block_number: u64,
    pub block_hash: String,
    pub state_revision: u64,
    pub transaction_count: u32,
    pub log_count: u32,
    pub relationship_count: u32,
    pub feature_count: u32,
    pub semantic_event_count: u32,
    pub trace_data_complete: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    RegistryValidation(#[from] RegistryValidationError),
    #[error("ClickHouse failed to {operation}")]
    ClickHouse {
        operation: &'static str,
        #[source]
        source: clickhouse::error::Error,
    },
    #[error("{field} exceeds its storage type")]
    CountOverflow { field: &'static str },
    #[error("block state revision overflow at block {block_number}")]
    StateRevisionOverflow { block_number: u64 },
    #[error("protocol registry revision overflow for {contract_address}")]
    RegistryRevisionOverflow { contract_address: String },
    #[error("block number overflow while advancing the sync cursor")]
    BlockNumberOverflow,
    #[error("sync state revision overflow")]
    SyncStateRevisionOverflow,
    #[error(
        "sync cursor expected block {expected_block}, but block {attempted_block} was committed"
    )]
    CursorGap {
        expected_block: u64,
        attempted_block: u64,
    },
    #[error("sync cursor at {current_next} cannot move backward through block {attempted_block}")]
    CursorRegression {
        current_next: u64,
        attempted_block: u64,
    },
    #[cfg(test)]
    #[error("injected ingestion crash boundary {point}")]
    InjectedFailure { point: &'static str },
    #[error("system clock is before Unix epoch")]
    Clock(#[source] std::time::SystemTimeError),
    #[error("system clock timestamp exceeds UInt64 milliseconds")]
    ClockOverflow,
}
