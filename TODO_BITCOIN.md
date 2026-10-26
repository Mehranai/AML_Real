# یکسان‌سازی دیتابیس چهار شبکه و تکمیل امکانات بیت‌کوین

**وضعیت: تکمیل نشده.** نسخه فعلی بیت‌کوین فقط زیرساخت دریافت بلاک و تحقیق پایه UTXO دارد. پایان‌یافته‌بودن فهرست قبلی و پاس‌شدن تست‌های آن، به معنی یکسان‌بودن دیتابیس‌ها یا هم‌سطح‌شدن امکانات AML نبود. این فهرست جایگزین معیار تکمیل قبلی است.

## الزامات قطعی

- دیتابیس TRON، Ethereum، BSC و Bitcoin باید از **یک schema نسخه‌بندی‌شده مشترک** استفاده کند: نام جدول و view، ستون‌ها، نوع داده، کلیدها، engine، index، قرارداد canonical و روش migration یکسان باشد. یکسان‌بودن نام ClickHouse یا نسخه image کافی نیست.
- مطابق توضیح نهایی شما، هر شبکه روی VM خودش، **سرویس ClickHouse، database، volume و تنظیمات اتصال مستقل** دارد؛ منظور از یکسان‌سازی، هماهنگی ساختار جدول‌هاست. هیچ ClickHouse مرکزی یا database مشترک بین شبکه‌ها در این معماری وجود ندارد.
- منبع تعریف schema و قرارداد داده باید مشترک باشد؛ چهار کپی مستقل که به‌مرور متفاوت شوند، معیار پذیرش را پاس نمی‌کنند. نام database و شناسه شبکه تنظیمات استقرار هستند.
- جدول‌های تخصصی UTXO و داده خام شبکه‌ها در همان بسته schema مشترک تعریف شوند؛ تفاوت شبکه با داده و capability مشخص شود. برای قابلیت نامرتبط، وضعیت `not_applicable` ثبت شود و رکورد یا مقدار معنایی ساختگی تولید نشود.
- امکانات قابل‌اعمال شبکه‌های فعلی باید برای بیت‌کوین هم پیاده شوند؛ انتقال EVM، ERC-20 یا یک آدرس فرستنده واحد نباید به تراکنش چندورودی بیت‌کوین تحمیل شود.
- معماری چهار VM شبکه و یک VM اصلی حفظ شود. Neo4j تحقیق‌ها در VM اصلی بماند و سیاست ریسک از مسیر مرکزی موجود، با شواهد قابل ممیزی و بدون افزودن خودسرانه ML، ارائه شود.

## نتیجه مقایسه با کد فعلی

| بخش | وضعیت مرجع در شبکه‌های فعلی | شکاف بیت‌کوین / اقدام لازم |
|---|---|---|
| schema پایه | جدول‌های `ingested_blocks`، `transactions`، `address_relationships`، `transaction_features`، `semantic_aml_events`، `sync_state` و viewهای مرتبط وجود دارند؛ مسیرهای قدیمی هنوز متفاوت‌اند | قرارداد مشترک نصب شده و دادهٔ پایهٔ بیت‌کوین از شش جدول `aml_*` مصرف می‌شود؛ انتقال سه شبکهٔ دیگر و جدول‌های تحلیلی باز است |
| شناسه و زمان | Ethereum/BSC از `network_id` و `block_timestamp_unix_ms` استفاده می‌کنند؛ در بخش‌هایی از TRON نام‌هایی مانند `chain` و `timestamp` وجود دارد | دادهٔ پایهٔ بیت‌کوین اکنون network/height/hash/commit/revision مشترک و زمان میلی‌ثانیه دارد؛ یکسان‌سازی مصرف کل چهار شبکه باز است |
| migration | Ethereum/BSC migrationهای مرحله‌ای دارند؛ TRON baseline و سازوکار schema زمان اجرا دارد | init/check مشترک نسخه‌دار و backfill مستقل دادهٔ پایهٔ بیت‌کوین اضافه شده‌اند؛ ارتقای تمام تاریخچهٔ تحلیلی و restore هنوز باز است |
| intelligence | source، برچسب، review و entity در شبکه‌های فعلی وجود دارد؛ نام‌گذاری یکسان نیست، از جمله `address_entity`، `address_entities` و `intelligence_claims` | storage، import و چرخه review وجود ندارد؛ `entities` همیشه خالی است |
| clustering و exposure | در TRON/Ethereum worker و در BSC تحلیل هنگام تحقیق و ابزار intelligence وجود دارد | کشف کاندیدا، عضویت نسخه‌دار، seed، propagation و نتیجه exposure وجود ندارد |
| ریسک | Analytical Node از شواهد برچسب و exposure استفاده می‌کند؛ Ethereum جدول‌های policy، signal و assessment نیز دارد | adapter شواهد ریسک تولید نمی‌کند؛ با زیادشدن تراکنش‌ها هم خروجی فعلی `UNKNOWN` می‌ماند |
| تحقیق آدرس | fingerprint، activity، asset flows، counterparties، semantic events، entities، clusters، exposure و coverage ارائه می‌شود؛ قالب پاسخ‌ها نیازمند هماهنگی است | fingerprint فقط دو شمارنده دارد؛ بیشتر پنل‌ها و داده‌های تحلیلی غایب‌اند |
| عملیات | ابزارهای probe، replay، repair، failure tracking، benchmark و worker در سطح‌های مختلف وجود دارند | backfill، probe، failure/dead-letter پایدار، اسکن gap و repair انتخابی اضافه شده‌اند؛ بازپخش مستقل تاریخچه و benchmark هنوز باز هستند |
| پنل و آزمون | پنل‌ها و آزمون‌های هر شبکه موجودند، ولی به معنی برابری کامل بین شبکه‌ها نیستند | پنل ساده و تست پایه اضافه شده؛ آزمون برابری schema، امکانات AML و پذیرش روی داده واقعی انجام نشده است |

