# BSC AML Platform

سرویس مستقل BNB Smart Chain برای پروژه `AML_Whole`.

این شبکه قرار است همان خروجی عملیاتی TRON را ارائه کند:

- نمودار جریان وجوه یک کیف پول از داده ذخیره‌شده در ClickHouse
- fingerprint رفتاری، holdings، counterparties و semantic events
- جست‌وجوی source-to-target تا سقف 10 hop با اعلام محدودیت و truncation
- projection قابل بازسازی در Neo4j
- entity labels، clustering و exposure paths با منبع و evidence قابل ممیزی
- bridge evidence استانداردشده برای ادامه مسیر در شبکه‌های دیگر

## وضعیت فعلی

قابلیت‌های **Phase 6: Semantic AML evidence** پیاده‌سازی شده‌اند. `bsc_ingest` حالت‌های range،
auto-resume و follow دارد؛ fetch بلاک‌ها concurrent و bounded است، اما continuity validation، commit و cursor
به ترتیب block انجام می‌شوند. reorg با common ancestor و tombstone append-only ترمیم می‌شود و ابزارهای مستقل
replay، gap/dead-letter repair و benchmark وجود دارند. semantic eventها از registry بررسی‌شده، event topic،
method mapping و fund-flow evidence تولید می‌شوند. metadata/holdings، API و Neo4j هنوز پیاده‌سازی نشده‌اند
و BSC عمداً در gateway نمایش داده نمی‌شود؛ بنابراین این پوشه هنوز investigation کامل BSC نیست.

تست ظرفیت با ۲۰ بلاک واقعی روی RPC عمومی در بهترین اجرا `1.65 block/s`، حدود `1453 row/s` و compression
حدود `7.93x` ثبت کرد. این از ingest ترتیبی سریع‌تر است، ولی gate حداقل `2x` نرخ زنجیره را پاس نکرد؛ برای
بستن release gate باید همین benchmark روی BSC node یا RPC production بدون throttling اجرا شود.

وضعیت مرحله‌ها و معیار پذیرش آن‌ها در [Production Roadmap](docs/production-roadmap.md) قرار دارد.
معماری و قرارداد داده در [Architecture](docs/architecture.md) توضیح داده شده است.
مالک، writer، reader و retention هر جدول و ستون در [Data Dictionary](docs/data-dictionary.md) ثبت شده است.

## قرارداد ثابت شبکه

| مورد | مقدار |
|---|---|
| Network | BNB Smart Chain Mainnet |
| EVM chain id | `56` |
| Canonical network id | `eip155:56` |
| Native asset | `eip155:56/native:bnb` |
| Canonical address | `0x` + 40 lowercase hex characters |
| ClickHouse database | `bsc_aml` |
| API prefix | `/api/bsc` |
| Default local API port | `6001` |
| Path-search ceiling | `10` hops |
| AI/ML risk | خارج از scope فعلی و خاموش |

در شروع هر process، مقدار `eth_chainId` باید دقیقاً `0x38` باشد؛ mismatch باید process را متوقف کند.
تغییر endpoint نباید بتواند داده شبکه دیگری را داخل `bsc_aml` بنویسد.

## اجرای capability probe

فایل نمونه را کپی و فقط مقدار `BSC_RPC_URL` را با endpoint خودتان جایگزین کنید:

```powershell
Copy-Item .env.example .env
cargo run --locked --bin bsc_rpc_probe
```

خروجی JSON یکی از این وضعیت‌ها را دارد:

- `ready`: همه قابلیت‌های موردنیاز mode فعلی حاضرند و capability اختیاری هم کم نیست.
- `degraded`: نیازهای اجباری حاضرند، اما قابلیت اختیاری مانند trace روی RPC عمومی در دسترس نیست.
- `unready`: حداقل یک قابلیت اجباری production غایب است و process با exit code ناموفق تمام می‌شود.

در production باید `BSC_MODE=production` باشد. در این mode، trace و block receipts اجباری هستند؛
endpoint اتریوم یا endpoint فاقد این قابلیت‌ها پذیرفته نمی‌شود. URL و secretهای داخل آن در log یا report
چاپ نمی‌شوند.

## اجرای ClickHouse و schema

فایل environment را بسازید و حداقل `BSC_CLICKHOUSE_PASSWORD` را به یک رمز قوی تغییر دهید:

```powershell
Copy-Item .env.example .env
docker compose up -d
docker compose ps -a
docker compose logs bsc-schema
```

ClickHouse HTTP روی `127.0.0.1:38123` و native protocol روی `127.0.0.1:39000` در دسترس است. سرویس
`bsc-schema` یک job کوتاه‌عمر است: منتظر healthy شدن ClickHouse می‌ماند، migrationهای جدید را اجرا
می‌کند، تمام table/viewها را validate می‌کند و با exit code صفر خارج می‌شود.

برای دیدن objectهای ساخته‌شده بدون نوشتن رمز در command history:

