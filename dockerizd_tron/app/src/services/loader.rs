use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use clickhouse::Client;
use tokio::sync::Semaphore;

use crate::config::AppConfig;
use crate::helper::tron::TronClient;
use crate::services::tron::batcher::relationships::RelationshipBatcher;
use crate::services::tron::batcher::semantic_events::SemanticEventBatcher;
use crate::services::tron::batcher::token_metadata_discoveries::TokenMetadataDiscoveryBatcher;
use crate::services::tron::batcher::transaction_features::TransactionFeatureBatcher;
use crate::services::tron::batcher::transactions::TransactionBatcher;
use crate::services::tron::tron_classifier::registry::ProtocolRegistry;

pub struct LoaderTron {
    pub clickhouse: Arc<Client>,
    pub tron_client: Arc<TronClient>,
    pub rpc_limiter: Arc<Semaphore>,
    pub transaction_batcher: Arc<TransactionBatcher>,
    pub relationship_batcher: Arc<RelationshipBatcher>,
    pub semantic_event_batcher: Arc<SemanticEventBatcher>,
    pub token_metadata_discovery_batcher: Arc<TokenMetadataDiscoveryBatcher>,
    pub config: Arc<AppConfig>,
    pub transaction_feature_batcher: Arc<TransactionFeatureBatcher>,
    pub protocol_registry: Arc<ProtocolRegistry>,
}

impl LoaderTron {
    pub async fn new(config: &AppConfig) -> Result<Self> {
        let clickhouse = Arc::new(
            Client::default()
                .with_url(&config.clickhouse_url)
                .with_user(&config.clickhouse_user)
                .with_password(&config.clickhouse_pass)
                .with_database(&config.clickhouse_db_tron),
        );
        let tron_rpc_url = config
            .tron_rpc_url
            .as_ref()
            .ok_or_else(|| anyhow!("TRON_RPC_URL or TRON_RPC_HTTP must be configured"))?;

        // برای آینده که میخواییم از نود واقعی استفاده کنیم
        // بررسی فایل
        let tron_client = Arc::new(TronClient::new(
            tron_rpc_url,
            config.tron_api_key.clone(),
            config.rpc_timeout_seconds,
        )?);
        let rpc_limiter = Arc::new(Semaphore::new(config.rpc_max_concurrency.max(1)));
        let max_batch_rows = config.tron_ingestion_batch_max_rows.clamp(100, 100_000);
        let flush_interval =
            Duration::from_secs(config.tron_ingestion_flush_interval_seconds.clamp(5, 3_600));
        let protocol_registry = Arc::new(
            ProtocolRegistry::load(&clickhouse, config.tron_protocol_registry_refresh_blocks)
                .await?,
        );

        Ok(Self {
            clickhouse: clickhouse.clone(),
            tron_client,
            rpc_limiter,
            transaction_batcher: TransactionBatcher::create(
                clickhouse.clone(),
                max_batch_rows,
                flush_interval,
            ),
            relationship_batcher: RelationshipBatcher::create(
                clickhouse.clone(),
                max_batch_rows,
                flush_interval,
            ),
            semantic_event_batcher: SemanticEventBatcher::create(
                clickhouse.clone(),
                max_batch_rows,
                flush_interval,
            ),
            token_metadata_discovery_batcher: TokenMetadataDiscoveryBatcher::create(
                clickhouse.clone(),
                max_batch_rows,
                flush_interval,
            ),
            transaction_feature_batcher: TransactionFeatureBatcher::create(
                clickhouse.clone(),
                max_batch_rows,
                flush_interval,
            ),
            protocol_registry,
            config: Arc::new(config.clone()),
        })
    }

    pub async fn flush_batches(&self) -> Result<()> {
        self.transaction_batcher.flush_all().await?;
        self.relationship_batcher.flush_all().await?;
        self.semantic_event_batcher.flush_all().await?;
        self.token_metadata_discovery_batcher.flush_all().await?;
        self.transaction_feature_batcher.flush_all().await?;
        Ok(())
    }
}