مرجع مقایسه: `dockerizd_tron/app/sql/init_database_tron.sql`، `dockerizd_tron/app/src/db/tron_schema.rs`، migrationهای `dockerizd_ethereum/sql` و `dockerizd_bsc/sql/migrations`، APIهای investigation و [راهنمای تحلیل‌های خودکار](docs/AUTOMATIC_ANALYTICS_FA.md). وجود جدول به‌تنهایی نشانه فعال‌بودن قابلیت نیست؛ writer، reader، worker و UI آن هم باید بررسی شوند.

## بخش پایه موجود؛ فقط در همین محدوده تأیید شده

- [x] سرویس Rust و اتصال خواندنی به Bitcoin Core، مدل ورودی/خروجی و اعتبارسنجی آدرس mainnet اضافه شده است.
- [x] ذخیره دقیق ساتوشی، checkpoint، انتشار پس از ثبت کامل بلاک و rollback پایه reorg پیاده شده است.
- [x] API پایه سلامت، وضعیت، investigation، holdings و paths با service key وجود دارد.
- [x] انتخاب شبکه در Gateway، اتصال به Analytical Node، snapshot مرکزی و نقش VM بیت‌کوین اضافه شده است.
- [x] آزمون‌های پایه Rust، ClickHouse موقت با RPC کنترل‌شده، launcher و مرورگر اجرا شده‌اند؛ دامنه آن‌ها در [گزارش قبلی](dockerizd_bitcoin/VALIDATION.md) مشخص است.

این موارد باید در جریان مهاجرت حفظ و دوباره آزمون شوند؛ آن‌ها مراحل باز زیر را تکمیل نمی‌کنند.

## تمرکز ادامهٔ کار

**اصلاح ترتیب کار پس از بازبینی ساختار:** عبور به ING-04 زود بود. تا تعیین و اجرای مسیر DB، قابلیت ingestion جدید اضافه نمی‌شود. پوشه‌بندی بیت‌کوین و SQL نصب فعال اصلاح شده‌اند؛ DB-01 با ممیزی همهٔ خانواده‌ها و DB-03 با قرارداد مقدار نسخهٔ ۲ تکمیل شده‌اند. یکسان‌سازی نهایی جدول‌ها و مسیرهای فعال در DB-04 تا DB-09 باقی است.

