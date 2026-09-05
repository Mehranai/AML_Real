use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use clickhouse::Client;
use serde::Serialize;
use tokio::sync::Mutex;
use tokio::time::sleep;

use super::traits::BatchInsert;

pub struct GenericBatcher<T>
where
    T: BatchInsert,
{
    clickhouse: Arc<Client>,
    rows: Arc<Mutex<Vec<T>>>,
    flush_lock: Mutex<()>,
    max_batch_size: usize,
    flush_interval: Duration,
}

impl<T> GenericBatcher<T>
where
    T: BatchInsert,
    for<'a> T::Value<'a>: Serialize + Send,
{
    pub fn create(
        clickhouse: Arc<Client>,
        max_batch_size: usize,
        flush_interval: Duration,
    ) -> Arc<Self> {
        let batcher = Arc::new(Self {
            clickhouse,
            rows: Arc::new(Mutex::new(Vec::new())),
            flush_lock: Mutex::new(()),
            max_batch_size,
            flush_interval,
        });

        Self::start_flush_task(batcher.clone());

        batcher
    }

    pub async fn push(&self, row: T) -> Result<()> {
        let should_flush = {
            let mut rows = self.rows.lock().await;
            rows.push(row);
            rows.len() >= self.max_batch_size
        };

        if should_flush {
            self.flush_pending().await
        } else {
            Ok(())
        }
    }

    pub async fn flush_all(&self) -> Result<()> {
        self.flush_pending().await
    }

    async fn flush_pending(&self) -> Result<()> {
        let _flush_guard = self.flush_lock.lock().await;

        let batch = {
            let rows = self.rows.lock().await;
            rows.clone()
        };

        if batch.is_empty() {
            return Ok(());
        }

        let row_count = batch.len();
        let mut insert = self.clickhouse.insert::<T>(T::TABLE).await?;

        for row in &batch {
            let value = row.as_value();
            insert.write(&value).await?;
        }

        insert.end().await?;

        let mut rows = self.rows.lock().await;
        rows.drain(..batch.len());

        println!("[CLICKHOUSE][{}] inserted {} row(s)", T::TABLE, row_count);

        Ok(())
    }

    fn start_flush_task(batcher: Arc<Self>) {
        tokio::spawn(async move {
            loop {
                sleep(batcher.flush_interval).await;

                if let Err(err) = batcher.flush_pending().await {
                    eprintln!("[BATCHER ERROR][{}] {:?}", T::TABLE, err);
                }
            }
        });
    }
}
