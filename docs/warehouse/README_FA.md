# قرارداد مشترک ClickHouse؛ وضعیت مهاجرت

وضعیت این مرحله: اسکیمای مشترک به مسیر init و check هر چهار adapter وصل شده است.
در گام دوم، نوشتن دادهٔ پایه و خواندن API بیت‌کوین به جدول‌های مشترک منتقل شده‌اند.
فرمان bounded و قابل‌ادامهٔ `bitcoin_aml warehouse-backfill --max-blocks N`
تاریخچهٔ قدیمی بیت‌کوین را با کنترل تمام ستون‌های fact منتقل می‌کند؛ این فرمان
روی دیتابیس مستقر اجرا نشده است. writer/reader سه شبکه دیگر، جدول‌های تحلیلی،
آزمون restore و یکسان‌سازی نهایی همچنان باز هستند.

دامنه آزمون‌ها و محدودیت آن‌ها در [گزارش اعتبارسنجی این مرحله](VALIDATION.md) ثبت شده است.

**اولویت اصلاح‌شده:** ادامهٔ ING-04 تا تکمیل مسیر DB جلو نمی‌رود. تفکیک کد
بیت‌کوین، SQL نصب فعال و تفاوت baseline با schema کامل شبکه‌ها در
[بازبینی ساختار DB](DB_STRUCTURE_AUDIT_FA.md) آمده‌اند. اکنون DB-01 با
[فرهنگ داده و نقشهٔ مهاجرت](DB01_DATA_MAP_FA.md) و DB-03 با
[قرارداد مقدار و ارتقای نسخهٔ ۲](DB03_COLUMN_CONTRACT_FA.md) تکمیل شده‌اند.
DB-04 تا DB-09 و انتقال مسیرهای فعال هنوز بازند؛ قدم بعد DB-04 است.

## یک منبع ساختار

[تعریف نسخه ۱](../../shared/warehouse/schema/0001_warehouse.json) شامل ۲۹ جدول،
۲۰ view و ۳۷۶ ستون مستند است. [بسته Rust](../../shared/warehouse/README.md)
همین فایل را در هر چهار برنامه embed می‌کند. نام همه اشیا با `aml_` شروع می‌شود؛
این نام‌گذاری برای نصب افزایشی کنار تاریخچه قبلی است. نسخه اجراشده تغییر داده
نمی‌شود و اختلاف checksum به خطا منجر می‌شود.
نسخهٔ جاری ۲، همین ستون‌ها را نگه می‌دارد و قواعد مقدار نسخه‌دار را از
`shared/warehouse/schema/0002_column_contract.json` به آن اضافه می‌کند؛
checksum تاریخی نسخهٔ ۱ تغییر نکرده است.

| شبکه | database پیش‌فرض | network_id |
|---|---|---|
| TRON | `tron_db` | `tron:mainnet` |
| Ethereum | `ethereum_aml` | `eip155:1` |
| BSC | `bsc_aml` | `eip155:56` |
| Bitcoin | `bitcoin_aml` | `bip122:000000000019d6689c085ae165831e93` |

هر شبکه سرویس ClickHouse، database و volume خودش را **روی VM همان شبکه** دارد.
`shared/warehouse` فقط بستهٔ Rust و تعریف مشترک schema در زمان ساخت برنامه است؛
سرویس شبکه‌ای یا محل ذخیرهٔ مرکزی ایجاد نمی‌کند. آدرس `http://clickhouse:8123`
در Compose هر شبکه به کانتینر محلی همان پروژه اشاره می‌کند؛ برابر بودن این نام
به معنی اتصال VMها به یک سرور نیست. VM اصلی از API شبکه‌ها استفاده می‌کند و
Neo4j مرکزی را نگه می‌دارد. [بازبینی معماری و وضعیت فعلی](ARCHITECTURE_AUDIT_FA.md).

databaseها جدا هستند و ساختار جدید مشترک است. هر جدول `network_id` دارد و کلید
مرتب‌سازی با آن شروع می‌شود. init هویت database را ثبت می‌کند و استفاده با شبکه
دیگر را رد می‌کند. مسیر write آینده هم باید این محدوده را کنترل کند؛ ثبت هویت
جایگزین مجوزهای ClickHouse نیست. مسیر انتشار بیت‌کوین هم هویت شبکه و انطباق
header تمام factها را کنترل می‌کند.

