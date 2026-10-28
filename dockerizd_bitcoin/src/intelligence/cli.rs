use super::*;
use std::io::Read;

pub const USAGE: &str = "Usage: bitcoin_aml intelligence source FILE.json|- [--dry-run]\n       bitcoin_aml intelligence import FILE.jsonl|- [--dry-run]\n       bitcoin_aml intelligence review FILE.json|- [--dry-run]\n       bitcoin_aml intelligence claims ADDRESS\n       bitcoin_aml intelligence history CLAIM_ID\nImports are pending. A separate review with expected_revision is required. No RPC is accessed.";
fn input(path: &str) -> Result<String> {
    let reader: Box<dyn Read> = if path == "-" {
        Box::new(std::io::stdin())
    } else {
        Box::new(std::fs::File::open(path)?)
    };
    let mut bytes = Vec::new();
    reader.take(8 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 8 * 1024 * 1024,
        "intelligence input exceeds 8 MiB"
    );
    Ok(String::from_utf8(bytes)?)
}
pub async fn run(db: &Database, args: &[String]) -> Result<Value> {
    ensure!((2..=3).contains(&args.len()), "{USAGE}");
    let mutation = matches!(args[0].as_str(), "source" | "import" | "review");
    ensure!(
        args.len() == 2 || (mutation && args[2] == "--dry-run"),
        "{USAGE}"
    );
    if mutation {
        crate::config::required("BITCOIN_WRITER_LOCK_DIR")?;
    }
    db.check_schema().await?;
    let dry = args.len() == 3;
    match args[0].as_str() {
        "source" => register(db, serde_json::from_str(&input(&args[1])?)?, dry).await,
        "import" => {
            let content = input(&args[1])?;
            let rows = content
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(serde_json::from_str)
                .collect::<std::result::Result<Vec<Claim>, _>>()?;
            import(db, rows, dry).await
        }
        "review" => review(db, serde_json::from_str(&input(&args[1])?)?, dry).await,
        "claims" => {
            let r = read(db, &args[1]).await?;
            Ok(
                json!({"intelligence_snapshot":r.snapshot,"claims":r.claims,"entities":r.entities,"sources":r.sources}),
            )
        }
        "history" => history(db, &args[1]).await,
        _ => anyhow::bail!("{USAGE}"),
    }
}
