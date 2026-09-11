# BSC ClickHouse Data Dictionary

## September 2026 Investigation Additions

Migration 0007 adds one append-only intelligence table. It does not duplicate transfers, paths or balances.
Migration 0008 refreshes the semantic canonical view so Phase 6 bridge columns are actually readable.

| New Column | Writer | Reader / Meaning |
|---|---|---|
| intelligence_claims.network_id | bsc_intelligence import-labels | Network isolation; always eip155:56 |
| claim_id | Import file | Stable claim identity across review revisions |
| address | Validated import | Wallet identity lookup |
| claim_kind | Import | label or cluster; cluster cannot seed exposure |
| entity_id | Import | Grouping for reviewed entity/cluster |
| entity_name | Import | Analyst-visible name; never inferred as fact |
| entity_type | Import | Entity context and exchange/bridge/custodian/DEX boundaries |
| address_role | Import | Deposit/hot wallet/protocol/other recorded role |
| confidence | Import | Attribution evidence strength, 0..1 |
| risk_level | Import | Explicit source designation, 0..100, not model probability |
| is_exposure_seed | Import | Explicit opt-in for risky label seeds |
| seed_category | Import | Reason/category carried with exposure paths |
| source_id | Import | Identifier of the intelligence source |
| source_reference | Import | Retrievable case/source reference |
| evidence_refs | Import | Required source evidence shown in API/UI |
| review_status | Import | pending/approved/rejected, latest revision wins |
| reviewed_by | Import | Required reviewer for decisions |
| review_note | Import | Audit detail, accessible with CLI claims |
| revision | CLI max(previous+1, clock) | Single-writer revision order |
| created_at_unix_ms | CLI | Recorded revision timestamp |
| inserted_at | ClickHouse default | Ingestion audit timestamp |
| token_metadata.reviewed_by | Manual import | Review provenance visible in asset metadata |
| token_metadata.source_reference | RPC worker/manual import | Observed block hash or source reference |

Active view: intelligence_claims_current deduplicates revisions; intelligence_active only exposes approved
claims with reviewer and source evidence. Rejection does not delete history.
Retention: preserve claim/review history as intelligence audit evidence; no automatic destructive TTL.

The investigation API reads existing canonical facts for graph, fingerprint, counterparties, daily activity
and semantic events. Metadata discoveries are consumed by bsc_token_metadata_worker; token_metadata_jobs
records bounded retry attempts and backoff. Verified manual metadata has precedence over RPC metadata.

Exposure is computed on demand and stored with the central immutable investigation snapshot. There is
no duplicate BSC path table or periodic risk assessment table. Live holdings are RPC reads, not stored
balance guesses; token discovery is explicitly incomplete when wallet history is incomplete.

این سند قرارداد schema نسخه `20260905_0006` است. SQL مرجع در `sql/migrations` قرار دارد و
`src/db/migrations.rs` نام و type ستون‌ها را هنگام startup اعتبارسنجی می‌کند.

## قواعد کلی

- `network_id` در تمام داده‌های شبکه‌ای فقط `eip155:56` است.
- آدرس EVM به شکل lowercase و hash به شکل `0x`-prefixed ذخیره می‌شود.
- زمان‌های `*_unix_ms` بر حسب millisecond UTC هستند؛ `inserted_at` زمان درج ClickHouse است.
- `inserted_at` evidence آن‌چین نیست و فقط برای deduplication/replay استفاده می‌شود.
- raw receipt ذخیره نمی‌شود؛ فقط فیلدهای مصرفی آن در `transactions` و `evm_logs` قرار می‌گیرند.
- raw trace JSON ذخیره نمی‌شود؛ فقط transferهای دارای value در `address_relationships` نوشته می‌شوند.
- جدول ML، risk prediction، bytecode عمومی، state trie و mempool وجود ندارد.
- `block_state_revision` هر fact را به همان commit بلاکی وصل می‌کند که آن را تولید کرده است؛ rowهای یک
  تلاش قطع‌شده حتی با block hash یکسان وارد view canonical نمی‌شوند.
- در Phase 5 جدول‌های `sync_state` و `ingestion_benchmarks` نیز producer و consumer واقعی دارند و failureها
  توسط repair worker مصرف می‌شوند.
- در Phase 6 فقط intelligence دارای آخرین review با وضعیت `approved` و `enabled=1` می‌تواند event semantic بسازد.

## مالکیت و retention

