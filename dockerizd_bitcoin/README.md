# Bitcoin AML adapter

> وضعیت: پیاده‌سازی پایه است و هنوز از نظر schema و امکانات AML با شبکه‌های دیگر هم‌سطح نیست. برنامه اصلاح شامل یکسان‌سازی ساختار دیتابیس هر چهار شبکه و تکمیل قابلیت‌ها در [TODO به‌روزشده](../TODO_BITCOIN.md) ثبت شده است. یکسان‌بودن نسخه ClickHouse به معنی یکسان‌بودن schema نیست.

اولویت عملی فعلی، رساندن دادهٔ بیت‌کوین به تحلیل قابل استفاده است؛ مهاجرت نهایی DB همچنان باز می‌ماند. [بازبینی ساختار و مسیرهای واقعی دیتابیس](../docs/warehouse/DB_STRUCTURE_AUDIT_FA.md).
DB-01 (فرهنگ داده و نگاشت) و DB-03 (قواعد ستون‌ها و ارتقای نسخهٔ ۲) تکمیل شده‌اند؛
قرارداد دادهٔ پایه در [DB-04](../docs/warehouse/DB04_CORE_CONTRACT_FA.md)، قرارداد تحلیلی/تخصصی نسخهٔ ۴ در [DB-05](../docs/warehouse/DB05_ANALYTICS_CONTRACT_FA.md) و مدل و query مشترک ارتباط‌ها در [DB-06](../docs/warehouse/DB06_RELATIONSHIPS_FA.md) قرار دارند؛ [DB-07: snapshot، visibility و نسخهٔ ۵](../docs/warehouse/DB07_VISIBILITY_FA.md) نیز تکمیل شده؛ مرحلهٔ بعد DB-08 است. [قرارداد و ارتقای نسخهٔ ۲](../docs/warehouse/DB03_COLUMN_CONTRACT_FA.md).
[خروجی DB-01 و شکاف‌های مانع انتقال داده](../docs/warehouse/DB01_DATA_MAP_FA.md).

زیرگام [DB-08A؛ backup و restore قابل‌تطبیق](../docs/warehouse/DB08A_BACKUP_RESTORE_FA.md)
با دیسک مستقل هر VM و آزمون بومی چهار شبکه تکمیل شد. schema نسخهٔ ۵ ثابت است؛
نگاشت/backfill و انتقال مسیرهای فعال همچنان DB-08B تا DB-08D هستند و DB-08 بسته نیست.
[نتیجهٔ آزمون](../docs/warehouse/DB08A_VALIDATION.md).

**AML-01 تکمیل شد:** source، import برچسب pending و review مستقل از جدول‌های مشترک
موجود مصرف می‌شوند؛ ادعای فعال به تحقیق، سیاست ریسک مرکزی و پنل می‌رسد. لغو یا
تغییر منبع، ریسک snapshot قبلی را از مصرف جاری خارج می‌کند. هیچ migration یا
مدل امتیازدهی تازه‌ای اضافه نشده است. [فرمان‌ها و مرز قابلیت](INTELLIGENCE.md)،
[گزارش پذیرش](INTELLIGENCE_VALIDATION.md). گام عملی بعد exposure است.

## ساختار کد و SQL فعال

بیت‌کوین اکنون همان تفکیک مسئولیت adapterهای ماژولی پروژه را دارد:
`src/db` برای client، migrations، SQL و query؛ `src/storage` برای writer و
قفل؛ `src/domain` برای مدل شواهد؛ `src/bitcoin` برای RPC، دریافت و ابزارها؛
`src/bin/bitcoin_init_schema.rs` برای job مستقل schema. نام‌های import قدیمی
در کتابخانه به‌صورت سازگار حفظ شده‌اند.

SQL نصب فعال در `sql/migrations/20261022_0001_shared_warehouse.sql` شامل همهٔ
۲۹ جدول و ۲۰ view مشترک است. از قرارداد مشترک تولید، در binary embed و قبل
از DDL با همان قرارداد تطبیق داده می‌شود. SQL قبلی به `sql/legacy/001_core.sql`
منتقل شده و فقط fixture ارتقا و ممیزی است. جزئیات و فرمان تولید: [راهنمای SQL](sql/README.md).
نسخهٔ ۲ از `sql/migrations/20261026_0002_column_contract.sql` مصرف می‌شود؛
قیدهای مقدار را پس از بررسی تاریخچه اضافه و ledger/identity را به‌روزرسانی
می‌کند. نسخهٔ ۳ از `sql/migrations/20261026_0003_core_evidence.sql`، ۲۸ ستون و view جاری benchmark را اضافه می‌کند. نسخهٔ ۴ از `sql/migrations/20261027_0004_analytics_specialized.sql` نتیجه را به ۳۸ جدول، ۴۰ view و ۶۶۸ ستون جدول می‌رساند. `--print-sql` هر چهار migration را چاپ می‌کند. ایجاد این جدول‌ها به معنی پیاده‌سازی workerها و قابلیت‌های AML بیت‌کوین نیست.

