# وضعیت واقعی پروژه و تصمیم‌های پاک‌سازی

تاریخ ممیزی: ۲۰۲۶-۱۱-۰۶؛ پذیرش DB: ۲۰۲۶-۱۱-۰۷؛ پذیرش محدود انتهابه‌انتها: ۲۰۲۶-۱۱-۰۸. دامنه هر چهار شبکهٔ TRON، Ethereum، BSC و Bitcoin
است. دیتابیس هنگام استقرار خالی خواهد بود؛ نگه‌داری دادهٔ آزمایشی و انتقال
ردیف‌های دیتابیس قدیمی جزو نیاز محصول نیست.

## نتیجهٔ اصلی

**یکسان‌سازی ساختار، نصب و مسیر داده در سطح کد پذیرفته شد.** قرارداد مشترک وجود دارد و
روی هر چهار adapter نصب می‌شود و writer هر چهار شبکه اکنون facts و
checkpoint را از مسیر مشترک منتشر می‌کند. در ترون، APIهای دادهٔ بلاک و
رکوردهای عملیاتی metadata، intelligence، clustering/exposure نیز روی `aml_*`
هستند. DDL بومی و mountها حذف و SQL شش فاز فقط در پوشهٔ مشترک تولید
می‌شود. initializer واقعی چهار شبکه روی چهار ClickHouse خالی اجرا شده و
engine/view/column/key/index یکسان تأیید شده است. هر ۱۳ سناریوی دریافت،
restart/retry، برخورد با تغییر شاخه و query/مصرف‌کنندگان چهار شبکه روی
قرارداد v6 با ClickHouse واقعی و RPC نمونه موفق شدند. سپس آزمون محدود RPC
عمومی واقعی برای هر چهار شبکه روی ۵ بلاک و ۸۷۹۱ تراکنش موفق شد: دریافت،
تطبیق با پاسخ مستقل RPC، restart بدون تکرار و API تحقیق دارای auth.
پوشش trace اتریوم/BSC صفر بود. سپس مسیر محدود واقعی RPC → چهار ClickHouse
مستقل → چهار API → Gateway/Analytical Node → Neo4j → پنل واقعی نیز برای
Wallet و Paths هر چهار شبکه پذیرفته شد؛ مبلغ RPC و reopen/Export دقیق ماند.
موجودی EVM در درخواست زنده snapshot مستقل و فرّار است، پس شواهد کاملِ همان
تحقیق پس از reopen تطبیق شد. پذیرش کامل قابلیت/ظرفیت RPC، بار و پنج VM کاربر
همچنان باقی است.

مرحلهٔ DB-1 تکمیل شده است: قرارداد اجرایی writer/reader مشترک با تطبیق
دادهٔ ذخیره‌شده روی چهار database خالی آزموده شد و writer فعال Bitcoin
از همان مسیر استفاده می‌کند. مسیرهای زندهٔ Ethereum نیز به قرارداد
مشترک متصل شدند و DB-2 با تست کامل RPC نمونه → ClickHouse → API/worker
پذیرفته شد. BSC نیز در DB-3 به مسیر مشترک متصل شد؛ replay/repair،
reorg، metadata، intelligence و API آن با RPC نمونه و ClickHouse واقعی
آزموده شدند. DB-4.1 قرارداد/source و DB-4.2 انتشار بلاک ترون را متصل کردند؛ DB-4.3 رکوردهای عملیاتی و مصرف‌کننده‌ها را به همان قرارداد وصل کرد و با تست زنده پذیرفته شد. نتیجهٔ دقیق در [TESTING](TESTING.md) ثبت شده است.

`shared/warehouse` نام کتابخانهٔ Rust قرارداد مشترک است؛ سرویس، VM یا
دیتابیس مرکزی دیگری نیست. هر شبکه ClickHouse، database و volume خودش را
دارد. تنها Neo4j مرکزی روی VM اصلی قرار می‌گیرد.

