# اعتبارسنجی ING-03 بیت‌کوین — ۲۰۲۶-۱۰-۲۳

ثبت failure، retry/dead-letter، اسکن محدود gap و repair انتخابی به adapter
بیت‌کوین اضافه شدند. دادهٔ عملیاتی از جدول و view مشترک موجود
`aml_ingestion_failures` و `aml_ingestion_failures_current` استفاده می‌کند؛
ClickHouse و database بیت‌کوین مستقل روی VM همان شبکه می‌مانند.

فایل schema نسخهٔ ۱ تغییر نکرد؛ SHA256 فایل:
`0ae654956ac2673cd67be2223b97e545612875007ffc1c57b2dec838b8d3c483`.
writer/readerهای Ethereum، TRON و BSC در این گام تغییر نکردند.

## رفتار و شواهد

| بررسی | نتیجه |
|---|---|
| ماندگاری تلاش | دو اجرای مستقل با prevout مفقود شمارنده را از ۱ به ۲ رساندند؛ وضعیت retry سپس dead_letter شد و checkpoint روی prefix سالم ماند |
| توقف و repair | اجرای عادی بعد از dead-letter بدون تماس RPC متوقف شد؛ repair ناموفق و سپس موفق شمارنده را به ۳ و ۴ رساندند؛ چهار بلاک canonical بدون publication تکراری باقی ماندند |
| fallback | خطای primary و موفقیت منبع معتبر، همان failure را با دو تلاش resolved کردند |
| پنجره و انتخاب | batch از failure بلاک میانی عبور نکرد؛ repair ارتفاع جلوتر بدون بلاک قبلی blocked شد؛ انتخاب تک‌بلاک، بلاک انتخاب‌نشده را ingest نکرد |
| crash و ساعت | تلاش repair قبل از RPC رزرو شد؛ recovery بدون افزایش دوبارهٔ شمارنده اجرا شد؛ revision بزرگ‌تر از ساعت فعلی نیز به‌صورت یکنواخت افزایش یافت |
| publication نامطمئن | failure ثبت‌شده برای بلاک canonical پس از restart resolved شد؛ checkpoint و facts دوباره منتشر نشدند |
| gap | حذف بلاک و تراکنش فقط در fixture موقت، دو gap ساخت؛ اسکن آن‌ها را ثبت کرد و repair تاریخی بدون تغییر checkpoint با دلیل روشن متوقف شد |
| API | service key، فیلتر وضعیت و ارتفاع، pagination و حد نتیجه آزمون شدند؛ fixture شبکهٔ دیگر وارد list یا summary نشد؛ گزینهٔ نامعتبر 400 و نبود کلید 401 داد |
| CLI واقعی | help بدون RPC/دیتابیس اجرا شد؛ repair و scan-gaps در نبود مسیر قفل صریح یا مالک موجود پیش از اتصال رد شدند؛ انتخاب تکراری، شناسه و بازهٔ نامعتبر رد شدند |
| پیام‌ها | کد و متن ثابت مرحله ذخیره شد؛ متن خصوصی fixture RPC در failure نیامد |
| Compose و VM | ابزارها در profile اختیاری tools، بدون port و dependency؛ repair همان volume ثابت writer را دارد؛ failures فقط خواندنی است؛ تست‌های Bash از mock استفاده کردند و startup ناخواستهٔ دیتابیس رخ نداد |

## اجرا

مجموع **۵۱ آزمون Rust**: ۹ واحد، ۲ pipeline، ۲۲ resilience، ۱۶ probe و
۲ mapping. از این مجموعه **۱۴ آزمون با ClickHouse واقعی موقت** اجرا شدند.
دو ایراد سبک Clippy پس از اجرای کامل رفع شدند؛ بررسی Clippy و آزمون‌های عادی
روی سورس نهایی تکرار شدند. تغییر نهایی فقط let-chain معادل و
`is_multiple_of(2)` بود. **۴۴ آزمون Python** و مجموعهٔ قرارداد CLI در Bash
نیز پاس شدند.

محیط آزمون: Rust 1.92، ClickHouse 23.8.16.16، Docker، RPC کنترل‌شده و
credential ساختگی. فقط سورس، schema و cache وابستگی‌ها mount شدند؛ فایل
خصوصی محیطی و دیتابیس عملیاتی وارد آزمون نشدند. کانتینرهای مختص این گام پس
از اعتبارسنجی حذف می‌شوند.

```bash
cd dockerizd_bitcoin
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
# Fresh disposable ClickHouse only, user bitcoin_test / password bitcoin-test-only:
BITCOIN_TEST_CLICKHOUSE_URL=http://127.0.0.1:8123 \
  cargo test --locked --all-targets -- --include-ignored --test-threads=2
# From repository root:
python -m unittest discover -s scripts/tests -p 'test_*.py'
bash scripts/tests/linux-cli.sh
```

fixtureهای دیتابیس نام ثابت دارند؛ اجرای مجدد کامل باید روی ClickHouse خالی
و موقت باشد. اسکن gap در این مرحله وجود بلاک، پیوستگی parent و تعداد تراکنش
را بررسی می‌کند؛ ممیزی کامل تمام vin/vout نیست. repair بلاک بعدی را بازیابی
می‌کند یا مورد از قبل canonical را تطبیق می‌دهد. **ترمیم شکاف داخل تاریخچهٔ
منتشرشده، replay مستقل و invalidation تحلیل‌های وابسته در ING-04 باز هستند.**

اگر ClickHouse قطع باشد ثبت failure نیز ممکن نیست؛ خطای نوشتن writer را متوقف
می‌کند. نتیجهٔ این گام، sync واقعی Core، benchmark، image release یا استقرار
پنج VM را تأیید نمی‌کند؛ این موارد در TEST-05 باقی‌اند.

فرمان‌ها و سیاست تلاش: [README](README.md). مراحل بعد: [TODO](../TODO_BITCOIN.md).
