# DB-07؛ canonical، snapshot و اعتبار خروجی‌های وابسته

قرارداد مشترک نسخهٔ **۵** شامل **۳۹ جدول، ۵۳ view و ۶۸۲ ستون جدول** است.
هر شبکه همچنان ClickHouse، database و volume مستقل روی VM خودش دارد.
`shared/warehouse` کتابخانهٔ داخل adapterهاست؛ سرویس دیتابیس مرکزی اضافه نشده است.

checksum نسخهٔ ۵:

`8fc65f6db710106cf6a476b2ac9469e0bcf1d4769c80e2326436a166fab6dcdf`

تعریف‌ها و SQL نسخه‌های ۱ تا ۴ تغییر نکرده‌اند. [نسخهٔ ۴ ثابت](analytics-contract-v4.json)
و [قرارداد مؤثر جاری](current-contract.json) جدا هستند. SQL قابل بررسی بیت‌کوین
در `sql/migrations/20261027_0005_visibility.sql` از منبع مشترک تولید می‌شود.

## انتشار و rollback

انتشار بلاک جدید به complete marker با revision دقیق همان انتشار، تراکنش‌های
کامل، ارتفاع بعدی و parent درست نیاز دارد. تعداد تراکنش‌ها باید با پوشش خانوادهٔ
`aml_transactions` برابر باشد. تغییر hash در همان ارتفاع به rollback قبلی نیاز
دارد؛ محدودهٔ شروع هنگام initialized بودن تغییر نمی‌کند. checkpoint پیش از
انتشار دوباره بررسی می‌شود.

rollback نشانگرهای `orphaned` را با revision جدید و بدون حذف factها ثبت می‌کند،
سپس fence جدید را منتشر می‌کند. قطع پیش از fence، snapshot قبلی را حفظ می‌کند؛
پس از موفقیت، بالا رفتن دوبارهٔ tip شاخهٔ قبلی را ظاهر نمی‌کند. سقف rollback
مشترک ۱۰هزار بلاک است و تنظیم بیت‌کوین نمی‌تواند از آن بیشتر شود. این مسیر
به همان مالکیت انحصاری writer و allocator مشترک revision نیاز دارد؛ lease
توزیع‌شده ارائه نمی‌کند.

view بلوک canonical آخرین ردیف کامل را در محدودهٔ start/tip منتشرشده و با
hash مطابق tip انتخاب می‌کند. هر ۱۲ خانوادهٔ fact، علاوه بر network، height،
hash، commit و revision، زمان بلاک یکسان دارند. view وضعیت و projectionهای
فیلترشده ابتدا آخرین ردیف کامل را با `argMax(tuple(...), revision)` انتخاب
می‌کنند؛ فیلتر status نمی‌تواند نسخهٔ قدیمی‌تر را دوباره ظاهر کند.

## snapshot و confirmation

`visibility::Snapshot` هویت شبکه، revision دقیق، محدوده، tip hash، ارتفاع RPC
ثبت‌شده و زمان publication را در یک شناسهٔ SHA256 می‌بندد. API مقدار revision
را به‌صورت رشتهٔ صحیح دقیق می‌فرستد. خواندن‌های core، analytics و روابط، fence
را قبل و بعد بررسی می‌کنند؛ تغییر آن نیازمند retry یا محاسبهٔ دوباره است.
این روش transaction چند-query یا snapshot توزیع‌شده نیست.

`confirmed_tip(head, confirmations)` بلاک را در شمار confirmation حساب می‌کند؛
در کمبود ارتفاع `None` می‌دهد و underflow نمی‌کند. ingestion بیت‌کوین همین
رفتار قبلی را از helper مشترک مصرف می‌کند. عمق confirmation اثبات finality
اجماعی نیست؛ خروجی `finality=not_asserted` است. انتخاب safe/finalized در شبکه‌های
دیگر باید از سیاست و شواهد همان adapter بیاید. ارتفاع RPC در این descriptor
مقدار ثبت‌شده هنگام publication است، نه یک probe زندهٔ نود.

## خروجی‌های تحلیلی و aggregate

جدول جدید `aml_analysis_publications`، خانواده، run/assessment، source revision،
as-of/hash، output revision و تعداد ردیف‌ها را seal می‌کند. `publish_analysis`
فقط یک attempt با ID جدید، ردیف‌های هم‌خانواده و revision یکسان را می‌پذیرد.
run باید complete و بدون truncation باشد. شمارش واقعی ردیف‌ها و snapshot پیش
از seal بررسی می‌شوند. قطع بین خروجی‌ها و seal، خروجی جاری ایجاد نمی‌کند.

