# نتیجه اعتبارسنجی بیت‌کوین

آخرین نتیجه، گام چهارم ING-02 و ۴۴ آزمون Rust، در
[گزارش دریافت مورخ ۲۰۲۶-۱۰-۲۳](INGESTION_VALIDATION.md) ثبت شده است.

گام سوم، ابزار بررسی آمادگی RPC و نتیجهٔ ۲۷ آزمون نهایی بیت‌کوین، در
[گزارش probe مورخ ۲۰۲۶-۱۰-۲۳](PROBE_VALIDATION.md) ثبت شده است. این بخش تازه
است؛ جدول زیر نتیجه‌های تاریخی پیاده‌سازی اولیه را حفظ می‌کند.

> این گزارش فقط آزمون‌های پیاده‌سازی پایه قبلی را ثبت می‌کند. همسانی schema چهار شبکه، تکمیل intelligence/clustering/exposure/risk و برابری امکانات تأیید نشده‌اند. معیار پذیرش جدید در [TODO به‌روزشده](../TODO_BITCOIN.md) آمده و هنوز باز است.

آزمون‌های مرحله اول قرارداد مشترک، از جمله schema چهار fixture و تکرار pipeline
بیت‌کوین، در [گزارش warehouse](../docs/warehouse/VALIDATION.md) ثبت شده‌اند.
انتقال writer/reader دادهٔ پایهٔ بیت‌کوین و آزمون backfill گام دوم نیز در همان
گزارش ثبت شده است. جدول زیر نتایج تاریخی پیاده‌سازی اولیه را نگه می‌دارد؛
هیچ‌کدام به معنی تکمیل امکانات AML یا اجرای migration روی دادهٔ مستقر نیستند.

بررسی‌های انجام‌شده برای این تغییر:

| بررسی | نتیجه |
|---|---|
| Rust بیت‌کوین، `cargo test --locked` | ۷ تست واحد پاس شد |
| `cargo clippy --locked --all-targets -- -D warnings` | بدون warning پاس شد |
| Pipeline با ClickHouse واقعی `23.8.16.16` و RPC کنترل‌شده | پاس شد: init تکرارپذیر، دیتابیس خالی، ingest، resume، UTXO و coinbase، مسیر، شواهد منتشرنشده، عقب‌رفتن ساعت، reorg و جلوگیری از انتشار ورودی حل‌نشده |
| Analytical Node، `cargo test --locked` | هر ۱۸ تست، شامل گره تراکنش بیت‌کوین و پرهیز از امتیاز ریسک بدون شواهد، پاس شد |
| Python launcher و VM provisioner | هر ۴۰ تست، شامل افزودن VM پنجم به state قبلی و پیش‌نیاز RPC، پاس شد |
| JavaScript routing | هر ۶ تست، شامل Legacy/Taproot و حفظ case آدرس بیت‌کوین، پاس شد |
| Bash Linux CLI contracts | تمام گروه‌های آزمون با Docker/HTTP/Cargo کنترل‌شده پاس شد؛ نقش بیت‌کوین و اجرای هم‌زمان چهار شبکه هم بررسی شد |
| Nginx | syntax معتبر؛ health/holdings از API واقعی بیت‌کوین و ClickHouse تستی خوانده شد و investigation به upstream مرکزی کنترل‌شده رفت |
| مرورگر واقعی Edge به‌صورت headless | صفحه بیت‌کوین با fixture محلی اجرا شد؛ ساتوشی دقیق، گراف، موجودی با تاریخچه ناقص، snapshot و ریسک UNKNOWN پاس شد؛ تصویر صفحه هم بررسی شد |
| Compose و JavaScript syntax، `git diff --check` | پاس شد |

تست ClickHouse روی کانتینر مستقل و با داده ساختگی انجام شد؛ داده هیچ شبکه واقعی یا دیتابیس قبلی تغییر نکرد. برای اجرای دوباره، دستورهای بخش تست در [README](README.md) را ببینید. تست مرورگر بدون npm و با مرورگر نصب‌شده اجرا می‌شود:

```powershell
python gateway/tests/bitcoin-console.py --browser "C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"
```

سورس با Rust 1.92 کامپایل شد. image کامل production، ایجاد VM واقعی، اتصال به Bitcoin Core واقعی و sync تاریخچه mainnet در این محیط اجرا نشدند. تنظیم RPC واقعی هنوز لازم است. منبع intelligence ریسک و exposure بیت‌کوین در این نسخه ارائه نمی‌شود؛ امتیاز بدون شواهد معتبر UNKNOWN است. این محدودیت با آماده‌بودن API یا موفقیت تست‌ها برطرف نمی‌شود.