- [x] **DB-STRUCT:** بیت‌کوین به `db/{mod,client,migrations,queries,sql}`، `storage`، `domain`، `bitcoin` و binary مستقل schema تفکیک شد؛ SQL نصب فعال در `sql/migrations` از قرارداد مشترک تولید و پیش از DDL کنترل می‌شود. SQL قدیمی در `sql/legacy` حفظ شد. [گزارش آزمون](docs/warehouse/DB_STRUCTURE_VALIDATION.md).
- [x] **DB-AUDIT پایه:** مسیر فعال init/write/read دادهٔ پایهٔ چهار شبکه و تفاوت ساختار کامل با baseline بررسی و مستند شد؛ برابری baseline چهار database موقت دوباره آزمون شد. [ممیزی](docs/warehouse/DB_STRUCTURE_AUDIT_FA.md). ممیزی تمام خانواده‌ها، ستون‌ها و مسیرهای worker در DB-01 زیر تکمیل شد؛ اجرای زندهٔ workerها و پذیرش داده همچنان باز است.

**قدم بعدی قطعی: DB-04**؛ DB-01 با فرهنگ داده و نگاشت تکمیل شد و DB-03 در ۲۰۲۶-۱۰-۲۶ با ۲۱۷ قاعدهٔ مقدار، اعتبارسنجی Rust، CHECKهای مشترک و ارتقای قابل‌ادامهٔ نسخهٔ ۱ به ۲ تکمیل شد. [قرارداد ستون‌ها](docs/warehouse/DB03_COLUMN_CONTRACT_FA.md)، [نتیجهٔ آزمون‌ها](docs/warehouse/DB03_VALIDATION.md). اکنون جدول‌ها/viewهای دادهٔ پایه و مسیرهای مصرف چهار شبکه، سپس DB-05 تا DB-09 و پذیرش P0 تکمیل می‌شوند. ING-04 تا این مرحله جلو نمی‌رود.

بازبینی معماری در ۲۰۲۶-۱۰-۲۲ انجام شد: جداسازی ClickHouse هر VM تأیید شد؛ تغییرات آزمایشی انتقال اتریوم در کد فعال وجود ندارند و کارهای قبلی بیت‌کوین حفظ شده‌اند. [گزارش بررسی و آزمون‌های مجدد](docs/warehouse/ARCHITECTURE_AUDIT_FA.md).

**گام سوم تکمیل شد — ING-01 بیت‌کوین:** ابزار فقط‌خواندنی و مستقل از ClickHouse برای بررسی نود و prevout و فرمان `bash scripts/vm.sh bitcoin probe` اضافه شدند. [گزارش probe](dockerizd_bitcoin/PROBE_VALIDATION.md).

**گام چهارم در ۲۰۲۶-۱۰-۲۳ تکمیل شد — ING-02 بیت‌کوین:** دریافت پنجره‌ای مرتب، resolve دسته‌ای، retry/backoff و rate limit، fallback با کنترل شبکه و checkpoint، و قفل writer مشترک همان VM پیاده و آزمون شدند. [گزارش دریافت](dockerizd_bitcoin/INGESTION_VALIDATION.md).

**گام پنجم در ۲۰۲۶-۱۰-۲۳ تکمیل شد — ING-03 بیت‌کوین:** ثبت پایدار failure و تلاش‌ها، retry/dead-letter، repair انتخابی بلاک بعدی، اسکن محدود gap و API مشاهدهٔ وضعیت پیاده و آزمون شدند. [گزارش خطا و repair](dockerizd_bitcoin/FAILURE_VALIDATION.md). ادامهٔ ING-04 پس از مراحل ناتمام DB خواهد بود؛ رفع شکاف تاریخی و reprocessing همچنان باز هستند. [ممیزی ساختار و مسیرهای واقعی DB](docs/warehouse/DB_STRUCTURE_AUDIT_FA.md).

## P0 — یکسان‌سازی واقعی دیتابیس هر چهار شبکه

پیشرفت مرحله اول در ۲۰۲۶-۱۰-۲۲؛ جزئیات در [راهنمای مهاجرت](docs/warehouse/README_FA.md):