| خانواده | خروجی‌های وابسته به seal |
|---|---|
| cluster | runs، claims، memberships و versions |
| exposure | runs، paths و aggregateهای `aml_address_exposures` |
| risk | assessments و signals؛ وابستگی exposure نیز باید معتبر باشد |

viewهای `_current` برای ممیزی آخرین ردیف ذخیره‌شده‌اند. `Mode::Current` در reader
مشترک، برای این خروجی‌ها از `_visible` استفاده می‌کند. `Mode::Audit` عمداً
ردیف‌های تاریخی، stale یا بدون seal را هم می‌خواند و ورودی امتیاز جاری نیست.
latest parent با وضعیت stale یا seal لغوشده، فرزندان را نیز پنهان می‌کند.

هر تغییر fence زنجیره، از جمله append عادی، sealهای snapshot قبلی را از مصرف
جاری خارج می‌کند. این سیاست محافظه‌کارانه است و فعلاً تحلیل مجدد لازم دارد؛
نگه‌داشتن انتخابی تحلیل‌های prefix معتبر یا بهینه‌سازی incremental پیاده نشده است.
`invalidate_analysis` نیز برای مالک policy/review/detector، لغو صریح و افزایشی
seal را فراهم می‌کند. اتصال خودکار تغییر intelligence به workerهای آینده در
AML-01 تا AML-08 باقی است؛ در این گام detector جدیدی اجرا نمی‌شود.

## metadata و صف کار

سه ستون nullable اضافه شده‌اند: `aml_cluster_versions.run_id` و
`aml_asset_metadata.{source_revision, observed_block_hash}`. NULL یعنی provenance
تاریخی نامعلوم؛ migration از روی ارتفاع، hash یا منبع حدسی تولید نمی‌کند.

metadata مشاهده‌شده روی زنجیره، فقط با snapshot جاری و hash canonical همان
observed block قابل مصرف است. job ناشی از discovery نیز به discovery canonical،
asset، height و source revision معتبر نیاز دارد. ردیف جدید با hash اشتباه یا
منبع stale، metadata قدیمی‌تر را دوباره ظاهر نمی‌کند. catalog/job صریحاً خارج
از زنجیره، با NULL بودن تمام locatorهای زنجیره، همچنان catalog/job باقی می‌ماند؛
این visibility تأیید intelligence یا مجوز استفاده در risk نیست.

## بیت‌کوین، پنل و تحقیق ذخیره‌شده

status، holdings، investigation و paths بیت‌کوین `source_snapshot` دارند.
پاسخ‌های محافظت‌شدهٔ API با `Cache-Control: no-store` ارسال می‌شوند. API و
aggregateهای موجود از factهای canonical می‌خوانند و پاسخ چندبخشی fence مشترک دارد.

نود مرکزی هنگام ایجاد، بازکردن و Export تحقیق بیت‌کوین، وضعیت منبع را از API
همان شبکه با مهلت و اندازهٔ پاسخ محدود بررسی می‌کند. تغییر source، `stale` و
عدم امکان تأیید آن، `unverified` است. در این دو حالت risk جاری `UNKNOWN` می‌شود؛
نتیجهٔ اصلی در `historical_risk_engine` و شواهد snapshot حفظ می‌شوند. پنل هشدار
تاریخی نشان می‌دهد. Export همچنان ذخیرهٔ شواهد تاریخی است و آن‌ها را تازه نمی‌کند.
اعتبار current به publication شناخته‌شده در لحظهٔ بررسی مربوط است؛ وضعیت زندهٔ
نود یا finality جهانی را تضمین نمی‌کند.

## محدودهٔ پایان و استقرار

قرارداد مقصد، migration/check مشترک، readerهای امن و اتصال فعال بیت‌کوین در
DB-07 تکمیل شده‌اند. reader/writerهای فعال legacy سه شبکه، انتقال تاریخچه،
سازگارکردن workerهای قبلی و restore، **DB-08** هستند. این مرحله ادعای یکسان‌شدن
کل دیتابیس فعال یا امکانات AML نمی‌کند. rollout روی VM واقعی انجام نشده است.

برای ارتقا باید پردازش‌های متکی به نسخهٔ قدیمی متوقف و backup/restore بررسی
شوند؛ schema job نسخهٔ جدید روی دیتابیس مستقل همان VM اجرا شود و پس از check
موفق، سرویس متناظر همان نسخه راه‌اندازی شود. binary قدیمی با identity جدید
سازگار فرض نمی‌شود. حذف volume یا ویرایش migration قبلی مسیر ارتقا نیست.

[گزارش اجرا و مرز آزمون‌ها](DB07_VALIDATION.md) و [رسید چهار fixture](db07-acceptance.json).
گام بعد **DB-08: انتقال بدون اتلاف، تغییر مسیرهای فعال و آزمون restore** است.