## قرارداد ستون‌ها و انتشار

- ارتفاع و revision از نوع `UInt64`، ترتیب تراکنش/رویداد `UInt32` و زمان‌ها Unix
  میلی‌ثانیه‌اند. تبدیل ثانیه به میلی‌ثانیه باید overflow را بررسی کند.
- مقدار خام دارایی `UInt256` است و در مرز JSON رشته عدد صحیح ده‌دهی است؛ از float
  عبور نمی‌کند. یک ساتوشی برابر مقدار خام `1` با decimals برابر `8` است. fee بیت‌کوین
  هم ساتوشی است؛ output total به معنی یک انتقال address-to-address نیست.
- آدرس، مبلغ و پوشش غیرقابل‌تعیین nullable هستند. صفر، رشته خالی و `null` قابل
  جایگزینی با یکدیگر نیستند. وضعیت قابلیت نامرتبط باید در API `not_applicable` باشد.
- `aml_sync_state_current` مرز انتشار شبکه است. fact فقط وقتی در view canonical
  ظاهر می‌شود که بلاک complete و canonical باشد، زیر tip منتشرشده قرار بگیرد و
  network/height/hash/commit/revision دقیقاً با بلاک معتبر تطبیق داشته باشد.
- fence جدید در rollback ارتفاع را پایین می‌آورد؛ evidence خام حذف نمی‌شود.
  writer بیت‌کوین checkpoint را پس از ثبت و تطبیق facts و block commit منتشر می‌کند.
  این قرارداد هنوز به معنی تبدیل خودکار finality و reorg همه writerهای قدیمی نیست.
- جدول‌های MergeTree از revision، partition و index تعریف‌شده در قرارداد استفاده
  می‌کنند. در baseline مشترک TTL وجود ندارد تا evidence در جریان مهاجرت حذف نشود.
  سیاست retention و تنظیمات اتصال یکسان همچنان کار باز DB-09 هستند.
- intelligence، cluster و exposure باید علاوه بر revision به نسخه detector/policy،
  evidence و snapshot منبع وابسته باشند. وجود جدول و view `current` به معنی معتبر
  بودن claim یا تازه‌بودن تحلیل نیست؛ reader آینده باید review، revocation و
  `source_revision` را کنترل کند.

## موجودی قدیمی و نگاشت

[موجودی ماشین‌خوان](legacy-inventory.json) از اجرای DDLهای داخل Git روی ClickHouse
موقت استخراج شده است؛ از دیتابیس مستقر یا داده مشتری خوانده نشده است. شامل تمام
ستون‌ها، کلیدها، engine، codec، TTL در engine_full، view SQL و checksum فایل DDL است:

| شبکه | table/view قدیمی | ستون |
|---|---:|---:|
| TRON | ۳۷ | ۴۵۳ |
| Ethereum | ۳۸ | ۵۷۱ |
| BSC | ۲۹ | ۵۰۷ |
| Bitcoin | ۱۱ | ۱۱۳ |

`source_references` محل اشاره به نام هر جدول در Rust را با شماره خط و حدس نقش
reader/writer ثبت می‌کند. اشاره متنی اثبات فعال‌بودن مسیر اجرا نیست؛ مالکیت runtime
و مسیرهای query ساخته‌شده پویا هنوز باید برای DB-01 تأیید شوند.

