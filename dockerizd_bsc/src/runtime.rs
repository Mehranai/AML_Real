use crate::{
    config::{AppConfig, ClickHouseConfig, ConfigError, IngestionConfig},
    db::{SchemaError, validate_bsc_schema},
    ingestion::{CanonicalIngestor, ClickHouseIngestionStore, IngestionError},
    rpc::{ProbeError, probe_configured_endpoints},
};

pub struct BscRuntime {
    pub ingestor: CanonicalIngestor,
    pub ingestion_config: IngestionConfig,
}

impl BscRuntime {
    pub async fn connect() -> Result<Self, RuntimeError> {
        let ingestion_config = IngestionConfig::from_env()?;
        let app_config = AppConfig::from_env()?;
        let clickhouse_config = ClickHouseConfig::from_env()?;
        validate_bsc_schema(&clickhouse_config).await?;
        let probe = probe_configured_endpoints(&app_config).await?;
        let store = ClickHouseIngestionStore::new(&clickhouse_config);
        let ingestor = CanonicalIngestor::new(
            &app_config,
            ingestion_config.clone(),
            store,
            probe.client_version,
        )?;
        Ok(Self {
            ingestor,
            ingestion_config,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Schema(#[from] SchemaError),
    #[error(transparent)]
    Probe(#[from] ProbeError),
    #[error(transparent)]
    Ingestion(#[from] IngestionError),
}
