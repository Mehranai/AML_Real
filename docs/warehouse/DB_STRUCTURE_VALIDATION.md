# آزمون اصلاح ساختار DB بیت‌کوین — ۲۰۲۶-۱۰-۲۳

این بازبینی، عبور زودهنگام به ING-04 را متوقف و پوشه‌بندی و SQL نصب بیت‌کوین
را با الگوی ماژولی adapterهای پروژه هماهنگ کرد. نتیجه و مراحل باز در
[ممیزی DB](DB_STRUCTURE_AUDIT_FA.md) آمده‌اند؛ یکسان‌سازی نهایی DB-01 و DB-03
تا DB-09 هنوز تکمیل نشده است.

## نتیجهٔ اجرا

| بررسی | نتیجه |
|---|---|
| بیت‌کوین | ۵۳ آزمون Rust پاس شدند: ۱۰ واحد، ۲ pipeline، ۲۳ resilience، ۱۶ probe و ۲ mapping؛ شامل ۱۴ آزمون با ClickHouse واقعی موقت |
| بستهٔ مشترک | ۸ آزمون پاس شدند: ۶ واحد و ۲ integration؛ شکل چهار database، حفظ evidence قدیمی، نصب تکراری و نیمه‌تمام، drift و visibility بررسی شدند |
| SQL فعال | init بیت‌کوین از SQL تولیدشده اجرا شد؛ خروجی هر statement با DDL قرارداد چهار شبکه یکسان بود؛ تغییر نوع و شناسهٔ نامعتبر رد شدند |
| ابزار تولید | `export_sql --check` روی فایل migration فعال پاس شد |
| binary واقعی schema | `bitcoin_init_schema --check` و init مجدد روی fixture موجود پاس شدند؛ schema_version=1، ۲۹ جدول، ۲۰ view و همان checksum گزارش شد |
| CLI و قفل | binary مستقل و alias قدیمی بدون تنظیمات DB همان SQL را چاپ کردند؛ نبود مسیر قفل یا مالک موجود، هر دو مسیر mutating را پیش از اتصال رد کرد |
| Rust Clippy | بیت‌کوین و بستهٔ مشترک با `--all-targets -- -D warnings` پاس شدند |
| Python | هر ۴۴ آزمون launcher/provisioner/inventory پاس شدند |
| Bash | مجموعهٔ قرارداد CLI پاس شد؛ check-schema بیت‌کوین binary مستقل را فراخواند؛ Docker/HTTP/Cargo در این مجموعه mock بودند |
| Compose | فرمان schema برابر `bitcoin_init_schema`، قفل مشترک روی `/var/lock/bitcoin` و database مستقل `bitcoin_aml` تأیید شد |
| نسخهٔ قبلی SQL | فایل قدیمی فقط جابه‌جا شد؛ SHA نرمال‌شده با manifest ممیزی قبلی تطبیق داشت |
| محدودهٔ تغییر | ۱۱ فایل بازگردانده‌شدهٔ Ethereum همچنان با نسخهٔ مبنا تطبیق دارند؛ writer/readerهای سه شبکهٔ دیگر در این بازبینی تغییر نکردند |

در اجرای اولیه پس از جابه‌جایی ماژول، نام تست فرزند قفل هنوز مسیر قدیمی داشت؛
مسیر آزمون اصلاح شد و مجموعهٔ کامل پس از آن پاس شد. بررسی برابری چهار
database مربوط به namespace مشترک `aml_*` است؛ یکسان‌بودن همهٔ اشیای قدیمی
چهار شبکه را ادعا نمی‌کند.

## هویت schema

فایل `shared/warehouse/schema/0001_warehouse.json` تغییر نکرد:

- SHA256 فایل: `0ae654956ac2673cd67be2223b97e545612875007ffc1c57b2dec838b8d3c483`.
- checksum قرارداد: `147ccdadef5b7c195af6dff3cc1d576fbc882a9f6912d99862e3d6919c1f6224`.

SQL داخل بیت‌کوین از این قرارداد تولید شده و مستقل قابل تغییر نیست. هیچ
migration اجراشده بازنویسی نشده، جدول یا volume عملیاتی حذف نشده و migration
روی دیتابیس مستقر انجام نشده است.

## بازتولید

```bash
cd dockerizd_bitcoin
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
# Fresh disposable ClickHouse only, bitcoin_test / bitcoin-test-only:
BITCOIN_TEST_CLICKHOUSE_URL=http://127.0.0.1:8123 \
  cargo test --locked --all-targets -- --include-ignored --test-threads=2

cd ../shared/warehouse
# Separate fixture user: warehouse_test / warehouse-test-only:
WAREHOUSE_TEST_URL=http://127.0.0.1:8123 \
  cargo test --locked --all-targets -- --include-ignored --test-threads=2
cargo clippy --locked --all-targets -- -D warnings
cargo run --locked --example export_sql -- --check \
  ../../dockerizd_bitcoin/sql/migrations/20261022_0001_shared_warehouse.sql
```

محیط اجرا Rust 1.92 و ClickHouse 23.8.16.16 در کانتینرهای مختص fixture بود.
فقط سورس، schema و cache وابستگی‌ها mount شدند؛ `.env` خصوصی وارد build/test
نشد. برای Compose فقط env ساختگی مصرف شد. این آزمون build image release،
rollout پنج VM، sync واقعی Core یا آزمون restore نیست؛ این موارد باز مانده‌اند.
