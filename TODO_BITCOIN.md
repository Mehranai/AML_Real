# یکسان‌سازی دیتابیس چهار شبکه و تکمیل امکانات بیت‌کوین

**وضعیت: تکمیل نشده.** بیت‌کوین دریافت بلاک، تحقیق UTXO، برچسب دارای review، exposure، ارزیابی ماندگار ریسک، آمار/فعالیت فیلترشده و کاندیداهای خوشه‌بندی نسخه‌دار دارد. مهاجرت نهایی چهار شبکه و بقیهٔ امکانات AML هنوز بازند. پایان‌یافته‌بودن فهرست قبلی و پاس‌شدن تست‌های آن، به معنی یکسان‌بودن دیتابیس‌ها یا هم‌سطح‌شدن امکانات AML نبود. این فهرست جایگزین معیار تکمیل قبلی است.

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
| migration | Ethereum/BSC migrationهای مرحله‌ای دارند؛ TRON baseline و سازوکار schema زمان اجرا دارد | init/check مشترک نسخه‌دار و backfill مستقل دادهٔ پایهٔ بیت‌کوین اضافه شده‌اند؛ backup/restore آزمایشی DB-08A پاس شده، اما انتقال کل تاریخچهٔ تحلیلی و مسیرهای فعال باز است |
| intelligence | source، برچسب، review و entity در شبکه‌های فعلی وجود دارد؛ نام‌گذاری یکسان نیست، از جمله `address_entity`، `address_entities` و `intelligence_claims` | ثبت source، import فقط pending، review جداگانه، تاریخچهٔ شواهد تصمیم و entities فعال به API و پنل بیت‌کوین وصل شدند؛ دادهٔ واقعی هنوز وارد نشده است |
| clustering و exposure | در TRON/Ethereum worker و در BSC تحلیل هنگام تحقیق و ابزار intelligence وجود دارد | exposure و کاندیداهای محدود با شواهد، run و review نسخه‌دار فعال‌اند؛ worker در sweepهای محدود آن‌ها را تازه‌سازی می‌کند؛ SLA تازگی یا عضویت قطعی ادعا نمی‌شود |
| ریسک | Analytical Node از شواهد برچسب و exposure استفاده می‌کند؛ Ethereum جدول‌های policy، signal و assessment نیز دارد | سیاست واحد Rust و ثبت/بازیابی policy، rules، signals و assessment بیت‌کوین در جدول‌های مشترک فعال‌اند؛ مرکز دوباره محاسبه و snapshot آن را کنترل می‌کند؛ نبود شاهد UNKNOWN است |
| تحقیق آدرس | fingerprint، activity، asset flows، counterparties، semantic events، entities، clusters، exposure و coverage ارائه می‌شود؛ قالب پاسخ‌ها نیازمند هماهنگی است | آمار دقیق، فیلتر، الگوها و کاندیداهای خوشه‌بندی با پوشش صریح به پنل و snapshot مرکزی متصل‌اند؛ یکسان‌سازی کامل قرارداد و پذیرش نهایی بازند |
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

**اصلاح ترتیب کار پس از بازبینی ساختار:** عبور به ING-04 زود بود. تا تعیین و اجرای مسیر DB، قابلیت ingestion جدید اضافه نمی‌شود. پوشه‌بندی بیت‌کوین و SQL نصب فعال اصلاح شده‌اند؛ DB-01 تا DB-07 در محدودهٔ قرارداد مقصد، روابط و visibility تکمیل شده‌اند. انتقال مسیرهای فعال و عملیات در DB-08 و DB-09 باقی است.

- [x] **DB-STRUCT:** بیت‌کوین به `db/{mod,client,migrations,queries,sql}`، `storage`، `domain`، `bitcoin` و binary مستقل schema تفکیک شد؛ SQL نصب فعال در `sql/migrations` از قرارداد مشترک تولید و پیش از DDL کنترل می‌شود. SQL قدیمی در `sql/legacy` حفظ شد. [گزارش آزمون](docs/warehouse/DB_STRUCTURE_VALIDATION.md).
- [x] **DB-AUDIT پایه:** مسیر فعال init/write/read دادهٔ پایهٔ چهار شبکه و تفاوت ساختار کامل با baseline بررسی و مستند شد؛ برابری baseline چهار database موقت دوباره آزمون شد. [ممیزی](docs/warehouse/DB_STRUCTURE_AUDIT_FA.md). ممیزی تمام خانواده‌ها، ستون‌ها و مسیرهای worker در DB-01 زیر تکمیل شد؛ اجرای زندهٔ workerها و پذیرش داده همچنان باز است.

