# SQL بیت‌کوین

| مسیر | نقش | مصرف‌کننده |
|---|---|---|
| `migrations/20261022_0001_shared_warehouse.sql` | migration فعال نسخهٔ ۱، شامل هر ۲۹ جدول و ۲۰ view قرارداد مشترک | `src/db/migrations.rs` و `bitcoin_init_schema` |
| `migrations/20261026_0002_column_contract.sql` | ارتقای نسخهٔ ۲؛ ۲۱۷ قاعدهٔ مقدار در یک CHECK برای هر جدول، بدون تغییر ستون‌های پایه | همان schema job؛ بعد از preflight داده و با حفظ ledger نسخهٔ ۱ |
| `migrations/20261026_0003_core_evidence.sql` | نسخهٔ ۳؛ ۲۸ ستون حفظ شواهد و عملیات، گسترش viewهای پایه و current برای benchmark؛ نتیجه ۲۹ جدول، ۲۱ view و ۴۰۴ ستون جدول | همان schema job، به‌ترتیب پس از نسخهٔ ۲ |
| `migrations/20261027_0004_analytics_specialized.sql` | نسخهٔ ۴؛ ۹ جدول تخصصی/تحلیلی، ۱۱۰ ستون تکمیلی و ۱۹ view جدید؛ نتیجه ۳۸ جدول، ۴۰ view و ۶۶۸ ستون جدول | همان schema job، پس از نسخهٔ ۳؛ تاریخچهٔ legacy حذف نمی‌شود |
| `queries/relationships.sql` | قالب query مشترک DB-06؛ تطبیق leg بیت‌کوین با fact ورودی/خروجی canonical؛ DDL یا migration نیست | `shared/warehouse/src/relationships/query.sql` منبع اجراست؛ کپی adapter با exporter و تست کنترل می‌شود |
| `migrations/20261027_0005_visibility.sql` | نسخهٔ ۵؛ seal تحلیل‌ها، سه ستون provenance، canonical/visible؛ نتیجه ۳۹ جدول، ۵۳ view و ۶۸۲ ستون جدول | همان schema job پس از نسخهٔ ۴؛ داده و migration قدیمی حذف یا بازنویسی نمی‌شوند |
| `legacy/001_core.sql` | DDL نسخهٔ قدیمی برای fixture ارتقا و ممیزی | فقط تست‌های انتقال؛ نصب تازه اجرا نمی‌کند |

DDL پایه از `shared/warehouse/schema/0001_warehouse.json` و ارتقا از قواعد
`shared/warehouse/schema/0002_column_contract.json` و `0003_core_evidence.json` تولید می‌شوند.
این SQL کپی مستقلی برای طراحی schema نیست: هنگام اجرا و در تست‌ها باید دقیقاً
با خروجی قرارداد مشترک تطبیق داشته باشد. drift پیش از DDL رد می‌شود. نام
database با placeholder معتبر جایگزین می‌شود؛ دیتابیس هر شبکه مستقل است.
نسخه و checksum migration قبلی تغییر نکرده‌اند.

نسخهٔ ۴ از `shared/warehouse/schema/0004_analytics_specialized.json` تولید می‌شود:

```bash
cargo run --locked --manifest-path shared/warehouse/Cargo.toml --example export_sql -- --analytics dockerizd_bitcoin/sql/migrations/20261027_0004_analytics_specialized.sql
cargo run --locked --manifest-path shared/warehouse/Cargo.toml --example export_sql -- --check --analytics dockerizd_bitcoin/sql/migrations/20261027_0004_analytics_specialized.sql
```

تولید و کنترل نسخهٔ ۳:

```bash
cargo run --locked --manifest-path shared/warehouse/Cargo.toml --example export_sql -- --core dockerizd_bitcoin/sql/migrations/20261026_0003_core_evidence.sql
cargo run --locked --manifest-path shared/warehouse/Cargo.toml --example export_sql -- --check --core dockerizd_bitcoin/sql/migrations/20261026_0003_core_evidence.sql
```

از ریشهٔ مخزن، تولید و بررسی فایل:

```bash
cargo run --locked --manifest-path shared/warehouse/Cargo.toml --example export_sql -- \
  dockerizd_bitcoin/sql/migrations/20261022_0001_shared_warehouse.sql
cargo run --locked --manifest-path shared/warehouse/Cargo.toml --example export_sql -- \
  --check dockerizd_bitcoin/sql/migrations/20261022_0001_shared_warehouse.sql
cargo run --locked --manifest-path shared/warehouse/Cargo.toml --example export_sql -- \
  --upgrade --check dockerizd_bitcoin/sql/migrations/20261026_0002_column_contract.sql
```

مشاهدهٔ SQL بدون اتصال به ClickHouse:

```bash
bitcoin_init_schema --print-sql
# Compatible alias:
bitcoin_aml schema --print-sql
```

نصب را با schema job انجام دهید تا ledger، identity، drift و قفل writer هم
بررسی شوند. اجرای دستی فایل SQL جایگزین این کنترل‌ها نیست. migration بعدی باید
نسخهٔ تازه داشته باشد؛ SQL یا قرارداد اعمال‌شده را برای تغییر جدول ویرایش نکنید.
`--print-sql` هر پنج migration را چاپ می‌کند. قواعد نوع، واحد، nullable و مسیر
ارتقا در [DB-03](../../docs/warehouse/DB03_COLUMN_CONTRACT_FA.md) مستند شده‌اند.

خانواده‌های حاضر در baseline:

| خانواده | جدول‌ها |
|---|---|
| schema و checkpoint | `aml_schema_migrations`, `aml_warehouse_identity`, `aml_sync_state`, `aml_ingested_blocks` |
| شواهد پایه | `aml_transactions`, `aml_address_relationships`, `aml_transaction_features`, `aml_semantic_aml_events` |
| UTXO و خام شبکه‌ها | `aml_bitcoin_inputs`, `aml_bitcoin_outputs`, `aml_evm_logs`, `aml_tron_contracts` |
| دارایی و عملیات | `aml_asset_metadata`, `aml_metadata_jobs`, `aml_ingestion_failures`, `aml_ingestion_benchmarks` |
| intelligence | `aml_intelligence_sources`, `aml_intelligence_claims`, `aml_intelligence_reviews` |
| clustering | `aml_cluster_claims`, `aml_cluster_memberships`, `aml_cluster_runs` |
| exposure | `aml_exposure_seeds`, `aml_exposure_runs`, `aml_exposure_paths` |
| ریسک | `aml_risk_policies`, `aml_risk_policy_rules`, `aml_wallet_risk_signals`, `aml_wallet_risk_assessments` |

وجود این جدول‌ها به معنی فعال‌شدن تحلیل‌های مربوط نیست؛ writer و reader هر
خانواده در [بازبینی DB](../../docs/warehouse/DB_STRUCTURE_AUDIT_FA.md) مشخص شده‌اند.


کنترل قالب query ارتباط‌ها بدون اتصال به دیتابیس:

```bash
cargo run --locked --manifest-path shared/warehouse/Cargo.toml --example export_sql -- --relationships --check dockerizd_bitcoin/sql/queries/relationships.sql
```

این فایل placeholder دارد و مستقیماً اجرا نمی‌شود؛ reader مشترک شناسهٔ معتبر
دیتابیس، شبکه و وقوع دقیق تراکنش را به‌صورت کنترل‌شده جایگزین می‌کند.
DB-06 نسخهٔ ۴ را تغییر نداد؛ ارتقای visibility در migration مستقل نسخهٔ ۵ است.
جزئیات در [قرارداد روابط](../../docs/warehouse/DB06_RELATIONSHIPS_FA.md).

تولید/کنترل نسخهٔ ۵:

```bash
cargo run --locked --manifest-path shared/warehouse/Cargo.toml --example export_sql -- --visibility --check dockerizd_bitcoin/sql/migrations/20261027_0005_visibility.sql
```

[محدودهٔ DB-07 و قرارداد snapshot](../../docs/warehouse/DB07_VISIBILITY_FA.md).