```bash
bitcoin_init_schema --print-sql  # No database or RPC configuration required.
bitcoin_init_schema --check     # Read-only schema validation.
# The existing bitcoin_aml schema [--check] remains a compatible alias.
```

ساختار کامل دیتابیس چهار شبکه هنوز یکسان نیست؛ سه شبکهٔ دیگر در کنار baseline
مشترک، جدول‌ها و reader/writerهای قدیمی دارند. وجود جدول تحلیلی در این SQL
به معنی پیاده‌شدن detector یا worker آن نیست.

مرحله اول: init هر چهار adapter اکنون [اسکیمای مشترک نسخه‌دار](../shared/warehouse/README.md)
را از یک منبع ایجاد و اعتبارسنجی می‌کند. در گام دوم، writer دادهٔ پایه و readerهای
بیت‌کوین از جدول‌های `aml_*` استفاده می‌کنند. تاریخچهٔ نسخه قبلی با فرمان مستقل
منتقل می‌شود؛ نصب baseline به‌تنهایی انتقال داده یا تکمیل امکانات AML نیست.
جزئیات و ترتیب انتقال در [راهنمای warehouse](../docs/warehouse/README_FA.md) آمده است.

### ارتقای دیتابیس قدیمی بیت‌کوین

ابتدا از داده و DDL نسخه قبلی backup بگیرید و تمام writerهای بیت‌کوین را متوقف
کنید. image جدید را بسازید. دیتابیس تازه فقط schema مشترک را ایجاد می‌کند؛
جدول‌های قدیمی موجود حذف یا بازنویسی نمی‌شوند.

```bash
bash scripts/vm.sh bitcoin pause
docker compose --project-directory dockerizd_bitcoin --env-file dockerizd_bitcoin/.env \
  -f dockerizd_bitcoin/docker-compose.yml build bitcoin-schema
docker compose --project-directory dockerizd_bitcoin --env-file dockerizd_bitcoin/.env \
  -f dockerizd_bitcoin/docker-compose.yml run --rm --no-deps bitcoin-schema bitcoin_aml schema
docker compose --project-directory dockerizd_bitcoin --env-file dockerizd_bitcoin/.env \
  -f dockerizd_bitcoin/docker-compose.yml run --rm --no-deps bitcoin-schema \
  bitcoin_aml warehouse-backfill --max-blocks 1000
# Repeat backfill until its JSON response has complete=true.
bash scripts/vm.sh bitcoin up --pull-never
```

ClickHouse موجود باید روشن بماند. هر اجرا حداکثر تعداد تعیین‌شده بلاک را از
checkpoint قبلی ادامه می‌دهد. count، ترتیب، hash، مجموع ورودی/خروجی و fee بررسی
می‌شوند و تمام ستون‌های چهار جدول fact پیش از انتشار با دادهٔ نوشته‌شده تطبیق
داده می‌شوند. بلاک خام منتشرنشده منتقل نمی‌شود؛ نقص یا gap انتقال را متوقف
می‌کند. API و ingest تا پایان انتقال تاریخچهٔ قبلی آماده نمی‌شوند. اجرای هم‌زمان
writer قدیمی و جدید مجاز نیست؛ این فرمان lease توزیع‌شده ایجاد نمی‌کند.

پس از انتقال، دریافت بلاک، resolve prevout، holdings، گراف و paths از viewهای
canonical مشترک می‌خوانند. رابطه‌های پایه `utxo_input` و `utxo_output` به ترتیب
آدرس/script→transaction و transaction→آدرس/script هستند؛ مقدار یک input به یک
output دلخواه تخصیص داده نمی‌شود. زمان در warehouse میلی‌ثانیه است و مبلغ خام
ساتوشی با decimals=8 و بدون float نگهداری می‌شود.

سرویس Rust مستقل بیت‌کوین برای VM پنجم. مثل شبکه‌های دیگر از ClickHouse استفاده می‌کند و دیتابیس اختصاصی آن `bitcoin_aml` است. نسخه پیش‌فرض ClickHouse همان `23.8` پروژه است. Neo4j فقط در VM اصلی قرار دارد؛ تحقیق، Export و بازکردن snapshot ذخیره‌شده از Analytical Node موجود عبور می‌کنند.

## راه‌اندازی

روی Bitcoin VM، از ریشه مخزن:

```bash
cp dockerizd_bitcoin/.env.example dockerizd_bitcoin/.env
# Edit dockerizd_bitcoin/.env before starting:
# BITCOIN_CLICKHOUSE_PASSWORD, AML_SERVICE_KEY, BITCOIN_API_BIND_ADDRESS
# For ingestion: BITCOIN_RPC_URL, BITCOIN_RPC_USER, BITCOIN_RPC_PASSWORD
bash scripts/vm.sh bitcoin up --build --api-only
bash scripts/vm.sh bitcoin check
bash scripts/vm.sh bitcoin check-schema
```

کلید `AML_SERVICE_KEY` باید با VM اصلی یکسان باشد. فایل خصوصی `.env` بیت‌کوین در Git ثبت نمی‌شود. در محیط توسعه این کامپیوتر یک فایل محلی با کلید اصلی و رمز آزمایشی ClickHouse آماده شده است؛ اطلاعات RPC باید تنظیم شوند. تنظیم نمونه IP برابر `10.20.0.24` است؛ IP واقعی همان VM را وارد کنید.

در `.env` ریشه روی VM اصلی:

```dotenv
AML_BITCOIN_UPSTREAM=http://10.20.0.24:6101
```

سپس Gateway و Analytical Node را بازسازی کنید:

```bash
bash scripts/vm.sh main up --build
```

بیت‌کوین در صفحه انتخاب شبکه و مسیر `/networks/bitcoin/` در دسترس است. API و ClickHouse بدون اتصال به Bitcoin Core بالا می‌آیند؛ آماده‌بودن API به معنی دریافت تاریخچه نیست. دیتابیس خالی صریحاً وضعیت بدون بلاک و تاریخچه ناقص نشان می‌دهد.

## Bitcoin Core و دریافت داده

یک Bitcoin Core روی mainnet با RPC خصوصی و دسترسی به بلاک‌های مورد نیاز لازم است. `getblock` با verbosity برابر ۳، در صورت وجود undo data، اطلاعات خروجی خرج‌شده را می‌دهد. اگر prevout در پاسخ نباشد، سرویس ابتدا خروجی‌های ذخیره‌شده را بررسی می‌کند و سپس `getrawtransaction` را به کار می‌گیرد؛ مسیر آخر برای تراکنش تاریخی به `txindex=1` نیاز دارد. نود pruned برای تاریخچه حذف‌شده کافی نیست. RPC باید از داخل کانتینر قابل دسترسی باشد؛ `127.0.0.1` داخل کانتینر، میزبان Core نیست.

نمونه تنظیم Core روی ماشین جدا، با محدودکردن شبکه دسترسی به RPC:

```ini
server=1
txindex=1
prune=0
# Configure rpcauth, rpcbind and rpcallowip for your private deployment.
```

نیازی به نصب Core در این image نیست؛ این سرویس فقط از RPC خواندنی آن استفاده می‌کند. شبکه و genesis در شروع ingestion بررسی می‌شوند. RPC عمومی سازگار با Ethereum یا REST explorer جایگزین Bitcoin Core RPC نیست.

### بررسی آمادگی نود پیش از دریافت داده

پس از ساخت image جدید و تنظیم RPC در `.env`، روی VM بیت‌کوین اجرا کنید:

```bash
bash scripts/vm.sh bitcoin probe
# If this VM already uses another Compose project name, keep that name:
bash scripts/vm.sh bitcoin probe --project YOUR_EXISTING_PROJECT
```

این فرمان ابزار یک‌بارمصرف `bitcoin-rpc-probe` را با image موجود اجرا می‌کند؛
ClickHouse، schema job، API و ingestion را راه‌اندازی نمی‌کند و checkpoint
را تغییر نمی‌دهد. فایل `.env` استاندارد Compose باید تنظیم شده باشد، ولی روشن
بودن دیتابیس لازم نیست. برای اجرای مستقیم باینری نیز فقط تنظیمات RPC و بازهٔ
دریافت لازم‌اند؛ رمز ClickHouse و کلید سرویس لازم نیستند:

```bash
bitcoin_aml probe
bitcoin_aml probe --compact
bitcoin_aml probe --help
```

خروجی JSON شامل `status`، علت‌های پایدار در `checks[].code`، ارتفاع بلاک و
header، وضعیت sync، زمان tip، pruning، وضعیت قابل‌مشاهدهٔ txindex و شواهد نمونه
است. آدرس endpoint، نام کاربری، رمز و متن خام خطای provider چاپ نمی‌شوند.

| وضعیت | کد خروج | معنی |
|---|---|---|
| `ready` | 0 | شرایط لازم نود و مسیر resolve ورودی‌های نمونه تأیید شده‌اند |
| `degraded` | 2 | محدودیت یا بخشی از آمادگی تأیید نشده؛ علت گزارش را بررسی کنید |
| `unready` | 1 | تنظیمات یا شواهد لازم برای شروع تازه با بازهٔ تعیین‌شده کافی نیست |