| مفهوم | نگاشت از schema فعلی | مقصد مشترک / قاعده تبدیل |
|---|---|---|
| بلاک | `ingested_blocks` در TRON/Ethereum/BSC؛ `bitcoin_block_commits` در BTC | `aml_ingested_blocks`؛ commit جدید فقط بعد از تکمیل facts و اعتبارسنجی hash/revision |
| تراکنش | `transactions`؛ `bitcoin_transactions` | `aml_transactions`؛ BTC: height→number، txid→tx_hash، time×1000؛ آدرس فرستنده واحد `null` |
| رابطه | `address_relationships`؛ input/output بیت‌کوین | `aml_address_relationships`؛ رابطه مستقیم account در سه شبکه، address/script→transaction→address/script در BTC |
| زمان | TRON `timestamp`، ETH/BSC `block_timestamp_unix_ms`، BTC `block_time` ثانیه‌ای | `block_timestamp_unix_ms`؛ واحد هر منبع قبل از تبدیل از writer بررسی شود |
| مبلغ | SUN/wei/token raw؛ BTC `*_sats` | `amount_raw UInt256` و `asset_id` و decimals؛ مبالغ قدیمی با مقیاس نامعلوم قرنطینه شوند |
| checkpoint | TRON `last_synced_block`؛ ETH همراه hash؛ BSC `next_block` و `state_revision`؛ BTC tip/revision | `aml_sync_state`؛ start/tip/hash/revision واقعی، بدون ساخت hash یا پوشش فرضی |
| feature / semantic | `transaction_features` و `semantic_aml_events` در شبکه‌های فعلی | جدول‌های هم‌نام با پیشوند `aml_`؛ provenance و detector version حفظ شود |
| metadata | `token_metadata` و job/evidenceهای discovery | `aml_asset_metadata` و `aml_metadata_jobs`؛ BTC native با decimals=8 و منبع مشخص |
| identity / exchange | TRON `address_entity`، ETH `address_entities`، BSC `intelligence_claims` و review/source | `aml_intelligence_sources/claims/reviews`؛ status و confidence و evidence حفظ شود، label تأییدنشده seed نشود |
| خوشه و exposure | claim/membership/run/seed/pathهای قدیمی | خانواده‌های `aml_cluster_*` و `aml_exposure_*`؛ نسخه، جهت، تخصیص مبلغ و stale بودن حفظ شود |
| ریسک | policy/rule/signal/assessment موجود اتریوم و خروجی مرکزی | خانواده `aml_risk_*` و `aml_wallet_risk_*`؛ نبود evidence→UNKNOWN، نه صفر |
| عملیات | failure / benchmark قدیمی | `aml_ingestion_failures` و `aml_ingestion_benchmarks`؛ attempt/error/scope حفظ شود |
| خام تخصصی | BTC vin/vout، EVM logs، TRON contracts | `aml_bitcoin_inputs/outputs`، `aml_evm_logs`، `aml_tron_contracts`؛ همه در هر چهار database ایجاد می‌شوند |

این جدول **نقشه تبدیل چهار شبکه** است؛ گام دوم فقط دادهٔ پایهٔ بیت‌کوین را پوشش
می‌دهد. داده‌ای که provenance، زمان یا واحد قابل‌اثبات ندارد نباید با مقدار ساختگی
به جدول canonical منتقل شود. داده اختصاصی باقی‌مانده در `raw_evidence_json` یا
جدول تخصصی نگهداری می‌شود؛ نیازهای نامپوشش قبل از cutover باید به نسخه بعدی قرارداد بروند.

مسیرهای اصلی write برای ممیزی بعدی: batcherهای TRON در
`dockerizd_tron/app/src/services/tron/batcher`، writer/ingestion اتریوم در
`dockerizd_ethereum/src/storage`، store در `dockerizd_bsc/src/ingestion` و
`dockerizd_bitcoin/src/bitcoin/ingestion.rs`، `src/storage/writer.rs` و `src/db`. مسیرهای API و worker جداگانه هم باید
پیش از انتقال readerها با موجودی کامل تطبیق داده شوند.

## گام دوم: دادهٔ پایهٔ بیت‌کوین

جدول‌های فعال عبارت‌اند از `aml_transactions`، `aml_bitcoin_inputs`،
`aml_bitcoin_outputs`، `aml_address_relationships`، `aml_ingested_blocks` و
`aml_sync_state`. نصب تازه جدول اختصاصی `bitcoin_*` ایجاد نمی‌کند. در ارتقا،
جدول‌های قبلی برای ممیزی حفظ می‌شوند و فقط ابزار backfill آن‌ها را می‌خواند.

انتشار شامل reservation، نوشتن factهای کامل، تطبیق همه ستون‌ها، complete marker
و سپس fence است. retry از revision ثبت‌شده عبور می‌کند؛ بلاک نیمه‌تمام حتی پس
از restart در canonical دیده نمی‌شود. rollback فقط fence را به ancestor معتبر
برمی‌گرداند. رسید تکمیل انتقال قدیمی پس از reorg همچنان معتبر می‌ماند و ingest
به‌اشتباه دوباره درخواست انتقال تاریخچهٔ قبلی نمی‌کند.