**اولویت عملی پس از بازنگری دامنه:** مسیر برچسب، کاندیداهای خوشه‌بندی، exposure، ریسک، فعالیت، الگوها و metadata/holdings بیت‌کوین و worker محدود و پیوسته در AML-01 تا AML-08 پیاده و با fixture آزمون شده‌اند. توسعهٔ schema بدون نیاز مشخص ادامه پیدا نمی‌کند. **گام عملی بعد، بررسی و تکمیل قرارداد پاسخ API-01 است**؛ replay تاریخی، مهاجرت نهایی و پذیرش نود/VM/Neo4j واقعی بازند. worker به‌تنهایی پوشش یا تازگی همهٔ آدرس‌ها را ثابت نمی‌کند. [worker و کنترل‌ها](dockerizd_bitcoin/ANALYTICS.md)، [پذیرش AML-08](dockerizd_bitcoin/ANALYTICS_VALIDATION.md)، [برچسب](dockerizd_bitcoin/INTELLIGENCE.md)، [خوشه‌بندی](dockerizd_bitcoin/CLUSTERING.md)، [پذیرش AML-02](dockerizd_bitcoin/CLUSTERING_VALIDATION.md)، [exposure](dockerizd_bitcoin/EXPOSURE.md)، [ریسک](dockerizd_bitcoin/RISK.md)، [فعالیت و پوشش](dockerizd_bitcoin/ACTIVITY.md)، [نتایج AML-05](dockerizd_bitcoin/ACTIVITY_VALIDATION.md)، [شواهد الگوها](dockerizd_bitcoin/SEMANTICS.md)، [پذیرش AML-06](dockerizd_bitcoin/SEMANTICS_VALIDATION.md)، [metadata و موجودی](dockerizd_bitcoin/HOLDINGS.md)، [پذیرش AML-07](dockerizd_bitcoin/HOLDINGS_VALIDATION.md).

DB-08B (نگاشت)، DB-08C (backfill و مسیرهای فعال)، DB-08D (تطبیق تحلیل) و DB-09 همچنان بازند و برای یکسان‌سازی نهایی چهار شبکه باید تکمیل شوند؛ پایان AML-01 به معنی پایان مهاجرت نیست. DB-08A، پشتیبان بومی و restore آزمایشی چهار شبکه، تکمیل شده است. schema نسخهٔ ۵ ثابت و دیتابیس هر VM مستقل است. [شرح DB-08A](docs/warehouse/DB08A_BACKUP_RESTORE_FA.md).

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
- [ ] ادامهٔ همین انتقال برای writer/readerهای TRON، Ethereum و BSC و سپس جدول‌ها و workerهای تحلیلی؛ قرارداد مقصد DB-03 تا DB-05 و مدل/query مشترک روابط DB-06 تکمیل شده، اما DB-08/DB-09 و یکسان‌شدن کل مسیر فعال بازند.

