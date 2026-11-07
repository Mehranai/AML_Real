//! The one shipped SQL bundle must remain an exact export, never an editable fork.
use std::{collections::BTreeSet, path::Path};
#[test]
fn shipped_migrations_are_exact_and_have_no_retired_copies() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("sql/migrations");
    let generated = aml_warehouse::migration::sql_templates().unwrap();
    let actual = std::fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "sql"))
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual,
        generated.iter().map(|(name, _)| name.clone()).collect()
    );
    for (name, sql) in generated {
        assert_eq!(
            std::fs::read_to_string(directory.join(&name))
                .unwrap()
                .replace("\r\n", "\n"),
            sql,
            "stale generated migration: {name}"
        );
    }
}