- [x] استخراج ساختار DDL فعلی هر چهار شبکه روی ClickHouse موقت و ثبت ستون، کلید، engine و view؛ snapshot جدید DB-01 دو ledger زمان اجرا را نیز پوشش می‌دهد. مالکیت کد، dispatch عمومی و workerها در فرهنگ داده ثبت شد؛ مشاهدهٔ داده و وضعیت زندهٔ VMها همچنان جزو پذیرش باز است.
- [x] تعریف یک قرارداد مشترک با ۲۹ جدول و ۲۰ view در `shared/warehouse`، شامل جدول‌های تحلیلی و داده تخصصی هر چهار شبکه.
- [x] افزودن init نسخه‌دار، checksum و هویت شبکه، تشخیص drift و بازیابی init نیمه‌تمام بدون حذف تاریخچه قدیمی.
- [x] اتصال بسته مشترک به مسیر schema هر چهار adapter و فرمان فقط‌خواندنی `scripts/vm.sh <network> check-schema`؛ Compose و Dockerfileها از source مشترک استفاده می‌کنند.
- [x] آزمون baseline مشترک روی چهار database واقعی موقت، query مشترک و جداسازی شبکه، init نیمه‌تمام و drift؛ بررسی دو نسخه درایور با `readonly=1` و تکرار pipeline بیت‌کوین. [دامنه دقیق آزمون‌ها](docs/warehouse/VALIDATION.md).
- [ ] اتصال writer/reader، backfill و تطبیق داده هر چهار شبکه؛ دادهٔ پایهٔ بیت‌کوین در گام دوم منتقل شده است، اما مسیرهای قدیمی سه شبکهٔ دیگر و جدول‌های تحلیلی هنوز فعال‌اند.

پیشرفت گام دوم؛ انتقال دادهٔ پایهٔ بیت‌کوین به قرارداد مشترک:

- [x] افزودن مسیر مشترک reservation، درج fact، complete marker و publication fence با کنترل شبکه، header و checkpoint قدیمی؛ فقط یک writer مجاز است و lease توزیع‌شده هنوز پیاده نشده است.
- [x] نوشتن بلاک، تراکنش، vin/vout، رابطه‌های نوع‌دار UTXO و checkpoint بیت‌کوین در شش جدول مشترک؛ نصب تازه هیچ جدول `bitcoin_*` ایجاد نمی‌کند.
- [x] انتقال resolve prevout، holdings، graph، paths، fingerprint و status بیت‌کوین به viewهای canonical مشترک؛ از fact خام منتشرنشده خوانده نمی‌شود.
- [x] افزودن `warehouse-backfill --max-blocks N` با ادامه از checkpoint، رد gap/تاریخچه ناقص، تطبیق تمام ستون‌های چهار جدول fact و مقادیر دقیق پیش از انتشار؛ جدول‌های قدیمی برای ممیزی حفظ می‌شوند.
- [x] جلوگیری از آماده‌شدن API و ingest تا تکمیل انتقال تاریخچهٔ قبلی؛ رسید انتقال پس از reorg معتبر می‌ماند و retry بلاک نیمه‌تمام revision تازه می‌گیرد.
- [x] ثبت نتیجهٔ اجرای نهایی آزمون‌های گام دوم در [گزارش اعتبارسنجی](docs/warehouse/VALIDATION.md): ۱۱ آزمون بیت‌کوین، ۷ آزمون بستهٔ مشترک، ۲ آزمون درایور، ۴۴ آزمون Python و Clippy سخت‌گیرانه پاس شدند؛ آزمون روی دیتابیس مستقر و restore جزو این گام نیست.
- [ ] ادامهٔ همین انتقال برای writer/readerهای TRON، Ethereum و BSC و سپس جدول‌ها و workerهای تحلیلی؛ DB-03 تا DB-09 هنوز در مقیاس چهار شبکه بسته نشده‌اند.

