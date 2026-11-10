# فرهنگ جدول‌ها و ستون‌های ClickHouse

مرجع خواندن ستون‌های قرارداد مشترک نسخهٔ ۶. این فهرست ۴۲ جدول فیزیکی و ۷۳۹ ستون آن‌ها را پوشش می‌دهد. بخش viewها ۵۶ view و ۱٬۰۵۷ ستون خروجی آن‌ها را با ارجاع به جدول مرجع و ثبت تفاوت‌های نوع و ترتیب توضیح می‌دهد. نام، نوع و ترتیب با `system.columns` روی ClickHouse آزمایشی 23.8.16.16 و DDL همین مخزن کنترل شد.

برای روش خواندن مبلغ، NULL، JSON و مثال پرس‌وجو، [راهنمای پایگاه داده](../DATABASE_GUIDE_FA.md) را ببینید. پیش‌فرض ستون، قاعدهٔ SQL است. writer همچنان باید همهٔ داده‌ها و شواهد لازم را معتبر کند. `—` یعنی DDL پیش‌فرض صریحی تعریف نکرده است.

## فهرست جدول‌ها

| جدول | کار آن |
| --- | --- |
| [`aml_schema_migrations`](#aml_schema_migrations) | دفتر ثبت فازهای نصب ساختار و checksum هر فاز |
| [`aml_warehouse_identity`](#aml_warehouse_identity) | هویت شبکهٔ مجاز برای این دیتابیس و نسخه و checksum قرارداد |
| [`aml_sync_state`](#aml_sync_state) | تاریخچهٔ checkpoint و مرز انتشار قابل خواندن هر شبکه |
| [`aml_ingested_blocks`](#aml_ingested_blocks) | شاهد انتشار بلاک، وضعیت شاخه، پوشش و شمار ردیف‌های مورد انتظار |
| [`aml_transactions`](#aml_transactions) | رخداد تراکنش به ازای بلاک. برای بیت‌کوین جزئیات ورودی و خروجی در جدول تخصصی است |
| [`aml_address_relationships`](#aml_address_relationships) | پایه‌های انتقال و ارتباط میان سوژه‌های نوع‌دار |
| [`aml_transaction_features`](#aml_transaction_features) | ویژگی‌های استخراج‌شده از تراکنش برای تحلیل و تشخیص |
| [`aml_semantic_aml_events`](#aml_semantic_aml_events) | رویدادهای معنایی نسخه‌دار همراه با مدرک و نوع تشخیص |
| [`aml_asset_metadata`](#aml_asset_metadata) | کاتالوگ دارایی بومی و توکن با استاندارد، اعشار و منشأ |
| [`aml_metadata_jobs`](#aml_metadata_jobs) | صف پایدار کشف، تکمیل و به‌روزرسانی متادیتا |
| [`aml_ingestion_failures`](#aml_ingestion_failures) | ردیابی خطا، تعداد تلاش، retry و وضعیت رفع خطای ingest |
| [`aml_ingestion_benchmarks`](#aml_ingestion_benchmarks) | نتایج زمان، حجم و نرخ پردازش یک سنجش |
| [`aml_intelligence_sources`](#aml_intelligence_sources) | کاتالوگ منابع هوشمندی و وضعیت فعال، اعتماد و مجوز آن‌ها |
| [`aml_intelligence_claims`](#aml_intelligence_claims) | ادعاهای نسخه‌دار دربارهٔ آدرس‌ها، نهادها و برچسب‌ها با منشأ و وضعیت بازبینی |
| [`aml_intelligence_reviews`](#aml_intelligence_reviews) | تاریخچهٔ تصمیم بازبین برای نسخهٔ مشخص ادعا |
| [`aml_cluster_claims`](#aml_cluster_claims) | ادعاها و سرنخ‌های خوشه‌بندی با روش، مدرک و وضعیت بازبینی |
| [`aml_cluster_memberships`](#aml_cluster_memberships) | عضویت سوژه‌ها در نسخهٔ یک خوشه با ارجاع به ادعای پشتیبان |
| [`aml_cluster_runs`](#aml_cluster_runs) | نوبت‌های اجرای خوشه‌بندی، ورودی مرجع و وضعیت تکمیل |
| [`aml_exposure_seeds`](#aml_exposure_seeds) | آدرس‌ها یا شواهد تأییدشدهٔ مبنا برای تحلیل مواجهه |
| [`aml_exposure_runs`](#aml_exposure_runs) | وضعیت اجرا و پارامترهای تحلیل مواجهه |
| [`aml_exposure_paths`](#aml_exposure_paths) | مسیرهای مواجهه با ترتیب سوژه‌ها، مقدار و محدودیت انتساب |
| [`aml_risk_policies`](#aml_risk_policies) | تعریف نسخه‌های سیاست ارزیابی ریسک و آستانه‌ها |
| [`aml_risk_policy_rules`](#aml_risk_policy_rules) | قواعد و سقف مشارکت سیگنال در هر نسخهٔ سیاست |
| [`aml_wallet_risk_signals`](#aml_wallet_risk_signals) | سیگنال‌های مستند و سهمشان در یک ارزیابی ریسک |
| [`aml_wallet_risk_assessments`](#aml_wallet_risk_assessments) | خروجی ارزیابی ریسک به همراه سیاست، دلایل و محدودیت شواهد |
| [`aml_bitcoin_inputs`](#aml_bitcoin_inputs) | شواهد ورودی‌ها و prevout تراکنش‌های بیت‌کوین |
| [`aml_bitcoin_outputs`](#aml_bitcoin_outputs) | شواهد خروجی‌های بیت‌کوین همراه مقدار، اسکریپت و vout |
| [`aml_evm_logs`](#aml_evm_logs) | لاگ‌های اصلی رسید Ethereum و BSC با موقعیت واقعی |
| [`aml_tron_contracts`](#aml_tron_contracts) | قراردادهای تراکنش ترون، protobuf اصلی و پرچم‌های تطابق |
| [`aml_metadata_discoveries`](#aml_metadata_discoveries) | مدرک روی زنجیره‌ای کشف یک دارایی و ارتباط آن با صف متادیتا |
| [`aml_protocol_registry`](#aml_protocol_registry) | قراردادها و نقش‌های پروتکل با مدرک، مفسر و وضعیت بازبینی |
| [`aml_entities`](#aml_entities) | کاتالوگ نهادها و سرویس‌ها با منشأ و وضعیت ثبت‌شده |
| [`aml_entity_addresses`](#aml_entity_addresses) | نگاشت مستند آدرس‌ها به نهادها بر اساس ادعای پشتیبان |
| [`aml_cluster_versions`](#aml_cluster_versions) | فهرست نسخه‌های خوشه و علت تغییر هر نسخه |
| [`aml_address_exposures`](#aml_address_exposures) | خلاصهٔ مواجههٔ آدرس یا سوژه با مبنای ریسک |
| [`aml_bitcoin_scripts`](#aml_bitcoin_scripts) | شاهد تخصصی اسکریپت بیت‌کوین و آدرس قابل استخراج از آن |
| [`aml_evm_traces`](#aml_evm_traces) | موقعیت و شواهد فراخوانی داخلی EVM در صورت پوشش trace |
| [`aml_raw_transactions`](#aml_raw_transactions) | بدنهٔ اصلی تراکنش و رسید RPC در قالب قابل ممیزی |
| [`aml_analysis_publications`](#aml_analysis_publications) | مهر انتشار خروجی‌های تحلیل با نسخهٔ دقیق ورودی و شمار خروجی |
| [`aml_tron_logs`](#aml_tron_logs) | لاگ‌های رسید ترون با emitter، topic و موقعیت اصلی |
| [`aml_tron_internals`](#aml_tron_internals) | فراخوانی‌های داخلی ترون، وضعیت رد و دادهٔ اصلی ارزش فراخوانی |
| [`aml_tron_resources`](#aml_tron_resources) | فیلدهای گزارش‌شدهٔ Energy، Bandwidth و هزینه برای تراکنش ترون |

## Viewهای قابل خواندن

نام و معنی تمام ستون‌های هر view از جدول مرجع خوانده می‌شود. در ستون آخر فقط تفاوت‌های ساختاری ذکر شده‌اند. view از دادهٔ مشتق‌شدهٔ SQL خوانده می‌شود و پیش‌فرض نوشتن جدول به آن منتقل نمی‌شود.

| View | کار آن | جدول مرجع | تفاوت با جدول مرجع |
| --- | --- | --- | --- |
| `aml_sync_state_current` | آخرین ردیف کامل checkpoint برای هر شبکه | [`aml_sync_state`](#aml_sync_state) | — |
| `aml_ingested_blocks_canonical` | آخرین نشانگر بلاک کامل در دامنهٔ منتشرشده با احترام به rollback | [`aml_ingested_blocks`](#aml_ingested_blocks) | `ingestion_status` از نوع `String` |
| `aml_transactions_canonical` | شاهد بلاک canonical با تطابق هویت دقیق انتشار | [`aml_transactions`](#aml_transactions) | — |
| `aml_address_relationships_canonical` | شاهد بلاک canonical با تطابق هویت دقیق انتشار | [`aml_address_relationships`](#aml_address_relationships) | — |
| `aml_transaction_features_canonical` | شاهد بلاک canonical با تطابق هویت دقیق انتشار | [`aml_transaction_features`](#aml_transaction_features) | — |
| `aml_semantic_aml_events_canonical` | شاهد بلاک canonical با تطابق هویت دقیق انتشار | [`aml_semantic_aml_events`](#aml_semantic_aml_events) | — |
| `aml_bitcoin_inputs_canonical` | شاهد بلاک canonical با تطابق هویت دقیق انتشار | [`aml_bitcoin_inputs`](#aml_bitcoin_inputs) | — |
| `aml_bitcoin_outputs_canonical` | شاهد بلاک canonical با تطابق هویت دقیق انتشار | [`aml_bitcoin_outputs`](#aml_bitcoin_outputs) | — |
| `aml_evm_logs_canonical` | شاهد بلاک canonical با تطابق هویت دقیق انتشار | [`aml_evm_logs`](#aml_evm_logs) | — |
| `aml_tron_contracts_canonical` | شاهد بلاک canonical با تطابق هویت دقیق انتشار | [`aml_tron_contracts`](#aml_tron_contracts) | — |
| `aml_asset_metadata_current` | آخرین نسخهٔ کامل برای هر کلید منطقی | [`aml_asset_metadata`](#aml_asset_metadata) | `asset_standard` از نوع `String`، `review_status` از نوع `String`، `metadata_status` از نوع `String` |
| `aml_metadata_jobs_current` | آخرین نسخهٔ کامل برای هر کلید منطقی | [`aml_metadata_jobs`](#aml_metadata_jobs) | `status` از نوع `String` |
| `aml_ingestion_failures_current` | آخرین نسخهٔ کامل برای هر کلید منطقی | [`aml_ingestion_failures`](#aml_ingestion_failures) | — |
| `aml_intelligence_sources_current` | آخرین نسخهٔ کامل برای هر کلید منطقی | [`aml_intelligence_sources`](#aml_intelligence_sources) | — |
| `aml_intelligence_claims_current` | آخرین نسخهٔ کامل برای هر کلید منطقی | [`aml_intelligence_claims`](#aml_intelligence_claims) | — |
| `aml_cluster_claims_current` | آخرین نسخهٔ کامل برای هر کلید منطقی | [`aml_cluster_claims`](#aml_cluster_claims) | `heuristic` از نوع `String`، `review_status` از نوع `String` |
| `aml_cluster_runs_current` | آخرین نسخهٔ کامل برای هر کلید منطقی | [`aml_cluster_runs`](#aml_cluster_runs) | `status` از نوع `String` |
| `aml_exposure_seeds_current` | آخرین نسخهٔ کامل برای هر کلید منطقی | [`aml_exposure_seeds`](#aml_exposure_seeds) | — |
| `aml_exposure_runs_current` | آخرین نسخهٔ کامل برای هر کلید منطقی | [`aml_exposure_runs`](#aml_exposure_runs) | `status` از نوع `String` |
| `aml_wallet_risk_assessments_current` | آخرین نسخهٔ کامل برای هر کلید منطقی | [`aml_wallet_risk_assessments`](#aml_wallet_risk_assessments) | `risk_level` از نوع `String` |
| `aml_ingestion_benchmarks_current` | آخرین نسخهٔ کامل برای هر کلید منطقی | [`aml_ingestion_benchmarks`](#aml_ingestion_benchmarks) | — |
| `aml_metadata_discoveries_canonical` | شاهد بلاک canonical با تطابق هویت دقیق انتشار | [`aml_metadata_discoveries`](#aml_metadata_discoveries) | — |
| `aml_protocol_registry_current` | آخرین نسخهٔ کامل برای هر کلید منطقی | [`aml_protocol_registry`](#aml_protocol_registry) | — |
| `aml_entities_current` | آخرین نسخهٔ کامل برای هر کلید منطقی | [`aml_entities`](#aml_entities) | — |
| `aml_entity_addresses_current` | آخرین نسخهٔ کامل برای هر کلید منطقی | [`aml_entity_addresses`](#aml_entity_addresses) | — |
| `aml_cluster_versions_current` | آخرین نسخهٔ کامل برای هر کلید منطقی | [`aml_cluster_versions`](#aml_cluster_versions) | — |
| `aml_address_exposures_current` | آخرین نسخهٔ کامل برای هر کلید منطقی | [`aml_address_exposures`](#aml_address_exposures) | ترتیب خروجی `network_id`، `run_id`، `exposure_id`، `seed_id`، `source_subject_id`، `subject_id`، `hop_distance`، `exposure_score`، `path_count`، `last_tx_hash`، `last_seen_block`، `exposure_type`، `best_path_amount_share`، `best_path_time_weight`، `service_mediated`، `as_of_block`، `source_revision`، `evidence_refs`، `raw_evidence_json`، `revision`، `created_at_unix_ms` |
| `aml_bitcoin_scripts_canonical` | شاهد بلاک canonical با تطابق هویت دقیق انتشار | [`aml_bitcoin_scripts`](#aml_bitcoin_scripts) | — |
| `aml_evm_traces_canonical` | شاهد بلاک canonical با تطابق هویت دقیق انتشار | [`aml_evm_traces`](#aml_evm_traces) | — |
| `aml_raw_transactions_canonical` | شاهد بلاک canonical با تطابق هویت دقیق انتشار | [`aml_raw_transactions`](#aml_raw_transactions) | — |
| `aml_intelligence_reviews_current` | آخرین نسخهٔ کامل برای هر کلید منطقی | [`aml_intelligence_reviews`](#aml_intelligence_reviews) | — |
| `aml_cluster_memberships_current` | آخرین نسخهٔ کامل برای هر کلید منطقی | [`aml_cluster_memberships`](#aml_cluster_memberships) | `status` از نوع `String` |
| `aml_exposure_paths_current` | آخرین نسخهٔ کامل برای هر کلید منطقی | [`aml_exposure_paths`](#aml_exposure_paths) | `direction` از نوع `String`، `attribution_method` از نوع `String`، ترتیب خروجی `network_id`، `run_id`، `path_id`، `subject_id`، `seed_id`، `direction`، `hop_count`، `asset_id`، `amount_raw`، `amount_share`، `exposure_score`، `attribution_method`، `fund_continuity_claimed`، `path_subjects`، `relationship_ids`، `tx_hashes`، `evidence_refs`، `revision`، `created_at_unix_ms`، `seed_entity_id`، `seed_category`، `seed_risk_level`، `time_weight`، `service_mediated`، `service_entity_id`، `continuity_type`، `first_transfer_unix_ms`، `last_transfer_unix_ms`، `detector_version`، `allocation_json`، `evidence_json`، `raw_evidence_json` |
| `aml_risk_policies_current` | آخرین نسخهٔ کامل برای هر کلید منطقی | [`aml_risk_policies`](#aml_risk_policies) | — |
| `aml_risk_policy_rules_current` | آخرین نسخهٔ کامل برای هر کلید منطقی | [`aml_risk_policy_rules`](#aml_risk_policy_rules) | — |
| `aml_wallet_risk_signals_current` | آخرین نسخهٔ کامل برای هر کلید منطقی | [`aml_wallet_risk_signals`](#aml_wallet_risk_signals) | `signal_type` از نوع `String`، ترتیب خروجی `network_id`، `assessment_id`، `signal_id`، `subject_id`، `policy_version`، `signal_type`، `contribution`، `summary`، `evidence_refs`، `evidence_json`، `as_of_block`، `source_revision`، `revision`، `created_at_unix_ms`، `signal_strength`، `as_of_unix_ms`، `raw_evidence_json` |
| `aml_intelligence_claims_active` | ادعاهای تأییدشده در بازهٔ اعتبار و متکی به منبع فعال | [`aml_intelligence_claims`](#aml_intelligence_claims) | `claim_kind` از نوع `String`، `entity_type` از نوع `String`، `address_role` از نوع `String`، `seed_category` از نوع `String`، `review_status` از نوع `String` |
| `aml_protocol_registry_active` | پروتکل فعال و بازبینی‌شده از منبع فعال | [`aml_protocol_registry`](#aml_protocol_registry) | `subject_type` از نوع `String`، `review_status` از نوع `String` |
| `aml_entity_addresses_active` | آدرس نهاد با مدرک انتساب معتبر و بازبینی‌شده | [`aml_entity_addresses`](#aml_entity_addresses) | `review_status` از نوع `String` |
| `aml_exchange_addresses_active` | آدرس نهاد با همان قواعد اعتبار و محدود به ردهٔ صرافی | [`aml_entity_addresses`](#aml_entity_addresses) | `review_status` از نوع `String` |
| `aml_analysis_publications_current` | آخرین نسخهٔ کامل برای هر کلید منطقی | [`aml_analysis_publications`](#aml_analysis_publications) | `status` از نوع `String` |
| `aml_analysis_publications_visible` | خروجی مهرشده برای snapshot جاری و بلاک مرجع canonical | [`aml_analysis_publications`](#aml_analysis_publications) | `status` از نوع `String` |
| `aml_cluster_runs_visible` | خروجی مهرشده که شرط تازگی ورودی را دارد | [`aml_cluster_runs`](#aml_cluster_runs) | `status` از نوع `String` |
| `aml_cluster_claims_visible` | خروجی مهرشده که شرط تازگی ورودی را دارد | [`aml_cluster_claims`](#aml_cluster_claims) | `heuristic` از نوع `String`، `review_status` از نوع `String` |
| `aml_cluster_memberships_visible` | خروجی مهرشده که شرط تازگی ورودی را دارد | [`aml_cluster_memberships`](#aml_cluster_memberships) | `status` از نوع `String` |
| `aml_cluster_versions_visible` | خروجی مهرشده که شرط تازگی ورودی را دارد | [`aml_cluster_versions`](#aml_cluster_versions) | — |
| `aml_exposure_runs_visible` | خروجی مهرشده که شرط تازگی ورودی را دارد | [`aml_exposure_runs`](#aml_exposure_runs) | `status` از نوع `String` |
| `aml_exposure_paths_visible` | خروجی مهرشده که شرط تازگی ورودی را دارد | [`aml_exposure_paths`](#aml_exposure_paths) | `direction` از نوع `String`، `attribution_method` از نوع `String` |
| `aml_address_exposures_visible` | خروجی مهرشده که شرط تازگی ورودی را دارد | [`aml_address_exposures`](#aml_address_exposures) | — |
| `aml_wallet_risk_assessments_visible` | خروجی مهرشده که شرط تازگی ورودی را دارد | [`aml_wallet_risk_assessments`](#aml_wallet_risk_assessments) | `risk_level` از نوع `String` |
| `aml_wallet_risk_signals_visible` | خروجی مهرشده که شرط تازگی ورودی را دارد | [`aml_wallet_risk_signals`](#aml_wallet_risk_signals) | `signal_type` از نوع `String` |
| `aml_asset_metadata_visible` | متادیتای معتبر. شاهد روی زنجیره باید با انتشار و هش مرجع سازگار باشد | [`aml_asset_metadata`](#aml_asset_metadata) | `asset_standard` از نوع `String`، `review_status` از نوع `String`، `metadata_status` از نوع `String` |
| `aml_metadata_jobs_visible` | کارهای صف متادیتا که شرط مرز منبع و کشف معتبر را دارند | [`aml_metadata_jobs`](#aml_metadata_jobs) | `status` از نوع `String` |
| `aml_tron_logs_canonical` | شاهد بلاک canonical با تطابق هویت دقیق انتشار | [`aml_tron_logs`](#aml_tron_logs) | — |
| `aml_tron_internals_canonical` | شاهد بلاک canonical با تطابق هویت دقیق انتشار | [`aml_tron_internals`](#aml_tron_internals) | — |
| `aml_tron_resources_canonical` | شاهد بلاک canonical با تطابق هویت دقیق انتشار | [`aml_tron_resources`](#aml_tron_resources) | — |

## جزئیات جدول‌های فیزیکی

<a id="aml_schema_migrations"></a>
### `aml_schema_migrations`

دفتر ثبت فازهای نصب ساختار و checksum هر فاز.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, migration_id)`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `migration_id` | `String` | — | شناسهٔ فاز نصب تغییرات ساختار |
| `schema_version` | `UInt32` | — | شمارهٔ نسخهٔ قرارداد نصب‌شده |
| `checksum` | `String` | — | هش محتوای migration یا کل قرارداد که برای کنترل تطابق استفاده می‌شود |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |

<a id="aml_warehouse_identity"></a>
### `aml_warehouse_identity`

هویت شبکهٔ مجاز برای این دیتابیس و نسخه و checksum قرارداد.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id)`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `schema_version` | `UInt32` | — | شمارهٔ نسخهٔ قرارداد نصب‌شده |
| `checksum` | `String` | — | هش محتوای migration یا کل قرارداد که برای کنترل تطابق استفاده می‌شود |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |

<a id="aml_sync_state"></a>
### `aml_sync_state`

تاریخچهٔ checkpoint و مرز انتشار قابل خواندن هر شبکه.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id)`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `revision` | `UInt64` | — | نسخهٔ checkpoint منتشرشده، مبنای سنجش تازگی خروجی‌ها |
| `initialized` | `UInt8` | — | آیا دامنهٔ منتشرشدهٔ بلاک وجود دارد. صفر یعنی cursor هنوز بلاکی را منتشر نکرده است |
| `start_block` | `UInt64` | — | ارتفاع آغاز دامنهٔ مشاهده یا اجرای benchmark |
| `last_block_number` | `UInt64` | — | ارتفاع آخرین بلاک منتشرشده در checkpoint |
| `last_block_hash` | `String` | — | هش آخرین بلاک منتشرشده در checkpoint |
| `observed_rpc_height` | `UInt64` | — | ارتفاع head که آخرین بار از RPC مشاهده شده است |
| `published_at_unix_ms` | `UInt64` | — | زمان ثبت انتشار، UTC و میلی‌ثانیه |
| `source_cursor_json` | `String` | `DEFAULT '{}'` | اطلاعات تکمیلی cursor و مشخصات منبع هنگام انتشار |

<a id="aml_ingested_blocks"></a>
### `aml_ingested_blocks`

شاهد انتشار بلاک، وضعیت شاخه، پوشش و شمار ردیف‌های مورد انتظار.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, block_number, revision)`.
تقسیم‌بندی داده با `intDiv(block_number, 100000)` انجام می‌شود.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `block_number` | `UInt64` | — | ارتفاع بلاک مربوط به رکورد |
| `block_hash` | `String` | — | هش بلاکی که این رخداد در آن مشاهده شده است. شمارهٔ بلاک به‌تنهایی برای تشخیص شاخه کافی نیست |
| `block_timestamp_unix_ms` | `UInt64` | — | زمان اعلام‌شده در بلاک، UTC بر حسب میلی‌ثانیه |
| `commit_id` | `String` | — | شناسهٔ انتشار این نوبتِ بلاک که رکوردها را به بستهٔ دقیق همان بلاک متصل می‌کند. با Git commit فرق دارد |
| `revision` | `UInt64` | — | نسخهٔ نشانگر بلاک و شناسهٔ مشترک با ردیف‌های همین نوبت انتشار |
| `parent_hash` | `String` | — | هش بلاک قبلی برای بررسی پیوستگی شاخه |
| `canonical` | `UInt8` | — | پرچم انتخاب این نسخهٔ بلاک برای شاخهٔ معتبر در فرایند انتشار |
| `ingestion_status` | `LowCardinality(String)` | — | مرحلهٔ ثبت نسخهٔ بلاک، مثل processing یا complete، طبق انتشار |
| `transaction_count` | `UInt32` | — | شمار تراکنش‌های بلاک طبق دادهٔ ورودی |
| `receipt_coverage` | `Nullable(UInt8)` | — | شاخص ثبت‌شدهٔ پوشش رسیدهای بلاک، در صورت کاربرد |
| `trace_coverage` | `Nullable(UInt8)` | — | شاخص پوشش trace ثبت‌شده برای بلاک. وجود بلاک معادل وجود trace نیست |
| `prevout_coverage` | `Nullable(UInt8)` | — | شاخص پوشش مشاهدهٔ خروجی‌های قبلی بیت‌کوین در بلاک، در صورت کاربرد |
| `source_id` | `String` | — | شناسهٔ منبع داده، مانند آداپتور یا خوراک ثبت‌شده |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |
| `expected_counts_json` | `String` | `DEFAULT '{}'` | شمار ردیف‌های مورد انتظار هر خانواده برای کنترل کامل‌بودن انتشار بلاک |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |

<a id="aml_transactions"></a>
### `aml_transactions`

رخداد تراکنش به ازای بلاک. برای بیت‌کوین جزئیات ورودی و خروجی در جدول تخصصی است.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, block_number, block_hash, commit_id, tx_hash)`.
تقسیم‌بندی داده با `intDiv(block_number, 100000)` انجام می‌شود.
ایندکس‌های کمکی `idx_tx_hash`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `block_number` | `UInt64` | — | ارتفاع بلاک مربوط به رکورد |
| `block_hash` | `String` | — | هش بلاکی که این رخداد در آن مشاهده شده است. شمارهٔ بلاک به‌تنهایی برای تشخیص شاخه کافی نیست |
| `block_timestamp_unix_ms` | `UInt64` | — | زمان اعلام‌شده در بلاک، UTC بر حسب میلی‌ثانیه |
| `commit_id` | `String` | — | شناسهٔ انتشار این نوبتِ بلاک که رکوردها را به بستهٔ دقیق همان بلاک متصل می‌کند. با Git commit فرق دارد |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `tx_hash` | `String` | — | هش تراکنش. برای رخداد دقیق، همراه شناسهٔ شبکه، بلاک و موقعیت رویداد خوانده می‌شود |
| `transaction_index` | `UInt32` | — | موقعیت تراکنش در بلاک |
| `from_address` | `Nullable(String)` | — | آدرس مبدأ در صورت وجود. برای پایهٔ غیرآدرسی یا تراکنش بیت‌کوین می‌تواند NULL باشد |
| `to_address` | `Nullable(String)` | — | آدرس مقصد در صورت وجود. برای پایهٔ غیرآدرسی یا تراکنش بیت‌کوین می‌تواند NULL باشد |
| `amount_raw` | `Nullable(UInt256)` | — | مقدار خام تراکنش اگر در مدل شبکه تعریف شده باشد. تراکنش بیت‌کوین مقدار واحدِ انتساب‌یافته به یک فرستنده ندارد |
| `fee_raw` | `UInt256` | — | کارمزد در کوچک‌ترین واحد ارز بومی شبکه، در صورت معلوم‌بودن |
| `asset_id` | `String` | — | شناسهٔ استاندارد دارایی در شبکه، مانند native:BTC یا شناسهٔ وابسته به قرارداد توکن |
| `decimals` | `UInt8` | — | تعداد رقم اعشار دارایی برای نمایش مقدار خام. NULL یعنی مقدار معتبر در دسترس نیست |
| `status` | `LowCardinality(String)` | — | نتیجهٔ تراکنش طبق مدل شبکه، مثل success، failed یا unknown در صورت کاربرد |
| `coinbase` | `UInt8` | — | پرچم تراکنش ایجاد پاداش بلاک بیت‌کوین یا خروجی متعلق به آن |
| `input_count` | `Nullable(UInt32)` | — | تعداد ورودی‌های تراکنش، در شبکه و پاسخ مربوط |
| `output_count` | `Nullable(UInt32)` | — | تعداد خروجی‌های تراکنش، در شبکه و پاسخ مربوط |
| `raw_evidence_json` | `String` | — | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |

<a id="aml_address_relationships"></a>
### `aml_address_relationships`

پایه‌های انتقال و ارتباط میان سوژه‌های نوع‌دار.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, block_number, block_hash, commit_id, relationship_id)`.
تقسیم‌بندی داده با `intDiv(block_number, 100000)` انجام می‌شود.
ایندکس‌های کمکی `idx_from_subject_id`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `block_number` | `UInt64` | — | ارتفاع بلاک مربوط به رکورد |
| `block_hash` | `String` | — | هش بلاکی که این رخداد در آن مشاهده شده است. شمارهٔ بلاک به‌تنهایی برای تشخیص شاخه کافی نیست |
| `block_timestamp_unix_ms` | `UInt64` | — | زمان اعلام‌شده در بلاک، UTC بر حسب میلی‌ثانیه |
| `commit_id` | `String` | — | شناسهٔ انتشار این نوبتِ بلاک که رکوردها را به بستهٔ دقیق همان بلاک متصل می‌کند. با Git commit فرق دارد |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `relationship_id` | `String` | — | شناسهٔ پایدار پایهٔ انتقال یا شاهد رابطه |
| `tx_hash` | `String` | — | هش تراکنش. برای رخداد دقیق، همراه شناسهٔ شبکه، بلاک و موقعیت رویداد خوانده می‌شود |
| `transaction_index` | `UInt32` | — | موقعیت تراکنش در بلاک |
| `event_index` | `UInt32` | — | موقعیت رویداد یا پایهٔ انتقال در تراکنش طبق مدل آن شبکه |
| `from_subject_id` | `String` | — | شناسهٔ سوژهٔ مبدأ پایهٔ انتقال |
| `from_subject_type` | `LowCardinality(String)` | — | نوع سوژهٔ مبدأ، مانند address، transaction یا script |
| `to_subject_id` | `String` | — | شناسهٔ سوژهٔ مقصد پایهٔ انتقال |
| `to_subject_type` | `LowCardinality(String)` | — | نوع سوژهٔ مقصد، مانند address، transaction یا script |
| `from_address` | `Nullable(String)` | — | آدرس مبدأ در صورت وجود. برای پایهٔ غیرآدرسی یا تراکنش بیت‌کوین می‌تواند NULL باشد |
| `to_address` | `Nullable(String)` | — | آدرس مقصد در صورت وجود. برای پایهٔ غیرآدرسی یا تراکنش بیت‌کوین می‌تواند NULL باشد |
| `asset_id` | `String` | — | شناسهٔ استاندارد دارایی در شبکه، مانند native:BTC یا شناسهٔ وابسته به قرارداد توکن |
| `amount_raw` | `UInt256` | — | مقدار صحیح در کوچک‌ترین واحد دارایی. تقسیم بر ۱۰ به توان decimals مقدار قابل نمایش را می‌دهد |
| `decimals` | `Nullable(UInt8)` | — | تعداد رقم اعشار دارایی برای نمایش مقدار خام. NULL یعنی مقدار معتبر در دسترس نیست |
| `transfer_type` | `LowCardinality(String)` | — | نوع پایه یا جریان، مثل انتقال بومی، رویداد توکن یا پایهٔ ورودی/خروجی UTXO |
| `attribution_method` | `LowCardinality(String)` | — | نام قاعده یا روش اتصال مقدار به شاهد، مثل observed_prevout در بیت‌کوین |
| `confidence` | `Float64` | — | وزن اطمینان روش یا منبع به این شاهد. معادل احتمال پول‌شویی نیست |
| `prev_tx_hash` | `Nullable(String)` | — | هش تراکنشی که خروجی قبلی بیت‌کوین را ایجاد کرده است |
| `prev_output_index` | `Nullable(UInt32)` | — | شمارهٔ خروجی قبلی بیت‌کوین که این ورودی به آن اشاره می‌کند |
| `output_index` | `Nullable(UInt32)` | — | شمارهٔ vout برای پایهٔ خروجی بیت‌کوین. برای ورودی، prev_output_index را بخوانید |
| `evidence_refs` | `Array(String)` | — | فهرست شناسه‌ها یا ارجاع‌ها به شواهد پشتیبان. همهٔ آن‌ها کلید خارجی SQL نیستند |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |
| `event_sub_index` | `UInt32` | `DEFAULT 0` | موقعیت فرعی در یک رویداد، مثلاً چند قلم یک لاگ گروهی. برای پایه‌های بیت‌کوین معمولاً صفر |
| `trace_address` | `Array(UInt32)` | `DEFAULT []` | ترتیب شمارهٔ فرزندها از ریشهٔ trace تا فراخوانی موردنظر |
| `token_id` | `Nullable(UInt256)` | `DEFAULT NULL` | شناسهٔ صحیح NFT یا قلم چندتوکنی، در صورت وجود |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |

<a id="aml_transaction_features"></a>
### `aml_transaction_features`

ویژگی‌های استخراج‌شده از تراکنش برای تحلیل و تشخیص.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, block_number, block_hash, commit_id, feature_id)`.
تقسیم‌بندی داده با `intDiv(block_number, 100000)` انجام می‌شود.
ایندکس‌های کمکی `idx_subject_id`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `block_number` | `UInt64` | — | ارتفاع بلاک مربوط به رکورد |
| `block_hash` | `String` | — | هش بلاکی که این رخداد در آن مشاهده شده است. شمارهٔ بلاک به‌تنهایی برای تشخیص شاخه کافی نیست |
| `block_timestamp_unix_ms` | `UInt64` | — | زمان اعلام‌شده در بلاک، UTC بر حسب میلی‌ثانیه |
| `commit_id` | `String` | — | شناسهٔ انتشار این نوبتِ بلاک که رکوردها را به بستهٔ دقیق همان بلاک متصل می‌کند. با Git commit فرق دارد |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `feature_id` | `String` | — | شناسهٔ مجموعهٔ ویژگی‌های یک رخداد تراکنش |
| `tx_hash` | `String` | — | هش تراکنش. برای رخداد دقیق، همراه شناسهٔ شبکه، بلاک و موقعیت رویداد خوانده می‌شود |
| `subject_id` | `String` | — | شناسهٔ سوژهٔ تحلیل، رجیستری یا بازبینی، همراه با دامنهٔ شبکه |
| `detector_version` | `String` | — | نسخهٔ دقیق الگوریتم تشخیص یا تحلیل |
| `features_json` | `String` | — | ویژگی‌های استخراج‌شدهٔ رفتاری یا شبکه‌ای به شکل JSON |
| `evidence_refs` | `Array(String)` | — | فهرست شناسه‌ها یا ارجاع‌ها به شواهد پشتیبان. همهٔ آن‌ها کلید خارجی SQL نیستند |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |

<a id="aml_semantic_aml_events"></a>
### `aml_semantic_aml_events`

رویدادهای معنایی نسخه‌دار همراه با مدرک و نوع تشخیص.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, block_number, block_hash, commit_id, event_id)`.
تقسیم‌بندی داده با `intDiv(block_number, 100000)` انجام می‌شود.
ایندکس‌های کمکی `idx_subject_id`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `block_number` | `UInt64` | — | ارتفاع بلاک مربوط به رکورد |
| `block_hash` | `String` | — | هش بلاکی که این رخداد در آن مشاهده شده است. شمارهٔ بلاک به‌تنهایی برای تشخیص شاخه کافی نیست |
| `block_timestamp_unix_ms` | `UInt64` | — | زمان اعلام‌شده در بلاک، UTC بر حسب میلی‌ثانیه |
| `commit_id` | `String` | — | شناسهٔ انتشار این نوبتِ بلاک که رکوردها را به بستهٔ دقیق همان بلاک متصل می‌کند. با Git commit فرق دارد |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `event_id` | `String` | — | شناسهٔ پایدار رخداد معنایی در دامنهٔ همان شاهد |
| `tx_hash` | `String` | — | هش تراکنش. برای رخداد دقیق، همراه شناسهٔ شبکه، بلاک و موقعیت رویداد خوانده می‌شود |
| `subject_id` | `String` | — | شناسهٔ سوژهٔ تحلیل، رجیستری یا بازبینی، همراه با دامنهٔ شبکه |
| `event_type` | `LowCardinality(String)` | — | نوع رویداد شناسایی‌شده، مثل انتقال NFT یا تعامل پروتکل |
| `protocol` | `String` | — | نام یا شناسهٔ پروتکل مربوط به رویداد یا رجیستری |
| `detector_version` | `String` | — | نسخهٔ دقیق الگوریتم تشخیص یا تحلیل |
| `confidence` | `Float64` | — | وزن اطمینان روش یا منبع به این شاهد. معادل احتمال پول‌شویی نیست |
| `asset_in` | `Nullable(String)` | — | شناسهٔ دارایی سمت ورودی رویداد |
| `asset_out` | `Nullable(String)` | — | شناسهٔ دارایی سمت خروجی رویداد |
| `amount_in` | `Nullable(UInt256)` | — | مقدار خام دارایی ورودی رویداد |
| `amount_out` | `Nullable(UInt256)` | — | مقدار خام دارایی خروجی رویداد |
| `remote_network_id` | `Nullable(String)` | — | شناسهٔ شبکهٔ طرف مقابل، در صورت وجود شاهد معتبر یا پیکربندی رجیستری |
| `evidence_refs` | `Array(String)` | — | فهرست شناسه‌ها یا ارجاع‌ها به شواهد پشتیبان. همهٔ آن‌ها کلید خارجی SQL نیستند |
| `evidence_json` | `String` | — | جزئیات دلیل، ورودی‌ها، محدودیت یا شاهد تحلیل به صورت JSON |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |
| `event_index` | `UInt32` | `DEFAULT 0` | موقعیت رویداد یا پایهٔ انتقال در تراکنش طبق مدل آن شبکه |
| `event_sub_index` | `UInt32` | `DEFAULT 0` | موقعیت فرعی در یک رویداد، مثلاً چند قلم یک لاگ گروهی. برای پایه‌های بیت‌کوین معمولاً صفر |
| `detector` | `String` | `DEFAULT ''` | نام یا شناسهٔ تولیدکنندهٔ رویداد معنایی |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |

<a id="aml_asset_metadata"></a>
### `aml_asset_metadata`

کاتالوگ دارایی بومی و توکن با استاندارد، اعشار و منشأ.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, asset_id)`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `asset_id` | `String` | — | شناسهٔ استاندارد دارایی در شبکه، مانند native:BTC یا شناسهٔ وابسته به قرارداد توکن |
| `symbol` | `String` | — | نماد متنی دارایی، مانند BTC |
| `name` | `String` | — | نام قابل نمایش دارایی |
| `decimals` | `Nullable(UInt8)` | — | تعداد رقم اعشار دارایی برای نمایش مقدار خام. NULL یعنی مقدار معتبر در دسترس نیست |
| `asset_standard` | `LowCardinality(String)` | — | استاندارد دارایی مثل native، ERC20 یا TRC20 بر اساس شاهد موجود |
| `source_id` | `String` | — | شناسهٔ منبع داده، مانند آداپتور یا خوراک ثبت‌شده |
| `source_reference` | `String` | — | ارجاع دقیق به رکورد یا مدرک در منبع |
| `review_status` | `LowCardinality(String)` | — | آخرین وضعیت بررسی‌شدهٔ مدرک، مثل pending یا approved طبق گردش همان نوع رکورد |
| `reviewed_by` | `String` | — | شناسهٔ بازبین ثبت‌شده |
| `evidence_refs` | `Array(String)` | — | فهرست شناسه‌ها یا ارجاع‌ها به شواهد پشتیبان. همهٔ آن‌ها کلید خارجی SQL نیستند |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |
| `total_supply_raw` | `Nullable(UInt256)` | `DEFAULT NULL` | کل عرضهٔ گزارش‌شده در واحد خام دارایی، در صورت مشاهده |
| `code_hash` | `Nullable(String)` | `DEFAULT NULL` | هش بایت‌کد قرارداد توکن، در صورت مشاهده |
| `is_verified` | `Nullable(UInt8)` | `DEFAULT NULL` | علامت تأیید متادیتا طبق منبع و وضعیت ثبت‌شده |
| `metadata_status` | `LowCardinality(String)` | `DEFAULT 'unknown'` | وضعیت پوشش یا تکمیل متادیتا طبق کارگر شبکه |
| `observed_block` | `Nullable(UInt64)` | `DEFAULT NULL` | ارتفاع بلاک متادیتای مشاهده‌شده |
| `verified_at_unix_ms` | `Nullable(UInt64)` | `DEFAULT NULL` | زمان تأیید متادیتا طبق ثبت منبع، UTC و میلی‌ثانیه |
| `reviewed_at_unix_ms` | `Nullable(UInt64)` | `DEFAULT NULL` | زمان انجام بازبینی، UTC و میلی‌ثانیه |
| `updated_at_unix_ms` | `Nullable(UInt64)` | `DEFAULT NULL` | زمان آخرین تغییر ثبت‌شدهٔ این نسخه، UTC و میلی‌ثانیه |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |
| `source_revision` | `Nullable(UInt64)` | `DEFAULT NULL` | شمارهٔ انتشار منبع داده‌ای که ردیف تحلیل یا متادیتا بر آن بنا شده است |
| `observed_block_hash` | `Nullable(String)` | `DEFAULT NULL` | هش بلاک مشاهدهٔ متادیتا. برای انتساب به شاخه، همراه observed_block استفاده می‌شود |

<a id="aml_metadata_jobs"></a>
### `aml_metadata_jobs`

صف پایدار کشف، تکمیل و به‌روزرسانی متادیتا.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, job_id)`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `job_id` | `String` | — | شناسهٔ کار صف متادیتا |
| `asset_id` | `String` | — | شناسهٔ استاندارد دارایی در شبکه، مانند native:BTC یا شناسهٔ وابسته به قرارداد توکن |
| `status` | `LowCardinality(String)` | — | وضعیت چرخهٔ عمر رکورد یا نتیجهٔ اجرا. مقادیر دقیق تابع جدول و نوع رکورد است |
| `attempts` | `UInt32` | — | تعداد تلاش‌های ثبت‌شده برای کار یا بازیابی خطا |
| `next_attempt_unix_ms` | `UInt64` | — | نخستین زمان مجاز تلاش بعدی، UTC و میلی‌ثانیه |
| `last_error` | `String` | — | متن آخرین خطای صف متادیتا |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |
| `discovery_id` | `Nullable(String)` | `DEFAULT NULL` | شناسهٔ شاهد کشف دارایی یا ارجاع کار متادیتا به آن |
| `discovered_block` | `Nullable(UInt64)` | `DEFAULT NULL` | ارتفاعی که سرنخ کشف دارایی در آن ثبت شده است |
| `source_revision` | `Nullable(UInt64)` | `DEFAULT NULL` | شمارهٔ انتشار منبع داده‌ای که ردیف تحلیل یا متادیتا بر آن بنا شده است |
| `last_error_class` | `String` | `DEFAULT ''` | دستهٔ آخرین خطای صف متادیتا، برای تفکیک دلیل retry یا توقف |
| `started_at_unix_ms` | `Nullable(UInt64)` | `DEFAULT NULL` | زمان شروع کار یا تحلیل، UTC و میلی‌ثانیه |
| `completed_at_unix_ms` | `Nullable(UInt64)` | `DEFAULT NULL` | زمان ثبت پایان کار به UTC و میلی‌ثانیه |
| `updated_at_unix_ms` | `Nullable(UInt64)` | `DEFAULT NULL` | زمان آخرین تغییر ثبت‌شدهٔ این نسخه، UTC و میلی‌ثانیه |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |

<a id="aml_ingestion_failures"></a>
### `aml_ingestion_failures`

ردیابی خطا، تعداد تلاش، retry و وضعیت رفع خطای ingest.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, failure_id)`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `failure_id` | `String` | — | شناسهٔ پایدار رخداد خطای دریافت یا انتشار |
| `block_number` | `UInt64` | — | ارتفاع بلاک مربوط به رکورد |
| `block_hash` | `Nullable(String)` | — | هش بلاکی که این رخداد در آن مشاهده شده است. شمارهٔ بلاک به‌تنهایی برای تشخیص شاخه کافی نیست |
| `operation` | `LowCardinality(String)` | — | نام عملیات ingestion یا maintenance که خطا در آن رخ داده است |
| `status` | `LowCardinality(String)` | — | وضعیت چرخهٔ عمر رکورد یا نتیجهٔ اجرا. مقادیر دقیق تابع جدول و نوع رکورد است |
| `attempts` | `UInt32` | — | تعداد تلاش‌های ثبت‌شده برای کار یا بازیابی خطا |
| `error_code` | `String` | — | کد دسته‌بندی خطای دریافت یا پردازش |
| `error_message` | `String` | — | شرح ثبت‌شدهٔ خطای اجرا یا دریافت |
| `next_attempt_unix_ms` | `UInt64` | — | نخستین زمان مجاز تلاش بعدی، UTC و میلی‌ثانیه |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |
| `tx_hash` | `Nullable(String)` | `DEFAULT NULL` | هش تراکنش. برای رخداد دقیق، همراه شناسهٔ شبکه، بلاک و موقعیت رویداد خوانده می‌شود |
| `retryable` | `Nullable(UInt8)` | `DEFAULT NULL` | پرچم امکان تلاش مجدد طبق طبقه‌بندی خطا |
| `first_failed_at_unix_ms` | `Nullable(UInt64)` | `DEFAULT NULL` | زمان نخستین خطای ثبت‌شده، UTC و میلی‌ثانیه |
| `last_failed_at_unix_ms` | `Nullable(UInt64)` | `DEFAULT NULL` | زمان آخرین خطای ثبت‌شده، UTC و میلی‌ثانیه |
| `resolved_at_unix_ms` | `Nullable(UInt64)` | `DEFAULT NULL` | زمان ثبت رفع یا بسته‌شدن خطای دریافت، UTC و میلی‌ثانیه |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |

<a id="aml_ingestion_benchmarks"></a>
### `aml_ingestion_benchmarks`

نتایج زمان، حجم و نرخ پردازش یک سنجش.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, benchmark_id)`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `benchmark_id` | `String` | — | شناسهٔ اجرای سنجش کارایی |
| `start_block` | `UInt64` | — | ارتفاع آغاز دامنهٔ مشاهده یا اجرای benchmark |
| `end_block` | `UInt64` | — | آخرین ارتفاع بازهٔ سنجش |
| `blocks` | `UInt64` | — | شمار بلاک‌های پردازش‌شده در سنجش |
| `transactions` | `UInt64` | — | تعداد تراکنش‌های پردازش‌شده در سنجش |
| `rows_written` | `UInt64` | — | شمار ردیف‌های نوشته‌شده در اجرای سنجش |
| `elapsed_ms` | `UInt64` | — | مدت اجرای اندازه‌گیری بر حسب میلی‌ثانیه |
| `bytes_written` | `UInt64` | — | حجم بایت ثبت‌شده برای خروجی اجرای سنجش، در صورت اندازه‌گیری |
| `settings_json` | `String` | — | تنظیمات و پارامترهای اجرای سنجش، تحلیل یا سیاست به صورت JSON |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |
| `source_kind` | `LowCardinality(String)` | `DEFAULT 'unknown'` | نوع منبع ورودی اجرای benchmark |
| `status` | `LowCardinality(String)` | `DEFAULT 'unknown'` | وضعیت چرخهٔ عمر رکورد یا نتیجهٔ اجرا. مقادیر دقیق تابع جدول و نوع رکورد است |
| `started_at_unix_ms` | `Nullable(UInt64)` | `DEFAULT NULL` | زمان شروع کار یا تحلیل، UTC و میلی‌ثانیه |
| `completed_at_unix_ms` | `Nullable(UInt64)` | `DEFAULT NULL` | زمان ثبت پایان کار به UTC و میلی‌ثانیه |
| `metrics_json` | `String` | `DEFAULT '{}'` | شاخص‌های سنجش اضافی به صورت JSON |
| `error_message` | `String` | `DEFAULT ''` | شرح ثبت‌شدهٔ خطای اجرا یا دریافت |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |
| `blocks_per_second` | `Nullable(Float64)` | `DEFAULT NULL` | نرخ پردازش بلاک در اجرای سنجش |
| `transactions_per_second` | `Nullable(Float64)` | `DEFAULT NULL` | نرخ پردازش تراکنش در سنجش |
| `rows_per_second` | `Nullable(Float64)` | `DEFAULT NULL` | نرخ ثبت ردیف‌ها در سنجش کارایی |

<a id="aml_intelligence_sources"></a>
### `aml_intelligence_sources`

کاتالوگ منابع هوشمندی و وضعیت فعال، اعتماد و مجوز آن‌ها.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, source_id)`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `source_id` | `String` | — | شناسهٔ منبع داده، مانند آداپتور یا خوراک ثبت‌شده |
| `source_name` | `String` | — | نام قابل خواندن منبع هوشمندی |
| `source_type` | `LowCardinality(String)` | — | نوع منبع هوشمندی، مانند خوراک بیرونی یا ثبت داخلی |
| `reference_url` | `String` | — | نشانی مرجع منبع هوشمندی، در صورت وجود |
| `trust_tier` | `LowCardinality(String)` | — | ردهٔ اعتماد تعریف‌شده برای منبع هوشمندی |
| `license` | `String` | — | اطلاعات مجوز استفاده از دادهٔ منبع |
| `is_active` | `UInt8` | — | وضعیت فعال‌بودن رکورد، که view ممکن است اعتبار منبع و ادعا را هم کنترل کند |
| `created_by` | `String` | — | شناسه یا نام ثبت‌شدهٔ ایجادکنندهٔ رکورد |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |
| `metadata_json` | `String` | `DEFAULT '{}'` | اطلاعات اضافی منبع هوشمندی به صورت JSON |
| `updated_at_unix_ms` | `Nullable(UInt64)` | `DEFAULT NULL` | زمان آخرین تغییر ثبت‌شدهٔ این نسخه، UTC و میلی‌ثانیه |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |

<a id="aml_intelligence_claims"></a>
### `aml_intelligence_claims`

ادعاهای نسخه‌دار دربارهٔ آدرس‌ها، نهادها و برچسب‌ها با منشأ و وضعیت بازبینی.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, claim_id)`.
تقسیم‌بندی داده با `intDiv(created_at_unix_ms, 2592000000)` انجام می‌شود.
ایندکس‌های کمکی `idx_address`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `claim_id` | `String` | — | شناسهٔ ادعای انتساب یا خوشه‌بندی که این ردیف تعریف می‌کند یا به آن استناد دارد |
| `address` | `String` | — | آدرس استاندارد شبکه. در رکورد اسکریپت، فقط وقتی اسکریپت آدرس قابل استخراج داشته باشد |
| `claim_kind` | `LowCardinality(String)` | — | نوع ادعای هوشمندی، مثلاً انتساب آدرس یا برچسب ریسک |
| `entity_id` | `String` | — | شناسهٔ نهاد یا سرویس در کاتالوگ هوشمندی |
| `entity_name` | `String` | — | نام قابل نمایش نهاد یا سرویس |
| `entity_type` | `LowCardinality(String)` | — | ردهٔ نهاد، مثل صرافی یا سرویس |
| `address_role` | `LowCardinality(String)` | — | نقش گزارش‌شدهٔ آدرس یا عضو، مثل نقش مرتبط با یک سرویس |
| `confidence` | `Float64` | — | وزن اطمینان روش یا منبع به این شاهد. معادل احتمال پول‌شویی نیست |
| `risk_level` | `UInt8` | — | طبقهٔ ریسک گزارش‌شده. در رکورد هوشمندی برچسب منبع و در ارزیابی خروجی سیاست است |
| `is_exposure_seed` | `UInt8` | — | علامت نامزد استفاده از این انتساب به عنوان مبنای مواجهه |
| `seed_category` | `LowCardinality(String)` | — | ردهٔ برچسب یا منبع ریسک که تحلیل مواجهه از آن آغاز شده است |
| `source_id` | `String` | — | شناسهٔ منبع داده، مانند آداپتور یا خوراک ثبت‌شده |
| `source_reference` | `String` | — | ارجاع دقیق به رکورد یا مدرک در منبع |
| `evidence_refs` | `Array(String)` | — | فهرست شناسه‌ها یا ارجاع‌ها به شواهد پشتیبان. همهٔ آن‌ها کلید خارجی SQL نیستند |
| `review_status` | `LowCardinality(String)` | — | آخرین وضعیت بررسی‌شدهٔ مدرک، مثل pending یا approved طبق گردش همان نوع رکورد |
| `reviewed_by` | `String` | — | شناسهٔ بازبین ثبت‌شده |
| `review_note` | `String` | — | متن توضیح بازبین دربارهٔ ادعا یا وضعیت |
| `valid_from_unix_ms` | `UInt64` | — | شروع اعتبار زمانی ادعا، UTC و میلی‌ثانیه |
| `valid_to_unix_ms` | `Nullable(UInt64)` | — | پایان اعتبار زمانی ادعا، UTC و میلی‌ثانیه. NULL در صورت بدون پایان مشخص |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |
| `source_record_id` | `Nullable(String)` | `DEFAULT NULL` | شناسهٔ رکورد در منبع اولیه، برای تطبیق با ورودی و deduplication |
| `supersedes_claim_id` | `Nullable(String)` | `DEFAULT NULL` | ارجاع به ادعای قبلی که ادعای جدید جای آن ثبت شده است |
| `submitted_by` | `Nullable(String)` | `DEFAULT NULL` | شناسهٔ ثبت‌کنندهٔ ادعای هوشمندی |
| `case_id` | `Nullable(String)` | `DEFAULT NULL` | شناسهٔ پروندهٔ مرتبط با ادعا یا بازبینی، در صورت وجود |
| `reviewed_at_unix_ms` | `Nullable(UInt64)` | `DEFAULT NULL` | زمان انجام بازبینی، UTC و میلی‌ثانیه |
| `evidence_json` | `String` | `DEFAULT '{}'` | جزئیات دلیل، ورودی‌ها، محدودیت یا شاهد تحلیل به صورت JSON |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |

<a id="aml_intelligence_reviews"></a>
### `aml_intelligence_reviews`

تاریخچهٔ تصمیم بازبین برای نسخهٔ مشخص ادعا.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, review_id)`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `review_id` | `String` | — | شناسهٔ رویداد بازبینی یا ارجاع به آن |
| `subject_type` | `LowCardinality(String)` | — | نوع سوژهٔ رجیستری یا بازبینی |
| `subject_id` | `String` | — | شناسهٔ سوژهٔ تحلیل، رجیستری یا بازبینی، همراه با دامنهٔ شبکه |
| `decision` | `LowCardinality(String)` | — | نتیجهٔ ثبت‌شدهٔ بازبینی، مانند تأیید یا رد طبق گردش همان ادعا |
| `reviewed_by` | `String` | — | شناسهٔ بازبین ثبت‌شده |
| `reason` | `String` | — | دلیل تصمیم بازبین |
| `evidence_refs` | `Array(String)` | — | فهرست شناسه‌ها یا ارجاع‌ها به شواهد پشتیبان. همهٔ آن‌ها کلید خارجی SQL نیستند |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |
| `subject_revision` | `Nullable(UInt64)` | `DEFAULT NULL` | نسخهٔ دقیق ادعا یا سوژه‌ای که بازبینی به آن اعمال شده است |
| `supersedes_review_id` | `Nullable(String)` | `DEFAULT NULL` | ارجاع به بازبینی قبلی که بازبینی جدید جای آن ثبت شده است |
| `case_id` | `Nullable(String)` | `DEFAULT NULL` | شناسهٔ پروندهٔ مرتبط با ادعا یا بازبینی، در صورت وجود |
| `evidence_json` | `String` | `DEFAULT '{}'` | جزئیات دلیل، ورودی‌ها، محدودیت یا شاهد تحلیل به صورت JSON |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |

<a id="aml_cluster_claims"></a>
### `aml_cluster_claims`

ادعاها و سرنخ‌های خوشه‌بندی با روش، مدرک و وضعیت بازبینی.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, claim_id)`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `claim_id` | `String` | — | شناسهٔ ادعای انتساب یا خوشه‌بندی که این ردیف تعریف می‌کند یا به آن استناد دارد |
| `cluster_id` | `String` | — | شناسهٔ خوشهٔ تحلیلی، در محدودهٔ شبکه |
| `subject_id` | `String` | — | شناسهٔ سوژهٔ تحلیل، رجیستری یا بازبینی، همراه با دامنهٔ شبکه |
| `heuristic` | `LowCardinality(String)` | — | نام قاعدهٔ اکتشافی خوشه‌بندی، مانند الگوی ورودی مشترک |
| `detector_version` | `String` | — | نسخهٔ دقیق الگوریتم تشخیص یا تحلیل |
| `confidence` | `Float64` | — | وزن اطمینان روش یا منبع به این شاهد. معادل احتمال پول‌شویی نیست |
| `review_status` | `LowCardinality(String)` | — | آخرین وضعیت بررسی‌شدهٔ مدرک، مثل pending یا approved طبق گردش همان نوع رکورد |
| `source_id` | `String` | — | شناسهٔ منبع داده، مانند آداپتور یا خوراک ثبت‌شده |
| `evidence_refs` | `Array(String)` | — | فهرست شناسه‌ها یا ارجاع‌ها به شواهد پشتیبان. همهٔ آن‌ها کلید خارجی SQL نیستند |
| `evidence_json` | `String` | — | جزئیات دلیل، ورودی‌ها، محدودیت یا شاهد تحلیل به صورت JSON |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |
| `left_subject_id` | `Nullable(String)` | `DEFAULT NULL` | سوژهٔ اولِ ادعای جفتی خوشه‌بندی |
| `right_subject_id` | `Nullable(String)` | `DEFAULT NULL` | سوژهٔ دوم ادعای جفتی خوشه‌بندی |
| `same_entity` | `Nullable(UInt8)` | `DEFAULT NULL` | محتوای ادعای جفتی دربارهٔ تعلق دو سوژه به یک نهاد. اعتبار بازبینی جدا بررسی می‌شود |
| `run_id` | `Nullable(String)` | `DEFAULT NULL` | شناسهٔ نوبت اجرای تحلیل که ردیف‌های وابسته را به هم وصل می‌کند |
| `source_record_id` | `Nullable(String)` | `DEFAULT NULL` | شناسهٔ رکورد در منبع اولیه، برای تطبیق با ورودی و deduplication |
| `supersedes_claim_id` | `Nullable(String)` | `DEFAULT NULL` | ارجاع به ادعای قبلی که ادعای جدید جای آن ثبت شده است |
| `created_by` | `Nullable(String)` | `DEFAULT NULL` | شناسه یا نام ثبت‌شدهٔ ایجادکنندهٔ رکورد |
| `reviewed_by` | `Nullable(String)` | `DEFAULT NULL` | شناسهٔ بازبین ثبت‌شده |
| `review_id` | `Nullable(String)` | `DEFAULT NULL` | شناسهٔ رویداد بازبینی یا ارجاع به آن |
| `cluster_type` | `String` | `DEFAULT ''` | نوع خوشه و شیوهٔ تفسیر عضویت آن |
| `address_role` | `String` | `DEFAULT ''` | نقش گزارش‌شدهٔ آدرس یا عضو، مثل نقش مرتبط با یک سرویس |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |

<a id="aml_cluster_memberships"></a>
### `aml_cluster_memberships`

عضویت سوژه‌ها در نسخهٔ یک خوشه با ارجاع به ادعای پشتیبان.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, cluster_id, cluster_version, subject_id)`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `cluster_id` | `String` | — | شناسهٔ خوشهٔ تحلیلی، در محدودهٔ شبکه |
| `cluster_version` | `String` | — | نسخهٔ خوشه‌ای که ردیف عضویت یا نسخه‌نامه به آن تعلق دارد |
| `subject_id` | `String` | — | شناسهٔ سوژهٔ تحلیل، رجیستری یا بازبینی، همراه با دامنهٔ شبکه |
| `claim_id` | `String` | — | شناسهٔ ادعای انتساب یا خوشه‌بندی که این ردیف تعریف می‌کند یا به آن استناد دارد |
| `status` | `LowCardinality(String)` | — | وضعیت چرخهٔ عمر رکورد یا نتیجهٔ اجرا. مقادیر دقیق تابع جدول و نوع رکورد است |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |
| `membership_id` | `Nullable(String)` | `DEFAULT NULL` | شناسهٔ رکورد عضویت نسخه‌دار در خوشه |
| `run_id` | `Nullable(String)` | `DEFAULT NULL` | شناسهٔ نوبت اجرای تحلیل که ردیف‌های وابسته را به هم وصل می‌کند |
| `entity_id` | `Nullable(String)` | `DEFAULT NULL` | شناسهٔ نهاد یا سرویس در کاتالوگ هوشمندی |
| `membership_type` | `String` | `DEFAULT ''` | نوع عضویت و نحوهٔ تفسیر آن |
| `confidence` | `Nullable(Float64)` | `DEFAULT NULL` | وزن اطمینان روش یا منبع به این شاهد. معادل احتمال پول‌شویی نیست |
| `review_id` | `Nullable(String)` | `DEFAULT NULL` | شناسهٔ رویداد بازبینی یا ارجاع به آن |
| `cluster_type` | `String` | `DEFAULT ''` | نوع خوشه و شیوهٔ تفسیر عضویت آن |
| `address_role` | `String` | `DEFAULT ''` | نقش گزارش‌شدهٔ آدرس یا عضو، مثل نقش مرتبط با یک سرویس |
| `evidence_json` | `String` | `DEFAULT '{}'` | جزئیات دلیل، ورودی‌ها، محدودیت یا شاهد تحلیل به صورت JSON |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |

<a id="aml_cluster_runs"></a>
### `aml_cluster_runs`

نوبت‌های اجرای خوشه‌بندی، ورودی مرجع و وضعیت تکمیل.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, run_id)`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `run_id` | `String` | — | شناسهٔ نوبت اجرای تحلیل که ردیف‌های وابسته را به هم وصل می‌کند |
| `status` | `LowCardinality(String)` | — | وضعیت چرخهٔ عمر رکورد یا نتیجهٔ اجرا. مقادیر دقیق تابع جدول و نوع رکورد است |
| `as_of_block` | `UInt64` | — | ارتفاع مرجع شواهد مورد استفادهٔ تحلیل یا انتشار |
| `source_revision` | `UInt64` | — | شمارهٔ انتشار منبع داده‌ای که ردیف تحلیل یا متادیتا بر آن بنا شده است |
| `detector_version` | `String` | — | نسخهٔ دقیق الگوریتم تشخیص یا تحلیل |
| `cursor_json` | `String` | — | جزئیات نقطهٔ پردازش یا وضعیت ادامهٔ اجرای تحلیل به شکل JSON |
| `started_at_unix_ms` | `UInt64` | — | زمان شروع کار یا تحلیل، UTC و میلی‌ثانیه |
| `completed_at_unix_ms` | `Nullable(UInt64)` | — | زمان ثبت پایان کار به UTC و میلی‌ثانیه |
| `truncated` | `UInt8` | — | پرچم رسیدن اجرا به حد جست‌وجو یا حجم خروجی |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |
| `entity_memberships` | `Nullable(UInt64)` | `DEFAULT NULL` | تعداد عضویت‌های مرتبط با نهاد که اجرای خوشه‌بندی گزارش کرده است |
| `control_claims` | `Nullable(UInt64)` | `DEFAULT NULL` | تعداد ادعاهای مربوط به کنترل یا پیوند اکتشافی که اجرا گزارش کرده است |
| `error_message` | `String` | `DEFAULT ''` | شرح ثبت‌شدهٔ خطای اجرا یا دریافت |
| `settings_json` | `String` | `DEFAULT '{}'` | تنظیمات و پارامترهای اجرای سنجش، تحلیل یا سیاست به صورت JSON |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |

<a id="aml_exposure_seeds"></a>
### `aml_exposure_seeds`

آدرس‌ها یا شواهد تأییدشدهٔ مبنا برای تحلیل مواجهه.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, seed_id)`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `seed_id` | `String` | — | شناسهٔ نقطهٔ شروع تحلیل مواجهه |
| `address` | `String` | — | آدرس استاندارد شبکه. در رکورد اسکریپت، فقط وقتی اسکریپت آدرس قابل استخراج داشته باشد |
| `claim_id` | `String` | — | شناسهٔ ادعای انتساب یا خوشه‌بندی که این ردیف تعریف می‌کند یا به آن استناد دارد |
| `category` | `LowCardinality(String)` | — | ردهٔ منبع مواجهه، مثل دستهٔ یک برچسب ریسک |
| `is_active` | `UInt8` | — | وضعیت فعال‌بودن رکورد، که view ممکن است اعتبار منبع و ادعا را هم کنترل کند |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |
| `entity_id` | `Nullable(String)` | `DEFAULT NULL` | شناسهٔ نهاد یا سرویس در کاتالوگ هوشمندی |
| `entity_name` | `Nullable(String)` | `DEFAULT NULL` | نام قابل نمایش نهاد یا سرویس |
| `entity_type` | `Nullable(String)` | `DEFAULT NULL` | ردهٔ نهاد، مثل صرافی یا سرویس |
| `risk_level` | `Nullable(UInt8)` | `DEFAULT NULL` | طبقهٔ ریسک گزارش‌شده. در رکورد هوشمندی برچسب منبع و در ارزیابی خروجی سیاست است |
| `source_id` | `Nullable(String)` | `DEFAULT NULL` | شناسهٔ منبع داده، مانند آداپتور یا خوراک ثبت‌شده |
| `claim_revision` | `Nullable(UInt64)` | `DEFAULT NULL` | شمارهٔ نسخهٔ همان ادعای پشتیبان که هنگام تولید شاهد به کار رفته است |
| `evidence_refs` | `Array(String)` | `DEFAULT []` | فهرست شناسه‌ها یا ارجاع‌ها به شواهد پشتیبان. همهٔ آن‌ها کلید خارجی SQL نیستند |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |

<a id="aml_exposure_runs"></a>
### `aml_exposure_runs`

وضعیت اجرا و پارامترهای تحلیل مواجهه.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, run_id)`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `run_id` | `String` | — | شناسهٔ نوبت اجرای تحلیل که ردیف‌های وابسته را به هم وصل می‌کند |
| `seed_id` | `String` | — | شناسهٔ نقطهٔ شروع تحلیل مواجهه |
| `status` | `LowCardinality(String)` | — | وضعیت چرخهٔ عمر رکورد یا نتیجهٔ اجرا. مقادیر دقیق تابع جدول و نوع رکورد است |
| `as_of_block` | `UInt64` | — | ارتفاع مرجع شواهد مورد استفادهٔ تحلیل یا انتشار |
| `source_revision` | `UInt64` | — | شمارهٔ انتشار منبع داده‌ای که ردیف تحلیل یا متادیتا بر آن بنا شده است |
| `policy_version` | `String` | — | شناسهٔ نسخهٔ سیاستی که محاسبه یا ذخیره با آن انجام شده است |
| `max_hops` | `UInt8` | — | بیشترین تعداد قدمی که اجرای مواجهه اجازه داده است |
| `started_at_unix_ms` | `UInt64` | — | زمان شروع کار یا تحلیل، UTC و میلی‌ثانیه |
| `completed_at_unix_ms` | `Nullable(UInt64)` | — | زمان ثبت پایان کار به UTC و میلی‌ثانیه |
| `truncated` | `UInt8` | — | پرچم رسیدن اجرا به حد جست‌وجو یا حجم خروجی |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |
| `seed_ids` | `Array(String)` | `DEFAULT []` | فهرست شناسه‌های مبنای ریسک استفاده‌شده در یک اجرا |
| `seed_snapshot_json` | `String` | `DEFAULT '{}'` | فهرست و وضعیت مبناهای مواجهه در لحظهٔ اجرا، برای بازتولید و ممیزی |
| `detector_version` | `Nullable(String)` | `DEFAULT NULL` | نسخهٔ دقیق الگوریتم تشخیص یا تحلیل |
| `hop_decay` | `Nullable(Float64)` | `DEFAULT NULL` | پارامتر کاهش وزن با افزایش قدم‌های مسیر |
| `time_half_life_days` | `Nullable(Float64)` | `DEFAULT NULL` | نیمه‌عمر زمانی وزن مواجهه بر حسب روز، طبق تنظیم اجرای تحلیل |
| `max_paths_per_subject` | `Nullable(UInt32)` | `DEFAULT NULL` | سقف تعداد مسیر بررسی یا نگه‌داری‌شده برای هر سوژه |
| `seed_count` | `Nullable(UInt64)` | `DEFAULT NULL` | تعداد seedهای استفاده‌شده در اجرای مواجهه |
| `path_count` | `Nullable(UInt64)` | `DEFAULT NULL` | تعداد مسیرهای مشاهده یا ثبت‌شده در اجرا یا خلاصه |
| `settings_json` | `String` | `DEFAULT '{}'` | تنظیمات و پارامترهای اجرای سنجش، تحلیل یا سیاست به صورت JSON |
| `error_message` | `String` | `DEFAULT ''` | شرح ثبت‌شدهٔ خطای اجرا یا دریافت |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |

<a id="aml_exposure_paths"></a>
### `aml_exposure_paths`

مسیرهای مواجهه با ترتیب سوژه‌ها، مقدار و محدودیت انتساب.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, run_id, path_id)`.
ایندکس‌های کمکی `idx_subject_id`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `path_id` | `String` | — | شناسهٔ مسیر مواجهه |
| `run_id` | `String` | — | شناسهٔ نوبت اجرای تحلیل که ردیف‌های وابسته را به هم وصل می‌کند |
| `subject_id` | `String` | — | شناسهٔ سوژهٔ تحلیل، رجیستری یا بازبینی، همراه با دامنهٔ شبکه |
| `seed_id` | `String` | — | شناسهٔ نقطهٔ شروع تحلیل مواجهه |
| `direction` | `LowCardinality(String)` | — | جهت مسیر نسبت به سوژه و منبع ریسک. برای تفسیر ورودی و خروجی به روش تحلیل نگاه کنید |
| `hop_count` | `UInt8` | — | تعداد قدم‌های مسیر ثبت‌شده |
| `asset_id` | `String` | — | شناسهٔ استاندارد دارایی در شبکه، مانند native:BTC یا شناسهٔ وابسته به قرارداد توکن |
| `amount_raw` | `UInt256` | — | مقدار صحیح در کوچک‌ترین واحد دارایی. تقسیم بر ۱۰ به توان decimals مقدار قابل نمایش را می‌دهد |
| `amount_share` | `Float64` | — | سهم مقداری مسیر طبق روش تحلیل. شاخص مدل است و سهم قطعی مالکیت را تعیین نمی‌کند |
| `exposure_score` | `Float64` | — | امتیاز مواجهه طبق روش تحلیل |
| `attribution_method` | `LowCardinality(String)` | — | نام قاعده یا روش اتصال مقدار به شاهد، مثل observed_prevout در بیت‌کوین |
| `fund_continuity_claimed` | `UInt8` | — | پرچم اینکه روش تحلیل ادعای تداوم همان وجوه دارد. برای مسیرهای فاقد شاهد مستقیم نباید به مالکیت یا تخصیص تعبیر شود |
| `path_subjects` | `Array(String)` | — | ترتیب سوژه‌هایی که مسیر از آن‌ها عبور می‌کند |
| `relationship_ids` | `Array(String)` | — | فهرست مرتب ارجاع‌ها به رابطه‌هایی که یک مسیر را تشکیل می‌دهند |
| `tx_hashes` | `Array(String)` | — | فهرست هش تراکنش‌های استفاده‌شده در مسیر مواجهه |
| `evidence_refs` | `Array(String)` | — | فهرست شناسه‌ها یا ارجاع‌ها به شواهد پشتیبان. همهٔ آن‌ها کلید خارجی SQL نیستند |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |
| `seed_entity_id` | `Nullable(String)` | `DEFAULT NULL` | شناسهٔ نهاد متناظر با seed، در صورت وجود |
| `seed_category` | `Nullable(String)` | `DEFAULT NULL` | ردهٔ برچسب یا منبع ریسک که تحلیل مواجهه از آن آغاز شده است |
| `seed_risk_level` | `Nullable(UInt8)` | `DEFAULT NULL` | سطح ریسک ثبت‌شدهٔ seed هنگام اجرای تحلیل |
| `time_weight` | `Nullable(Float64)` | `DEFAULT NULL` | وزن زمانی انتقال یا مسیر طبق مدل کاهش اثر با زمان |
| `service_mediated` | `Nullable(UInt8)` | `DEFAULT NULL` | پرچم اینکه مسیر به واسطهٔ یک سرویس شناسایی‌شده عبور می‌کند |
| `service_entity_id` | `Nullable(String)` | `DEFAULT NULL` | شناسهٔ سرویس واسطه در مسیر، اگر با شواهد ثبت شده باشد |
| `continuity_type` | `Nullable(String)` | `DEFAULT NULL` | نوع پیوندی که مسیر ادعا می‌کند، همراه با محدودیت‌های مدل گردش وجه |
| `first_transfer_unix_ms` | `Nullable(UInt64)` | `DEFAULT NULL` | زمان نخستین انتقال مستند در مسیر، UTC و میلی‌ثانیه |
| `last_transfer_unix_ms` | `Nullable(UInt64)` | `DEFAULT NULL` | زمان آخرین انتقال مستند در مسیر، UTC و میلی‌ثانیه |
| `detector_version` | `Nullable(String)` | `DEFAULT NULL` | نسخهٔ دقیق الگوریتم تشخیص یا تحلیل |
| `allocation_json` | `String` | `DEFAULT '{}'` | جزئیات مدل تخصیص مقدار در مسیر مواجهه، همراه با فرض‌ها و محدودیت‌های آن |
| `evidence_json` | `String` | `DEFAULT '{}'` | جزئیات دلیل، ورودی‌ها، محدودیت یا شاهد تحلیل به صورت JSON |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |

<a id="aml_risk_policies"></a>
### `aml_risk_policies`

تعریف نسخه‌های سیاست ارزیابی ریسک و آستانه‌ها.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, policy_version)`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `policy_version` | `String` | — | شناسهٔ نسخهٔ سیاستی که محاسبه یا ذخیره با آن انجام شده است |
| `policy_name` | `String` | — | نام قابل خواندن سیاست ریسک |
| `enabled` | `UInt8` | — | پرچم فعال‌بودن قاعده، سیاست یا رجیستری. اعتبار فعلی می‌تواند شروط دیگری هم داشته باشد |
| `medium_threshold` | `Float64` | — | آستانهٔ طبقهٔ ریسک متوسط طبق نسخهٔ سیاست |
| `high_threshold` | `Float64` | — | آستانهٔ طبقهٔ ریسک بالا طبق نسخهٔ سیاست |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |
| `settings_json` | `String` | `DEFAULT '{}'` | تنظیمات و پارامترهای اجرای سنجش، تحلیل یا سیاست به صورت JSON |
| `source_id` | `Nullable(String)` | `DEFAULT NULL` | شناسهٔ منبع داده، مانند آداپتور یا خوراک ثبت‌شده |
| `updated_at_unix_ms` | `Nullable(UInt64)` | `DEFAULT NULL` | زمان آخرین تغییر ثبت‌شدهٔ این نسخه، UTC و میلی‌ثانیه |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |

<a id="aml_risk_policy_rules"></a>
### `aml_risk_policy_rules`

قواعد و سقف مشارکت سیگنال در هر نسخهٔ سیاست.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, policy_version, rule_id)`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `policy_version` | `String` | — | شناسهٔ نسخهٔ سیاستی که محاسبه یا ذخیره با آن انجام شده است |
| `rule_id` | `String` | — | شناسهٔ قاعده در نسخهٔ سیاست ریسک |
| `signal_type` | `LowCardinality(String)` | — | نوع شاهد ریسک یا نام سیگنال مورد استفادهٔ قاعده |
| `max_contribution` | `Float64` | — | حداکثر امتیاز مجاز یک قاعدهٔ ریسک |
| `description` | `String` | — | شرح انسانی قاعدهٔ ریسک |
| `enabled` | `UInt8` | — | پرچم فعال‌بودن قاعده، سیاست یا رجیستری. اعتبار فعلی می‌تواند شروط دیگری هم داشته باشد |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |
| `settings_json` | `String` | `DEFAULT '{}'` | تنظیمات و پارامترهای اجرای سنجش، تحلیل یا سیاست به صورت JSON |
| `updated_at_unix_ms` | `Nullable(UInt64)` | `DEFAULT NULL` | زمان آخرین تغییر ثبت‌شدهٔ این نسخه، UTC و میلی‌ثانیه |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |

<a id="aml_wallet_risk_signals"></a>
### `aml_wallet_risk_signals`

سیگنال‌های مستند و سهمشان در یک ارزیابی ریسک.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, assessment_id, signal_id)`.
ایندکس‌های کمکی `idx_subject_id`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `signal_id` | `String` | — | شناسهٔ پایدار شاهد یا سیگنال ارزیابی |
| `assessment_id` | `String` | — | شناسهٔ ارزیابی ریسک که سیگنال‌ها به آن وصل می‌شوند |
| `subject_id` | `String` | — | شناسهٔ سوژهٔ تحلیل، رجیستری یا بازبینی، همراه با دامنهٔ شبکه |
| `policy_version` | `String` | — | شناسهٔ نسخهٔ سیاستی که محاسبه یا ذخیره با آن انجام شده است |
| `signal_type` | `LowCardinality(String)` | — | نوع شاهد ریسک یا نام سیگنال مورد استفادهٔ قاعده |
| `contribution` | `Float64` | — | مقداری که سیگنال به امتیاز ارزیابی ریسک اضافه می‌کند |
| `summary` | `String` | — | توضیح کوتاه سیگنال و دلیل نمایش آن |
| `evidence_refs` | `Array(String)` | — | فهرست شناسه‌ها یا ارجاع‌ها به شواهد پشتیبان. همهٔ آن‌ها کلید خارجی SQL نیستند |
| `evidence_json` | `String` | — | جزئیات دلیل، ورودی‌ها، محدودیت یا شاهد تحلیل به صورت JSON |
| `as_of_block` | `UInt64` | — | ارتفاع مرجع شواهد مورد استفادهٔ تحلیل یا انتشار |
| `source_revision` | `UInt64` | — | شمارهٔ انتشار منبع داده‌ای که ردیف تحلیل یا متادیتا بر آن بنا شده است |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |
| `signal_strength` | `Nullable(Float64)` | `DEFAULT NULL` | شدت یا قدرت شاهد طبق سیاست، جدا از contribution |
| `as_of_unix_ms` | `Nullable(UInt64)` | `DEFAULT NULL` | زمان مرجع شواهد تحلیل به UTC و میلی‌ثانیه |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |

<a id="aml_wallet_risk_assessments"></a>
### `aml_wallet_risk_assessments`

خروجی ارزیابی ریسک به همراه سیاست، دلایل و محدودیت شواهد.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, assessment_id)`.
ایندکس‌های کمکی `idx_subject_id`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `assessment_id` | `String` | — | شناسهٔ ارزیابی ریسک که سیگنال‌ها به آن وصل می‌شوند |
| `subject_id` | `String` | — | شناسهٔ سوژهٔ تحلیل، رجیستری یا بازبینی، همراه با دامنهٔ شبکه |
| `policy_version` | `String` | — | شناسهٔ نسخهٔ سیاستی که محاسبه یا ذخیره با آن انجام شده است |
| `risk_score` | `Nullable(Float64)` | — | امتیاز سیاست با شواهد موجود. NULL یعنی امتیاز معتبر در این وضعیت قابل ارائه نیست |
| `risk_level` | `LowCardinality(String)` | — | طبقهٔ ریسک گزارش‌شده. در رکورد هوشمندی برچسب منبع و در ارزیابی خروجی سیاست است |
| `probability_claimed` | `UInt8` | — | پرچم اینکه خروجی ادعای احتمال دارد. امتیاز سیاست AML به‌تنهایی احتمال نیست |
| `signal_count` | `UInt32` | — | تعداد سیگنال‌هایی که در ارزیابی ثبت شده‌اند |
| `exposure_run_id` | `Nullable(String)` | — | ارجاع ارزیابی ریسک به اجرای تحلیل مواجهه |
| `limitations` | `Array(String)` | — | فهرست محدودیت‌هایی که بر تفسیر ارزیابی اثر دارند |
| `as_of_block` | `UInt64` | — | ارتفاع مرجع شواهد مورد استفادهٔ تحلیل یا انتشار |
| `source_revision` | `UInt64` | — | شمارهٔ انتشار منبع داده‌ای که ردیف تحلیل یا متادیتا بر آن بنا شده است |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |
| `top_reasons` | `Array(String)` | `DEFAULT []` | دلایل اصلی نمایش سطح ریسک، طبق ترتیب سیاست |
| `evidence_refs` | `Array(String)` | `DEFAULT []` | فهرست شناسه‌ها یا ارجاع‌ها به شواهد پشتیبان. همهٔ آن‌ها کلید خارجی SQL نیستند |
| `as_of_unix_ms` | `Nullable(UInt64)` | `DEFAULT NULL` | زمان مرجع شواهد تحلیل به UTC و میلی‌ثانیه |
| `scoring_method` | `Nullable(String)` | `DEFAULT NULL` | روش امتیازدهی ثبت‌شده در ارزیابی |
| `evidence_json` | `String` | `DEFAULT '{}'` | جزئیات دلیل، ورودی‌ها، محدودیت یا شاهد تحلیل به صورت JSON |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |

<a id="aml_bitcoin_inputs"></a>
### `aml_bitcoin_inputs`

شواهد ورودی‌ها و prevout تراکنش‌های بیت‌کوین.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, block_number, block_hash, commit_id, tx_hash, input_index)`.
تقسیم‌بندی داده با `intDiv(block_number, 100000)` انجام می‌شود.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `block_number` | `UInt64` | — | ارتفاع بلاک مربوط به رکورد |
| `block_hash` | `String` | — | هش بلاکی که این رخداد در آن مشاهده شده است. شمارهٔ بلاک به‌تنهایی برای تشخیص شاخه کافی نیست |
| `block_timestamp_unix_ms` | `UInt64` | — | زمان اعلام‌شده در بلاک، UTC بر حسب میلی‌ثانیه |
| `commit_id` | `String` | — | شناسهٔ انتشار این نوبتِ بلاک که رکوردها را به بستهٔ دقیق همان بلاک متصل می‌کند. با Git commit فرق دارد |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `tx_hash` | `String` | — | هش تراکنش. برای رخداد دقیق، همراه شناسهٔ شبکه، بلاک و موقعیت رویداد خوانده می‌شود |
| `transaction_index` | `UInt32` | — | موقعیت تراکنش در بلاک |
| `input_index` | `UInt32` | — | موقعیت ورودی در تراکنش خرج‌کنندهٔ بیت‌کوین، همان vin |
| `address` | `Nullable(String)` | — | آدرس استاندارد شبکه. در رکورد اسکریپت، فقط وقتی اسکریپت آدرس قابل استخراج داشته باشد |
| `script_hex` | `String` | — | بایت‌های اسکریپت به hex. در ورودی بیت‌کوین، اسکریپت خروجی قبلی است |
| `script_type` | `LowCardinality(String)` | — | نوع اسکریپت بیت‌کوین طبق شاهد منبع، مانند P2WSH یا P2PKH |
| `value_sats` | `UInt64` | — | مقدار خروجی قبلی که این ورودی خرج کرده، به ساتوشی |
| `prev_tx_hash` | `String` | — | هش تراکنشی که خروجی قبلی بیت‌کوین را ایجاد کرده است |
| `prev_output_index` | `UInt32` | — | شمارهٔ خروجی قبلی بیت‌کوین که این ورودی به آن اشاره می‌کند |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |

<a id="aml_bitcoin_outputs"></a>
### `aml_bitcoin_outputs`

شواهد خروجی‌های بیت‌کوین همراه مقدار، اسکریپت و vout.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, block_number, block_hash, commit_id, tx_hash, output_index)`.
تقسیم‌بندی داده با `intDiv(block_number, 100000)` انجام می‌شود.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `block_number` | `UInt64` | — | ارتفاع بلاک مربوط به رکورد |
| `block_hash` | `String` | — | هش بلاکی که این رخداد در آن مشاهده شده است. شمارهٔ بلاک به‌تنهایی برای تشخیص شاخه کافی نیست |
| `block_timestamp_unix_ms` | `UInt64` | — | زمان اعلام‌شده در بلاک، UTC بر حسب میلی‌ثانیه |
| `commit_id` | `String` | — | شناسهٔ انتشار این نوبتِ بلاک که رکوردها را به بستهٔ دقیق همان بلاک متصل می‌کند. با Git commit فرق دارد |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `tx_hash` | `String` | — | هش تراکنش. برای رخداد دقیق، همراه شناسهٔ شبکه، بلاک و موقعیت رویداد خوانده می‌شود |
| `transaction_index` | `UInt32` | — | موقعیت تراکنش در بلاک |
| `output_index` | `UInt32` | — | موقعیت خروجی در تراکنش ایجادکنندهٔ بیت‌کوین، همان vout |
| `address` | `Nullable(String)` | — | آدرس استاندارد شبکه. در رکورد اسکریپت، فقط وقتی اسکریپت آدرس قابل استخراج داشته باشد |
| `script_hex` | `String` | — | بایت‌های اسکریپت به hex. در ورودی بیت‌کوین، اسکریپت خروجی قبلی است |
| `script_type` | `LowCardinality(String)` | — | نوع اسکریپت بیت‌کوین طبق شاهد منبع، مانند P2WSH یا P2PKH |
| `value_sats` | `UInt64` | — | مقدار همین خروجی تراکنش، به ساتوشی |
| `coinbase` | `UInt8` | — | پرچم تراکنش ایجاد پاداش بلاک بیت‌کوین یا خروجی متعلق به آن |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |

<a id="aml_evm_logs"></a>
### `aml_evm_logs`

لاگ‌های اصلی رسید Ethereum و BSC با موقعیت واقعی.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, block_number, block_hash, commit_id, tx_hash, log_index)`.
تقسیم‌بندی داده با `intDiv(block_number, 100000)` انجام می‌شود.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `block_number` | `UInt64` | — | ارتفاع بلاک مربوط به رکورد |
| `block_hash` | `String` | — | هش بلاکی که این رخداد در آن مشاهده شده است. شمارهٔ بلاک به‌تنهایی برای تشخیص شاخه کافی نیست |
| `block_timestamp_unix_ms` | `UInt64` | — | زمان اعلام‌شده در بلاک، UTC بر حسب میلی‌ثانیه |
| `commit_id` | `String` | — | شناسهٔ انتشار این نوبتِ بلاک که رکوردها را به بستهٔ دقیق همان بلاک متصل می‌کند. با Git commit فرق دارد |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `tx_hash` | `String` | — | هش تراکنش. برای رخداد دقیق، همراه شناسهٔ شبکه، بلاک و موقعیت رویداد خوانده می‌شود |
| `log_index` | `UInt32` | — | موقعیت اصلی لاگ در دادهٔ شبکه. در ترون جایگاه در آرایهٔ رسید و در EVM موقعیت لاگ گزارش‌شده |
| `address` | `String` | — | آدرس قرارداد صادرکنندهٔ لاگ EVM |
| `topics` | `Array(String)` | — | آرایهٔ topicهای لاگ با ترتیب اصلی شبکه. برای رویداد ABI معمولاً topic اول امضای رویداد است |
| `data` | `String` | — | بایت دادهٔ لاگ EVM به شکل hex. معنی فیلدها تابع ABI رویداد است |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |
| `transaction_index` | `Nullable(UInt32)` | `DEFAULT NULL` | موقعیت تراکنش در بلاک |
| `removed` | `Nullable(UInt8)` | `DEFAULT NULL` | پرچم removed در لاگ EVM طبق منبع |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |

<a id="aml_tron_contracts"></a>
### `aml_tron_contracts`

قراردادهای تراکنش ترون، protobuf اصلی و پرچم‌های تطابق.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, block_number, block_hash, commit_id, tx_hash, contract_index)`.
تقسیم‌بندی داده با `intDiv(block_number, 100000)` انجام می‌شود.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `block_number` | `UInt64` | — | ارتفاع بلاک مربوط به رکورد |
| `block_hash` | `String` | — | هش بلاکی که این رخداد در آن مشاهده شده است. شمارهٔ بلاک به‌تنهایی برای تشخیص شاخه کافی نیست |
| `block_timestamp_unix_ms` | `UInt64` | — | زمان اعلام‌شده در بلاک، UTC بر حسب میلی‌ثانیه |
| `commit_id` | `String` | — | شناسهٔ انتشار این نوبتِ بلاک که رکوردها را به بستهٔ دقیق همان بلاک متصل می‌کند. با Git commit فرق دارد |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `tx_hash` | `String` | — | هش تراکنش. برای رخداد دقیق، همراه شناسهٔ شبکه، بلاک و موقعیت رویداد خوانده می‌شود |
| `contract_index` | `UInt32` | — | جایگاه قرارداد در آرایهٔ قراردادهای تراکنش ترون |
| `contract_type` | `LowCardinality(String)` | — | نام نوع قرارداد در دادهٔ ترون |
| `payload_json` | `String` | — | بدنهٔ اصلی تراکنش یا قرارداد ترون، بسته به جدول، به صورت JSON |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |
| `transaction_index` | `Nullable(UInt32)` | `DEFAULT NULL` | موقعیت تراکنش در بلاک |
| `proto_contract_type` | `Nullable(UInt32)` | `DEFAULT NULL` | شمارهٔ enum نوع قرارداد در بایت‌های protobuf ترون |
| `type_url` | `Nullable(String)` | `DEFAULT NULL` | شناسهٔ نوع پیام داخل protobuf Any در قرارداد ترون |
| `parameter_value_hex` | `Nullable(String)` | `DEFAULT NULL` | بایت اصلی محتوای فیلد value در پیام protobuf Any قرارداد ترون |
| `raw_data_hash_verified` | `UInt8` | `DEFAULT 0` | پرچم تطابق SHA-256 بایت‌های اصلی raw_data_hex ترون با txID. صرفاً همین تطابق را ثبت می‌کند |
| `parameter_fields_verified` | `UInt8` | `DEFAULT 0` | پرچم تطابق فیلدهای پشتیبانی‌شدهٔ قرارداد ترون با protobuf اصلی |

<a id="aml_metadata_discoveries"></a>
### `aml_metadata_discoveries`

مدرک روی زنجیره‌ای کشف یک دارایی و ارتباط آن با صف متادیتا.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, block_number, block_hash, commit_id, discovery_id)`.
تقسیم‌بندی داده با `intDiv(block_number,100000)` انجام می‌شود.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `block_number` | `UInt64` | — | ارتفاع بلاک مربوط به رکورد |
| `block_hash` | `String` | — | هش بلاکی که این رخداد در آن مشاهده شده است. شمارهٔ بلاک به‌تنهایی برای تشخیص شاخه کافی نیست |
| `block_timestamp_unix_ms` | `UInt64` | — | زمان اعلام‌شده در بلاک، UTC بر حسب میلی‌ثانیه |
| `commit_id` | `String` | — | شناسهٔ انتشار این نوبتِ بلاک که رکوردها را به بستهٔ دقیق همان بلاک متصل می‌کند. با Git commit فرق دارد |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `discovery_id` | `String` | `DEFAULT ''` | شناسهٔ شاهد کشف دارایی یا ارجاع کار متادیتا به آن |
| `tx_hash` | `String` | `DEFAULT ''` | هش تراکنش. برای رخداد دقیق، همراه شناسهٔ شبکه، بلاک و موقعیت رویداد خوانده می‌شود |
| `transaction_index` | `UInt32` | `DEFAULT 0` | موقعیت تراکنش در بلاک |
| `asset_id` | `String` | `DEFAULT ''` | شناسهٔ استاندارد دارایی در شبکه، مانند native:BTC یا شناسهٔ وابسته به قرارداد توکن |
| `standard_hint` | `String` | `DEFAULT ''` | سرنخ اولیهٔ استاندارد دارایی در لحظهٔ کشف، پیش از تأیید متادیتا |
| `evidence_id` | `String` | `DEFAULT ''` | شناسهٔ رکورد شاهدی که کشف دارایی به آن متکی است |
| `source_id` | `String` | `DEFAULT ''` | شناسهٔ منبع داده، مانند آداپتور یا خوراک ثبت‌شده |
| `evidence_refs` | `Array(String)` | `DEFAULT []` | فهرست شناسه‌ها یا ارجاع‌ها به شواهد پشتیبان. همهٔ آن‌ها کلید خارجی SQL نیستند |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |

<a id="aml_protocol_registry"></a>
### `aml_protocol_registry`

قراردادها و نقش‌های پروتکل با مدرک، مفسر و وضعیت بازبینی.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, registry_id)`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `registry_id` | `String` | `DEFAULT ''` | شناسهٔ ورودی رجیستری پروتکل |
| `subject_id` | `String` | `DEFAULT ''` | شناسهٔ سوژهٔ تحلیل، رجیستری یا بازبینی، همراه با دامنهٔ شبکه |
| `subject_type` | `LowCardinality(String)` | `DEFAULT 'address'` | نوع سوژهٔ رجیستری یا بازبینی |
| `protocol` | `String` | `DEFAULT ''` | نام یا شناسهٔ پروتکل مربوط به رویداد یا رجیستری |
| `protocol_type` | `String` | `DEFAULT ''` | ردهٔ پروتکل، مانند پل یا صرافی غیرمتمرکز |
| `contract_role` | `String` | `DEFAULT ''` | نقش قرارداد در پروتکل، مثل استخر یا قرارداد پل |
| `remote_network_id` | `Nullable(String)` | `DEFAULT NULL` | شناسهٔ شبکهٔ طرف مقابل، در صورت وجود شاهد معتبر یا پیکربندی رجیستری |
| `remote_subject_id` | `Nullable(String)` | `DEFAULT NULL` | شناسهٔ سوژه یا قرارداد طرف مقابل که در رجیستری ثبت شده است |
| `decoder` | `String` | `DEFAULT ''` | شناسهٔ مفسر ABI یا parser ثبت‌شده برای پروتکل |
| `decoder_config_json` | `String` | `DEFAULT '{}'` | تنظیمات مفسر، از جمله داده‌های لازم برای خواندن رویداد یا نگاشت پروتکل |
| `source_id` | `String` | `DEFAULT ''` | شناسهٔ منبع داده، مانند آداپتور یا خوراک ثبت‌شده |
| `source_reference` | `String` | `DEFAULT ''` | ارجاع دقیق به رکورد یا مدرک در منبع |
| `confidence` | `Float64` | `DEFAULT 0` | وزن اطمینان روش یا منبع به این شاهد. معادل احتمال پول‌شویی نیست |
| `enabled` | `UInt8` | `DEFAULT 0` | پرچم فعال‌بودن قاعده، سیاست یا رجیستری. اعتبار فعلی می‌تواند شروط دیگری هم داشته باشد |
| `review_status` | `LowCardinality(String)` | `DEFAULT 'pending'` | آخرین وضعیت بررسی‌شدهٔ مدرک، مثل pending یا approved طبق گردش همان نوع رکورد |
| `reviewed_by` | `String` | `DEFAULT ''` | شناسهٔ بازبین ثبت‌شده |
| `reviewed_at_unix_ms` | `Nullable(UInt64)` | `DEFAULT NULL` | زمان انجام بازبینی، UTC و میلی‌ثانیه |
| `review_note` | `String` | `DEFAULT ''` | متن توضیح بازبین دربارهٔ ادعا یا وضعیت |
| `evidence_refs` | `Array(String)` | `DEFAULT []` | فهرست شناسه‌ها یا ارجاع‌ها به شواهد پشتیبان. همهٔ آن‌ها کلید خارجی SQL نیستند |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |

<a id="aml_entities"></a>
### `aml_entities`

کاتالوگ نهادها و سرویس‌ها با منشأ و وضعیت ثبت‌شده.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, entity_id)`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `entity_id` | `String` | `DEFAULT ''` | شناسهٔ نهاد یا سرویس در کاتالوگ هوشمندی |
| `entity_name` | `String` | `DEFAULT ''` | نام قابل نمایش نهاد یا سرویس |
| `entity_type` | `String` | `DEFAULT ''` | ردهٔ نهاد، مثل صرافی یا سرویس |
| `source_id` | `String` | `DEFAULT ''` | شناسهٔ منبع داده، مانند آداپتور یا خوراک ثبت‌شده |
| `source_reference` | `String` | `DEFAULT ''` | ارجاع دقیق به رکورد یا مدرک در منبع |
| `is_active` | `UInt8` | `DEFAULT 0` | وضعیت فعال‌بودن رکورد، که view ممکن است اعتبار منبع و ادعا را هم کنترل کند |
| `review_status` | `LowCardinality(String)` | `DEFAULT 'pending'` | آخرین وضعیت بررسی‌شدهٔ مدرک، مثل pending یا approved طبق گردش همان نوع رکورد |
| `reviewed_by` | `String` | `DEFAULT ''` | شناسهٔ بازبین ثبت‌شده |
| `reviewed_at_unix_ms` | `Nullable(UInt64)` | `DEFAULT NULL` | زمان انجام بازبینی، UTC و میلی‌ثانیه |
| `review_note` | `String` | `DEFAULT ''` | متن توضیح بازبین دربارهٔ ادعا یا وضعیت |
| `evidence_refs` | `Array(String)` | `DEFAULT []` | فهرست شناسه‌ها یا ارجاع‌ها به شواهد پشتیبان. همهٔ آن‌ها کلید خارجی SQL نیستند |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |

<a id="aml_entity_addresses"></a>
### `aml_entity_addresses`

نگاشت مستند آدرس‌ها به نهادها بر اساس ادعای پشتیبان.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, entity_id, address, claim_id)`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `entity_id` | `String` | `DEFAULT ''` | شناسهٔ نهاد یا سرویس در کاتالوگ هوشمندی |
| `address` | `String` | `DEFAULT ''` | آدرس استاندارد شبکه. در رکورد اسکریپت، فقط وقتی اسکریپت آدرس قابل استخراج داشته باشد |
| `address_role` | `String` | `DEFAULT ''` | نقش گزارش‌شدهٔ آدرس یا عضو، مثل نقش مرتبط با یک سرویس |
| `claim_id` | `String` | `DEFAULT ''` | شناسهٔ ادعای انتساب یا خوشه‌بندی که این ردیف تعریف می‌کند یا به آن استناد دارد |
| `source_id` | `String` | `DEFAULT ''` | شناسهٔ منبع داده، مانند آداپتور یا خوراک ثبت‌شده |
| `confidence` | `Float64` | `DEFAULT 0` | وزن اطمینان روش یا منبع به این شاهد. معادل احتمال پول‌شویی نیست |
| `is_active` | `UInt8` | `DEFAULT 0` | وضعیت فعال‌بودن رکورد، که view ممکن است اعتبار منبع و ادعا را هم کنترل کند |
| `first_seen_block` | `Nullable(UInt64)` | `DEFAULT NULL` | قدیمی‌ترین ارتفاع مشاهده یا پوشش ثبت‌شده |
| `last_seen_block` | `Nullable(UInt64)` | `DEFAULT NULL` | آخرین ارتفاع مشاهده یا پوشش ثبت‌شده |
| `review_status` | `LowCardinality(String)` | `DEFAULT 'pending'` | آخرین وضعیت بررسی‌شدهٔ مدرک، مثل pending یا approved طبق گردش همان نوع رکورد |
| `reviewed_by` | `String` | `DEFAULT ''` | شناسهٔ بازبین ثبت‌شده |
| `reviewed_at_unix_ms` | `Nullable(UInt64)` | `DEFAULT NULL` | زمان انجام بازبینی، UTC و میلی‌ثانیه |
| `review_note` | `String` | `DEFAULT ''` | متن توضیح بازبین دربارهٔ ادعا یا وضعیت |
| `evidence_refs` | `Array(String)` | `DEFAULT []` | فهرست شناسه‌ها یا ارجاع‌ها به شواهد پشتیبان. همهٔ آن‌ها کلید خارجی SQL نیستند |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |

<a id="aml_cluster_versions"></a>
### `aml_cluster_versions`

فهرست نسخه‌های خوشه و علت تغییر هر نسخه.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, cluster_id, cluster_version)`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `cluster_id` | `String` | `DEFAULT ''` | شناسهٔ خوشهٔ تحلیلی، در محدودهٔ شبکه |
| `cluster_version` | `String` | `DEFAULT ''` | نسخهٔ خوشه‌ای که ردیف عضویت یا نسخه‌نامه به آن تعلق دارد |
| `cluster_type` | `String` | `DEFAULT ''` | نوع خوشه و شیوهٔ تفسیر عضویت آن |
| `display_name` | `String` | `DEFAULT ''` | نام نمایشی نسخهٔ خوشه |
| `change_type` | `String` | `DEFAULT ''` | نوع عملیات نسخه‌بندی خوشه |
| `change_reason` | `String` | `DEFAULT ''` | علت ثبت نسخه یا تغییر خوشه |
| `source_claim_ids` | `Array(String)` | `DEFAULT []` | شناسهٔ ادعاهای پشتیبان که برای ایجاد نسخهٔ خوشه به کار رفته‌اند |
| `active_member_count` | `Nullable(UInt64)` | `DEFAULT NULL` | شمار اعضای فعال در این نسخهٔ خوشه |
| `created_by` | `String` | `DEFAULT ''` | شناسه یا نام ثبت‌شدهٔ ایجادکنندهٔ رکورد |
| `evidence_refs` | `Array(String)` | `DEFAULT []` | فهرست شناسه‌ها یا ارجاع‌ها به شواهد پشتیبان. همهٔ آن‌ها کلید خارجی SQL نیستند |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |
| `run_id` | `Nullable(String)` | `DEFAULT NULL` | شناسهٔ نوبت اجرای تحلیل که ردیف‌های وابسته را به هم وصل می‌کند |

<a id="aml_address_exposures"></a>
### `aml_address_exposures`

خلاصهٔ مواجههٔ آدرس یا سوژه با مبنای ریسک.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, run_id, exposure_id)`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `exposure_id` | `String` | `DEFAULT ''` | شناسهٔ خلاصهٔ مواجههٔ یک سوژه |
| `run_id` | `String` | `DEFAULT ''` | شناسهٔ نوبت اجرای تحلیل که ردیف‌های وابسته را به هم وصل می‌کند |
| `seed_id` | `String` | `DEFAULT ''` | شناسهٔ نقطهٔ شروع تحلیل مواجهه |
| `source_subject_id` | `String` | `DEFAULT ''` | شناسهٔ سوژهٔ مبدأ در خلاصهٔ مسیر مواجهه |
| `subject_id` | `String` | `DEFAULT ''` | شناسهٔ سوژهٔ تحلیل، رجیستری یا بازبینی، همراه با دامنهٔ شبکه |
| `hop_distance` | `UInt8` | `DEFAULT 0` | فاصلهٔ گزارش‌شده از منبع ریسک به تعداد قدم در خلاصهٔ مواجهه |
| `exposure_score` | `Nullable(Float64)` | `DEFAULT NULL` | امتیاز مواجهه طبق روش تحلیل |
| `path_count` | `Nullable(UInt64)` | `DEFAULT NULL` | تعداد مسیرهای مشاهده یا ثبت‌شده در اجرا یا خلاصه |
| `last_tx_hash` | `Nullable(String)` | `DEFAULT NULL` | هش آخرین تراکنش گزارش‌شده در خلاصهٔ مواجهه |
| `last_seen_block` | `Nullable(UInt64)` | `DEFAULT NULL` | آخرین ارتفاع مشاهده یا پوشش ثبت‌شده |
| `exposure_type` | `String` | `DEFAULT ''` | نوع مواجههٔ خلاصه‌شده، در دامنهٔ روش اجرا |
| `best_path_amount_share` | `Nullable(Float64)` | `DEFAULT NULL` | بیشترین یا بهترین سهم مقداری گزارش‌شده برای مسیر منتخب طبق خروجی تحلیل |
| `best_path_time_weight` | `Nullable(Float64)` | `DEFAULT NULL` | وزن زمانی مسیر منتخب در خلاصهٔ مواجهه |
| `service_mediated` | `Nullable(UInt8)` | `DEFAULT NULL` | پرچم اینکه مسیر به واسطهٔ یک سرویس شناسایی‌شده عبور می‌کند |
| `as_of_block` | `Nullable(UInt64)` | `DEFAULT NULL` | ارتفاع مرجع شواهد مورد استفادهٔ تحلیل یا انتشار |
| `source_revision` | `Nullable(UInt64)` | `DEFAULT NULL` | شمارهٔ انتشار منبع داده‌ای که ردیف تحلیل یا متادیتا بر آن بنا شده است |
| `evidence_refs` | `Array(String)` | `DEFAULT []` | فهرست شناسه‌ها یا ارجاع‌ها به شواهد پشتیبان. همهٔ آن‌ها کلید خارجی SQL نیستند |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |

<a id="aml_bitcoin_scripts"></a>
### `aml_bitcoin_scripts`

شاهد تخصصی اسکریپت بیت‌کوین و آدرس قابل استخراج از آن.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, block_number, block_hash, commit_id, tx_hash, script_hash)`.
تقسیم‌بندی داده با `intDiv(block_number,100000)` انجام می‌شود.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `block_number` | `UInt64` | — | ارتفاع بلاک مربوط به رکورد |
| `block_hash` | `String` | — | هش بلاکی که این رخداد در آن مشاهده شده است. شمارهٔ بلاک به‌تنهایی برای تشخیص شاخه کافی نیست |
| `block_timestamp_unix_ms` | `UInt64` | — | زمان اعلام‌شده در بلاک، UTC بر حسب میلی‌ثانیه |
| `commit_id` | `String` | — | شناسهٔ انتشار این نوبتِ بلاک که رکوردها را به بستهٔ دقیق همان بلاک متصل می‌کند. با Git commit فرق دارد |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `tx_hash` | `String` | `DEFAULT ''` | هش تراکنش. برای رخداد دقیق، همراه شناسهٔ شبکه، بلاک و موقعیت رویداد خوانده می‌شود |
| `script_hash` | `String` | `DEFAULT ''` | هش اسکریپت برای شناسایی یا ارجاع به شاهد اسکریپت بیت‌کوین |
| `script_hex` | `String` | `DEFAULT ''` | بایت‌های اسکریپت به hex. در ورودی بیت‌کوین، اسکریپت خروجی قبلی است |
| `script_type` | `String` | `DEFAULT ''` | نوع اسکریپت بیت‌کوین طبق شاهد منبع، مانند P2WSH یا P2PKH |
| `address` | `Nullable(String)` | `DEFAULT NULL` | آدرس استاندارد شبکه. در رکورد اسکریپت، فقط وقتی اسکریپت آدرس قابل استخراج داشته باشد |
| `evidence_refs` | `Array(String)` | `DEFAULT []` | فهرست شناسه‌ها یا ارجاع‌ها به شواهد پشتیبان. همهٔ آن‌ها کلید خارجی SQL نیستند |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |

<a id="aml_evm_traces"></a>
### `aml_evm_traces`

موقعیت و شواهد فراخوانی داخلی EVM در صورت پوشش trace.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, block_number, block_hash, commit_id, tx_hash, trace_id)`.
تقسیم‌بندی داده با `intDiv(block_number,100000)` انجام می‌شود.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `block_number` | `UInt64` | — | ارتفاع بلاک مربوط به رکورد |
| `block_hash` | `String` | — | هش بلاکی که این رخداد در آن مشاهده شده است. شمارهٔ بلاک به‌تنهایی برای تشخیص شاخه کافی نیست |
| `block_timestamp_unix_ms` | `UInt64` | — | زمان اعلام‌شده در بلاک، UTC بر حسب میلی‌ثانیه |
| `commit_id` | `String` | — | شناسهٔ انتشار این نوبتِ بلاک که رکوردها را به بستهٔ دقیق همان بلاک متصل می‌کند. با Git commit فرق دارد |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `tx_hash` | `String` | `DEFAULT ''` | هش تراکنش. برای رخداد دقیق، همراه شناسهٔ شبکه، بلاک و موقعیت رویداد خوانده می‌شود |
| `trace_id` | `String` | `DEFAULT ''` | شناسهٔ رکورد trace در دامنهٔ تراکنش و بلاک |
| `transaction_index` | `UInt32` | `DEFAULT 0` | موقعیت تراکنش در بلاک |
| `trace_address` | `Array(UInt32)` | `DEFAULT []` | ترتیب شمارهٔ فرزندها از ریشهٔ trace تا فراخوانی موردنظر |
| `trace_type` | `String` | `DEFAULT ''` | نوع فراخوانی یا رخداد trace در EVM |
| `from_address` | `Nullable(String)` | `DEFAULT NULL` | آدرس مبدأ در صورت وجود. برای پایهٔ غیرآدرسی یا تراکنش بیت‌کوین می‌تواند NULL باشد |
| `to_address` | `Nullable(String)` | `DEFAULT NULL` | آدرس مقصد در صورت وجود. برای پایهٔ غیرآدرسی یا تراکنش بیت‌کوین می‌تواند NULL باشد |
| `value_raw` | `Nullable(UInt256)` | `DEFAULT NULL` | ارزش خام گزارش‌شده برای اجرای trace، در کوچک‌ترین واحد ارز بومی EVM |
| `success` | `Nullable(UInt8)` | `DEFAULT NULL` | نتیجهٔ مشاهده‌شدهٔ اجرای trace. NULL یعنی وضعیت معلوم نیست |
| `error_message` | `Nullable(String)` | `DEFAULT NULL` | شرح ثبت‌شدهٔ خطای اجرا یا دریافت |
| `source_id` | `String` | `DEFAULT ''` | شناسهٔ منبع داده، مانند آداپتور یا خوراک ثبت‌شده |
| `evidence_refs` | `Array(String)` | `DEFAULT []` | فهرست شناسه‌ها یا ارجاع‌ها به شواهد پشتیبان. همهٔ آن‌ها کلید خارجی SQL نیستند |
| `raw_evidence_json` | `String` | `DEFAULT '{}'` | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |

<a id="aml_raw_transactions"></a>
### `aml_raw_transactions`

بدنهٔ اصلی تراکنش و رسید RPC در قالب قابل ممیزی.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, block_number, block_hash, commit_id, tx_hash)`.
تقسیم‌بندی داده با `intDiv(block_number,100000)` انجام می‌شود.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `block_number` | `UInt64` | — | ارتفاع بلاک مربوط به رکورد |
| `block_hash` | `String` | — | هش بلاکی که این رخداد در آن مشاهده شده است. شمارهٔ بلاک به‌تنهایی برای تشخیص شاخه کافی نیست |
| `block_timestamp_unix_ms` | `UInt64` | — | زمان اعلام‌شده در بلاک، UTC بر حسب میلی‌ثانیه |
| `commit_id` | `String` | — | شناسهٔ انتشار این نوبتِ بلاک که رکوردها را به بستهٔ دقیق همان بلاک متصل می‌کند. با Git commit فرق دارد |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `tx_hash` | `String` | `DEFAULT ''` | هش تراکنش. برای رخداد دقیق، همراه شناسهٔ شبکه، بلاک و موقعیت رویداد خوانده می‌شود |
| `transaction_index` | `UInt32` | `DEFAULT 0` | موقعیت تراکنش در بلاک |
| `source_id` | `String` | `DEFAULT ''` | شناسهٔ منبع داده، مانند آداپتور یا خوراک ثبت‌شده |
| `payload_json` | `String` | `DEFAULT '{}'` | بدنهٔ اصلی تراکنش یا قرارداد ترون، بسته به جدول، به صورت JSON |
| `receipt_json` | `Nullable(String)` | `DEFAULT NULL` | رسید اصلی دریافتی از RPC به صورت JSON. NULL یعنی رسید در این رکورد موجود نیست |
| `evidence_refs` | `Array(String)` | `DEFAULT []` | فهرست شناسه‌ها یا ارجاع‌ها به شواهد پشتیبان. همهٔ آن‌ها کلید خارجی SQL نیستند |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |

<a id="aml_analysis_publications"></a>
### `aml_analysis_publications`

مهر انتشار خروجی‌های تحلیل با نسخهٔ دقیق ورودی و شمار خروجی.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, family, publication_id)`.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `family` | `LowCardinality(String)` | — | خانوادهٔ خروجی تحلیل، مانند cluster، exposure یا risk |
| `publication_id` | `String` | — | شناسهٔ seal یا انتشار یک خروجی تحلیلی |
| `source_revision` | `UInt64` | — | شمارهٔ انتشار منبع داده‌ای که ردیف تحلیل یا متادیتا بر آن بنا شده است |
| `as_of_block` | `UInt64` | — | ارتفاع مرجع شواهد مورد استفادهٔ تحلیل یا انتشار |
| `block_hash` | `String` | — | هش بلاکی که این رخداد در آن مشاهده شده است. شمارهٔ بلاک به‌تنهایی برای تشخیص شاخه کافی نیست |
| `output_revision` | `UInt64` | — | نسخهٔ دقیق ردیف‌های خروجی که seal تحلیل برای نمایش مجاز کرده است |
| `status` | `LowCardinality(String)` | — | وضعیت انتشار خروجی. view visible خروجی مهرشده با ورودی جاری را نشان می‌دهد |
| `row_counts_json` | `String` | — | شمار ردیف‌های واقعی خروجی sealشده برای هر جدول تحلیل |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |

<a id="aml_tron_logs"></a>
### `aml_tron_logs`

لاگ‌های رسید ترون با emitter، topic و موقعیت اصلی.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, block_number, block_hash, commit_id, tx_hash, log_index)`.
تقسیم‌بندی داده با `intDiv(block_number, 100000)` انجام می‌شود.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `block_number` | `UInt64` | — | ارتفاع بلاک مربوط به رکورد |
| `block_hash` | `String` | — | هش بلاکی که این رخداد در آن مشاهده شده است. شمارهٔ بلاک به‌تنهایی برای تشخیص شاخه کافی نیست |
| `block_timestamp_unix_ms` | `UInt64` | — | زمان اعلام‌شده در بلاک، UTC بر حسب میلی‌ثانیه |
| `commit_id` | `String` | — | شناسهٔ انتشار این نوبتِ بلاک که رکوردها را به بستهٔ دقیق همان بلاک متصل می‌کند. با Git commit فرق دارد |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `tx_hash` | `String` | — | هش تراکنش. برای رخداد دقیق، همراه شناسهٔ شبکه، بلاک و موقعیت رویداد خوانده می‌شود |
| `transaction_index` | `UInt32` | — | موقعیت تراکنش در بلاک |
| `log_index` | `UInt32` | — | موقعیت اصلی لاگ در دادهٔ شبکه. در ترون جایگاه در آرایهٔ رسید و در EVM موقعیت لاگ گزارش‌شده |
| `address` | `String` | — | آدرس emitter لاگ ترون در قالب canonical شبکه |
| `topics` | `Array(String)` | — | آرایهٔ topicهای لاگ با ترتیب اصلی شبکه. برای رویداد ABI معمولاً topic اول امضای رویداد است |
| `data_hex` | `String` | — | بایت دادهٔ لاگ رسید ترون به شکل hex |
| `raw_evidence_json` | `String` | — | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |

<a id="aml_tron_internals"></a>
### `aml_tron_internals`

فراخوانی‌های داخلی ترون، وضعیت رد و دادهٔ اصلی ارزش فراخوانی.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, block_number, block_hash, commit_id, tx_hash, internal_index)`.
تقسیم‌بندی داده با `intDiv(block_number, 100000)` انجام می‌شود.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `block_number` | `UInt64` | — | ارتفاع بلاک مربوط به رکورد |
| `block_hash` | `String` | — | هش بلاکی که این رخداد در آن مشاهده شده است. شمارهٔ بلاک به‌تنهایی برای تشخیص شاخه کافی نیست |
| `block_timestamp_unix_ms` | `UInt64` | — | زمان اعلام‌شده در بلاک، UTC بر حسب میلی‌ثانیه |
| `commit_id` | `String` | — | شناسهٔ انتشار این نوبتِ بلاک که رکوردها را به بستهٔ دقیق همان بلاک متصل می‌کند. با Git commit فرق دارد |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `tx_hash` | `String` | — | هش تراکنش. برای رخداد دقیق، همراه شناسهٔ شبکه، بلاک و موقعیت رویداد خوانده می‌شود |
| `transaction_index` | `UInt32` | — | موقعیت تراکنش در بلاک |
| `internal_index` | `UInt32` | — | موقعیت رکورد در آرایهٔ internal transactions رسید ترون |
| `internal_hash` | `Nullable(String)` | — | شناسه یا هش رکورد فراخوانی داخلی ترون طبق رسید |
| `caller_address` | `Nullable(String)` | — | آدرس آغازکنندهٔ فراخوانی داخلی ترون |
| `transfer_to_address` | `Nullable(String)` | — | آدرس هدف انتقال یا فراخوانی داخلی ترون |
| `rejected` | `Nullable(UInt8)` | — | پرچم رد شدن فراخوانی داخلی ترون طبق رسید. NULL یعنی فیلد گزارش نشده است |
| `note_hex` | `Nullable(String)` | — | فیلد note فراخوانی داخلی ترون به شکل hex، مانند نوع عمل ثبت‌شده در رسید |
| `call_value_info_json` | `String` | — | آرایهٔ اصلی callValueInfo ترون با ترتیب RPC و جزئیات ارزش/شناسهٔ دارایی هر آیتم |
| `raw_evidence_json` | `String` | — | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |

<a id="aml_tron_resources"></a>
### `aml_tron_resources`

فیلدهای گزارش‌شدهٔ Energy، Bandwidth و هزینه برای تراکنش ترون.

موتور `ReplacingMergeTree(revision)` و کلید ترتیب `(network_id, block_number, block_hash, commit_id, tx_hash)`.
تقسیم‌بندی داده با `intDiv(block_number, 100000)` انجام می‌شود.

| ستون | نوع در ClickHouse | پیش‌فرض صریح | معنی و کاربرد |
| --- | --- | --- | --- |
| `network_id` | `LowCardinality(String)` | — | هویت ثابت شبکه. همواره در اتصال و فیلتر داده لحاظ می‌شود |
| `block_number` | `UInt64` | — | ارتفاع بلاک مربوط به رکورد |
| `block_hash` | `String` | — | هش بلاکی که این رخداد در آن مشاهده شده است. شمارهٔ بلاک به‌تنهایی برای تشخیص شاخه کافی نیست |
| `block_timestamp_unix_ms` | `UInt64` | — | زمان اعلام‌شده در بلاک، UTC بر حسب میلی‌ثانیه |
| `commit_id` | `String` | — | شناسهٔ انتشار این نوبتِ بلاک که رکوردها را به بستهٔ دقیق همان بلاک متصل می‌کند. با Git commit فرق دارد |
| `revision` | `UInt64` | — | شمارهٔ نسخهٔ رکورد یا انتشار. برای تشخیص تازه‌ترین نسخه و اتصال دقیق شاهد به انتشار به کار می‌رود |
| `tx_hash` | `String` | — | هش تراکنش. برای رخداد دقیق، همراه شناسهٔ شبکه، بلاک و موقعیت رویداد خوانده می‌شود |
| `transaction_index` | `UInt32` | — | موقعیت تراکنش در بلاک |
| `fee_raw` | `Nullable(UInt256)` | — | هزینهٔ کلی ثبت‌شدهٔ تراکنش ترون به sun، در صورت گزارش در رسید |
| `energy_usage` | `Nullable(UInt64)` | — | Energy مصرف‌شده از سهم حساب طبق فیلد energy_usage رسید |
| `energy_usage_total` | `Nullable(UInt64)` | — | کل Energy مصرف‌شده طبق فیلد energy_usage_total رسید |
| `origin_energy_usage` | `Nullable(UInt64)` | — | Energy که طبق رسید از سهم سازندهٔ قرارداد تأمین شده است |
| `energy_fee` | `Nullable(UInt64)` | — | هزینهٔ انرژی ترون به sun، طبق فیلد رسید |
| `net_usage` | `Nullable(UInt64)` | — | مصرف Bandwidth گزارش‌شده در رسید ترون |
| `net_fee` | `Nullable(UInt64)` | — | هزینهٔ Bandwidth ترون به sun طبق رسید |
| `energy_penalty_total` | `Nullable(UInt64)` | — | جمع Energy جریمه یا مؤلفهٔ اضافهٔ انرژی طبق رسید ترون |
| `receipt_result` | `Nullable(String)` | — | نتیجهٔ اجرای ثبت‌شده در بخش receipt ترون |
| `transaction_result` | `Nullable(String)` | — | نتیجهٔ تراکنش ترون طبق دادهٔ مربوط به نتیجهٔ تراکنش |
| `raw_evidence_json` | `String` | — | شاهد اصلی یا جزئیات حفظ‌شده از دادهٔ ورودی برای ممیزی. ساختار داخلی به جدول و نسخه بستگی دارد |
| `created_at_unix_ms` | `UInt64` | — | زمان ثبت اولیهٔ رکورد، UTC و میلی‌ثانیه |