- [x] **DB-01: فرهنگ داده و ماتریس نگاشت.** تمام table/viewها و ستون‌های DDL فعلی هر چهار شبکه و ledgerهای کد، writer/reader و call chain، واحد، کلید، revision، index/TTL، وابستگی view و وضعیت مصرف source مستند شدند. مقصد پیشنهادی، ورودی لازم مقصد و داده‌های فاقد نگاشت بدون اتلاف صریح‌اند؛ gap با حذف داده بسته نمی‌شود. [فرهنگ داده و تصمیم‌ها](docs/warehouse/DB01_DATA_MAP_FA.md)، [JSON قابل کنترل](docs/warehouse/data-dictionary.json). این مرحله ممیزی و طراحی نگاشت است؛ تغییر runtime/backfill و پذیرش زنده در DB-03 تا DB-09 باقی است.
- [x] **DB-02: یک منبع DDL و migration مشترک.** بسته مشترک schema برای هر چهار شبکه تهیه و از init/schema job همه آن‌ها مصرف شود. ثبت نسخه و checksum، تشخیص schema ناقص یا تغییرکرده و فرمان check یکسان پیاده شود. migrationهای اجراشده قبلی بازنویسی نشوند. **انجام‌شده برای baseline مشترک نسخه ۱ در namespace `aml_*`؛ backfill و تغییر reader/writer در DB-08 همچنان باز است.**
- [x] **DB-03: استاندارد ستون‌ها و انواع در قرارداد مشترک.** نوع و بازهٔ همهٔ ستون‌های درج، قالب network/hash، زمان میلی‌ثانیه و تبدیل صریح بدون overflow، revision مثبت، مقدار UInt256 به‌صورت رشتهٔ دقیق، BTC/ساتوشی با decimals=8، شناسهٔ دارایی/توکن NFT، confidence/score، review و nullable استاندارد و کنترل شدند. نسخهٔ ۲ همان ۲۹ جدول و ۲۰ view را با ۲۱۷ قاعده و یک CHECK در هر جدول مصرف می‌کند؛ نسخهٔ ۱ دست‌نخورده و ارتقای نیمه‌تمام قابل ادامه است. [شرح و مرز انجام‌شده](docs/warehouse/DB03_COLUMN_CONTRACT_FA.md). تغییر writer/readerهای legacy و یکسان‌شدن کل دیتابیس فعال همچنان DB-04 تا DB-08 است.
- [x] **DB-04: قرارداد جدول‌ها و viewهای داده پایه.** هشت خانوادهٔ `ingested_blocks`، `transactions`، `address_relationships`، `transaction_features`، `semantic_aml_events`، `sync_state`، `ingestion_failures` و `ingestion_benchmarks` در schema مشترک نسخهٔ ۳ تکمیل شدند؛ ۲۸ ستون حفظ شواهد/trace/ترتیب رویداد/چرخهٔ خطا/benchmark، viewهای canonical/current و reader مشترک محدود برای هر چهار شبکه اضافه شدند. writer بیت‌کوین شمارش factهای commit و شواهد leg و خطا را ثبت می‌کند. ارتقای نسخهٔ ۱ و ۲، حفظ مقادیر قبلی، ادامهٔ قطع وسط ستون/constraint/view و رد drift روی ClickHouse موقت آزمون شدند. **این تیک مربوط به قرارداد مقصد و دسترسی مشترک است؛ تغییر نهایی writer/readerهای legacy سه شبکه و انتقال تاریخچه همچنان DB-08 است و P0 بسته نیست.** [شرح محدوده](docs/warehouse/DB04_CORE_CONTRACT_FA.md)، [نتیجهٔ تست‌ها](docs/warehouse/DB04_VALIDATION.md).
- [x] **DB-05: قرارداد جدول‌های تحلیلی و تخصصی.** نسخهٔ ۴ با ۹ جدول جدید، ۱۱۰ ستون تکمیلی و ۱۹ view جدید، خانواده‌های metadata/job/discovery، source/claim/review، entity/exchange، protocol registry، cluster، exposure و risk را تکمیل کرد. input/output/outpoint موجود با script، trace و raw envelope در یک بستهٔ schema قرار دارند. reader مشترک، انتخاب آخرین revision پیش از فیلتر تأیید، حفظ JSON اصلی و migration افزایشی روی چهار دیتابیس موقت آزمون شدند. **محدودهٔ انجام‌شده، قرارداد مقصد و دسترسی مشترک است؛ worker/APIهای AML و تغییر نهایی writer/readerهای legacy یا انتقال تاریخچه تکمیل نشده‌اند.** [شرح](docs/warehouse/DB05_ANALYTICS_CONTRACT_FA.md)، [پذیرش و تست](docs/warehouse/DB05_VALIDATION.md).
- [x] **DB-06: رابطه‌های سازگار با UTXO.** مدل `typed_relationships_v1`، گره‌های address/transaction/script با آدرس nullable و کلید محدود به شبکه و query مشترک canonical اضافه شدند. هر leg بیت‌کوین با fact ورودی/خروجی و outpoint/index/مبلغ/subject تطبیق می‌یابد؛ شاهد مفقود یا ناسازگار و snapshot متغیر خطا می‌دهند. writer و گراف/API بیت‌کوین، پنل و تبدیل payload مرکزی این مدل را حفظ می‌کنند؛ حاصل‌ضرب ورودی/خروجی یا ادعای مالکیت/continuity ساخته نمی‌شود. **schema نسخهٔ ۴ ثابت است؛ انتقال legacy سه شبکه در DB-08 و اجرای زندهٔ Neo4j/VMها در پذیرش نهایی باز است.** [شرح](docs/warehouse/DB06_RELATIONSHIPS_FA.md)، [تست و پذیرش](docs/warehouse/DB06_VALIDATION.md).
- [x] **DB-07: یک قرارداد canonical و visibility.** نسخهٔ ۵ با seal خروجی‌های cluster/exposure/risk و aggregate، provenance nullable برای metadata/cluster versions، viewهای canonical/visible و کنترل snapshot مشترک پیاده شد. انتشار ترتیب/parent/revision و کامل‌بودن خانوادهٔ تراکنش را کنترل می‌کند؛ rollback نشانگر orphan افزایشی دارد و تغییر source fence خروجی وابسته را از مصرف جاری خارج می‌کند. API بیت‌کوین snapshot و no-store دارد؛ create/reopen/Export مرکزی، source قدیمی یا تأییدنشده را با UNKNOWN و هشدار تاریخی نشان می‌دهد. **این تیک قرارداد مقصد و اتصال فعال بیت‌کوین است؛ reader/writer و worker/cacheهای legacy سه شبکه در DB-08، و اتصال چرخهٔ intelligence به detectorها در مراحل AML باقی است.** [شرح و محدودیت‌ها](docs/warehouse/DB07_VISIBILITY_FA.md)، [گزارش آزمون](docs/warehouse/DB07_VALIDATION.md).
- [ ] **DB-08: مهاجرت بدون از‌دست‌رفتن داده.** تبدیل داده schema فعلی هر چهار شبکه، backfill جدول‌های تحلیلی و تغییر reader/writerها مرحله‌ای باشد. شمارش، جمع مبالغ، checkpoint، hash/revision و نتایج تحقیق قبل و بعد مقایسه شوند؛ بازیابی از backup آزمون شود. حذف volume یا پاک‌کردن تاریخچه راه‌حل migration نباشد.
- [ ] **DB-09: تنظیمات و نگهداری مشترک.** نسخه و image پین‌شده ClickHouse، سیاست اتصال و timeout، batch insert، partition، index، TTL و کنترل دسترسی یکسان و قابل پیکربندی شوند. مسیر اتصال به ClickHouse محلی یا موجود برای همه شبکه‌ها قرارداد واحد داشته باشد و جداسازی شبکه با کلید و validation حفظ شود.

