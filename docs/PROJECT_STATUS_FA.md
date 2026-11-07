# وضعیت واقعی پروژه و تصمیم‌های پاک‌سازی

تاریخ ممیزی: ۲۰۲۶-۱۱-۰۶؛ پذیرش DB: ۲۰۲۶-۱۱-۰۷. دامنه هر چهار شبکهٔ TRON، Ethereum، BSC و Bitcoin
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
پوشش trace اتریوم/BSC صفر بود و موجودی RPC در آخرین آزمون آن‌ها unavailable
شد؛ بنابراین پذیرش کامل قابلیت/ظرفیت RPC، VM و مسیر مرکزی/Neo4j باقی است.

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
| VM اصلی | پنل، API مرکزی تحقیق، snapshot، ذخیره/بازگشایی و Export در Neo4j وجود دارد | [سرویس مرکزی](../analytical_node/src/main.rs)، [Compose](../compose.yaml) |
| قرارداد API | قرارداد مستند موجود است؛ parity کامل هر چهار پاسخ و خطا و pagination هنوز باید پذیرفته شود | [OpenAPI](../contracts/chain-api-v1.openapi.yaml) |
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
| attribution، sanctions و intelligence | source/claim/review و import وجود دارد؛ دیتاست جامع معتبر همراه کد نداریم | خوراک معتبر/مجاز، نسخه، refresh، provenance و آزمون لغو/تغییر برچسب |
| خوشه‌بندی و entity | heuristic و candidate/review در سطح‌های متفاوت موجود است | parity و ارزیابی دقت؛ تشخیص CoinJoin/ابهام و جلوگیری از انتساب قطعی بی‌شاهد |
| exposure مستقیم/غیرمستقیم و ریسک | مسیرها و سیاست شواهدی موجودند | اجرای یکسان، fresh بودن ورودی‌ها، توضیح score و ارزیابی false positive/negative |
| KYT پیوسته | worker تحلیلی با سامانهٔ کامل پایش تراکنش مشتری متفاوت است | watchlist، ثبت customer/transaction، rules/threshold، alert پایدار و webhook |
| رسیدگی به alert و پرونده | ذخیرهٔ snapshot تحقیق وجود دارد؛ workflow کامل پرونده/alert نداریم | وضعیت، مسئول، یادداشت، شواهد، timeline، audit و export گزارش |
| cross-chain و DeFi | برخی decoderها و correlation key موجودند | تطبیق دو طرف bridge/swap با شواهد؛ attribution پروتکل و پوشش نسخه‌دار |
| تیم و عملیات | service auth و جداسازی تحقیق وجود دارد | RBAC، audit عملیات، TLS، پایش lag، backup/restore واقعی و آزمون ظرفیت |

دیتاست attribution و کیفیت آن بخش مستقلی از محصول است؛ صرف افزودن جدول،
قانون یا ML جای آن را نمی‌گیرد. ترتیب کار برای کوتاه‌کردن زمان تحویل در
[فهرست واحد](../TODO_LIST.md) آمده است.

## تنظیمات خصوصی Git

فایل‌های `.env` ریشه و `dockerizd_bitcoin/.env` در index گیت بودند؛ از
ردیابی خارج شدند و نسخهٔ محلی آن‌ها حفظ شد. نمونه‌های `.env.example`
قابل commit هستند. اگر مقدار واقعی قبلاً push شده، کلیدها/رمزهای مربوط
باید عوض شوند؛ این تغییر تاریخچهٔ commitهای قبلی را پاک نمی‌کند.
