use std::sync::Arc;

use crate::db::Client;
use anyhow::{Result, anyhow};
use tokio::sync::Semaphore;

use crate::config::AppConfig;
use crate::helper::tron::TronClient;
use crate::services::tron::tron_classifier::registry::ProtocolRegistry;

pub struct LoaderTron {
    pub clickhouse: Arc<Client>,
    pub tron_client: Arc<TronClient>,
    pub rpc_limiter: Arc<Semaphore>,
    pub config: Arc<AppConfig>,
    pub protocol_registry: Arc<ProtocolRegistry>,
    pub(crate) legacy_assets:
        tokio::sync::Mutex<std::collections::BTreeMap<String, serde_json::Value>>,
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
        let protocol_registry = Arc::new(
            ProtocolRegistry::load(&clickhouse, config.tron_protocol_registry_refresh_blocks)
                .await?,
        );

        Ok(Self {
            clickhouse: clickhouse.clone(),
            tron_client,
            rpc_limiter,
            protocol_registry,
            legacy_assets: Default::default(),
            config: Arc::new(config.clone()),
        })
    }
}
