//! Regenerate or verify the Bitcoin adapter's reviewable migration artifact.
use anyhow::{Result, ensure};
fn main() -> Result<()> {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    if args == ["--help"] || args == ["-h"] {
        println!(
            "Usage: export_sql [--check] [--upgrade|--core|--analytics|--visibility|--contract-json|--analytics-contract-json|--core-contract-json|--relationships] OUTPUT"
        );
        return Ok(());
    }
    let upgrade = args.iter().any(|arg| arg == "--upgrade");
    let visibility = args.iter().any(|arg| arg == "--visibility");
    let analytics_json = args.iter().any(|arg| arg == "--analytics-contract-json");
    let relationships = args.iter().any(|arg| arg == "--relationships");
    let core = args.iter().any(|arg| arg == "--core");
    let analytics = args.iter().any(|arg| arg == "--analytics");
    let core_json = args.iter().any(|arg| arg == "--core-contract-json");
    let contract = args.iter().any(|arg| arg == "--contract-json");
    ensure!(
        usize::from(upgrade)
            + usize::from(visibility)
            + usize::from(analytics_json)
            + usize::from(core)
            + usize::from(contract)
            + usize::from(analytics)
            + usize::from(core_json)
            + usize::from(relationships)
            <= 1,
        "choose one artifact type"
    );
    args.retain(|arg| {
        !matches!(
            arg.as_str(),
            "--visibility"
                | "--analytics-contract-json"
                | "--upgrade"
                | "--core"
                | "--contract-json"
                | "--analytics"
                | "--core-contract-json"
                | "--relationships"
        )
    });
    let (check, path) = match args.as_slice() {
        [path] => (false, path),
        [flag, path] if flag == "--check" => (true, path),
        _ => anyhow::bail!("usage: export_sql [--check] OUTPUT.sql"),
    };
    let sql = if visibility {
        aml_warehouse::visibility_schema::sql_template()?
    } else if analytics_json {
        format!(
            "{}\n",
            serde_json::to_string_pretty(&aml_warehouse::analytics_contract()?)?
        )
    } else if relationships {
        aml_warehouse::relationships::query_template().to_string()
    } else if core_json {
        format!(
            "{}\n",
            serde_json::to_string_pretty(&aml_warehouse::core_contract()?)?
        )
    } else if analytics {
        aml_warehouse::analytics_schema::sql_template()?
    } else if contract {
        format!(
            "{}\n",
            serde_json::to_string_pretty(&aml_warehouse::current_contract()?)?
        )
    } else if core {
        aml_warehouse::core_schema::sql_template()?
    } else if upgrade {
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
