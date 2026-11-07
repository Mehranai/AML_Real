//! Export or verify SQL/JSON generated from the shared operational contract.
use anyhow::{Result, ensure};
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "export_sql [--check] [--upgrade|--core|--analytics|--visibility|--tron-evidence|--contract-json|--relationships|--migrations] OUTPUT\n--migrations exports/checks one shared directory containing every ordered migration."
        );
        return Ok(());
    }
    let mut check = false;
    let mut kind = "baseline";
    let mut path = None;
    for arg in &args {
        if arg == "--check" {
            ensure!(!check, "duplicate --check");
            check = true;
        } else if let Some(flag) = arg.strip_prefix("--") {
            ensure!(kind == "baseline", "choose one artifact type");
            ensure!(
                [
                    "upgrade",
                    "core",
                    "analytics",
                    "visibility",
                    "tron-evidence",
                    "contract-json",
                    "relationships",
                    "migrations"
                ]
                .contains(&flag),
                "unknown artifact type"
            );
            kind = flag;
        } else {
            ensure!(path.is_none(), "provide one output path");
            path = Some(arg);
        }
    }
    let path = path.ok_or_else(|| anyhow::anyhow!("output path required"))?;
    if kind == "migrations" {
        let directory = std::path::Path::new(path);
        let phases = aml_warehouse::migration::sql_templates()?;
        if check {
            ensure!(
                directory.is_dir(),
                "generated migration directory missing: {path}"
            );
        } else {
            std::fs::create_dir_all(directory)?;
        }
        for (name, sql) in &phases {
            let file = directory.join(name);
            if check {
                ensure!(
                    std::fs::read_to_string(&file)?.replace("\r\n", "\n") == *sql,
                    "generated migration is stale: {}",
                    file.display()
                );
            } else {
                std::fs::write(file, sql)?;
            }
        }
        let mut actual = Vec::new();
        for entry in std::fs::read_dir(directory)? {
            let file = entry?.path();
            if file.extension().is_some_and(|ext| ext == "sql") {
                actual.push(file.file_name().unwrap().to_string_lossy().into_owned());
            }
        }
        actual.sort();
        let mut expected = phases.into_iter().map(|(name, _)| name).collect::<Vec<_>>();
        expected.sort();
        ensure!(
            actual == expected,
            "unexpected SQL files in generated migration directory; remove retired files explicitly"
        );
        return Ok(());
    }
    let output = match kind {
        "baseline" => aml_warehouse::baseline_contract()?.sql_template()?,
        "upgrade" => aml_warehouse::semantics::upgrade_template()?,
        "core" => aml_warehouse::core_schema::sql_template()?,
        "analytics" => aml_warehouse::analytics_schema::sql_template()?,
        "visibility" => aml_warehouse::visibility_schema::sql_template()?,
        "tron-evidence" => aml_warehouse::tron_schema::sql_template()?,
        "contract-json" => format!(
            "{}\n",
            serde_json::to_string_pretty(&aml_warehouse::current_contract()?)?
        ),
        "relationships" => format!(
            "{}\n",
            aml_warehouse::relationships::query_template().trim_end()
        ),
        _ => unreachable!(),
    };
    if check {
        ensure!(
            std::fs::read_to_string(path)?.replace("\r\n", "\n") == output,
            "generated artifact is stale: {path}"
        );
    } else {
        std::fs::write(path, output)?;
    }
    Ok(())
}