**معیار بستن P0:** پس از init و پس از upgrade، فهرست table/viewها، `DESCRIBE`، نوع ستون‌ها، engine، partition و index هر چهار database با schema مصوب تطبیق داشته باشد. queryهای مشترک با تغییر فقط database و مقدار `network_id` روی هر چهار شبکه اجرا شوند و fixture شبکه دیگر را برنگردانند.

جزئیات گام تکمیل‌شدهٔ DB-05:

- [x] تطبیق شکاف‌های DB-01 و تعریف قرارداد دادهٔ تحلیلی و تخصصی بدون حذف ستون مبدأ.
- [x] migration افزایشی نسخهٔ ۴، SQL قابل بررسی بیت‌کوین و reader مشترک با جداسازی شبکه.
- [x] پذیرش روی چهار دیتابیس ClickHouse آزمایشی، ارتقای دادهٔ fixture، ادامه پس از قطع، review/revocation و هر دو درایور.
- [x] ثبت گزارش نهایی، checksum و نتیجهٔ تست‌ها و تعیین DB-06 به‌عنوان مرحلهٔ بعد.

جزئیات DB-08؛ ترتیب اجرای وابسته:

- [x] **DB-08A:** ابزار backup بومی مستقل هر VM، manifest شامل schema و fingerprint همهٔ ستون‌ها/ردیف‌های فیزیکی، restore فقط در database جدید و ادامه با UUID ثبت‌شده پیاده شد؛ مبلغ صحیح بالاتر از UInt256، NULL، باینری/بیت float، ترتیب آرایه، زمان، نسخه‌ها، checkpoint/ledger و عدم دوباره‌شماری MV روی دو ClickHouse موقت و هر چهار شبکه آزمون شدند. config اختیاری و ابزار در بستهٔ VMهای شبکه قرار دارند. **این تیک مربوط به ابزار و اثبات fixture است؛ backup دادهٔ واقعی یا cutover انجام نشده است.** [گزارش](docs/warehouse/DB08A_VALIDATION.md).
- [ ] **DB-08B:** codec و نگاشت بدون اتلاف هر خانواده و گزارش صریح دادهٔ فاقد hash/ترتیب/provenance؛ انتقال آزمایشی و مقایسهٔ شمارش، جمع مبلغ، checkpoint و تحقیق.
- [ ] **DB-08C:** backfill قابل ادامه و اتصال writer/readerهای فعال TRON، Ethereum و BSC با حفظ barrier و rollback؛ دیتابیس خالی جای تاریخچهٔ قبلی ننشیند.
- [ ] **DB-08D:** انتقال خانواده‌های تحلیلی و سازگاری worker/cacheهای قدیمی، تطبیق رفتار تحقیق و خروجی ML موجود TRON؛ پذیرش نهایی cutover و مسیر بازگشت.

پاس‌شدن backup/restore فقط پیش‌نیاز DB-08 است؛ مجوز تغییر مسیر فعال یا پایان P0 نیست.

جزئیات گام تکمیل‌شدهٔ DB-07:

- [x] snapshot مشترک، مرز confirmation/finality و کنترل خواندن سازگار.
- [x] migration نسخهٔ ۵ با viewهای visibility و publication خروجی‌های تحلیلی؛ حفظ نسخه‌های قبلی.
- [x] کنترل ترتیب/والد در انتشار، orphan marker افزایشی و آزمون عدم بازگشت دادهٔ شاخهٔ قدیمی.
- [x] اتصال API و snapshot مرکزی بیت‌کوین، نشان‌دادن stale و منع cache نتیجهٔ قدیمی به‌عنوان نتیجهٔ جاری.
- [x] آزمون چهار شبکه، migration/قطع/ادامه، دادهٔ منتشرنشده و نتایج وابسته؛ ثبت گزارش پذیرش.

