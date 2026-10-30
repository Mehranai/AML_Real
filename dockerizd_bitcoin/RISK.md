# ریسک بیت‌کوین؛ محاسبه، ذخیره و بازیابی

مرحلهٔ AML-04، سیاست ریسک موجود را به جدول‌های مشترک نسخهٔ ۵ وصل می‌کند.
ClickHouse، database و volume همچنان روی VM مستقل بیت‌کوین هستند؛ Neo4j تحقیق‌ها
در VM اصلی باقی می‌ماند. جدول یا migration جدیدی اضافه نشده است.

## مسیر اجرا

```text
دادهٔ canonical و برچسب معتبر و exposure
    → سیاست مشترک Rust
    → policy/rules و assessment/signals در ClickHouse بیت‌کوین
    → seal انتشار و بازخوانی اعتبار نتیجه
    → پاسخ investigation همراه شناسه و snapshot ارزیابی
    → محاسبهٔ دوباره و کنترل freshness در مرکز
    → snapshot تحقیق و گراف در Neo4j
```

پیاده‌سازی قبلی سیاست مرکزی به [کتابخانهٔ مشترک](../shared/risk_policy/README.md)
منتقل شده است. مرکز و بیت‌کوین همان `assess` را مصرف می‌کنند؛ وزن‌ها و آستانه‌ها
تغییر نکرده‌اند. نسخهٔ فعلی `aml_evidence_v2_verified_inputs` است. policy export
توصیف کد اجرایی است؛ ویرایش یک weight در دیتابیس، موتور را تنظیم نمی‌کند و باعث
ردشدن سازگاری همان نسخه می‌شود. سیاست جدید باید نسخهٔ تازه و استقرار هماهنگ داشته باشد.

## چه چیزی ذخیره می‌شود؟

| جدول موجود | محتوای فعال |
|---|---|
| `aml_risk_policies` | نسخه، نام، فعال‌بودن، آستانه‌ها و تعریف کامل سیاست |
| `aml_risk_policy_rules` | شش قاعدهٔ موجود، سقف سهم و تنظیمات/توضیح هر قاعده |
| `aml_wallet_risk_assessments` | subject، نتیجه، policy، coverage/limitations، source revision، run وابسته و شواهد ورودی |
| `aml_wallet_risk_signals` | signal، contribution، summary، evidence و ارجاع‌ها |
| `aml_analysis_publications` | seal موجود برای خانوادهٔ risk پس از کنترل شمارش و snapshot |

خانوادهٔ risk، سیاست عمومی موجود را ثبت می‌کند؛ وجود قاعدهٔ mixer یا forwarding
به معنی تولید detector جدید برای بیت‌کوین نیست. legهای UTXO به الگوی account-based
تبدیل نمی‌شوند. فقط شواهدی که adapter واقعاً ساخته است ارزیابی می‌شوند.

`evidence_json` ارزیابی شامل ورودی‌های دقیق مورد استفادهٔ سیاست، `risk_engine`
و `risk_snapshot` است. graph نمایش و holdings به‌عنوان ورودی اضافی ریسک کپی نمی‌شوند.
شواهد signal جداگانه نیز ساختاریافته ذخیره می‌شوند و reader آن‌ها را با نتیجهٔ
محاسبه‌شده تطبیق می‌دهد. خلاصهٔ parent، تعداد و سهم signal، روش و evidence refs
کنترل می‌شوند. مبلغ و revision صحیح با float همسان‌سازی نمی‌شوند؛ تنها نمایش
معادل عددهای اعشاری ذخیره‌شده، مانند `55` و `55.0`، پذیرفته می‌شود.

هر تلاش شناسهٔ تازهٔ `btc-risk-HEX` می‌گیرد. تلاش‌های هم‌زمان ID مشترک ندارند؛
ثبت دوبارهٔ همان شواهد، assessment تازه می‌سازد. نیمه‌کاره‌ماندن درج، رکورد بدون
seal را فقط برای ممیزی باقی می‌گذارد؛ resume یا استفادهٔ دوباره از ID وجود ندارد.

## UNKNOWN و محدودیت پوشش

بدون signal معتبر، `risk_score=null` و `risk_level=UNKNOWN` ثبت می‌شود؛ صفر یا
برچسب سالم جایگزین نبود شواهد نیست. یک ارزیابی UNKNOWN می‌تواند کامل و sealed
باشد: کامل‌بودن اجرای سیاست روی شواهد موجود، با کامل‌بودن تاریخچه یا نبود ریسک فرق دارد.

قبل از اولین بلاک منتشرشده، سیاست می‌تواند شواهد مستقیم برچسب را گزارش کند، اما
assessment زنجیره‌ای sealed ساخته نمی‌شود: `risk_snapshot=null` و persistence
دارای وضعیت `waiting_for_blocks` است. ارتفاع ساختگی یا رکورد کاملِ ظاهری تولید نمی‌شود.

بازهٔ ناقص و truncation در ورودی و limitations حفظ می‌شوند. exposure ناقص، دارای
حد پایین صفر، service-mediated یا فاقد seed معتبر امتیاز نمی‌سازد. یک برچسب مستقیم
معتبر همچنان می‌تواند مستقل از مسیر exposure سیگنال بدهد. سقف ورودی ذخیرهٔ ریسک
۸ MiB است؛ محدودیت‌های قبلی exposure و API نیز برقرارند.

