use crate::{model, rpc::Rpc};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use tokio::task::JoinSet;

/// Retain only a bounded window; completion order never determines commit order.
pub async fn blocks(
    rpc: &Rpc,
    start: u64,
    end: u64,
    limit: usize,
    concurrency: usize,
    verbosity: u64,
) -> Result<Vec<Value>> {
    ensure!(
        start <= end
            && end <= u32::MAX as u64
            && (1..=8).contains(&limit)
            && (1..=4).contains(&concurrency),
        "invalid block fetch bounds"
    );
    let last = end.min(start + limit as u64 - 1);
    let heights: Vec<u64> = (start..=last).collect();
    let hashes = rpc
        .batch(
            heights
                .iter()
                .map(|h| ("getblockhash", json!([h])))
                .collect(),
        )
        .await?
        .into_iter()
        .map(|v| model::hash(v.as_str().context("invalid block hash")?))
        .collect::<Result<Vec<_>>>()?;
    let mut fetched = Vec::with_capacity(heights.len());
    let mut retained_bytes = 0usize;
    let jobs: Vec<_> = heights
        .iter()
        .copied()
        .zip(hashes.iter().cloned())
        .collect();
    for chunk in jobs.chunks(concurrency) {
        let mut pending = JoinSet::new();
        for (height, hash) in chunk {
            let rpc = rpc.clone();
            let height = *height;
            let hash = hash.clone();
            pending.spawn(async move {
                Ok::<_, anyhow::Error>((height, hash.clone(), rpc.block(&hash, verbosity).await?))
            });
        }
        while let Some(result) = pending.join_next().await {
            let item = result.context("block fetch task stopped")??;
            let mut counter = ByteCounter(0);
            serde_json::to_writer(&mut counter, &item.2)?;
            retained_bytes = retained_bytes.saturating_add(counter.0);
            ensure!(
                retained_bytes <= 128 * 1024 * 1024,
                "block window exceeds 128 MiB JSON evidence budget; reduce block batch size"
            );
            fetched.push(item);
        }
    }
    fetched.sort_by_key(|(height, _, _)| *height);
    for (index, (height, hash, block)) in fetched.iter().enumerate() {
        ensure!(
            block["height"].as_u64() == Some(*height) && block["hash"].as_str() == Some(hash),
            "RPC block identity mismatch"
        );
        if index > 0 {
            ensure!(
                block["previousblockhash"] == fetched[index - 1].2["hash"],
                "non-contiguous RPC window"
            );
        }
    }
    let current = rpc
        .batch(
            heights
                .iter()
                .map(|h| ("getblockhash", json!([h])))
                .collect(),
        )
        .await?;
    ensure!(
        current
            .iter()
            .zip(&hashes)
            .all(|(a, b)| a.as_str() == Some(b)),
        "chain changed during block fetch; discard window"
    );
    Ok(fetched.into_iter().map(|(_, _, block)| block).collect())
}

struct ByteCounter(usize);
impl std::io::Write for ByteCounter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