| Table | Producer | Primary consumers | Retention |
|---|---|---|---|
| `schema_migrations` | `bsc_schema` | schema validator/operator | دائمی |
| `ingested_blocks` | Phase 3 block commit coordinator | canonical views, gap/reorg scanner | دائمی |
| `transactions` | Phase 3 receipt-aware ingestor | investigation, features, fee analysis | دائمی |
| `evm_logs` | Phase 3 receipt/log ingestor | transfer and semantic decoders | دائمی |
| `address_relationships` | Phase 4 transfer/trace extractor | graph, holdings, paths, exposure | دائمی |
| `transaction_features` | Phase 6 deterministic semantic classifier | fingerprint and investigation | دائمی |
| `semantic_aml_events` | Phase 6 versioned event producers | investigation, bridge correlation | دائمی |
| `protocol_contract_registry` | `bsc_import_protocols` | semantic classifier, audit/review | دائمی و append-only |
| `token_metadata` | Phase 7 metadata worker/manual import | holdings and UI | دائمی، آخرین row در view |
| `token_metadata_discoveries` | Phase 4 transfer/log extractor | metadata queue and provenance | دائمی |
| `token_metadata_jobs` | Phase 7 metadata scheduler/worker | worker and operations | دائمی، آخرین state در view |
| `sync_state` | Phase 5 commit coordinator | auto-resume and lag status | دائمی، یک state جاری |
| `ingestion_failures` | Phase 3/5 ingestion stages | retry worker and operations | resolved: 180 روز؛ active: بدون TTL |
| `ingestion_benchmarks` | Phase 5 benchmark command | capacity/storage review | 365 روز |

## schema_migrations

- `migration_id String`: شناسه immutable و مرتب migration.
- `description String`: توضیح انسانی migration.
- `checksum String`: SHA-256 متن SQL؛ تغییر فایل اعمال‌شده را متوقف می‌کند.
- `applied_at DateTime64(3)`: زمان ثبت موفق migration.

## ingested_blocks

- `network_id LowCardinality(String)`: هویت شبکه.
- `block_number UInt64`: ارتفاع بلاک و کلید اصلی state.
- `block_hash String`: hash canonical انتخاب‌شده برای این ارتفاع.
- `parent_hash String`: کنترل continuity و تشخیص reorg/gap.
- `block_timestamp_unix_ms UInt64`: زمان بلاک برای partition facts.
- `transaction_count UInt32`: تعداد transaction اعلام‌شده در block.
- `log_count UInt32`: تعداد log ذخیره‌شده پس از تکمیل receipts.
- `receipt_data_complete UInt8`: یک فقط وقتی receipt همه transactionها حاضر است.
- `trace_data_complete UInt8`: یک فقط وقتی trace لازم همه transactionها حاضر است.
- `canonical UInt8`: صفر برای invalidate کردن height و یک برای state canonical.
- `ingestion_status LowCardinality(String)`: `pending`, `complete` یا `failed`.
- `rpc_provider LowCardinality(String)`: نام امن provider برای reproducibility.
- `rpc_client_version String`: نسخه client/node بدون credential.
- `state_revision UInt64`: revision یکنواخت صعودی برای replace کردن state یک height.
- `indexed_at_unix_ms UInt64`: زمان پایان commit بلاک.
- `updated_at DateTime64(3)`: زمان درج ClickHouse.

`ingested_blocks_canonical` برای هر `(network_id, block_number)` بیشترین revision را انتخاب می‌کند و فقط
state دارای `canonical=1` و `ingestion_status='complete'` را نشان می‌دهد.

## transactions

- `network_id`: شبکه transaction.
- `tx_hash`: شناسه transaction و lookup مستقیم.
- `block_hash`: اتصال fact به hash canonical، نه فقط ارتفاع.
- `block_number`: ارتفاع بلاک.
- `block_timestamp_unix_ms`: زمان بلاک.
- `block_state_revision`: revision دقیق marker بلاک که این transaction را visible می‌کند.
- `transaction_index`: ترتیب transaction در بلاک.
- `from_address`: sender بازیابی‌شده و normalizeشده.
- `to_address`: recipient؛ برای contract creation خالی است.
- `contract_address`: آدرس contract ساخته‌شده از receipt؛ در غیر creation خالی است.
- `nonce`: nonce sender.
- `transaction_type`: type عددی envelope EVM.
- `value`: مقدار native BNB در واحد wei.
- `input_selector`: چهار byte اول calldata؛ برای call بدون selector خالی است.
- `input_data`: calldata کامل فشرده برای semantic replay.
- `status`: نتیجه execution؛ صفر یا یک.
- `gas_limit`: gas limit envelope.
- `gas_used`: gas مصرف‌شده از receipt.
- `effective_gas_price`: قیمت مؤثر gas از receipt.
- `fee_paid`: حاصل دقیق `gas_used * effective_gas_price`.
- `inserted_at`: version درج برای deduplicate replay.

