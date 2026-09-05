use clickhouse::Row;
use serde::Serialize;

pub trait BatchInsert: Row + Serialize + Clone + Send + Sync + 'static {
    const TABLE: &'static str;

    fn as_value(&self) -> Self::Value<'_>;
}