- [x] **DB-01: فرهنگ داده و ماتریس نگاشت.** تمام table/viewها و ستون‌های DDL فعلی هر چهار شبکه و ledgerهای کد، writer/reader و call chain، واحد، کلید، revision، index/TTL، وابستگی view و وضعیت مصرف source مستند شدند. مقصد پیشنهادی، ورودی لازم مقصد و داده‌های فاقد نگاشت بدون اتلاف صریح‌اند؛ gap با حذف داده بسته نمی‌شود. [فرهنگ داده و تصمیم‌ها](docs/warehouse/DB01_DATA_MAP_FA.md)، [JSON قابل کنترل](docs/warehouse/data-dictionary.json). این مرحله ممیزی و طراحی نگاشت است؛ تغییر runtime/backfill و پذیرش زنده در DB-03 تا DB-09 باقی است.
- [x] **DB-02: یک منبع DDL و migration مشترک.** بسته مشترک schema برای هر چهار شبکه تهیه و از init/schema job همه آن‌ها مصرف شود. ثبت نسخه و checksum، تشخیص schema ناقص یا تغییرکرده و فرمان check یکسان پیاده شود. migrationهای اجراشده قبلی بازنویسی نشوند. **انجام‌شده برای baseline مشترک نسخه ۱ در namespace `aml_*`؛ backfill و تغییر reader/writer در DB-08 همچنان باز است.**
- [x] **DB-03: استاندارد ستون‌ها و انواع در قرارداد مشترک.** نوع و بازهٔ همهٔ ستون‌های درج، قالب network/hash، زمان میلی‌ثانیه و تبدیل صریح بدون overflow، revision مثبت، مقدار UInt256 به‌صورت رشتهٔ دقیق، BTC/ساتوشی با decimals=8، شناسهٔ دارایی/توکن NFT، confidence/score، review و nullable استاندارد و کنترل شدند. نسخهٔ ۲ همان ۲۹ جدول و ۲۰ view را با ۲۱۷ قاعده و یک CHECK در هر جدول مصرف می‌کند؛ نسخهٔ ۱ دست‌نخورده و ارتقای نیمه‌تمام قابل ادامه است. [شرح و مرز انجام‌شده](docs/warehouse/DB03_COLUMN_CONTRACT_FA.md). تغییر writer/readerهای legacy و یکسان‌شدن کل دیتابیس فعال همچنان DB-04 تا DB-08 است.
- [ ] **DB-04: جدول‌ها و viewهای داده پایه.** ساختار مصوب `ingested_blocks`، `transactions`، `address_relationships`، `transaction_features`، `semantic_aml_events`، `sync_state`، `ingestion_failures`، `ingestion_benchmarks` و viewهای canonical/current در هر چهار database یکسان شود؛ صرفاً تغییر نام `bitcoin_transactions` کافی نیست.
- [ ] **DB-05: جدول‌های تحلیلی و تخصصی.** قرارداد واحد برای asset/token metadata، jobها، intelligence source/claim/review، entity/exchange، cluster claim/membership/run، exposure seed/run/path و risk policy/rule/signal/assessment تعریف و در هر چهار شبکه ایجاد شود. اختلاف نام‌ها و مدل‌های فعلی با migration و readerهای سازگار رفع شود. جدول‌های input/output/outpoint/script بیت‌کوین و raw/log شبکه‌های دیگر هم در بسته DDL مشترک قرار بگیرند.
- [ ] **DB-06: رابطه‌های سازگار با UTXO.** گره آدرس، تراکنش و script با نوع مشخص در مدل مشترک قابل نمایش باشند. leg ورودی و خروجی بیت‌کوین به شواهد outpoint قابل برگشت باشد؛ آدرس فرستنده ساختگی یا حاصل‌ضرب همه ورودی‌ها در خروجی‌ها به‌عنوان انتقال قطعی ذخیره نشود. queryهای مشترک address relationships و گراف باید این معنا را رعایت کنند.
- [ ] **DB-07: یک قرارداد canonical و visibility.** قواعد commit، hash/revision، finality/confirmation، پوشش داده، orphan و checkpoint در همه adapterها با قرارداد مشترک سازگار شوند. هیچ تحلیل، aggregate، cache یا API از fact خام منتشرنشده استفاده نکند؛ reorg باید خروجی‌های وابسته را هم stale یا نامعتبر کند.
- [ ] **DB-08: مهاجرت بدون از‌دست‌رفتن داده.** تبدیل داده schema فعلی هر چهار شبکه، backfill جدول‌های تحلیلی و تغییر reader/writerها مرحله‌ای باشد. شمارش، جمع مبالغ، checkpoint، hash/revision و نتایج تحقیق قبل و بعد مقایسه شوند؛ بازیابی از backup آزمون شود. حذف volume یا پاک‌کردن تاریخچه راه‌حل migration نباشد.
- [ ] **DB-09: تنظیمات و نگهداری مشترک.** نسخه و image پین‌شده ClickHouse، سیاست اتصال و timeout، batch insert، partition، index، TTL و کنترل دسترسی یکسان و قابل پیکربندی شوند. مسیر اتصال به ClickHouse محلی یا موجود برای همه شبکه‌ها قرارداد واحد داشته باشد و جداسازی شبکه با کلید و validation حفظ شود.

**معیار بستن P0:** پس از init و پس از upgrade، فهرست table/viewها، `DESCRIBE`، نوع ستون‌ها، engine، partition و index هر چهار database با schema مصوب تطبیق داشته باشد. queryهای مشترک با تغییر فقط database و مقدار `network_id` روی هر چهار شبکه اجرا شوند و fixture شبکه دیگر را برنگردانند.

