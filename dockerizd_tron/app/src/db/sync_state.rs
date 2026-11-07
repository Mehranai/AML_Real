use crate::db::Client;
pub async fn get_last_synced_block(client: &Client) -> anyhow::Result<Option<u64>> {
    let state =
        aml_warehouse::publication::state(client, client.database(), aml_warehouse::Network::Tron)
            .await?;
    Ok((state.initialized == 1).then_some(state.last_block_number))
}
