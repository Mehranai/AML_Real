# Bitcoin exposure؛ روش و استفاده

مسیر تحقیق اکنون exposure ورودی و خروجی را از دادهٔ canonical بیت‌کوین محاسبه و در
ClickHouse همان VM ثبت می‌کند. schema نسخهٔ **۵** ثابت است؛ جدول تازه یا ClickHouse
مرکزی اضافه نشده است. ارزیابی ریسک و snapshot تحقیق همچنان از Analytical Node و
Neo4j موجود در VM اصلی عبور می‌کنند.

## معنی مسیر و مبلغ

هر hop یک تراکنش است و ادامهٔ hop فقط وقتی پذیرفته می‌شود که ورودیِ تراکنش بعدی
دقیقاً output قبلی را با txid/vout خرج کرده باشد؛ occurrence بلاک، ترتیب تراکنش،
subject و مقدار هم تطبیق می‌یابند. یکسان‌بودن آدرس در دو تراکنش مسیر نمی‌سازد.
برای txidهای تاریخی تکرارشده، output واقعاً موجود پیش از input انتخاب می‌شود و
spend به occurrence دیگر وصل نمی‌شود. script بدون آدرس، گرهٔ script می‌ماند.

این استفاده از outpoint و بقای مقدار بر مدل تراکنش توضیح‌داده‌شده در
[راهنمای Bitcoin](https://developer.bitcoin.org/devguide/transactions.html) استوار است.
از مشترک‌بودن ورودی‌ها مالکیت مشترک نتیجه گرفته نمی‌شود؛
[BIP-78](https://github.com/bitcoin/bips/blob/master/bip-0078.mediawiki) نمونه‌ای از
تراکنش مشارکتی و محدودیت چنین heuristicهایی را توضیح می‌دهد.

روش نسخه‌دار این پروژه `bitcoin_utxo_bounds_v1` است. اگر مقدار قابل انتساب در
ورودی `[L,U]`، کل ورودی تراکنش `I` و output مورد بررسی `O` باشد:

```text
lower = max(0, L - (I - O))
upper = min(U, O)
```

کارمزد جزو بقیهٔ مقاصد مقدار است؛ از output هدف کنار گذاشته نمی‌شود. ابتدا مجموع
ورودی/خروجی و fee، تعداد و موقعیت legها و شاهدهای canonical کنترل می‌شوند. ورودی‌های
متعدد همان آدرس seed در hop اول با هم بررسی می‌شوند؛ آدرس‌های مختلف به یک مالک
فرضی تبدیل نمی‌شوند. ادامهٔ هر مسیر فقط مقدار همان outpoint را حمل می‌کند.

این بازه یک **استنتاج محافظه‌کارانه تحت مدل برچسب‌گذاری مقدار** است؛ تخصیص دقیق
ساتوشی، مالکیت یا اثبات پول‌شویی نیست. حد پایین صفر یعنی سهم آن مسیر اثبات نشده
و فقط امکان اتصال دیده شده است. مسیرهای همگرا می‌توانند هم‌پوشان باشند؛ بازهٔ
مسیرها با هم جمع نمی‌شود. summary هر seed/direction، قوی‌ترین مسیر را نگه می‌دارد
و فقط تعداد مسیرها را گزارش می‌کند.

## seed، سرویس و سیاست ریسک

seed باید از reader برچسب مرحلهٔ AML-01 بیاید: منبع فعال، claim تأییدشده، review
منطبق با revision، evidence refs و designation صریح ریسک. بازهٔ اعتبار claim باید
زمان تراکنشِ برخورد با seed را نیز پوشش دهد؛ تأیید امروز خودبه‌خود ادعای مالکیت
در گذشته نمی‌سازد. `valid_from_unix_ms` در import بدون مقدار، زمان import است؛
برای دادهٔ تاریخی بازهٔ معتبر واقعی را صریح و با شواهد ثبت کنید.

مرز exchange/custodian/bridge/dex/mixer که با claim فعال شناخته شده باشد، traversal
را متوقف می‌کند. مسیر مستقیم شامل چنین مرزی فقط context است. این روش CoinJoin
یا change را تشخیص نمی‌دهد و استفاده از آدرس مشترکِ سرویس را به گناه مشترک تبدیل
نمی‌کند. مسیرِ فقط احتمالی، service-mediated، ناقص یا تأییدنشده امتیاز نمی‌سازد.

برای مسیر معتبرِ دارای حد پایین مثبت، سهم حد پایین در output، confidence و ریسک
اعلام‌شدهٔ seed، decay هر hop برابر **۰٫۶۵** و نیمه‌عمر **۳۶۵ روز** به strength
تبدیل می‌شوند. decayها از مسیر موجود BSC استفاده می‌کنند؛ ساعت مبنا، timestamp
بلاکِ snapshot تحلیل است. ترتیب علّی با `(block_number, transaction_index)` کنترل
می‌شود؛ timestamp هدر الزاماً صعودی نیست و وزن زمانی از ۱ بیشتر نمی‌شود.

مرکز همان وزن‌های موجود policy را مصرف می‌کند: قوی‌ترین exposure ورودی مستقیم،
خروجی مستقیم یا چندمرحله‌ای؛ مسیرهای تکراری رأی مستقل نیستند. مرکز scope، seed،
گام‌ها، outpoint، بازهٔ مبلغ و سقف strength بیت‌کوین را دوباره کنترل می‌کند.
امتیاز، evidence score سیاست موجود است؛ احتمال کالیبره‌شده یا نتیجهٔ حقوقی نیست.
نبود signal برای بیت‌کوین همچنان UNKNOWN می‌ماند.

## ذخیره و تازگی

- `aml_exposure_seeds`: snapshot ادعای seed و revision مربوط، همراه با evidence.
- `aml_exposure_runs`: شناسهٔ تازهٔ هر تلاش، source fence، تنظیمات و وابستگی برچسب‌ها.
- `aml_exposure_paths`: مسیر، legهای واقعی، tx/outpoint و بازهٔ صحیح ساتوشی.
- `aml_address_exposures`: قوی‌ترین summary هر seed/direction با شمارش مسیرها.
- `aml_analysis_publications`: seal مشترک موجود، بعد از کنترل شمارش و source fence.

مقدار `amount_raw` در ردیف مسیر **حد پایین** است؛ بازهٔ کامل در `allocation_json`
و شواهد خواندنی در `evidence_json` قرار دارد. مبلغ در محاسبه integer و در مرز JSON
رشته است؛ float فقط برای نسبت و امتیاز استفاده می‌شود.

فقط run کامل و sealed می‌تواند current باشد. تغییر زنجیره، block/revision، برچسب،
منبع، review یا شروع/پایان اعتبار وابسته، نتیجهٔ قدیمی را از مصرف جاری خارج
می‌کند. آدرس‌هایی که برچسبی نداشتند هم dependency هستند؛ تأیید seed یا service
جدید در آن‌ها نیز نیاز به محاسبهٔ دوباره دارد. ایجاد تحقیق، reopen و Export در
مرکز علاوه بر source/label، snapshot همین run را بررسی می‌کنند؛ شواهد و امتیاز
قدیمی تاریخی می‌مانند و ریسک جاری UNKNOWN می‌شود.

`is_active` در snapshot seed، وضعیت هنگام capture است. جدول seed یا view عمومی
`*_visible` به‌تنهایی شاهد تازگی intelligence نیست؛ مصرف‌کنندهٔ بیت‌کوین باید
`exposure::inspect` و کنترل attribution آن را به کار ببرد. workerهای آینده هم
باید همین مرز را حفظ کنند.

تلاش truncated یا فاقد seal امتیاز نمی‌سازد. خطای رسیدگی‌شده parent با وضعیت
failed ثبت می‌کند؛ قطع فرایند پیش از اولین درج ممکن است هیچ ردیفی ایجاد نکند.
قطع وسط درج، run را بدون seal و فقط برای ممیزی باقی می‌گذارد. ID تلاش دوباره
استفاده نمی‌شود؛ درخواست تازه run تازه می‌سازد. resume و worker پیوسته جزو این
مرحله نیستند. هیچ volume یا evidence قدیمی حذف نمی‌شود.

## فرمان‌ها و API

```bash
bitcoin_aml exposure --help
bitcoin_aml exposure BITCOIN_MAINNET_ADDRESS
bitcoin_aml exposure inspect BITCOIN_MAINNET_ADDRESS btc-exp-HEX_RUN_ID
```

CLI فقط ClickHouse را مصرف می‌کند و RPC لازم ندارد. روی VM بیت‌کوین از ریشهٔ مخزن:

```bash
docker compose --project-directory dockerizd_bitcoin --project-name aml-bitcoin \
  --env-file dockerizd_bitcoin/.env -f dockerizd_bitcoin/docker-compose.yml \
  --profile tools run --rm --no-deps bitcoin-exposure \
  bitcoin_aml exposure BITCOIN_MAINNET_ADDRESS
```

project باید همان استقرار موجود باشد. این ابزار و API محاسبه، مجوز نوشتن جدول‌های
derived همان database را نیاز دارند. endpointها service key و no-store مسیرهای
موجود را دارند:

```text
GET /api/bitcoin/wallet/{address}/exposure?max_hops=3&max_transactions=100&max_paths=50
GET /api/bitcoin/wallet/{address}/exposure/{run_id}
GET /api/bitcoin/wallet/{address}/exposure/{run_id}/snapshot
```

فرمان اول و تحقیق آدرس، محاسبه و ذخیره انجام می‌دهند؛ دو endpoint دارای run_id
فقط آن run را می‌خوانند و بررسی می‌کنند. پاسخ تحقیق شامل `exposure_paths`،
`exposure_coverage` و `exposure_snapshot` است. پنل جهت، seed، hopها، tx/outpoint،
بازهٔ BTC و وضعیت context/current/historical را نشان می‌دهد.

پیش‌فرض‌ها ۳ hop، ۱۰۰ تراکنش و ۵۰ مسیرند؛ سقف درخواست ۵ hop، ۲۰۰ تراکنش و ۱۰۰
مسیر است. سقف‌های داخلی: ۶۴ آدرسِ dependency، ۴۰۰۰ leg، ۲۰۰۰ leg در هر تراکنش،
۱۰۰۰ expansion/queue و ۴ MiB مسیر. رسیدن به سقف، partial/truncated است و امتیاز
exposure نمی‌سازد. قبل از دریافت اولین بلاک، وضعیت waiting_for_blocks برمی‌گردد.
نبود مسیر، مدرک نبود exposure نیست؛ کل زنجیره جست‌وجو نمی‌شود و پوشش window
دریافت‌شده و محدودیت‌ها همیشه باید خوانده شوند. ظرفیت production هنوز benchmark نشده است.

[اعتبارسنجی](EXPOSURE_VALIDATION.md)، [برچسب و review](INTELLIGENCE.md)،
[مراحل باقی‌مانده](../TODO_BITCOIN.md).
