# راهنمای پایگاه دادهٔ AML

اگر می‌خواهید بدانید یک تراکنش کجا ثبت شده، از ClickHouse همان شبکه شروع کنید. اگر می‌خواهید تحقیق ذخیره‌شده، هشدار یا پرونده را باز کنید، مرجع Neo4j مرکزی است.

## ۱. هر داده کجا می‌رود؟

| محل | نمونهٔ داده | نام پایگاه پیش‌فرض |
| --- | --- | --- |
| ClickHouse روی VM ترون | بلاک، تراکنش، قرارداد protobuf، لاگ رسید، فراخوانی داخلی، Energy و Bandwidth | `tron_db` |
| ClickHouse روی VM اتریوم | بلاک، تراکنش، انتقال، لاگ توکن و NFT، trace در صورت پوشش | `ethereum_aml` |
| ClickHouse روی VM BSC | بلاک، تراکنش، انتقال، لاگ توکن و trace در صورت پوشش | `bsc_aml` |
| ClickHouse روی VM بیت‌کوین | تراکنش، ورودی و خروجی UTXO، اسکریپت و رابطهٔ نوع‌دار | `bitcoin_aml` |
| Neo4j روی Main | تحقیق و گراف ذخیره‌شده، شاهد پل، فهرست پایش، هشدار و پرونده | `neo4j` |

چهار ClickHouse ساختار v6 مشترک دارند. `shared/warehouse` کتابخانهٔ Rust و منبع SQL همین ساختار است. هر VM نسخهٔ خودش از جدول‌ها، داده و volume را دارد. جدول تخصصی، مثل `aml_bitcoin_outputs`، در همه ایجاد می‌شود و فقط جایی پر می‌شود که همان نوع شاهد کاربرد دارد. Main شواهد هر شبکه را از API آن می‌گیرد.

مقدار `network_id` در ترون `tron:mainnet`، در اتریوم `eip155:1`، در BSC `eip155:56` و در بیت‌کوین `bip122:000000000019d6689c085ae165831e93` است. این مقدار را هنگام ترکیب داده‌ها حفظ کنید. یک رشتهٔ آدرس مشابه در اتریوم و BSC دو سوژهٔ مستقل دارد.

## ۲. بلاک چگونه قابل خواندن می‌شود؟

۱. ingestor بلاک و رسیدهای لازم را از RPC می‌گیرد و هویت شبکه، هش بلاک و شواهد را کنترل می‌کند.
۲. برای هر نوبت ثبت، ردیف‌های خام با `network_id`، `block_number`، `block_hash`، `block_timestamp_unix_ms`، `commit_id` و `revision` مشترک می‌نویسد.
۳. بعد از بررسی کامل‌بودن بسته، بلاک و checkpoint را منتشر می‌کند. اگر شاخه عوض شود، نشانگر rollback باعث می‌شود شواهد شاخهٔ قبلی از خواندن جاری خارج شوند.
۴. API به viewهای معتبر نگاه می‌کند. شمارهٔ بلاک به‌تنهایی برای پیوند دادن ردیف‌ها کافی نیست. در خواندن عادی از view آماده استفاده کنید.

| پسوند view | معنی کاربردی |
| --- | --- |
| `_canonical` | شاهد متعلق به بلاک کامل و شاخهٔ منتشرشده |
| `_current` | آخرین نسخهٔ ردیف در دامنهٔ کلید آن. ممکن است هنوز غیرفعال، ناقص یا قدیمی نسبت به ورودی باشد |
| `_active` | رکورد واجد شرط بازبینی، اعتبار زمانی یا فعال‌بودن منبع، بسته به view |
| `_visible` | خروجی تحلیلی یا متادیتایی که شرط انتشار و تازگی منبع را دارد |

شمارهٔ `revision` نسخهٔ رکورد است و زمان نیست. `source_revision` نسخهٔ ورودی تحلیل است و `output_revision` نسخهٔ خروجی مهرشدهٔ آن. یک ارزیابی قدیمی می‌تواند برای ممیزی باقی بماند، ولی در نمای جاری نامعتبر شود. در پنل `UNKNOWN` یعنی سیستم برای آن درخواست امتیاز معتبر جاری ندارد. تحلیل‌گر باید آن را از امتیاز صفر جدا بخواند.

## ۳. خواندن نوع و مقدار ستون‌ها