## P1 — تکمیل زنجیره داده و ابزارهای عملیاتی بیت‌کوین

**مرز گام پنجم تکمیل‌شده:** ساختار schema نسخهٔ ۱ ثابت است؛ جدول failure مشترک موجود در دیتابیس مستقل بیت‌کوین مصرف می‌شود. repair ترتیب checkpoint را حفظ می‌کند. بازپخش تاریخچهٔ منتشرشده در ING-04 باقی است.

- [x] **ING-01: probe قابل استفاده عملیاتی.** mainnet/genesis، ارتفاع و sync نود، زمان tip، دسترسی نمونهٔ مرزهای بازه، pruning، prevout/undo data و lookup تاریخی واقعی بررسی می‌شوند؛ JSON با علت و کد خروج ready/degraded/unready ارائه می‌شود. حداکثر دو بلاک و هشت ورودی در هر بلاک، با مهلت کل ۱۲۰ ثانیه. بررسی checkpoint/تاریخچهٔ محلی و تأیید کل تاریخچه ادعا نمی‌شود؛ اجرای Core واقعی و rollout هنوز در TEST-05 باز است. [راهنما](dockerizd_bitcoin/README.md).
- [x] **ING-02: دریافت تاریخی و follow قابل اتکا.** پنجره و concurrency محدود، سقف پاسخ/شواهد، query دسته‌ای canonical و RPC batch با کنترل هویت، commit مرتب، retry/backoff و rate limit و fallback با mainnet/genesis و checkpoint پیاده شدند. ورودی حل‌نشده انتشار را متوقف می‌کند. فرمان‌های تغییردهنده قفل سیستم‌عامل مشترک کانتینرهای همان VM را می‌گیرند؛ اجرای native به تنظیم صریح همان filesystem نیاز دارد. قطع RPC، prefix سالم، resume، ورودی مفقود، دو writer، crash و دو کانتینر غیر root آزمون شدند. lease میان میزبان‌های مستقل در معماری فعلی ارائه نمی‌شود؛ benchmark/rollout همچنان TEST-05 است.
- [x] **ING-03: ثبت failure و repair.** خطای بلاک و شمارندهٔ پایدار، retry/dead-letter و gap در جدول مشترک ذخیره می‌شوند؛ ابزارهای failures/scan-gaps/repair و API محافظت‌شده فعال‌اند. repair فقط بلاک بعدیِ انتخاب‌شده یا تطبیق مورد قبلاً منتشرشده را انجام می‌دهد؛ شکاف تاریخی با دلیل روشن تا ING-04 باقی می‌ماند و cursor عقب نمی‌رود. restart، قطع RPC، fallback، repair ناموفق/موفق، قفل، عقب‌رفتن ساعت و API با ClickHouse موقت آزمون شدند.
- [ ] **ING-04: replay مستقل و reprocessing.** بازپخش بازه/بلاک بدون جابه‌جاکردن ناخواسته cursor دریافت زنده پیاده شود. اصلاح شواهد، reorg و تغییر نسخه detector یا intelligence باید بازتحلیل وابستگی‌ها را فعال کند و نتیجه قدیمی به‌عنوان نتیجه تازه مصرف نشود.
- [ ] **ING-05: سنجش و پیشرفت.** benchmark با ذخیره نتیجه در schema مشترک، نرخ بلاک/تراکنش/ردیف، اندازه داده، lag، بازه دریافت‌شده، gap، آخرین موفقیت و heartbeat اضافه شود. «فرایند روشن است» از «داده تازه و کامل است» جدا گزارش شود.

## P1 — امکانات AML و تحلیل هم‌سطح شبکه‌های موجود