جزئیات گام تکمیل‌شدهٔ DB-06:

- [x] مدل مشترک address/transaction/script، هویت محدود به شبکه و معنای متفاوت انتقال مشاهده‌شده و leg بیت‌کوین.
- [x] query مشترک canonical با تطبیق outpoint/index/مبلغ/subject و واکشی بر اساس وقوع دقیق تراکنش؛ بدون حاصل‌ضرب ورودی‌ها و خروجی‌ها.
- [x] اتصال writer و گراف/API بیت‌کوین، نمایش پنل و حفظ subjectهای نوع‌دار در مسیر مرکزی تحقیق.
- [x] تست چندورودی/چندخروجی، script بدون آدرس، یک ساتوشی، هم‌هش در دو بلاک، دادهٔ ناقص/منتشرنشده و دو درایور؛ ثبت گزارش پذیرش.

## P1 — تکمیل زنجیره داده و ابزارهای عملیاتی بیت‌کوین

**مرز گام پنجم تکمیل‌شده:** ساختار schema نسخهٔ ۱ ثابت است؛ جدول failure مشترک موجود در دیتابیس مستقل بیت‌کوین مصرف می‌شود. repair ترتیب checkpoint را حفظ می‌کند. بازپخش تاریخچهٔ منتشرشده در ING-04 باقی است.

- [x] **ING-01: probe قابل استفاده عملیاتی.** mainnet/genesis، ارتفاع و sync نود، زمان tip، دسترسی نمونهٔ مرزهای بازه، pruning، prevout/undo data و lookup تاریخی واقعی بررسی می‌شوند؛ JSON با علت و کد خروج ready/degraded/unready ارائه می‌شود. حداکثر دو بلاک و هشت ورودی در هر بلاک، با مهلت کل ۱۲۰ ثانیه. بررسی checkpoint/تاریخچهٔ محلی و تأیید کل تاریخچه ادعا نمی‌شود؛ اجرای Core واقعی و rollout هنوز در TEST-05 باز است. [راهنما](dockerizd_bitcoin/README.md).
- [x] **ING-02: دریافت تاریخی و follow قابل اتکا.** پنجره و concurrency محدود، سقف پاسخ/شواهد، query دسته‌ای canonical و RPC batch با کنترل هویت، commit مرتب، retry/backoff و rate limit و fallback با mainnet/genesis و checkpoint پیاده شدند. ورودی حل‌نشده انتشار را متوقف می‌کند. فرمان‌های تغییردهنده قفل سیستم‌عامل مشترک کانتینرهای همان VM را می‌گیرند؛ اجرای native به تنظیم صریح همان filesystem نیاز دارد. قطع RPC، prefix سالم، resume، ورودی مفقود، دو writer، crash و دو کانتینر غیر root آزمون شدند. lease میان میزبان‌های مستقل در معماری فعلی ارائه نمی‌شود؛ benchmark/rollout همچنان TEST-05 است.
- [x] **ING-03: ثبت failure و repair.** خطای بلاک و شمارندهٔ پایدار، retry/dead-letter و gap در جدول مشترک ذخیره می‌شوند؛ ابزارهای failures/scan-gaps/repair و API محافظت‌شده فعال‌اند. repair فقط بلاک بعدیِ انتخاب‌شده یا تطبیق مورد قبلاً منتشرشده را انجام می‌دهد؛ شکاف تاریخی با دلیل روشن تا ING-04 باقی می‌ماند و cursor عقب نمی‌رود. restart، قطع RPC، fallback، repair ناموفق/موفق، قفل، عقب‌رفتن ساعت و API با ClickHouse موقت آزمون شدند.
- [ ] **ING-04: replay مستقل و reprocessing.** بازپخش بازه/بلاک بدون جابه‌جاکردن ناخواسته cursor دریافت زنده پیاده شود. اصلاح شواهد، reorg و تغییر نسخه detector یا intelligence باید بازتحلیل وابستگی‌ها را فعال کند و نتیجه قدیمی به‌عنوان نتیجه تازه مصرف نشود.
- [ ] **ING-05: سنجش و پیشرفت.** benchmark با ذخیره نتیجه در schema مشترک، نرخ بلاک/تراکنش/ردیف، اندازه داده، lag، بازه دریافت‌شده، gap، آخرین موفقیت و heartbeat اضافه شود. «فرایند روشن است» از «داده تازه و کامل است» جدا گزارش شود.