`transactions_canonical` ابتدا replay تکراری را حذف می‌کند و سپس فقط row متصل به hash و
`current_revision` جاری در `ingested_blocks_canonical` را برمی‌گرداند.

## evm_logs

- `event_id`: شناسه deterministic از network/tx/log index.
- `network_id`: شبکه log.
- `block_hash`, `block_number`, `block_timestamp_unix_ms`: evidence بلاک.
- `block_state_revision`: revision commit بلاکی که log در آن validate و flush شده است.
- `tx_hash`, `transaction_index`: transaction و موقعیت آن.
- `log_index`: ترتیب log در receipt/block.
- `contract_address`: contract صادرکننده event.
- `topic0`: event signature برای فیلتر سریع؛ اگر topic ندارد خالی است.
- `topics`: همه topicها به ترتیب RPC.
- `data`: event data فشرده و بدون decode مخرب.
- `inserted_at`: version درج.

`evm_logs_canonical` فقط logهای hash و revision جاری بلاک را نشان می‌دهد.

## address_relationships

- `relationship_id`: شناسه deterministic هر movement مستقل.
- `network_id`: شبکه movement.
- `block_hash`, `block_number`, `block_timestamp_unix_ms`: evidence بلاک.
- `block_state_revision`: revision بلاکی که extractor Phase 4 باید از آن خوانده و edge را به آن متصل کند.
- `tx_hash`, `transaction_index`: transaction مبنا.
- `event_index`: log index یا index سطح بالای trace/native movement.
- `event_sub_index`: عضو ERC-1155 batch یا movement فرعی یک event.
- `trace_address`: مسیر callTracer برای internal transfer؛ برای log/native خالی است.
- `from_address`: مبدأ normalizeشده.
- `to_address`: مقصد normalizeشده.
- `asset_id`: native BNB یا canonical contract asset id.
- `token_id`: شناسه ERC-721/ERC-1155؛ برای asset fungible خالی است.
- `amount`: مقدار خام UInt256؛ decimals در metadata اعمال می‌شود، نه هنگام ingest.
- `transfer_type`: `native`, `internal`, `erc20`, `erc721` یا `erc1155`; mint/burn از zero-address
  endpoint و standard همین ستون استنتاج می‌شود.
- `inserted_at`: version درج.

zero address برای mint/burn evidence در raw table حفظ می‌شود؛ graph wallet در لایه consumer آن را wallet عادی نمی‌سازد.

## transaction_features

- `feature_id`: شناسه deterministic خروجی detector.
- `network_id`, `block_hash`, `block_number`, `block_timestamp_unix_ms`, `block_state_revision`,
  `tx_hash`: evidence transaction و revision دقیق producer input.
- `transaction_type`: دسته اصلی مانند transfer/swap/bridge/liquidity.
- `transaction_subtype`: زیرنوع versioned detector.
- `protocol`: نام entity/protocol از registry بررسی‌شده.
- `method_id`: selector اصلی call.
- `is_swap`, `is_bridge`, `is_mint`, `is_burn`: indicatorهای semantic.
- `is_liquidity_add`, `is_liquidity_remove`, `is_contract_call`: indicatorهای تکمیلی.
- `unique_assets`: تعداد assetهای یکتا در movementهای transaction.
- `participants`: تعداد addressهای یکتای مشاهده‌شده.
- `classification_confidence`: کیفیت evidence classification، نه احتمال پول‌شویی.
- `classification_source`: registry/decoder/evidence pattern مورد استفاده.
- `detector`, `detector_version`: نام و نسخه implementation تولیدکننده.
- `evidence_refs`: شناسه log/relationship/trace evidence.
- `inserted_at`: version درج.

## semantic_aml_events

- `event_id`: شناسه deterministic event semantic.
- `network_id`, `block_hash`, `block_number`, `block_timestamp_unix_ms`, `block_state_revision`,
  `tx_hash`: evidence transaction و revision دقیق producer input.