مبالغ UInt256 رشتهٔ عدد صحیح‌اند؛ برای تراکنش چندورودی `from_address`،
`to_address` و `amount_raw` برابر null هستند. جزئیات و مجموع خروجی تراکنش در
`raw_evidence_json` و legs ورودی/خروجی حفظ می‌شوند. آدرس نامشخص null است و
subject نوع script دارد. receipt/trace coverage نامرتبط null است؛ prevout فقط
وقتی کامل منتشر می‌شود که تمام ورودی‌ها resolve شده باشند.

فرمان‌های Compose و کنترل توقف writer در [راهنمای بیت‌کوین](../../dockerizd_bitcoin/README.md)
آمده‌اند. توقف writer قدیمی یک پیش‌شرط عملیاتی است؛ stale-check جایگزین مالکیت
writer نیست. از گام چهارم، فرمان‌های تغییردهندهٔ بیت‌کوین قفل سیستم‌عامل روی
volume مشترک همان VM می‌گیرند؛ این قفل lease بین چند میزبان مستقل نیست.
رسید انتقال به revision ثابت منبع متصل است. تغییر checkpoint قدیمی، حتی با hash
یکسان، آماده‌بودن و ادامهٔ انتقال را متوقف می‌کند تا snapshot سازگار بازیابی شود.
حذف جدول‌های قدیمی، backfill تحلیلی و restore هنوز در DB-08 باز هستند.

## ترتیب اجرای امن

۱. backup داده و DDL و checkpoint موجود و آزمون restore در محیط مجزا.

۲. ساخت image جدید با Compose؛ named context مشترک توسط فایل‌های Compose تنظیم
شده است. init سه شبکهٔ قبلی، baseline قدیمی و سپس مشترک را بررسی/نصب می‌کند؛
init بیت‌کوین فقط قرارداد مشترک را نصب می‌کند و legacy موجود را دست‌نخورده نگه می‌دارد.
این مرحله هیچ جدول قدیمی را حذف نمی‌کند. آرشیو آفلاین وابستگی‌ها پس از تغییر
Cargo.lock باید دوباره تولید شود.

۳. بررسی فقط خواندنی روی هر VM:

```bash
bash scripts/vm.sh tron check-schema
bash scripts/vm.sh ethereum check-schema
bash scripts/vm.sh bsc check-schema
bash scripts/vm.sh bitcoin check-schema
```

این فرمان‌ها نسخه، checksum، ساختار و viewهای قرارداد مشترک و بررسی‌های legacy
مربوط به هر adapter را کنترل می‌کنند. `check-schema` به container API روشن نیاز
دارد و migration اجرا نمی‌کند. برای نصب از schema job موجود همان شبکه استفاده شود.
خطای drift نیازمند بررسی است؛ پاک‌کردن volume یا جایگزینی خودکار view مجاز نیست.

۴. مراحل باز سه شبکهٔ دیگر و جدول‌های تحلیلی: snapshot مبنا، backfill کنترل‌شده، write مشترک، مقایسه شمارش و جمع
مقدار/height/hash/revision، تست crash/reorg و سپس تغییر readerها. تا پایان این
مرحله‌ها API تاریخچهٔ موجود به داده جدید خالی وصل نمی‌شود. مسیر پایهٔ بیت‌کوین
طبق گام دوم منتقل شده است. بازنشسته‌کردن legacy بعد از تأیید
تطبیق و با migration مستقل انجام خواهد شد.

## بازتولید ممیزی و آزمون

روی ClickHouse موقت جدید با کاربر آزمایشی راهنمای بسته مشترک:

```bash
python scripts/warehouse_inventory.py --url http://127.0.0.1:8123 --disposable
python -m unittest discover -s scripts/tests -p 'test_warehouse_inventory.py'
```

ابزار ممیزی فقط localhost، نام‌های ثابت `aml_inventory_*` و database خالی را
می‌پذیرد؛ هیچ داده‌ای حذف نمی‌کند. تست integration مشترک ساختار چهار شبکه،
نصب تکراری، بازیابی init نیمه‌تمام، حفظ evidence قدیمی، drift ستون/view/checksum،
تداخل شبکه و انتشار canonical را روی ClickHouse واقعی بررسی می‌کند.