```powershell
docker compose exec clickhouse sh -lc 'clickhouse-client --user "$CLICKHOUSE_USER" --password "$CLICKHOUSE_PASSWORD" --query "SHOW TABLES FROM bsc_aml"'
```

برای validation دستی schema موجود از host:

```powershell
cargo run --locked --offline --bin bsc_schema -- --check
```

فایل‌های `sql/migrations` پس از اجرا تغییر داده نمی‌شوند. هر تغییر schema باید migration جدید داشته باشد؛
تغییر checksum یک migration قبلی عمداً startup را fail می‌کند.

## اجرای reliable canonical ingestion

یک range مشخص را بدون حرکت cursor پردازش کنید:

```powershell
cargo run --locked --offline --bin bsc_ingest -- --mode range --start 119947828 --end 119947900
```

اولین اجرای auto به start نیاز دارد؛ اجراهای بعدی فقط از `sync_state_current.next_block` ادامه می‌یابند:

```powershell
cargo run --locked --offline --bin bsc_ingest -- --mode auto --start 119947828
cargo run --locked --offline --bin bsc_ingest -- --mode auto
```

برای follow دائمی در Docker، start را فقط بار اول در `.env` تنظیم کنید:

```powershell
docker compose --profile runtime up -d bsc-follow
docker compose logs -f bsc-follow
```

Replay cursor را جابه‌جا نمی‌کند و فقط همان evidence را با revision جدید بازسازی می‌کند:

```powershell
cargo run --locked --offline --bin bsc_replay -- --start 119947828 --end 119947900
cargo run --locked --offline --bin bsc_replay -- --hash 0xBLOCK_HASH
```

Repair فقط gapها، traceهای ناقص و failureهای باز را بازسازی می‌کند. dead-letter فقط با opt-in دوباره اجرا می‌شود:

```powershell
cargo run --locked --offline --bin bsc_repair -- --start 119947828 --end 119947900
cargo run --locked --offline --bin bsc_repair -- --start 119947828 --end 119947900 --include-dead
```

Benchmark نتیجه را در `ingestion_benchmarks` نگه می‌دارد:

```powershell
cargo run --release --locked --offline --bin bsc_benchmark -- --start 119947828 --end 119948027
```

`BSC_BLOCK_FETCH_CONCURRENCY` فقط fetch/receipt/trace را موازی می‌کند. `BSC_INGEST_BATCH_SIZE`،
`BSC_RPC_REQUEST_DELAY_MILLIS`، `BSC_REORG_MAX_DEPTH` و `BSC_FAILURE_MAX_ATTEMPTS` مرزهای عملیاتی هستند.
تمام endpointهای `BSC_RPC_FALLBACK_URLS` در startup بررسی و درخواست‌ها میان endpointهای معتبر round-robin
می‌شوند؛ خطای retryable همان درخواست را به endpoint بعدی منتقل می‌کند.

در `BSC_RECEIPT_FETCH_MODE=auto` ابتدا `eth_getBlockReceipts` استفاده می‌شود و فقط در صورت unavailable
بودن method به receiptهای تکی با `BSC_RECEIPT_CONCURRENCY` محدود fallback می‌کند. response ناقص، receipt
گم‌شده، index ناسازگار یا block جدیدتر از finalized باعث failure می‌شود و marker کامل نوشته نمی‌شود.

در `BSC_TRACE_MODE=auto`، ingestion برای هر block از `debug_traceBlockByNumber` و `callTracer` استفاده
می‌کند. اگر RPC عمومی متد trace یا historical state آن block را نداشته باشد، block با
`trace_data_complete=false` و بدون internal edge commit می‌شود؛ native و token edgeها همچنان کامل‌اند.
در `required` این gap خطاست و marker نوشته نمی‌شود. برای backfill کامل production باید node/RPC دارای
trace تاریخی استفاده شود. raw trace ذخیره نمی‌شود و فقط movementهای واقعی `CALL`، `CREATE/CREATE2` و
`SELFDESTRUCT` ذخیره می‌شوند؛ `DELEGATECALL`، `STATICCALL` و `CALLCODE` edge جعلی تولید نمی‌کنند.

برای بررسی ingest:

```powershell
docker compose exec clickhouse sh -lc 'clickhouse-client --user "$CLICKHOUSE_USER" --password "$CLICKHOUSE_PASSWORD" --query "SELECT block_number, transaction_count, log_count, trace_data_complete, current_revision FROM bsc_aml.ingested_blocks_canonical ORDER BY block_number DESC LIMIT 10"'
```

برای دیدن edgeهای استخراج‌شده:

```powershell
docker compose exec clickhouse sh -lc 'clickhouse-client --user "$CLICKHOUSE_USER" --password "$CLICKHOUSE_PASSWORD" --query "SELECT transfer_type, count() FROM bsc_aml.address_relationships_canonical GROUP BY transfer_type ORDER BY transfer_type"'
```