| بخش | وضعیت کد فعلی | شاهد |
|---|---|---|
| قرارداد مشترک | نسخهٔ عملیاتی ۶، ۴۲ جدول، ۵۶ view، ۷۳۹ ستون؛ namespace مشترک `aml_*` | [قرارداد](warehouse/current-contract.json)، [init/check](../shared/warehouse/src/migration.rs) |
| writer/reader مشترک | اعتبارسنجی کل batch، کنترل شناسه/تراکنش/پروفایل، تطبیق تمام ستون‌های ذخیره‌شده و انتشار checkpoint؛ writer هر چهار شبکه به آن متصل است | [core](../shared/warehouse/src/core.rs)، [آزمون](../shared/warehouse/tests/core_contract.rs) |
| Bitcoin | دریافت و مسیرهای اصلی تحقیق و تحلیل از قرارداد مشترک استفاده می‌کنند | [writer](../dockerizd_bitcoin/src/storage/writer.rs)، [نگاشت](../dockerizd_bitcoin/src/db/warehouse.rs)، [API](../dockerizd_bitcoin/src/api.rs) |
| Ethereum | ingest/checkpoint، API تحقیق، metadata و workerهای intelligence/cluster/exposure/risk از `aml_*` استفاده می‌کنند؛ DDL قبلی حذف شده است | [schema](../dockerizd_ethereum/src/db/migrations.rs)، [writer](../dockerizd_ethereum/src/storage/writer.rs)، [تحقیق](../dockerizd_ethereum/src/investigation.rs) |
| BSC | ingest/checkpoint، replay/repair، metadata، protocol/intelligence و API از `aml_*` استفاده می‌کنند؛ exposure هنگام درخواست و ارزیابی ریسک در مرکز باقی است | [schema](../dockerizd_bsc/src/db/migrations.rs)، [writer](../dockerizd_bsc/src/ingestion/store.rs)، [تحقیق](../dockerizd_bsc/src/investigation/wallet.rs) |
| TRON | انتشار بلاک/receipt/انتقال و checkpoint مشترک، replay، خطا/benchmark مشترک و projection دادهٔ بلاک فعال‌اند؛ metadata/intelligence، خوشه‌بندی و exposure نیز مشترک‌اند؛ SQL و mount بومی حذف شدند و schema job پیش از runtime اجرا می‌شود | [schema](../dockerizd_tron/app/src/db/tron_schema.rs)، [publisher](../dockerizd_tron/app/src/services/tron/publication_store.rs)، [ingest](../dockerizd_tron/app/src/services/tron/fetcher.rs) |
| VM اصلی | پنل، API مرکزی تحقیق، snapshot، ذخیره/بازگشایی و Export در Neo4j وجود دارد؛ مسیر زندهٔ محدود هر چهار شبکه تا پنل واقعی پذیرفته شده است | [سرویس مرکزی](../analytical_node/src/main.rs)، [پذیرش](TESTING.md) |
| دریافت و بازیابی | ING-1 دریافت پیوسته و restart را پذیرفت. ING-2 ابزارهای probe/failures/scan-gaps/replay/repair را برای هر چهار VM به ingestor موجود متصل کرد؛ قفل writer، حفظ tip در refresh تاریخی و repair صریح dead-letter برقرارند. probe نمونه‌ای است؛ archive کامل، پذیرش طولانی RPC و اتصال پیوسته تا مرکز روی VM کاربر هنوز باز است | [قرارداد عملیات](../shared/warehouse/src/maintenance.rs)، [launcher](../scripts/vm.sh)، [استفاده](USAGE_FA.md)، [آزمون](TESTING.md) |
| قرارداد API | API-1 ورودی/خطا، API-2 فهرست transfers با فیلتر و cursor مقید به snapshot، و API-3 پاسخ مشترک تحقیق/مسیر و مصرف مرکزی پذیرفته شدند. مبلغ/NFT/UTXO و مدل موجودی حفظ می‌شوند؛ تغییر منبع در بازگشایی/Export ریسک جاری را UNKNOWN می‌کند. چهار API با ClickHouse واقعی و هشت حالت مرکزی با Neo4j واقعی آزموده شدند؛ پذیرش پیوستهٔ RPC/VM جداست | [OpenAPI](../contracts/chain-api-v1.openapi.yaml)، [پاسخ مشترک](../shared/chain_api/src/response.rs)، [خواندن صفحه](../shared/warehouse/src/relationships/page.rs)، [آزمون‌ها](TESTING.md) |
| استقرار | helper هر پنج role و Composeها وجود دارند؛ اجرای واقعی روی VMهای کاربر تأیید نشده | [vm.sh](../scripts/vm.sh)، [راهنمای دستی](VM_SETUP_FA.md) |