| نوع یا قرارداد | طرز خواندن |
| --- | --- |
| `UInt8/UInt32/UInt64/UInt256` | عدد صحیح بدون علامت. `UInt256` برای مبلغ بزرگ و دقیق استفاده می‌شود |
| `Nullable(T)` | مقدار می‌تواند `NULL` باشد. به معنی صفر، رشتهٔ خالی یا عدم وقوع نیست |
| `LowCardinality(String)` | رشتهٔ دسته‌ای که ClickHouse برای مقادیر تکراری بهینه نگه می‌دارد |
| `Array(T)` | فهرست از نوع مشخص. ترتیب در topicها، مسیر و trace اهمیت دارد |
| `Float64` | عدد اعشاری تقریبی برای وزن، نسبت یا امتیاز. مبلغ دقیق از `amount_raw` یا `value_sats` خوانده می‌شود |
| `String` با نام `*_json` | سند JSON داخل یک ستون رشته‌ای. قالب داخلی به خانواده و نسخهٔ شاهد بستگی دارد |
| `*_unix_ms` | زمان UTC بر حسب میلی‌ثانیه از مبدأ Unix. برای نمایش تهران، timezone را در query مشخص کنید |
| `UInt8` پرچم | برای فیلد پرچم، معمولاً صفر و یک. بعضی ستون‌های UInt8 مثل `decimals` تعداد هستند |

مبلغ قابل نمایش برابر مقدار خام تقسیم بر ۱۰ به توان `decimals` است. ۱٬۰۵۰٬۰۰۰ ساتوشی با ۸ رقم اعشار برابر ۰٫۰۱۰۵ BTC است. TRX شش و ETH و BNB هجده رقم اعشار دارند. اعشار توکن از متادیتای معتبر می‌آید. رشتهٔ خام مبلغ یا شناسهٔ NFT را هنگام انتقال به JavaScript به `Number` تبدیل نکنید.

[فرهنگ همهٔ جدول‌ها، ستون‌ها، نوع‌ها، پیش‌فرض‌ها و viewها](warehouse/DATA_DICTIONARY_FA.md) | [قرارداد ماشینی](warehouse/current-contract.json)

### فیلدهای درون JSON و آرایه‌ها

| فیلد | معنی محتوا |
| --- | --- |
| `source_cursor_json` | مشخصات اضافهٔ منبع یا cursor. checkpoint قابل اتکا را از ستون‌های صریح `aml_sync_state_current` بخوانید |
| `expected_counts_json` و `row_counts_json` | نگاشت نام جدول یا خانواده به شمار ردیف. اولی انتظار writer از بستهٔ بلاک، دومی شمار خروجی منتشرشدهٔ تحلیل |
| `payload_json`، `receipt_json` و `raw_evidence_json` | جزئیات اصلی RPC یا بستهٔ شاهد. کلیدهای داخلی مانند فیلدهای receipt و contract به شبکه و نوع رکورد وابسته‌اند |
| `features_json`، `evidence_json` و `allocation_json` | ویژگی‌های محاسبه‌شده، دلیل نتیجه، ارجاع مدرک و فرض‌های تخصیص. برای تفسیر به `detector_version`، `policy_version` و ستون‌های نوع رجوع کنید |
| `call_value_info_json` | آرایهٔ اصلی ارزش‌های فراخوانی داخلی ترون. ترتیب، مقدار و شناسهٔ دارایی هر آیتم طبق رسید نگه‌داری می‌شود |
| `topics` و `trace_address` | ترتیب topicهای لاگ و شمارهٔ فرزندهای مسیر trace. اولین topic در ClickHouse با `topics[1]` خوانده می‌شود |
| `path_subjects`، `relationship_ids` و `tx_hashes` | سوژه‌ها، شناسه‌های پایهٔ انتقال و هش‌های تراکنش پشت یک مسیر تحلیل |

در خروجی API، شیء `source_snapshot` فیلدهای ثابتی دارد. `contract`
نسخهٔ قالب، `snapshot_id` هش هویت کل snapshot، `network_id` شبکه،
`source_revision` نسخهٔ منبع و `initialized` وجود دامنهٔ منتشرشده را
می‌گویند. `start_block` ابتدای پوشش و `as_of_block` و `block_hash`
آخرین ارتفاع و هش منتشرشده‌اند. `observed_rpc_height` ارتفاع مشاهده‌شده
از RPC و `successor_blocks_observed` فاصلهٔ آن با بلاک منتشرشده است.
`history_complete` در این قرارداد به انتشار از ارتفاع صفر اشاره دارد.
`mempool_included` شمول mempool را مشخص می‌کند و `finality=not_asserted`
یعنی snapshot خودش ادعای قطعیت اجماع ثبت نمی‌کند.