## گام AML-07 — تکمیل شد

- [x] ثبت idempotent مشخصات BTC و منبع/روش تأیید در `aml_asset_metadata` موجود؛ رد metadata ناسازگار.
- [x] قرارداد موجودی با تفکیک دادهٔ ناموجود/بازهٔ ناقص/صفر، maturity و وضعیت دقیق outpoint با BIP30.
- [x] اتصال metadata و snapshot موجودی به API، پنل و حفظ/کنترل آن در snapshot مرکزی.
- [x] آزمون ClickHouse موقت، رگرسیون‌های مرتبط و مستندات؛ بدون تغییر schema یا ادغام دیتابیس VMها.

## P1 — امکانات AML و تحلیل هم‌سطح شبکه‌های موجود

- [x] **AML-01: intelligence قابل ممیزی.** source و import فقط pending، evidence refs/confidence و بازهٔ اعتبار، review جداگانه با expected_revision، وضعیت approved/rejected/revoked و تاریخچهٔ تصمیم با snapshot شواهد در سه جدول موجود پیاده شدند. API و پنل برچسب فعال را نمایش می‌دهند؛ source غیرفعال، ویرایش، انقضا و review ناقص approval قبلی را فعال نمی‌کنند. سیاست مرکزی از برچسب معتبر سیگنال موجود می‌سازد و ایجاد/reopen/Export، snapshot برچسب را هم کنترل می‌کنند. CLI واقعی، ClickHouse، سیاست مرکزی و نمایش پنل روی fixture آزمون شدند. **بدون دادهٔ واقعی/rollout؛ تاریخچهٔ review ماندگار است، نه تمام نسخه‌های فیزیکی claim یا تمام تغییرهای registry؛ exposure و workerهای آینده باید این reader/snapshot را مصرف کنند.** [راهنما](dockerizd_bitcoin/INTELLIGENCE.md)، [پذیرش](dockerizd_bitcoin/INTELLIGENCE_VALIDATION.md).
- [x] **AML-02: clustering و کاندیداها.** کاندیداهای محدود common-input و change با guard تراکنش مشارکتی از occurrence کامل canonical تولید و همراه شواهد، confidence و review در چهار جدول موجود ذخیره می‌شوند. API/CLI و پنل، create/Export/reopen و کنترل منبع، detector، publication و آخرین نسخهٔ review متصل‌اند. تأیید فقط پذیرش سرنخ است؛ entity، مالکیت، seed یا امتیاز ریسک نمی‌سازد. نتیجهٔ capped فقط audit منتشرنشده است و review نمی‌شود. review به snapshot دقیق وابسته است؛ با تغییر revision یا detector، بازبینی تازه لازم است. **بدون تغییر schema یا ادغام ClickHouseها؛ worker، خوشه‌بندی سراسری و پذیرش نود/VM/Neo4j واقعی انجام نشده‌اند.** [روش و فرمان‌ها](dockerizd_bitcoin/CLUSTERING.md)، [گزارش پذیرش](dockerizd_bitcoin/CLUSTERING_VALIDATION.md).
  - [x] استخراج محدود کاندیدای common-input و change از تراکنش کامل canonical با شواهد و محدودیت صریح
  - [x] ثبت run و claim و membership و version در چهار جدول موجود با انتشار قابل کنترل و تاریخچهٔ review مستقل
  - [x] API و CLI مشاهده و بررسی با کنترل نسخه و جلوگیری از تأیید خودکار
  - [x] اتصال تحقیق و پنل و snapshot مرکزی با کنترل تغییر منبع و detector و review
  - [x] آزمون Rust و ClickHouse و مسیر مرکزی و به‌روزرسانی مستندات و TODO