وضعیت «وجود کد» با «صحت روی RPC/VM واقعی» یکی نیست. تاریخچهٔ بلاک‌های
ذخیره‌شده، completeness، finality، prevout و trace باید در پذیرش واقعی
اندازه‌گیری شوند. دادهٔ نمونه، پوشش آدرس‌های واقعی یا کیفیت intelligence
در سطح تجاری را اثبات نمی‌کند.

## چه چیزی حذف و چه چیزی حفظ شد

حذف شدند:

- ابزارهای `warehouse_migrate` و `warehouse_migration` برای نگاشت و حفظ
  تاریخچهٔ قبلی و integrationهای مخصوص همان انتقال.
- مسیر موازی `warehouse_replay`، دو binary آفلاین `warehouse_source_decode`
  و proofهای آن؛ دریافت و replay بلاک در ingestor اصلی باقی می‌ماند.
- نسخه‌های صرفاً تاریخی ۶ تا ۱۰ و جدول‌های `aml_ml_*`،
  `aml_analysis_subjects/snapshots/evidence` و
  `aml_legacy_operational_observations`؛ هیچ writer فعال محصول به آن‌ها
  وابسته نبود. CHECKهای سهل‌شده برای provenance نامعلوم قدیمی هم حذف شدند.
- CLI انتقال دیتابیس قدیمی Bitcoin، SQL قدیمی مخصوص آن و شرط آماده‌سازی
  وابسته به backfill؛ تست اصلی Bitcoin اکنون از دیتابیس خالی و RPC نمونه
  شروع می‌شود. کنترل checkpoint، writer lock، prevout و reorg حفظ شده است.
- سازندهٔ خودکار VM، تنظیمات Multipass و تست‌های آن؛ VMها را کاربر طبق
  راهنمای دستی می‌سازد.
- گزارش‌های متوالی DB-01 تا DB-08، JSONهای پذیرش مرحله‌ای، نگاشت‌های
  تولیدشدهٔ قدیمی و گزارش‌های تکراری validation. مرجع ادامه فقط
  [TODO_LIST](../TODO_LIST.md) است.

- ذخیرهٔ snapshot و executor/آموزش ML محلی TRON پس از اتصال مسیر معادل
  به ذخیره/بازگشایی/Export مرکزی کنار گذاشته شد؛ جدول مشترک جایگزینِ موازی
  ساخته نشد. ML محصولی همچنان نیازمند داده و ارزیابی مستقل است.

حفظ شدند:

- ingestion، decoder، API و قابلیت‌های تحلیلی زندهٔ هر چهار شبکه؛ وابستگی
  به جدول‌های native پس از پذیرش جایگزین مشترک قطع شد.
- نسخه‌های DDL عملیاتی ۱ تا ۵، validation نوع/مبلغ، publication fence،
  checkpoint و تست‌های واقعی خطا و reorg. migration ساختار جدول با انتقال
  دادهٔ تاریخی تفاوت دارد و برای init و تغییرات بعدی لازم است.
- backup/restore بومی هر شبکه، تنظیم دیسک backup و تست‌های واحدش؛ این قابلیت
  برای داده‌ای است که پس از راه‌اندازی تولید می‌شود و پیش‌نیاز مهاجرت قبلی نیست.
- سندهای Word و بستهٔ تحویلی که قبلاً صریحاً درخواست شده بودند؛ این‌ها
  تحویل مشتری‌اند و مرجع جاری وضعیت کد نیستند.

این پاک‌سازی برای **استقرار تازه** است. دیتابیس آزمایشی با نسخه‌های حذف‌شده
۶ تا ۱۰ به قرارداد عملیاتی فعلی downgrade نمی‌شود؛ برای تست/استقرار جدید volume تازه
استفاده می‌شود. هیچ دیتابیس یا volume پیشین پروژه در این نوبت پاک نمی‌شود.

## فاصله با هدف AML هم‌سطح قابلیت‌های محصول تجاری