آزمون فقط ابتدا و انتهای بازهٔ تأییدشده را نمونه‌گیری می‌کند؛ در هر بلاک حداکثر
۸ ورودی و در مجموع حداکثر ۱۶ lookup تاریخی انجام می‌شود. سقف زمان کل ۱۲۰ ثانیه
است و timeout هر درخواست همان `BITCOIN_RPC_TIMEOUT_SECONDS` است. `ready` به معنی
تأیید کل تاریخچه، سلامت ClickHouse یا هم‌سطح‌شدن امکانات AML نیست.

نکته‌های تفسیر گزارش:

- بررسی بر اساس `BITCOIN_START_BLOCK` و `BITCOIN_END_BLOCK` تنظیم‌شده و شروع
  تازه است؛ checkpoint و خروجی‌های موجود در دیتابیس خوانده نمی‌شوند.
- تاریخچهٔ prune‌شده پیش از شروع درخواستی، شبکه/genesis اشتباه و نبود مسیر
  resolve ورودی برای شروع تازه از وسط زنجیره، `unready` هستند.
- اگر از genesis شروع می‌کنید ولی RPC اطلاعات prevout و lookup تاریخی نمی‌دهد،
  گزارش `degraded` است: دریافت ترتیبی باید خروجی‌های قبلی را از دیتابیس محلی
  حل کند؛ وجود و کامل‌بودن آن‌ها در probe بررسی نشده است.
- نبود `txindex` به‌تنهایی مانع نیست؛ undo/prevout قابل استفاده مسیر دیگری است.
  ادعای synced بودن index هم جای آزمون واقعی `getrawtransaction` را نمی‌گیرد.
  اگر بلاک نمونه ورودی خارجی نداشته باشد، این قابلیت آزمون‌نشده باقی می‌ماند.
- اگر verbosity تنظیم‌شدهٔ ۳ کار نکند اما ۲ قابل استفاده باشد، پیشنهاد در
  گزارش می‌آید؛ تنظیمات ingestion خودکار تغییر نمی‌کنند. ابتدا verbosity و
  fallback را صریحاً تنظیم و probe را دوباره اجرا کنید.