- [x] **AML-03: exposure هنگام درخواست.** seed فعال و تأییدشده با اعتبار در زمان تراکنش، run قابل پیگیری، مسیر ورودی/خروجی مستقیم و چندمرحله‌ای، ترتیب occurrence، decay و مرز سرویس با روش نسخه‌دار `bitcoin_utxo_bounds_v1` پیاده شدند. مسیر فقط از outpoint واقعاً خرج‌شده ادامه می‌یابد؛ حد پایین/بالای integer ساتوشی، seed، run، مسیر و summary در جدول‌های موجود v5 ذخیره می‌شوند. نتیجهٔ کامل sealed و وابستگی برچسب‌ها پیش از مصرف جاری کنترل می‌شود؛ نتیجهٔ احتمالی، service-mediated، ناقص، stale و مسیر تکراری امتیاز اضافی نمی‌سازند. CLI/API، سیاست مرکزی، create/reopen/Export و پنل با خروجی واقعیِ pipeline روی fixture مصنوعی آزمون شدند. **پوشش محدود و on-demand است؛ دادهٔ mainnet، ظرفیت production و worker پیوسته هنوز تأیید نشده‌اند.** [روش و استفاده](dockerizd_bitcoin/EXPOSURE.md)، [پذیرش و محدودیت‌ها](dockerizd_bitcoin/EXPOSURE_VALIDATION.md).
- [x] **AML-04: ریسک توضیح‌پذیر و ماندگار.** سیاست موجود بدون تغییر وزن به کتابخانهٔ Rust مشترک منتقل شد؛ بیت‌کوین policy/rules و assessment/signals را در چهار جدول موجود v5 ذخیره و با seal منتشر می‌کند. CLI/API با ID ارزیابی، شواهد و قواعد ذخیره‌شده را بازیابی می‌کنند. مرکز با همان کد محاسبه را تکرار و snapshot ارزیابی را در create/reopen/Export کنترل می‌کند. ثبت هم‌زمان، بازیابی با client تازه، درج بدون seal، seed تازه، لغو/غیرفعال‌شدن منبع، تغییر سیاست و source fence آزمون شدند؛ نتیجهٔ قدیمی تاریخی می‌ماند و ریسک جاری UNKNOWN می‌شود. بدون signal معتبر امتیاز null است؛ قبل از اولین بلاک، persistence منتظر داده می‌ماند. **آزمون با ClickHouse موقت و دادهٔ مصنوعی است؛ Neo4j زنده، imageهای release، ظرفیت و VM مقصد همچنان پذیرش جدا دارند.** [روش و فرمان‌ها](dockerizd_bitcoin/RISK.md)، [نتایج](dockerizd_bitcoin/RISK_VALIDATION.md).