## ۴. تمرین خواندن ClickHouse

روی VM همان شبکه، از ریشهٔ مخزن، CLI فقط‌خواندنی پروژه را باز کنید. `bitcoin` را برای شبکهٔ دیگر با `tron`، `ethereum` یا `bsc` جایگزین کنید.

```bash
bash scripts/vm.sh bitcoin db
```

ابتدا هویت و نقطهٔ انتشار را ببینید. فقط وقتی `initialized=1` است، دامنهٔ بلاک منتشرشده را تفسیر کنید.

```sql
SELECT network_id, schema_version, checksum
FROM aml_warehouse_identity FINAL;

SELECT network_id, initialized, start_block, last_block_number,
       last_block_hash, observed_rpc_height, revision
FROM aml_sync_state_current;
```

تعریف واقعی ستون‌ها را بررسی کنید. `default_kind` و `default_expression` می‌گویند ClickHouse در DDL برای مقدار غایب چه تعریفی دارد. اعتبار کاربردی هر رکورد را قواعد انتشار و کنترل ورودی تعیین می‌کنند. [مرجع رسمی system.columns](https://clickhouse.com/docs/reference/system-tables/columns)

```sql
SELECT position, name, type, default_kind, default_expression
FROM system.columns
WHERE database = currentDatabase()
  AND table = 'aml_address_relationships_canonical'
ORDER BY position;
```

یک نمونهٔ کوچک از تراکنش و پایه‌های انتقال را بخوانید. روی دیتابیس بزرگ، برای بررسی عملی یک بازهٔ ارتفاع یا هش مشخص هم اضافه کنید.

```sql
SELECT block_number, tx_hash, status, asset_id, toString(amount_raw) AS raw
FROM aml_transactions_canonical
ORDER BY block_number DESC LIMIT 10;

SELECT block_number, tx_hash, event_index, event_sub_index,
       from_subject_type, from_subject_id, to_subject_type, to_subject_id,
       asset_id, toString(amount_raw) AS raw, decimals, transfer_type
FROM aml_address_relationships_canonical
ORDER BY block_number DESC LIMIT 10;
```

در بیت‌کوین، ردیف ورودی به خروجی قبلی اشاره می‌کند. شناسهٔ دقیق خروجی، جفت هش تراکنش و `vout` است. برای جست‌وجوی یک ورودی خرج‌کننده، هر دو جزء را در شرط بگذارید.

```sql
SELECT tx_hash, output_index AS vout, address, value_sats, script_type
FROM aml_bitcoin_outputs_canonical
ORDER BY block_number DESC LIMIT 10;

SELECT tx_hash AS spending_tx, input_index AS vin, prev_tx_hash,
       prev_output_index AS previous_vout, value_sats
FROM aml_bitcoin_inputs_canonical
ORDER BY block_number DESC LIMIT 10;
```

یک تراکنش چندورودی و چندخروجی نشان می‌دهد چه چیزهایی خرج و ایجاد شده‌اند. گراف پروژه آدرس یا اسکریپت را به گره تراکنش وصل می‌کند و مقدار هر پایه را حفظ می‌کند. انتساب یک ورودی مشخص به یک خروجی مشخص از همین گراف نتیجه نمی‌شود.

روی اتریوم و BSC، برای یافتن لاگ یک رویداد، از تراکنش، آدرس قرارداد،
`log_index` و اولین topic کمک بگیرید. `data` با ABI قرارداد تفسیر
می‌شود. روی ترون از view جداگانهٔ لاگ و منابع استفاده کنید.

```sql
SELECT block_number, tx_hash, log_index, address, topics[1] AS signature, data
FROM aml_evm_logs_canonical
ORDER BY block_number DESC LIMIT 10;

SELECT block_number, tx_hash, log_index, address, topics[1] AS first_topic, data_hex
FROM aml_tron_logs_canonical
ORDER BY block_number DESC LIMIT 10;

SELECT block_number, tx_hash, toString(fee_raw) AS fee_sun,
       energy_usage_total, net_usage, receipt_result
FROM aml_tron_resources_canonical
ORDER BY block_number DESC LIMIT 10;
```

برای بررسی خطای دریافت یا نداشتن تحلیل جاری، این viewها نقطهٔ شروع‌اند. جدول ارزیابی ممکن است برای کیف پول موردنظر خالی باشد و نتیجهٔ تحقیق زنده در پاسخ API یا snapshot ذخیره‌شدهٔ Neo4j موجود باشد.

```sql
SELECT block_number, status, attempts, retryable, error_code
FROM aml_ingestion_failures_current
ORDER BY last_failed_at_unix_ms DESC LIMIT 10;

SELECT subject_id, as_of_block, source_revision, risk_level, risk_score
FROM aml_wallet_risk_assessments_visible
ORDER BY as_of_block DESC LIMIT 10;
```

## ۵. مدل Neo4j و معنی propertyها

در Neo4j به‌جای ستون ثابت، گره یا یال property دارد. property اختیاری ممکن است اصلاً روی رکورد وجود نداشته باشد. زمان‌هایی که با `_ms` یا `_unix_ms` تمام می‌شوند میلی‌ثانیهٔ UTC هستند. `owner` و شناسه‌های hashدار، محدودهٔ دسترسی را به کاربر یا نشست وصل می‌کنند. `lock_version` برای سری‌کردن تغییرات هم‌زمان است.

| گره | propertyها و کاربرد |
| --- | --- |
| `Investigation` | `id` شناسهٔ تحقیق، `network_id` هویت ثابت شبکه، `network` نام کوتاه، `mode` نوع تحقیق wallet یا paths، `address` ورودی اصلی، `target` مقصد تحلیل مسیر در صورت کاربرد، `owner` محدودهٔ مالک، `state` موقت یا ذخیره‌شده، `created_at_unix_ms` زمان ایجاد، `expires_at_unix_ms` انقضای تحقیق موقت، `saved_at_unix_ms` زمان ذخیرهٔ دائمی، `snapshot_hash` هش بسته برای کنترل یکپارچگی، `payload_json` پاسخ کامل تحقیق و `lock_version` کنترل هم‌زمانی |
| `InvestigationWallet` | `key` کلید گره در دامنهٔ تحقیق، `investigation_id` شناسهٔ تحقیق، `network_id` هویت شبکه، `subject_id` شناسهٔ سوژه، `subject_type` نوع سوژه مانند آدرس، تراکنش یا اسکریپت، `subject_key` شناسهٔ دامنه‌دار سوژه، `address` و `address_value` آدرس در صورت کاربرد، `label` متن نمایشی، `node_type` نوع نمایشی گره، `evidence_json` شاهد گره |
| `BridgeOccurrence` | `key` شناسهٔ یکی از دو سر شاهد پل، `network_id` شبکه، `tx_hash` تراکنش، `block_hash` هش بلاک، `block_number` ارتفاع، `log_index` موقعیت لاگ یا `input_index`/ `output_index` موقعیت ورودی یا خروجی بیت‌کوین، بسته به نوع رخداد |
| `KytWatch` | `id` شناسهٔ پایش، `owner` محدودهٔ مالک، `network` و `network_id` شبکه، `kind` آدرس یا تراکنش، `value` آدرس یا هش، `system_id` و `customer_id` شناسه‌های اختیاری سامانه و مشتری، `threshold` آستانهٔ قاعده، `rule` نام قاعده، `active` وضعیت پایش، `created_at_ms` و `updated_at_ms` زمان ایجاد و تغییر، `last_checked_at_ms` و `next_check_at_ms` زمان آخرین و نوبت بعدی بررسی، `result_json` نتیجهٔ آخر، `lease` شناسهٔ نوبت درحال اجرا، `match_alert_id` هشدار دورهٔ تطبیق جاری و `lock_version` کنترل هم‌زمانی |
| `KytAlert` | `id` شناسهٔ هشدار، `watch_id` پایش مبدأ، `owner` مالک، `network_id` شبکه، `kind` و `value` نوع و مقدار سوژه، `system_id` و `customer_id` شناسه‌های ارجاع، `rule` و `threshold` قاعده و آستانهٔ هنگام ثبت، `severity` شدت، `status` وضعیت رسیدگی، `created_at_ms` زمان ایجاد، `evidence_json` مدرک تصمیم، `assignee` و `assignee_hash` مسئول و کلید مجوز او، `assigned_at_ms` و `assigned_by` زمان و عامل واگذاری، `acknowledged_at_ms` و `acknowledged_by` زمان و عامل پذیرش، `resolved_at_ms` و `resolved_by` زمان و عامل بستن، `lock_version` کنترل هم‌زمانی |
| `KytAlert` و تحویل webhook | `delivery_status` مرحلهٔ تحویل، `delivery_attempts` تعداد تلاش، `delivery_due_at_ms` نوبت ارسال، `delivery_expires_at_ms` سقف زمانی صف، `delivery_last_attempt_at_ms` زمان آخرین تلاش، `delivery_last_error` علت خطای آخر، `delivery_last_http_status` وضعیت HTTP گیرنده، `delivery_lease` مالک تلاش جاری، `delivery_lock` نسخهٔ کنترل هم‌زمانی ارسال، `delivered_at_ms` زمان تحویل موفق |
| `InvestigationCase` | `id` شناسهٔ پرونده، `title` عنوان، `owner` مالک، `status` وضعیت گردش کار، `reviewer` و `reviewer_hash` بازبین و شناسهٔ دسترسی او، `created_at_ms`/ `updated_at_ms`/ `closed_at_ms` زمان ایجاد، تغییر و بستن، `revision` شمارهٔ تغییر، `evidence_count` شمار مدارک، `create_hash` هش محتوای درخواست ایجاد برای تکرار امن و `lock_version` کنترل هم‌زمانی |
| `CaseEvent` | `id` شناسهٔ رویداد، `case_id` پروندهٔ مربوط، `sequence` ترتیب در تاریخچهٔ همان پرونده، `kind` نوع اقدام مانند یادداشت یا پیوست، `actor` عامل ثبت، `at_ms` زمان اقدام، `details_json` جزئیات، `request_hash` هش ورودی برای تکرار امن و `evidence_key` کلید جلوگیری از پیوست تکراری |

| یال | معنی و propertyها |
| --- | --- |
| `CONTAINS` | تحقیق گرهٔ گراف یا یکی از دو سر شاهد پل را در بر دارد |
| `TRANSFER` | رابطهٔ مشاهده‌شده در تحقیق. `edge_id` شناسه، `from_key` و `to_key` کلید گره‌های دو طرف، `from_address`/ `to_address` آدرس در صورت کاربرد، `from_subject_id`/ `to_subject_id` شناسهٔ سوژه، `from_subject_type`/ `to_subject_type` نوع سوژه، `tx_hash` و `block_number` محل تراکنش، `transaction_index`/ `event_index`/ `event_sub_index`/ `trace_address` موقعیت دقیق، `amount` مقدار خام رشته‌ای، `asset_id` دارایی، `token_id` شناسهٔ NFT، `transfer_type` نوع پایه، `fund_continuity_claimed` و `amount_allocation_claimed` حدود ادعا، `evidence_json` مدرک، `investigation_id` و `network_id` دامنهٔ ذخیره |
| `BRIDGE_OPERATION` | پیوند دو رخداد در تحقیق. `id` شناسهٔ پایدار تطبیق، `operation_id` شناسهٔ پروتکلی یا outpoint، `confidence` درجهٔ اطمینان و `evidence_json` رسیدها، snapshotها و حدود ادعا |
| `HAS_EVENT` | پرونده یک رویداد تاریخچه دارد |
| `REFERS_TO` | رویداد پرونده به تحقیق ذخیره‌شده یا هشدار اشاره می‌کند. جزئیات شاهد تراکنش در `details_json` رویداد نگه داشته می‌شود |

روی Main از داخل کانتینر Neo4j وارد `cypher-shell` شوید و رمز Neo4j را در prompt وارد کنید. دستور زیر نام کاربری را تعیین می‌کند و رمز در خط فرمان قرار نمی‌گیرد. نمونه‌ها فقط خواندنی هستند.

```bash
docker compose --project-name aml-main --env-file .env exec neo4j cypher-shell -u neo4j
```

```cypher
MATCH (i:Investigation)
RETURN i.network, i.mode, i.state, count(*) AS investigations;

MATCH (i:Investigation)-[:CONTAINS]->(a:BridgeOccurrence)-[r:BRIDGE_OPERATION]->(b:BridgeOccurrence)
RETURN i.id, a.network_id, b.network_id, r.operation_id, r.confidence
LIMIT 10;

MATCH (c:InvestigationCase)-[:HAS_EVENT]->(e:CaseEvent)
RETURN c.id, c.status, e.sequence, e.kind
ORDER BY e.at_ms DESC LIMIT 10;
```

دسترسی مستقیم DBA به Neo4j در سطح پایگاه است. برای کار روزمره از پنل و API استفاده کنید تا محدودیت مالک تحقیق، واگذاری هشدار و دسترسی بازبین اجرا شود.