- [ ] **AML-01: intelligence قابل ممیزی.** ثبت source، import برچسب entity/exchange، evidence refs، confidence، reviewer، وضعیت pending/approved/rejected/revoked و تاریخچه review پیاده شود. فقط ادعای فعال و تأییدشده وارد attribution و seed ریسک شود؛ اصلاح یا لغو برچسب نتایج وابسته را نامعتبر کند.
- [ ] **AML-02: clustering و کاندیداها.** تولید candidate، ثبت claim و membership نسخه‌دار با منبع و confidence و endpoint مشاهده/بررسی اضافه شود. common-input و change heuristic فقط lead باشند؛ در سناریوهای مشارکتی یا CoinJoin احتمالی نباید به مالکیت قطعی یا seed ریسک تبدیل شوند.
- [ ] **AML-03: exposure واقعی.** seedهای تأییدشده، run قابل پیگیری، مسیر ورودی/خروجی مستقیم و چندمرحله‌ای، سهم مقدار، جهت، ترتیب زمانی، hop، decay و مرز سرویس/صرافی پیاده و در ClickHouse ذخیره شود. `exposure_paths` از داده واقعی پر شود؛ اتصال صرف دو آدرس از طریق تراکنش به معنی تخصیص تمام مبلغ یا پیوستگی قطعی همان ساتوشی‌ها نباشد. روش تخصیص UTXO و حدود عدم‌قطعیت باید نسخه‌دار و آزمون‌شده باشد.
- [ ] **AML-04: ریسک توضیح‌پذیر.** داده معتبر بیت‌کوین به policy مرکزی برسد و policy version، signal، contribution، evidence و assessment مطابق schema مشترک قابل ذخیره و بازیابی باشند. آدرس با شواهد معتبر باید واقعاً ارزیابی شود؛ `UNKNOWN` ثابت جایگزین پیاده‌سازی نباشد. نبود شواهد هم نباید امتیاز صفر یا برچسب سالم بسازد.
- [ ] **AML-05: fingerprint و activity.** ورودی/خروجی، تعداد تراکنش و انتقال، کارمزد، زمان اولین/آخرین فعالیت، روند زمانی، fan-in/fan-out و counterparties با فیلتر و پوشش داده محاسبه شوند؛ fingerprint به دو شمارنده محدود نماند.
- [ ] **AML-06: semantic evidence بیت‌کوین.** detectorهای قابل‌اعمال مانند consolidation، fan-out، الگوی peeling و CoinJoin احتمالی با نسخه، confidence، tx/outpoint refs و محدودیت روشن اضافه شوند. الگوی رفتاری به‌تنهایی برچسب پول‌شویی نباشد؛ eventهای bridge/mixer فقط با evidence و registry معتبر تولید شوند.
- [ ] **AML-07: asset metadata و holdings.** BTC در قرارداد مشترک دارایی، symbol/decimals/source/verification و snapshot موجودی قرار بگیرد. UTXO خرج‌شده/خرج‌نشده، coinbase immature، بازه ناقص و ارتفاع snapshot درست نمایش داده شوند؛ net flow معادل balance نباشد و برای آدرس دلخواه به RPC مختص wallet مالک تکیه نشود. توکن‌ها و eventهای مختص EVM با `not_applicable` مشخص شوند.
- [ ] **AML-08: workerهای پیوسته.** محاسبه feature و semantic evidence، clustering و refresh exposure با cursor، version، heartbeat و وضعیت waiting/running/idle/failed اجرا شوند؛ restart، seed نامعتبر، خطای یک job و سقف منابع کنترل شود. job دستی import/review خودکار تأیید نشود. workerهای لازم در Compose، launcher و کنترل‌های VM هم وارد شوند.

## P2 — API، گراف و رابط کاربری مشترک

- [ ] **API-01: قرارداد پاسخ همسان.** `fingerprint`، `activity`، `asset_flows`، `top_counterparties`، `semantic_events`، `entities`، `clusters`، `exposure_paths`، `exposure_coverage`، `holdings`، `risk_engine`، `graph`، `query_scope` و `data_coverage` در API شبکه‌ها قرارداد مشخص و قابل اعتبارسنجی داشته باشند. فیلدهای خالی دائمی به‌عنوان قابلیت پیاده‌شده حساب نشوند.
- [ ] **API-02: query و paths هم‌سطح.** فیلتر زمان/ارتفاع/جهت/دارایی، pagination، limit و budget مشخص اضافه شود. جست‌وجوی تا ۱۰ hop مطابق قرارداد شبکه‌ها و با معنای transaction hop بیت‌کوین آزمون شود؛ محدودیت فعلی «یک مسیر از ۲۰ تراکنش اخیر هر آدرس» باید قابل کنترل و صریح باشد، نه ادعای جست‌وجوی کامل.
- [ ] **API-03: freshness و خطا.** ingestion lag، تاریخچه جزئی، وضعیت jobهای تحلیلی، stale exposure/labels، تغییر revision و truncation در status و پاسخ تحقیق دیده شود؛ خطا یا قطع RPC/worker به خروجی ظاهراً کامل تبدیل نشود.
- [ ] **UI-01: تکمیل پنل تحقیق.** پنل fingerprint/activity، جریان دارایی، counterparties، entities/exchanges، کاندیدا و evidence خوشه، مسیر exposure، دلایل ریسک، holdings و وضعیت sync/analytics برای بیت‌کوین اضافه و با تجربه شبکه‌های فعلی هماهنگ شود. جدول JSON خام جایگزین پنل‌ها نباشد.
- [ ] **UI-02: Neo4j مرکزی و snapshot کامل.** گراف نوع‌دار آدرس/تراکنش/script و تمام evidence تحلیلی، snapshot یکسان و قابل بازسازی بسازند. Export و بازکردن مجدد، همان مبلغ، فیلتر، coverage، policy/label version و دلیل ریسک را حفظ کنند؛ درخواست هم‌زمان یا داده ناقص snapshot اشتباه نسازد.

