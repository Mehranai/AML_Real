//! Regenerate or verify the Bitcoin adapter's reviewable migration artifact.
use anyhow::{Result, ensure};
fn main() -> Result<()> {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    if args == ["--help"] || args == ["-h"] {
        println!("Usage: cargo run --example export_sql -- [--check] [--upgrade] OUTPUT.sql");
        return Ok(());
    }
    let upgrade = args.iter().any(|arg| arg == "--upgrade");
    args.retain(|arg| arg != "--upgrade");
    let (check, path) = match args.as_slice() {
        [path] => (false, path),
        [flag, path] if flag == "--check" => (true, path),
        _ => anyhow::bail!("usage: export_sql [--check] OUTPUT.sql"),
    };
    let sql = if upgrade {
        aml_warehouse::semantics::upgrade_template()?
    } else {
        aml_warehouse::baseline_contract()?.sql_template()?
    };
    if check {
        ensure!(
            std::fs::read_to_string(path)?.replace("\r\n", "\n") == sql,
            "migration SQL differs from the shared contract"
        );
    } else {
        std::fs::write(path, sql)?;
    }
    Ok(())
}