قرارداد RPC مطابق مستندات رسمی
[وضعیت نود](https://bitcoincore.org/en/doc/30.0.0/rpc/blockchain/getblockchaininfo/)،
[بلاک و undo/prevout](https://bitcoincore.org/en/doc/30.0.0/rpc/blockchain/getblock/)،
[وضعیت index](https://bitcoincore.org/en/doc/30.0.0/rpc/util/getindexinfo/) و
[lookup تراکنش](https://bitcoincore.org/en/doc/30.0.0/rpc/rawtransactions/getrawtransaction/)
است؛ آزمون‌های این مرحله با RPC کنترل‌شده انجام شده‌اند.

```bash
# Start the single writer; it resumes from the committed checkpoint.
bash scripts/vm.sh bitcoin up --pull-never
bash scripts/vm.sh bitcoin logs
bash scripts/vm.sh bitcoin check-runtime
bash scripts/vm.sh bitcoin pause-ingestion
bash scripts/vm.sh bitcoin resume
bash scripts/vm.sh bitcoin db --query 'SELECT * FROM aml_sync_state_current FORMAT Vertical'
```

پیش‌فرض از بلاک صفر و با ۶ confirmation است. مقدار `BITCOIN_START_BLOCK` را پس از شروع دیتابیس تغییر ندهید. با شروع از وسط تاریخچه، گراف و موجودی فقط پنجره مشاهده‌شده را پوشش می‌دهند. `BITCOIN_END_BLOCK` اختیاری و شامل همان بلاک است؛ worker پس از تکمیل بازه پایان می‌یابد، بنابراین برای اجرای مداوم آن را خالی بگذارید. فرمان `bitcoin_aml once` تا سر تأییدشده یا انتهای بازه پیش می‌رود و خارج می‌شود.

فقط **یک writer** مالک checkpoint است. از گام چهارم، فرمان‌های `ingest`، `once`،
`warehouse-backfill` و نصب `schema` روی VM بیت‌کوین قفل سیستم‌عامل می‌گیرند؛
فرمان رقیب پیش از دسترسی به داده رد می‌شود. جزئیات محدودهٔ قفل در بخش بعد است.

### دریافت محدود، retry و RPC جایگزین

بلاک‌ها در پنجره‌های محدود دریافت می‌شوند. fetch می‌تواند هم‌زمان باشد، اما
کنترل parent/hash، resolve، تطبیق شواهد و انتشار checkpoint به ترتیب ارتفاع
انجام می‌شود. hash بلاک پس از fetch و دوباره پیش از ثبت بررسی می‌شود. ورودی
مفقود، بلاک ناقص یا پنجرهٔ ناپیوسته با مبلغ صفر جایگزین نمی‌شود.

برای prevout فاقد undo، ابتدا خروجی‌های canonical دیتابیس دسته‌ای خوانده
می‌شوند؛ خروجی تراکنش قبلی همان بلاک محلی حل می‌شود. lookup تاریخی فقط برای
txidهای باقی‌مانده و یکتا انجام می‌شود. از پاسخ خام فقط خروجی‌های لازم نگه
داشته می‌شوند؛ کل تراکنش‌های تاریخی تا پایان بلاک در حافظه انباشته نمی‌شوند.

| تنظیم | پیش‌فرض و سقف |
|---|---|
| `BITCOIN_BLOCK_BATCH_SIZE` | ۲ بلاک، حداکثر ۸ |
| `BITCOIN_BLOCK_FETCH_CONCURRENCY` | ۲ درخواست هم‌زمان، حداکثر ۴ |
| `BITCOIN_PREVOUT_BATCH_SIZE` | ۱۲۸ outpoint/txid در هر دسته، حداکثر ۲۵۶ |
| `BITCOIN_PREVOUT_CONCURRENCY` | ۲ دستهٔ RPC هم‌زمان، حداکثر ۴ |
| `BITCOIN_MAX_PREVOUTS_PER_BLOCK` | ۱۰۰٬۰۰۰ outpoint فاقد undo، حداکثر ۵۰۰٬۰۰۰ |
| `BITCOIN_MAX_RESOLVED_BYTES` | ۶۴ MiB شواهد prevout، حداکثر ۱۲۸ MiB |
| `BITCOIN_RPC_MAX_RESPONSE_BYTES` | ۶۴ MiB پاسخ HTTP، حداکثر ۱۲۸ MiB |

پنجرهٔ نگه‌داری‌شدهٔ بلاک‌ها سقف ثابت ۱۲۸ MiB بر اساس اندازهٔ JSON دارد؛ اگر
پر شود، اندازهٔ batch را کاهش دهید. این اعداد سقف شواهدند؛ ظرفیت واقعی RAM
و نرخ mainnet باید روی نود مقصد benchmark شوند.

`BITCOIN_RPC_MAX_ATTEMPTS=4` شامل درخواست اول است. خطای transport، HTTP 429/5xx
موقت و warmup نود با backoff نمایی از `BITCOIN_RPC_RETRY_BASE_MS=250` و سقف
۱۰ ثانیه تکرار می‌شوند. خطای احراز هویت، خطای دائمی RPC و پاسخ با هویت/اندازهٔ
نامعتبر روی همان endpoint تکرار نمی‌شوند. تمام cloneهای client همان endpoint
فاصلهٔ حداقل `BITCOIN_RPC_REQUEST_INTERVAL_MS=50` را رعایت می‌کنند؛ تعداد اعضای
batch نیز جداگانه محدود است. اگر provider batch ندارد،
`BITCOIN_RPC_BATCH_ENABLED=false` درخواست‌های محدودِ تکی را فعال می‌کند.

حداکثر چهار RPC جایگزین را با originهای جداشده با کاما در
`BITCOIN_RPC_FALLBACK_URLS` قرار دهید. نام کاربری و رمز تنظیم‌شده به **همهٔ این
endpointهای صریح** ارسال می‌شوند؛ فقط نودهای مورداعتماد با همان دسترسی را
تعریف کنید. credential داخل URL، redirect و endpoint تکراری پذیرفته نمی‌شوند.
هر منبع پیش از مصرف باید mainnet/genesis معتبر داشته باشد. منبع جایگزین باید
حداقل تا ارتفاع منتشرشده جلو آمده و hash همان checkpoint را داشته باشد؛
عقب‌بودن یا اختلاف آن باعث rollback خودکار نمی‌شود. فقط primary اجازهٔ شروع
reorg کنترل‌شده را دارد. پس از failover، primary به‌طور دوره‌ای با
`BITCOIN_FALLBACK_COOLDOWN_SECONDS=30` دوباره امتحان می‌شود.

اگر تمام منبع‌ها نامناسب باشند، `once` با خطا و checkpoint آخرین بلاک سالم
خارج می‌شود. `ingest` منتظر می‌ماند و دوباره تلاش می‌کند؛ پس از رفع قطع RPC
تا پیش از رسیدن به سقف تلاش نیازی به restart نیست. خطای نوشتن ClickHouse اجرای writer را متوقف می‌کند؛
publication نامطمئن کورکورانه تکرار نمی‌شود و restart از وضعیت ذخیره‌شده ادامه
می‌دهد. نتیجهٔ خطا و شمارندهٔ تلاش در جدول مشترک `aml_ingestion_failures` حفظ می‌شوند.

### مالکیت writer روی VM بیت‌کوین

Compose برای schema/backfill، ingestion/once و repair/scan-gaps یک volume محلی به نام ثابت
`aml_bitcoin_writer_lock` روی `/var/lock/bitcoin` mount می‌کند. این نام با تغییر
نام پروژهٔ Compose عوض نمی‌شود. image پوشه را برای کاربر 10001 آماده می‌کند.
قفل با بسته‌شدن فرایند، حتی kill/crash، آزاد می‌شود؛ فایل lock را حذف نکنید.

این قفل برای **همان VM و همان filesystem مشترک** است؛ lease توزیع‌شده بین
میزبان‌های مستقل نیست. معماری پروژه برای هر شبکه یک VM دارد. همهٔ writerهای
آن دیتابیس باید همین محل را ببینند؛ API، probe و `schema --check` قفل writer
نمی‌گیرند. نسخهٔ قدیمی که قفل را نمی‌شناسد پیش از upgrade باید متوقف شود.

برای اجرای مستقیم باینری، فرمان‌های تغییردهنده به تنظیم صریح
`BITCOIN_WRITER_LOCK_DIR` نیاز دارند و در نبود آن خطا می‌دهند. مسیرهای متفاوت
یا اجرای native کنار Docker بدون اشتراک همان filesystem، مالکیت مشترک ایجاد
نمی‌کند. روی VM مسیر توصیه‌شده اجرای writer از Compose موجود است.

قرارداد پاسخ batch طبق [رابط رسمی JSON-RPC بیت‌کوین](https://github.com/bitcoin/bitcoin/blob/master/doc/JSON-RPC-interface.md)
و قفل مطابق [API رسمی File در Rust](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock)
است. [نتیجهٔ آزمون گام چهارم](INGESTION_VALIDATION.md).

## خطا، dead-letter و repair انتخابی

خطاهای source، پنجرهٔ دریافت، resolve، reconciliation و publication با شناسهٔ
`bitcoin:block:N` در همان دیتابیس محلی بیت‌کوین ثبت می‌شوند. جدول و view از
schema مشترک موجود استفاده می‌کنند؛ migration جدیدی ایجاد نشده است. پیام‌های
ذخیره‌شده کد و متن ثابت مرحله‌اند و متن خام RPC یا credential ذخیره نمی‌شود.
خطای fetch به ارتفاع ابتدای پنجره اشاره دارد؛ لزوماً بلاک خراب را تعیین نمی‌کند.

`BITCOIN_FAILURE_MAX_ATTEMPTS=10` سقف تلاش پایدار است؛
`BITCOIN_FAILURE_RETRY_SECONDS=1` فاصلهٔ retry خودکار را تعیین می‌کند. این
شمارنده تلاش منطقی بلاک/منبع است و با retry داخلی HTTP فرق دارد. هر تلاش
ناموفق منبع یک واحد، و موفقیت بعد از خطای قبلی یک واحد اضافه می‌کند. عبور از
سقف، وضعیت را `dead_letter` می‌کند و دریافت مرتب پشت آن می‌ماند. restart یا
تغییر endpoint شمارنده را صفر نمی‌کند. `once` درخواست دستی است و فاصلهٔ زمانی
retry را نادیده می‌گیرد؛ dead-letter را فقط repair صریح می‌تواند دوباره اجرا کند.

```bash
bash scripts/vm.sh bitcoin failures --status dead_letter --limit 50
# Paginate the next page using next_after_block from the JSON response:
bash scripts/vm.sh bitcoin failures --after-block 12345 --limit 50
# All mutating tools share the ingestion writer lock. Stop ingestion first:
bash scripts/vm.sh bitcoin pause-ingestion
bash scripts/vm.sh bitcoin scan-gaps --from-block 100 --to-block 199 --max-blocks 100
bash scripts/vm.sh bitcoin repair --failure-id bitcoin:block:12345 --max-blocks 1
bash scripts/vm.sh bitcoin resume
```

فرمان‌ها فقط ابزار یک‌بارمصرف را با `--no-deps --pull never` اجرا می‌کنند؛
ClickHouse باید از قبل روشن باشد. فیلترهای `failures` شامل `--status`،
`--from-block`، `--to-block`، `--after-block` و `--limit` با سقف ۲۰۰ است.
API معادل، `GET /api/bitcoin/ingestion/failures` با پارامترهای underscore و
همان service key است. `/status` شمار وضعیت‌ها و `next_failure` را اضافه می‌کند؛
این داده آخرین وضعیت عملیاتی ثبت‌شده است و snapshot تحلیلی یا اثبات freshness نیست.

repair فقط شناسه‌های صریح و یکتا را می‌پذیرد؛ برای چند بلاک `--failure-id` را
تکرار کنید و `--max-blocks` را حداکثر ۱۰۰۰ قرار دهید. بلاک جدید باید دقیقاً
بعد از checkpoint باشد؛ موارد انتخاب‌نشده یا فاصلهٔ بین انتخاب‌ها خودکار ingest
نمی‌شوند. هر اجرای repair پیش از RPC یک تلاش رزرو می‌کند و نتیجه همان تلاش را
ثبت می‌کند؛ پس از crash مالک بعدی وضعیت `repairing` را بدون افزایش دوباره
شمارنده به retry/dead-letter برمی‌گرداند. موفقیت قبلاً منتشرشده پس از restart
از وضعیت canonical تشخیص داده و failure مربوط resolved می‌شود.

scan فقط بازهٔ منتشرشده و حداکثر ۱۰٬۰۰۰ ارتفاع را بررسی می‌کند: وجود بلاک،
parent و تعداد تراکنش‌های canonical. این اسکن ممیزی کامل input/output نیست.
شکاف تاریخی با وضعیت `gap` ثبت می‌شود. repair برای شکاف واقعی پشت checkpoint
با `historical_gap_requires_replay` متوقف می‌شود؛ cursor را عقب نمی‌برد و
داده حذف نمی‌کند. بازپخش مستقل این بازه‌ها و بازتحلیل وابستگی‌ها کار **ING-04** است.
کد خروج repair صفر برای complete و یک برای blocked/failed است. اگر خود
ClickHouse قطع باشد، ثبت failure هم ممکن نیست؛ writer با خطا متوقف می‌شود.

راهنمای اجرای مستقیم: `bitcoin_aml failures --help`، `scan-gaps --help` و
`repair --help`. نتیجهٔ آزمون این گام در [FAILURE_VALIDATION.md](FAILURE_VALIDATION.md) ثبت شده است.

## شواهد و قرارداد خواندن

- مبالغ خام در schema مشترک از نوع `UInt256`، با واحد ساتوشی و decimals برابر ۸ هستند؛ مدل بیت‌کوین محدودهٔ عرضه را کنترل می‌کند و مبلغ API به رشته تبدیل می‌شود. تبدیل مقدار RPC با decimal دقیق انجام می‌شود.
- ورودی‌ها، خروجی‌ها، script، txid، outpoint، شماره بلاک و کارمزد جدا ذخیره می‌شوند. Coinbase فرستنده ساختگی ندارد.
- آدرس از script واقعی استخراج و برای mainnet با checksum اعتبارسنجی می‌شود: P2PKH، P2SH، SegWit و Taproot. scriptهای بدون آدرس یکتا، از جمله bare multisig و P2PK، به گره script متصل می‌شوند.
- گراف `address → transaction → address` است. وصل‌کردن یک ورودی به خروجی خاص، تشخیص change، مالکیت مشترک و برچسب CoinJoin به‌عنوان پول‌شویی ادعا نمی‌شوند.
- همه factها همگام نوشته می‌شوند؛ سپس commit و در پایان checkpoint منتشر می‌شود. خوانندگان فقط viewهای `*_canonical` را می‌خوانند. داده پیش از checkpoint، حتی در توقف ناگهانی، قابل مشاهده نیست.
- هر reorg تا `BITCOIN_REORG_MAX_DEPTH` به ancestor مشترک برمی‌گردد؛ checkpoint پایین می‌آید و شواهد orphan بلافاصله از viewها خارج می‌شوند. reorg عمیق‌تر با خطای مشخص متوقف می‌شود. داده خام orphan برای ممیزی باقی می‌ماند.
- تحقیق و holdings در ابتدا و انتها revision را کنترل می‌کنند؛ اگر ingestion بین queryها تغییر کند، درخواست با 503 قابل تکرار پایان می‌یابد و snapshot مخلوط ذخیره نمی‌شود.
- holdings از UTXOهای تاریخچه دریافت‌شده است، mempool را شامل نمی‌شود و موجودی immature coinbase را تا گذشتن ۱۰۰ بلاک از بلاک ایجاد، جدا نشان می‌دهد. outpointهای تکراری تاریخی BIP30 با آخرین occurrence محاسبه می‌شوند. مالکیت کل کیف پول از یک آدرس نتیجه نمی‌شود.
- مسیرها به‌صورت خواندنی و محدود جست‌وجو می‌شوند؛ حداکثر ۱۰ transaction hop، ۲۰۰ آدرس و یک مسیر. فقط ۲۰ تراکنش اخیر هر آدرس بررسی می‌شود و خروجی محدودیت را اعلام می‌کند. عدم نتیجه به معنی نبود مسیر نیست؛ مسیر پیدا‌شده هم اثبات پیوستگی همان ساتوشی‌ها نیست.

جدول‌های فعال دادهٔ پایه: `aml_transactions`، `aml_bitcoin_inputs`،
`aml_bitcoin_outputs`، `aml_address_relationships`، `aml_ingested_blocks` و
`aml_sync_state`. تمام readerها از viewهای canonical مشترک استفاده می‌کنند؛
UTXO با ترتیب occurrence برای outpointهای تاریخی تکراری محاسبه می‌شود. جدول خام
ممکن است دادهٔ منتشرنشده یا orphan داشته باشد. DDL مشترک داخل binary بسته‌بندی
می‌شود. جدول‌های قدیمی `bitcoin_*` فقط در ارتقای نسخه قبلی و ابزار انتقال مصرف
می‌شوند؛ نصب تازه آن‌ها را ایجاد نمی‌کند.

APIهای محافظت‌شده با `X-AML-Service-Key`:

```text
GET /status
GET /api/bitcoin/ingestion/failures?status=dead_letter&limit=50
GET /api/bitcoin/wallet/{address}/investigation?limit=40
GET /api/bitcoin/wallet/{address}/holdings
GET /api/bitcoin/wallet/{source}/paths/{target}?max_hops=3&max_nodes=200
```

`/health` و `/ready` عمومی‌اند. شناسه شبکه `bip122:000000000019d6689c085ae165831e93` است. API تحقیق حداکثر ۱۰۰ تراکنش و ۱۸۰۰۰ ورودی/خروجی برمی‌گرداند؛ گراف محدودشده اعلام می‌شود. پنل مرورگر برای خوانایی حداکثر ۱۰۰ گره و ۵۰۰ یال رسم می‌کند؛ JSON کامل برگشتی در بخش شواهد قابل مشاهده است.

در این نسخه، adapter بیت‌کوین هنوز منبع برچسب تأییدشده یا تحلیل exposure ریسک ندارد. سیاست مرکزی بدون چنین شواهدی `UNKNOWN` و امتیاز `null` برمی‌گرداند، حتی اگر تراکنش زیادی وجود داشته باشد. پوشش AML فعلی شامل تحقیق UTXO، شواهد قابل ممیزی، مسیر و snapshot است؛ امتیاز ریسک معتبر به منبع intelligence نیاز دارد.

## VM، اجرای محلی و image آفلاین

```bash
# All APIs locally, including Bitcoin; configure its .env first.
python scripts/run_local.py up --build
# Linux single-host launcher (explicit optional chains):
bash scripts/aml.sh up --build --with-bsc --with-bitcoin
# New deployment: main + four chain VMs.
python scripts/provision_vms.py up --build
# Include the Bitcoin image in the offline delivery bundle:
bash scripts/export-images.sh --include-bsc --include-bitcoin
# Confirm health and optionally investigate an ingested Bitcoin address:
bash scripts/smoke-test.sh --bitcoin-address 1BoatSLRHtKNngkdXEeobR76b53LETtpyT
```

افزودن Bitcoin VM به state چهار VM قبلی تنها زمانی خودکار پذیرفته می‌شود که prefix، Ubuntu و منابع هر چهار VM قبلی و رمزهایشان تغییر نکرده باشند. سپس همان VMها استفاده و فقط VM جدید ساخته می‌شود. image bundle باید شامل هر ۸ image باشد. منابع نمونه برای تست‌اند؛ ۵۰ GiB برای تاریخچه کامل بیت‌کوین و Core کافی نیست و ظرفیت واقعی باید جدا تعیین شود.

پورت API برابر 6101 و پورت‌های HTTP/Native دیتابیس 48123/49000 هستند. دیتابیس روی localhost منتشر می‌شود؛ API فقط برای Main VM در firewall مجاز باشد. می‌توانید از همان سرور ClickHouse موجود هم استفاده کنید، مشروط به دیتابیس مجزای `bitcoin_aml` و دسترسی مناسب؛ Compose پیش‌فرض مثل شبکه‌های فعلی کانتینر ClickHouse همان VM را بالا می‌آورد.

## آزمون

```bash
cd dockerizd_bitcoin
cargo test --locked
cargo test --locked --test rpc_probe
cargo clippy --locked --all-targets -- -D warnings
# Only with a disposable, empty ClickHouse using bitcoin_test / bitcoin-test-only:
BITCOIN_TEST_CLICKHOUSE_URL=http://127.0.0.1:8123 cargo test --locked --all-targets -- --include-ignored
```

آزمون یکپارچگی از RPC کنترل‌شده و ClickHouse واقعی استفاده می‌کند و دیتابیس‌های
`bitcoin_aml`، `bitcoin_migration_incomplete` و fixtureهای `bitcoin_ing02_*` و `bitcoin_ing03_*` را پر می‌کند؛ آن را به دیتابیس واقعی
وصل نکنید. استقرار روی VM واقعی یا sync کامل mainnet جزو نتیجه این تست نیست.

منابع قرارداد RPC و آدرس:
[Bitcoin Core getblock](https://bitcoincore.org/en/doc/25.0.0/rpc/blockchain/getblock/)،
[rust-bitcoin Address](https://docs.rs/bitcoin/latest/bitcoin/struct.Address.html).
