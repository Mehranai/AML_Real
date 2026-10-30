use super::*;
use crate::db::Database;
use anyhow::{Result, ensure};
use std::io::Read;
pub const USAGE: &str = "Usage: bitcoin_aml clusters ADDRESS\n       bitcoin_aml clusters inspect ADDRESS RUN_ID\n       bitcoin_aml clusters review ADDRESS FILE.json|-\n       bitcoin_aml clusters config\nReviews accept or reject investigative leads only. They never create ownership, labels or risk seeds. No RPC.";
pub async fn run(db: &Database, args: &[String]) -> Result<Value> {
    db.check_schema().await?;
    match args {
        [op] if op == "config" => Ok(config()),
        [address] => {
            let activity = crate::activity::read(db, address, &Default::default()).await?;
            compute(db, address, &activity).await
        }
        [op, address, id] if op == "inspect" => inspect(db, address, id).await,
        [op, address, path] if op == "review" => {
            let input: Box<dyn Read> = if path == "-" {
                Box::new(std::io::stdin())
            } else {
                Box::new(std::fs::File::open(path)?)
            };
            let mut data = vec![];
            input.take(65537).read_to_end(&mut data)?;
            ensure!(data.len() <= 65536, "review input exceeds 64 KiB");
            review(db, address, serde_json::from_slice(&data)?).await
        }
        _ => anyhow::bail!("{USAGE}"),
    }
}