مسیر برچسب مستقیم و exposure معتبر تا ارزیابی ساختاریافتهٔ ClickHouse و snapshot مرکزی در AML-01، AML-03 و AML-04 تکمیل شده است؛ پوشش همهٔ detectorها و تکمیل تاریخچه از این نتیجه استنباط نمی‌شود.
- [x] **AML-05: fingerprint و activity هنگام درخواست.** جمع دقیق received/spent/net، تعداد leg و occurrence تراکنش، fee یک‌بار در سطح تراکنش، زمان/ارتفاع اولین و آخرین فعالیت، روزهای فعال UTC، شکل ورودی/خروجی و peerهای جهت‌دار از viewهای canonical محاسبه می‌شوند. API و پنل فیلتر ارتفاع/زمان/جهت دارند؛ summary از محدودیت نمایش مستقل است و coverage و scope هر بخش صریح‌اند. peer و shape، مالکیت یا تخصیص قطعی مبلغ نمی‌سازند و net flow موجودی نیست. txid تکراری، rollback، orphan، coinbase، تاریخچهٔ جزئی، فیلتر و اعشار دقیق آزمون شدند؛ snapshot مرکزی و بازکردن بعد از Export همان آمار و فیلتر را حفظ کردند. **سقف ۱۰۰ هزار تراکنش منطبق، محدودیت query و پذیرش مصنوعی؛ ظرفیت و mainnet/VM/Neo4j زنده همچنان بازند.** [معنا و API](dockerizd_bitcoin/ACTIVITY.md)، [گزارش پذیرش](dockerizd_bitcoin/ACTIVITY_VALIDATION.md).
- [x] **AML-06: semantic evidence بیت‌کوین.** consolidation، fan-out، CoinJoin احتمالی و peel-step با نسخه و hash تعریف، confidence، ساتوشی دقیق و tx/outpoint/relationship refs همراه خود بلاک در جدول موجود ذخیره می‌شوند. reader canonical، fallback تاریخچهٔ فاقد event و peel-chain سه تا پنج‌مرحله‌ای با اتصال واقعی outpoint را هنگام تحقیق می‌سازد؛ source، event و coverage به assessment ماندگار و snapshot مرکزی متصل‌اند. API فقط‌خواندنی، نمایش پنل، حفظ داده در Export/reopen و invalidation با تغییر detector آزمون شدند. **الگوها به‌تنهایی امتیاز ریسک، مالکیت، change، mixer یا پول‌شویی را ثابت نمی‌کنند. سقف ۲۵ ریشه، ۶۰ context، ۴۰۹۶ leg و ۲۵۶ leg برای هر تراکنش صریح است. historical fact backfill و worker مستقل بازند؛ پذیرش مصنوعی است و mainnet/VM/Neo4j زنده را تأیید نمی‌کند.** [روش و API](dockerizd_bitcoin/SEMANTICS.md)، [گزارش پذیرش](dockerizd_bitcoin/SEMANTICS_VALIDATION.md).
- [x] **AML-07: asset metadata و holdings.** مشخصات native BTC با منبع، نسخه و تأیید سیستمی protocol در `aml_asset_metadata` موجود ثبت و کنترل می‌شوند؛ تأیید انسانی intelligence یا supply جاری ساخته نمی‌شود. موجودی دقیق از آخرین occurrence هر outpoint پیش از فیلتر آدرس محاسبه می‌شود؛ spent/unspent، BIP30، بلوغ coinbase در tip و امکان ورود به بلاک بعدی، تاریخچهٔ ناقص و ارتفاع snapshot صریح‌اند. قبل از بلاک اول مبلغ null است، mature ادعای داشتن کلید نیست، net flow موجودی نیست و wallet RPC استفاده نمی‌شود. token/EVM نامرتبط `not_applicable` است. API، پنل، منبع/metadata/hash snapshot و حفظ بدون تغییر در create/Export/reopen با ۱۰۱ بلاک مصنوعی و reorg آزمون شدند. **بدون تغییر schema؛ نصب موجود به اجرای schema job جدید برای bootstrap رکورد BTC نیاز دارد. ظرفیت production و نود/VM/Neo4j واقعی همچنان بازند.** [روش و راه‌اندازی](dockerizd_bitcoin/HOLDINGS.md)، [گزارش پذیرش](dockerizd_bitcoin/HOLDINGS_VALIDATION.md).
- [x] **AML-08: workerهای پیوسته.** sweep صفحه‌بندی‌شدهٔ آدرس‌های canonical، صف حداکثر ۱۲۸ job، یک job هم‌زمان، timeout، backoff، heartbeat و وضعیت waiting/running/idle/failed اضافه شدند. worker همان مسیر fingerprint/semantic/clustering/exposure/risk موجود را مصرف می‌کند؛ خروجی تحلیلی در جدول‌های موجود، و cursor/صف در checkpoint اتمیک و checksum‌دارِ volume عملیاتی مستقل همان VM ثبت می‌شوند. قفل analytics از ingestion و review جداست. SIGKILL/restart، توقف سالم، رد worker دوم، seed pending/approved/revoked، rollback و خطای دو job مستقل آزمون شدند. CLI/API، وضعیت ثبت‌شده در پنل و حفظ آن در create/Export/reopen و کنترل‌های startup/pause/resume متصل‌اند. **بدون تغییر schema یا سیاست ریسک؛ refresh پس از پایان sweep است، SLA پنج‌دقیقه‌ای یا پوشش همهٔ آدرس‌ها نیست. retry حداقل یک‌بار است؛ historical fact backfill، هشدار/پرونده، ظرفیت production و نود/VM/Neo4j واقعی همچنان بازند.** [روش](dockerizd_bitcoin/ANALYTICS.md)، [پذیرش](dockerizd_bitcoin/ANALYTICS_VALIDATION.md).
  - [x] کشف صفحه‌بندی‌شدهٔ آدرس‌های canonical، صف محدود و چرخهٔ refresh با سقف منابع و ادامهٔ cursor
  - [x] اجرای مسیر فعلی feature/semantic/clustering/exposure/risk، بدون تأیید خودکار یا تغییر schema
  - [x] checkpoint پایدار، قفل مستقل، heartbeat، retry و بازیابی پس از قطع یا reorg
  - [x] CLI/API وضعیت، سرویس Compose و اتصال به launcher و کنترل‌های VM
  - [x] آزمون واقعی ClickHouse و restart/failure/invalidation و مستندسازی محدودهٔ پذیرش

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

با اولویت عملی جدید، قابلیت‌های AML بیت‌کوین روی قرارداد موجود نسخهٔ ۵ تا API، سیاست مرکزی و پنل کامل می‌شوند؛ زیرساخت تازه فقط برای رفع مانع مشخص همان قابلیت اضافه می‌شود. مهاجرت نهایی هر چهار شبکه و پذیرش P0 همچنان پیش‌نیاز اعلام یکسان‌سازی کامل و rollout نهایی هستند. صرف افزودن جدول، endpoint، worker روشن یا داده خالی، تکمیل قابلیت نیست؛ برای هر قابلیت writer، داده ذخیره‌شده، reader، نمایش و آزمون پذیرش لازم است.

**مرحله اول، انتقال دادهٔ پایهٔ بیت‌کوین، probe، دریافت قابل‌اتکاتر و failure/repair گام پنجم انجام شده‌اند؛ replay مستقل، مهاجرت نهایی چهار شبکه، جدول‌های تحلیلی و تکمیل امکانات AML همچنان باز هستند. نصب و آزمون‌ها روی ClickHouse موقت و RPC کنترل‌شده انجام شده‌اند و به معنی اجرای migration روی دیتابیس مستقر یا تأیید نود واقعی نیستند.**