## P3 — آزمون پذیرش و تحویل

- [ ] **TEST-01: آزمون خودکار schema هر چهار شبکه.** ایجاد دیتابیس خالی، upgrade از schema موجود، checksum migration، اجرای مجدد و query مشترک روی چهار fixture بررسی شود. تغییر ناخواسته ستون/نوع/view در یک شبکه باید تست قرارداد را fail کند.
- [ ] **TEST-02: صحت داده و بازیابی.** چندورودی/چندخروجی، change، self-transfer، BIP30، coinbase/maturity، آدرس‌ها و scriptهای معتبر، دقت مقدار، crash بین insert و commit، عقب‌رفتن ساعت، قطع RPC/ClickHouse، resume، replay و reorg همراه invalidation تحلیل‌ها آزمون شوند.
- [ ] **TEST-03: صحت AML.** pending/revoked/stale label امتیاز نسازد، approved label معتبر اثر قابل توضیح داشته باشد، exposure به run/tx/outpoint معتبر متصل باشد، ترتیب زمان و مرز صرافی رعایت شود و candidate یا CoinJoin احتمالی مالکیت/ریسک قطعی نسازد.
- [ ] **TEST-04: پذیرش از ابتدا تا انتها.** داده دریافت‌شده واقعاً از warehouse به API، policy مرکزی، پنل، Neo4j، Export و reopen برسد. تست مرورگر با داده ساختگی، به‌تنهایی پذیرش چرخه داده واقعی نیست. برابری قرارداد و پنل‌ها برای هر چهار شبکه بررسی شود.
- [ ] **TEST-05: ظرفیت و استقرار.** build imageهای release، migration روی نسخه ClickHouse پین‌شده، اجرای چهار VM شبکه و VM اصلی، pause/resume/repair، بسته آفلاین، firewall و benchmark روی RPC/نود هدف بررسی شوند. معیار ظرفیت پیش از اجرا مشخص و نتیجه واقعی همراه محدودیت گزارش شود.
- [ ] **DOC-01: مستندات و اعلام پایان.** data dictionary، جدول تطبیق امکانات، فرمان‌های migration/worker/repair، محدودیت‌های واقعی و راهنمای VM به‌روزرسانی شود. «بیت‌کوین هم‌سطح شد» فقط بعد از بسته‌شدن P0 تا P3 با evidence گفته شود.

## ترتیب اجرا و معیار نهایی

اول قرارداد و schema مشترک هر چهار شبکه و طرح migration، سپس ingestion/ابزارها و زنجیره AML، بعد API/پنل و در پایان پذیرش و rollout انجام شود. صرف افزودن جدول، endpoint، worker روشن یا داده خالی، تکمیل قابلیت نیست؛ برای هر قابلیت writer، داده ذخیره‌شده، reader، نمایش و آزمون پذیرش لازم است.

**مرحله اول، انتقال دادهٔ پایهٔ بیت‌کوین، probe، دریافت قابل‌اتکاتر و failure/repair گام پنجم انجام شده‌اند؛ replay مستقل، مهاجرت نهایی چهار شبکه، جدول‌های تحلیلی و تکمیل امکانات AML همچنان باز هستند. نصب و آزمون‌ها روی ClickHouse موقت و RPC کنترل‌شده انجام شده‌اند و به معنی اجرای migration روی دیتابیس مستقر یا تأیید نود واقعی نیستند.**
