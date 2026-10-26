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
    Ok(format!("{}{}", baseline_sql()?, column_sql()?))
}