فهرست هدف از قابلیت‌های منتشرشدهٔ
[Chainalysis KYT](https://www.chainalysis.com/product/kyt/) و
[Reactor](https://www.chainalysis.com/product/reactor/) استخراج شده است؛
وضعیت زیر حاصل ممیزی همین کد است، نه تأیید یا ادعای برابری با آن محصولات.
دامنهٔ این پروژه همچنان همین چهار شبکه است.

| قابلیت هدف | اکنون | کار لازم |
|---|---|---|
| دریافت داده و گراف تحقیق | موجود، با تفاوت شبکه‌ها و پوشش محدود به دادهٔ ذخیره‌شده | schema/IO مشترک و پذیرش از صفر برای هر چهار شبکه |
| attribution، sanctions و intelligence | ثبت منبع، بازبینی و import چهار شبکه. نگاشت نمونه‌های snapshot رسمی OFAC و تفکیک آدرس BNB Beacon از BSC با آزمون قابل تکرار بررسی شده‌اند | مجموعهٔ عملیاتی مجاز و بازبینی‌شده، refresh و ارزیابی مستقل کیفیت انتساب |
| خوشه‌بندی و entity | پاسخ مشترک سرنخ را از ارتباط بازبینی‌شده جدا می‌کند. سرنخ Bitcoin بعد از بازبینی هم در همان دسته می‌ماند و BSC سرنخ pending را در تحقیق نمایش می‌دهد | ارزیابی مستقل دقت خوشه‌بندی و پوشش ابهام و CoinJoin روی دادهٔ برچسب‌دار |
| exposure مستقیم/غیرمستقیم و ریسک | جهت و ارجاع شواهد بومی در پاسخ مشترک. ارزیاب آفلاین خطا و پوشش سیاست را به تفکیک شبکه، تاریخچه و سیگنال می‌سنجد | اجرای ارزیاب روی مجموعهٔ مستقل پرونده‌های برچسب‌دار و تأیید آستانهٔ عملیاتی |
| KYT پیوسته | Watchlist و بررسی دوره‌ای چهار شبکه، هشدار پایدار، واگذاری و رسیدگی، صف webhook و اتصال هشدار به پروندهٔ تحقیق | پذیرش عملیاتی روی VMها |
| رسیدگی به alert و پرونده | ذخیرهٔ snapshot تحقیق وجود دارد؛ workflow کامل پرونده/alert نداریم | وضعیت، مسئول، یادداشت، شواهد، timeline، audit و export گزارش |
| cross-chain و DeFi | شواهد نسخه‌دار ABI، انتقال NFT و رخداد Seaport 1.6 در تحقیق اتریوم. تطبیق cBridge اتریوم و BSC، Allbridge Core برای مقصد USDT بین BSC و ترون، و شواهد ثبت، جمع‌آوری و صدور خوش‌بینانهٔ سپردهٔ استاندارد tBTC v2 از بیت‌کوین به اتریوم. شاهد هر دو شبکه در Neo4j ثبت می‌شود | تأیید صدور عادی و بازخرید tBTC، گسترش پل‌ها، بازارهای NFT و میکسر TRON با شواهد معتبر هر پروتکل |
| تیم و عملیات | service auth و جداسازی تحقیق وجود دارد | RBAC، audit عملیات، TLS، پایش lag، backup/restore واقعی و آزمون ظرفیت |

دیتاست attribution و کیفیت آن بخش مستقلی از محصول است؛ صرف افزودن جدول،
قانون یا ML جای آن را نمی‌گیرد. ترتیب کار برای کوتاه‌کردن زمان تحویل در
[فهرست واحد](../TODO_LIST.md) آمده است.

## تنظیمات خصوصی Git

فایل‌های `.env` ریشه و `dockerizd_bitcoin/.env` در index گیت بودند؛ از
ردیابی خارج شدند و نسخهٔ محلی آن‌ها حفظ شد. نمونه‌های `.env.example`
قابل commit هستند. اگر مقدار واقعی قبلاً push شده، کلیدها/رمزهای مربوط
باید عوض شوند؛ این تغییر تاریخچهٔ commitهای قبلی را پاک نمی‌کند.