Cursor فقط بعد از marker کامل جلو می‌رود. اگر process پس از marker و قبل از cursor قطع شود، restart marker را
می‌بیند، factها را دوباره نمی‌نویسد و cursor را ترمیم می‌کند. اختلاف hash موجب rewind تا common ancestor و
نامرئی‌شدن revision orphan از viewهای canonical می‌شود؛ raw evidence برای audit حذف نمی‌شود.

## رجیستری protocol و semantic evidence

فایل JSONL ابتدا بدون نوشتن در دیتابیس validate می‌شود:

```powershell
cargo run --locked --offline --bin bsc_import_protocols -- --file docs/protocol-registry.example.jsonl --dry-run
```

برای import واقعی، هر خط یک revision جدید append می‌کند. تنها رکورد `approved` که `enabled=true` و
`reviewed_by` معتبر دارد در `protocol_contract_registry_active` دیده می‌شود؛ `pending` و `rejected` هیچ eventی
تولید نمی‌کنند. اجرای import همزمان برای یک آدرس پشتیبانی نمی‌شود و باید از یک operator/job واحد انجام شود.

```powershell
cargo run --locked --offline --bin bsc_import_protocols -- --file .\registry-reviewed.jsonl
```

اجرای همان ابزار داخل Docker با mount فقط‌خواندنی:

```powershell
docker compose --profile tools run --rm -v "${PWD}/registry-reviewed.jsonl:/input/registry.jsonl:ro" bsc-import-protocols --file /input/registry.jsonl
```

decoderهای `amm_v2`, `amm_v3`, `aggregator`, `bridge_generic`, `lending_generic`, `staking_generic`,
`tornado_cash_v1`, `mixer_generic`, `reviewed_interaction` و `bsc_system` پشتیبانی می‌شوند. برای ABIهای خاص
هر protocol، آرایه‌های هم‌اندازه `method_ids/method_event_types` یا `event_topics/event_types` استفاده می‌شوند.
در bridge، `remote_receiver_topic_index` و `message_topic_index` اندیس topic را با احتساب `topic0=0` مشخص
می‌کنند؛ مقدار `-1` یعنی آن evidence قابل استخراج نیست.

خروجی‌ها در `transaction_features_canonical` و `semantic_aml_events_canonical` قابل مشاهده‌اند:

```powershell
docker compose exec clickhouse sh -lc 'clickhouse-client --user "$CLICKHOUSE_USER" --password "$CLICKHOUSE_PASSWORD" --query "SELECT event_type, protocol, count() FROM bsc_aml.semantic_aml_events_canonical GROUP BY event_type, protocol ORDER BY event_type, protocol"'
```

`confidence` و `classification_confidence` فقط کیفیت شواهد و decoder را نشان می‌دهند و هرگز احتمال پول‌شویی
یا نتیجه حقوقی نیستند. mixer/scam event فقط با intelligence تأییدشده تولید می‌شود؛ وجود تعامل نیز به‌تنهایی
اثبات رفتار غیرقانونی نیست.

## کنترل کیفیت

```powershell
.\scripts\check.ps1
```

تست واقعی ClickHouse شامل lifecycle migration، checksum drift، پنج مرز crash، reorg، dead-letter/requeue،
receipt/trace fallback، native/token/internal extraction و replay با canonical count ثابت است:

```powershell
.\scripts\test-clickhouse.ps1
```

Docker build برای Rust به شبکه وابسته نیست و archive کامل source dependencyها را مصرف می‌کند. فایل
`vendor-linux.tar.gz` به دلیل حجم در Git نگهداری نمی‌شود؛ پیش از اولین build و پس از هر تغییر در
`Cargo.lock` آن را روی host دارای cache به‌روزرسانی کنید:

```powershell
.\scripts\refresh-linux-vendor.ps1
docker build --network=none -t bsc-aml-service:local .
```

برای اجرای dependency audit پس از نصب `cargo-audit`:

```powershell
cargo install cargo-audit --locked
.\scripts\check.ps1 -Audit
```

## سیاست ساخت فایل‌ها

فایل‌ها و پوشه‌های runtime فقط زمانی ساخته می‌شوند که به binary و test واقعی متصل باشند.
از ساخت module، table، worker یا environment variable بلااستفاده خودداری می‌کنیم. هر ستون schema باید:

1. producer مشخص داشته باشد؛
2. consumer، query یا دلیل retention مشخص داشته باشد؛
3. test درج و خواندن داشته باشد؛
4. در data dictionary مستند شود.

## منابع رسمی BSC

- [BNB Smart Chain wallet configuration](https://docs.bnbchain.org/bnb-smart-chain/developers/wallet-configuration/)
- [BNB Smart Chain quick guide](https://docs.bnbchain.org/bnb-smart-chain/developers/quick-guide/)
- [Official BSC JSON-RPC reference](https://github.com/bnb-chain/bsc/blob/master/rpc/json-rpc-api.md)
- [Official BSC client repository](https://github.com/bnb-chain/bsc)