- `event_index`: index لاگ یا transaction-level signal که event را ایجاد کرده است.
- `event_type`: swap/bridge/liquidity/lending/staking/mixer interaction و موارد مشابه.
- `subject_address`: wallet اصلی event.
- `protocol`: نام protocol از registry.
- `protocol_contract`: contract تأییدشده‌ای که decoder را فعال کرده است.
- `counterparty_address`: قرارداد/طرف عملیاتی event برای query مستقیم.
- `correlation_key`: message/transfer key برای bridge یا correlation؛ در صورت نبود خالی.
- `asset_in`, `asset_out`: شناسه canonical دارایی ورودی/خروجی.
- `remote_network_id`: شبکه مقصد/مبدأ bridge در صورت وجود evidence رجیستری.
- `remote_asset`: فقط وقتی decoder دارایی remote را واقعاً اثبات کند؛ در decoder عمومی خالی می‌ماند.
- `bridge_direction`: `outbound` برای deposit و `inbound` برای withdrawal؛ برای eventهای دیگر خالی.
- `remote_receiver`: گیرنده bridge که از topic تنظیم‌شده استخراج شده؛ در صورت نبود evidence خالی.
- `bridge_message_id`: شناسه پیام bridge استخراج‌شده از topic؛ با `correlation_key` برای اتصال legها مصرف می‌شود.
- `amount_in`, `amount_out`: مقدار خام decimal string وقتی بیش از یک UInt256 یا مدل event لازم است.
- `detector`, `detector_version`: producer و نسخه منطق.
- `confidence`: confidence کیفیت evidence semantic، نه risk probability.
- `evidence_refs`: شناسه facts اصلی قابل query.
- `evidence_json`: attributes محدود و versioned که ستون ثابت جداگانه ندارند.
- `inserted_at`: version درج.

## protocol_contract_registry

- `network_id`: در این سرویس فقط `eip155:56`.
- `contract_address`: آدرس normalizeشده قرارداد؛ برای `system` می‌تواند actor بررسی‌شده BSC باشد.
- `protocol`, `protocol_type`, `contract_role`: نام، خانواده و نقش قرارداد در protocol.
- `decoder`: decoder allowlisted که classifier را فعال می‌کند.
- `remote_network_id`, `remote_contract_address`: metadata اتصال bridge در صورت شناخته‌شدن.
- `method_ids`, `method_event_types`: mapping هم‌اندازه selector چهار-byte به event semantic.
- `event_topics`, `event_types`: mapping هم‌اندازه topic0 به event semantic.
- `remote_receiver_topic_index`, `message_topic_index`: محل topicهای bridge با مقدار `-1` برای unknown.
- `source_id`, `source_reference`: منشأ intelligence و reference قابل ممیزی.
- `review_status`: `pending`, `approved` یا `rejected`.
- `evidence_confidence`: کیفیت منبع/طبقه‌بندی بین صفر و یک، نه احتمال پول‌شویی.
- `enabled`: کنترل فعال‌سازی آخرین revision بدون حذف تاریخچه.
- `registry_revision`: شمارنده صعودی برای همان آدرس؛ import همزمان یک آدرس مجاز نیست.
- `reviewed_by`, `review_note`: هویت reviewer و توضیح تصمیم؛ `approved` بدون reviewer رد می‌شود.
- `created_at_unix_ms`, `inserted_at`: زمان ایجاد revision در application و زمان درج ClickHouse.

`protocol_contract_registry_current` آخرین revision هر آدرس را نشان می‌دهد. view
`protocol_contract_registry_active` فقط آخرین revision تأییدشده و فعال را به classifier می‌دهد. خود جدول هیچ
revision قبلی را حذف نمی‌کند.

## token_metadata

- `network_id`: شبکه contract.
- `token_address`: contract normalizeشده.
- `token_standard`: `erc20`, `erc721`, `erc1155` یا `unknown`.
- `name`, `symbol`: خروجی ABI/manual source؛ ممکن است خالی باشند.
- `decimals Nullable(UInt8)`: null یعنی unknown؛ صفر یک مقدار معتبر است.
- `metadata_status`: complete/partial/failed/manual.
- `metadata_source`: RPC، reviewed import یا manual override.
- `is_verified`: یک فقط برای metadata بررسی‌شده.
- `observed_block`: finalized block مرجع RPC lookup.
- `created_at_unix_ms`: زمان ایجاد این observation؛ validity interval وجود ندارد.
- `inserted_at`: version درج برای `token_metadata_current`.

## token_metadata_discoveries

