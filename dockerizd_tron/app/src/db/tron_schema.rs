use crate::db::Client;
use anyhow::Result;
fn database(client: &Client) -> &str {
    if client.database().is_empty() {
        "tron_db"
    } else {
        client.database()
    }
}
pub async fn validate_tron_schema(client: &Client) -> Result<()> {
    let admin = client.clone().with_database("default");
    aml_warehouse::initialize(&admin, database(client), aml_warehouse::Network::Tron).await?;
    Ok(())
}
pub async fn check_tron_schema(client: &Client) -> Result<()> {
    let admin = client.clone().with_database("default");
    aml_warehouse::validate(&admin, database(client), aml_warehouse::Network::Tron).await?;
    Ok(())
}
