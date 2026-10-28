//! Reviewable SQL exported from the same contract used by all four networks.
pub fn baseline_sql() -> anyhow::Result<String> {
    let contract = aml_warehouse::baseline_contract()?;
    contract.statements_from_template("bitcoin_aml", super::migrations::BASELINE_SQL)?;
    Ok(super::migrations::BASELINE_SQL.replace("\r\n", "\n"))
}
pub fn column_sql() -> anyhow::Result<String> {
    aml_warehouse::semantics::upgrade_statements("bitcoin_aml", super::migrations::COLUMN_SQL)?;
    Ok(super::migrations::COLUMN_SQL.replace("\r\n", "\n"))
}
pub fn schema_sql() -> anyhow::Result<String> {
    Ok(format!(
        "{}{}{}{}{}",
        baseline_sql()?,
        column_sql()?,
        core_sql()?,
        analytics_sql()?,
        visibility_sql()?
    ))
}
pub fn core_sql() -> anyhow::Result<String> {
    aml_warehouse::core_schema::statements("bitcoin_aml", super::migrations::CORE_SQL)?;
    Ok(super::migrations::CORE_SQL.replace("\r\n", "\n"))
}
pub fn analytics_sql() -> anyhow::Result<String> {
    aml_warehouse::analytics_schema::statements("bitcoin_aml", super::migrations::ANALYTICS_SQL)?;
    Ok(super::migrations::ANALYTICS_SQL.replace("\r\n", "\n"))
}

pub fn visibility_sql() -> anyhow::Result<String> {
    aml_warehouse::visibility_schema::statements("bitcoin_aml", super::migrations::VISIBILITY_SQL)?;
    Ok(super::migrations::VISIBILITY_SQL.replace("\r\n", "\n"))
}