- `discovery_id`: شناسه deterministic discovery evidence.
- `network_id`, `token_address`: token کشف‌شده.
- `standard_hint`: حدس اولیه بر اساس event، بدون ادعای metadata نهایی.
- `discovered_block`: اولین/مرتبط‌ترین بلاک مشاهده.
- `block_hash`, `block_state_revision`: discovery را به commit دقیق block وصل می‌کند؛ فقط rowهای view
  `token_metadata_discoveries_canonical` باید توسط worker خوانده شوند.
- `tx_hash`: transaction ایجادکننده discovery.
- `evidence_id`: event/relationship مبنا.
- `created_at_unix_ms`: زمان ثبت discovery.
- `inserted_at`: version درج.

producer در هر block یک discovery برای هر `(token_address, standard_hint)` می‌سازد و پیش از insert،
discovery canonical بلاک‌های قبلی را با یک query دسته‌ای حذف می‌کند؛ replay همان block دوباره نوشته می‌شود
تا revision جدید کامل بماند. این قرارداد هم idempotency را حفظ می‌کند و هم write amplification توکن‌های پرتکرار را محدود می‌کند.

## token_metadata_jobs

- `network_id`, `token_address`: کلید job.
- `status`: pending/running/retry/complete/dead.
- `attempt_count`: تعداد تلاش bounded.
- `last_error_class`: کلاس sanitizeشده خطا، بدون URL یا secret.
- `next_attempt_at_unix_ms`: زمان retry با backoff.
- `updated_at_unix_ms`: زمان state transition.
- `inserted_at`: version درج برای `token_metadata_jobs_current`.

## sync_state

- `network_id`: یک cursor مستقل برای BSC.
- `next_block`: اولین بلاکی که هنوز commit کامل نشده است.
- `last_finalized_block`: آخرین height finalized commitشده.
- `last_finalized_block_hash`: hash همان height برای continuity check.
- `state_revision`: version صعودی cursor.
- `updated_at_unix_ms`: زمان commit cursor.
- `inserted_at`: زمان درج ClickHouse.

Cursor فقط بعد از flush موفق همه facts و ثبت block به حالت complete جلو می‌رود.

## ingestion_failures

- `failure_id`: شناسه پایدار failure/work item.
- `network_id`: شبکه.
- `block_number`, `block_hash`: scope بلاکی failure؛ hash تا زمانی که fetch انجام نشده می‌تواند خالی باشد.
- `stage`: fetch/receipt/log/trace/decode/write/commit.
- `error_class`: طبقه‌بندی bounded برای retry/metrics.
- `error_summary`: متن sanitizeشده کوتاه؛ response یا credential خام ذخیره نمی‌شود.
- `retryable`: تصمیم retry policy.
- `attempt_count`: تعداد تلاش.
- `status`: open/retrying/resolved/dead.
- `created_at_unix_ms`, `updated_at_unix_ms`, `resolved_at_unix_ms`: lifecycle failure.
- `inserted_at`: version درج برای `ingestion_failures_current`.

## ingestion_benchmarks

- `benchmark_id`: شناسه اجرای benchmark.
- `network_id`: شبکه تست.
- `start_block`, `end_block`, `completed_blocks`: محدوده و coverage.
- `transaction_count`, `log_count`, `relationship_count`, `feature_count`, `semantic_event_count`: حجم facts تولیدشده.
- `elapsed_ms`: مدت اجرا.
- `blocks_per_second`: throughput بلاک‌های commitشده در اجرای benchmark.
- `observed_live_blocks_per_second`: نرخ واقعی تولید بلاک از timestampهای همان canonical range.
- `live_rate_multiple`: نسبت سرعت ingest به نرخ مشاهده‌شده شبکه؛ release gate حداقل `2.0` است.
- `rows_per_second`: throughput کل rows.
- `compressed_bytes`, `uncompressed_bytes`: اندازه parts برای نسبت compression.
- `created_at`: زمان benchmark و مبنای TTL 365 روز.

## Viewها و هزینه storage

Viewهای `*_canonical` و `*_current` داده جدا ذخیره نمی‌کنند. آن‌ها query contract هستند و storage اضافه ندارند.
Monthly partition فقط روی facts پرحجم block-scoped استفاده شده است. جدول‌های state/metadata به‌علت اندازه کوچک partition
نمی‌شوند. index فقط برای queryهای قطعی wallet، tx hash، contract/topic، asset، event type و status تعریف شده است.
