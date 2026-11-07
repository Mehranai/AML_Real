# Generated shared SQL

The six files in [migrations](migrations) are one reviewable export of the
immutable JSON/Rust contract used by TRON, Ethereum, BSC and Bitcoin. They are
not four per-network schema copies. Each network keeps its own database and
volume on its VM; `{{database}}` is a deployment parameter.

From the repository root:

```bash
cargo run --manifest-path shared/warehouse/Cargo.toml --example export_sql -- \
  --migrations shared/warehouse/sql/migrations
cargo run --manifest-path shared/warehouse/Cargo.toml --example export_sql -- \
  --check --migrations shared/warehouse/sql/migrations
```

`--check` rejects missing, modified or extra SQL files. The `sql_bundle` test
also checks this export. Edit the authoritative contract through an additive
migration, then regenerate; never hand-edit an applied phase. v1–v6 checksums
remain unchanged by DB-5.

Use the product schema commands (`tron_schema`, `ethereum_init_schema`,
`bsc_schema`, `bitcoin_init_schema`) to install/check a database. They generate
the same phases from the embedded shared definitions, validate network identity
and schema drift, and maintain the migration ledger. Executing these templates
manually does not replace that lifecycle.