## تازگی و نتیجهٔ تاریخی

`risk_snapshot` هویت شبکه/آدرس/assessment، شناسهٔ snapshot زنجیره، نسخه و hash
سیاست، hash ورودی‌ها و hash نتیجه را به هم متصل می‌کند. hash سیاست شامل digest
نرمال‌شدهٔ source کتابخانه نیز هست؛ تغییر فایل آن، حتی تغییر بدون اثر بر وزن،
با نسخهٔ موجود در دیتابیس یکسان فرض نمی‌شود و به نسخهٔ تازه نیاز دارد.

`risk::inspect` seal، شمارش/محتوای ردیف‌ها، سیاست ذخیره‌شده، برچسب همان آدرس و
run exposure وابسته را کنترل می‌کند و نتیجه را با سیاست اجرایی دوباره می‌سنجد.
تغییر checkpoint، source، review، اعتبار claim، seed تازه یا سیاست، نتیجهٔ قبلی
را از مصرف جاری خارج می‌کند. view عمومی `*_visible` به‌تنهایی کنترل تمام این
وابستگی‌های intelligence را انجام نمی‌دهد؛ مصرف‌کننده باید این reader را به کار ببرد.

پاسخ inspect شواهد و `historical_risk_engine` را حفظ می‌کند؛ اگر current نباشد،
`risk_engine` جاری UNKNOWN است. مرکز هنگام create/reopen/Export علاوه بر بررسی‌های
زنجیره و intelligence/exposure، endpoint snapshot همین assessment را هم می‌خواند.
پاسخ نامعتبر یا unavailable، ریسک جاری تأییدشده ایجاد نمی‌کند. Export شواهد قدیمی
را تازه نمی‌کند و نسخهٔ تاریخی آن‌ها را حفظ می‌کند.

## CLI و API

```bash
bitcoin_aml risk --help
bitcoin_aml risk BITCOIN_MAINNET_ADDRESS
bitcoin_aml risk inspect BITCOIN_MAINNET_ADDRESS btc-risk-HEX_ID
bitcoin_aml risk policy
```

فرمان آدرس، تحقیق و exposure محدود را محاسبه و assessment را ثبت می‌کند. inspect
همان ID را می‌خواند و policy، rules و signalهای ذخیره‌شده را هم برمی‌گرداند.
این فرمان‌ها RPC لازم ندارند؛ فقط ClickHouse همان شبکه را مصرف می‌کنند.

از ریشهٔ مخزن، روی VM بیت‌کوین با نام project استقرار موجود:

```bash
docker compose --project-directory dockerizd_bitcoin --project-name aml-bitcoin \
  --env-file dockerizd_bitcoin/.env -f dockerizd_bitcoin/docker-compose.yml \
  --profile tools run --rm --no-deps bitcoin-risk \
  bitcoin_aml risk BITCOIN_MAINNET_ADDRESS
```

APIهای محافظت‌شده با service key و پاسخ `no-store`:

```text
GET /api/bitcoin/risk/policy
GET /api/bitcoin/wallet/{address}/risk
GET /api/bitcoin/wallet/{address}/risk/{assessment_id}
GET /api/bitcoin/wallet/{address}/risk/{assessment_id}/snapshot
```

policy تا قبل از نخستین ثبت، `persisted=false` و تعریف compiled را نشان می‌دهد.
پس از ثبت، ردیف‌های واقعی policy و rules نیز در پاسخ موجودند. endpoint اول آدرس
و investigation، محاسبه و نوشتن انجام می‌دهند؛ خواندن دارای ID فقط همان assessment
را بررسی می‌کند. endpointی برای قبول امتیاز یا شواهد دلخواه caller وجود ندارد.

پاسخ investigation شامل `risk_engine`، `risk_snapshot` و `risk_persistence` است.
مرکز قبل از ذخیرهٔ تحقیق، برابر بودن نتیجه با محاسبهٔ خودش و اتصال snapshot به
شواهد را بررسی می‌کند. پنل مرجع assessment و وضعیت جاری/تاریخی را کنار دلایل ریسک نشان می‌دهد.

## استقرار و مرز تحویل

imageهای بیت‌کوین و Analytical Node باید هر دو با کتابخانهٔ مشترک بازسازی شوند.
Composeها named context مربوط به `shared/risk_policy` را دارند؛ سرویس runtime
جدید اضافه نشده است. مرکز برای پاسخ‌های قدیمی فاقد risk snapshot سازگاری قبلی
را حفظ می‌کند؛ برای تضمین مسیر persisted جدید، image جدید هر دو سمت لازم است.

این مرحله clustering، semantic detector، worker پیوسته، replay، benchmark یا
مهاجرت legacy سه شبکهٔ دیگر را تکمیل نمی‌کند. پذیرش نهایی با mainnet، Neo4j زنده،
VMهای مقصد و بار production همچنان جداست. [TODO](../TODO_BITCOIN.md)،
[گزارش پذیرش](RISK_VALIDATION.md).
