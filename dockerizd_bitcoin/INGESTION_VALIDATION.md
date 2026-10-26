# اعتبارسنجی گام چهارم بیت‌کوین — ۲۰۲۶-۱۰-۲۳

دامنه: **ING-02**، دریافت تاریخی و follow، کنترل منابع، RPC جایگزین و مالکیت
writer روی VM مستقل بیت‌کوین. قرارداد schema مشترک نسخهٔ ۱ و checksum آن،
مکان دیتابیس‌ها و runtime سه شبکهٔ دیگر تغییر نکرده‌اند.

## تغییر قابل مشاهده

بلاک‌ها می‌توانند هم‌زمان دریافت شوند، ولی parent/hash، resolve، صحت شواهد و
انتشار به ترتیب ارتفاع کنترل می‌شوند. خواندن prevoutهای محلی دسته‌ای است؛
lookup تاریخی txidهای یکتا از RPC به‌صورت batch انجام می‌شود. پنجرهٔ بلاک،
concurrency، اندازهٔ پاسخ، تعداد prevout و حجم شواهد سقف دارند. فقط خروجی‌های
لازم از تراکنش تاریخی نگه‌داری می‌شوند.

RPC خطاهای موقت را با backoff محدود تکرار می‌کند و cloneها rate gate مشترک
دارند. batch به شناسهٔ پاسخ‌ها متکی است و پاسخ ناقص/اضافی/تکراری یا جابه‌جایی
نامعتبر را نمی‌پذیرد. ترتیب معتبرِ متفاوتِ پاسخ‌ها پشتیبانی می‌شود.

هر endpoint پیش از مصرف mainnet/genesis را تأیید می‌کند. fallback عقب‌تر از
checkpoint یا دارای hash متفاوت رد می‌شود و نمی‌تواند rollback آغاز کند.
فقط primary مسیر reorg محدود را اجرا می‌کند. پس از خطای منبع، وضعیت منتشرشده
دوباره خوانده می‌شود تا failover پس از بخشی از یک پنجره، داده را تکرار نکند.
خطای نوشتن ClickHouse به retry کورکورانهٔ publication تبدیل نمی‌شود.

فرمان‌های تغییردهنده از قفل کرنل روی volume مشترک همان VM استفاده می‌کنند.
Compose نام ثابتی برای volume قفل دارد و image پوشه را برای UID 10001 آماده
می‌کند. فرمان native بدون `BITCOIN_WRITER_LOCK_DIR` صریح اجرا نمی‌شود؛ تمام
writerها باید واقعاً همان filesystem را ببینند. قفل بین میزبان‌های مستقل
توزیع نشده است؛ معماری هدف برای هر شبکه یک VM دارد.

## نتیجهٔ آزمون‌ها

در مجموع **۴۴ آزمون Rust** پاس شدند: ۹ آزمون واحد، ۱۵ آزمون resilience،
۱۶ آزمون probe، ۲ نگاشت و ۲ pipeline قبلی. از این مجموعه ۸ آزمون به ClickHouse
واقعی موقت نیاز دارند. آزمون‌های عادی و Clippy پس از افزودن guard نهایی CLI
دوباره اجرا شدند؛ کد کتابخانهٔ آزمون‌های ClickHouse در این تغییر نهایی ثابت بود.

| بررسی | نتیجه |
|---|---|
| دریافت موازی | سقف concurrency رعایت شد؛ تکمیل خارج از ترتیب به ثبت خارج از ترتیب تبدیل نشد؛ پنجرهٔ ناپیوسته رد شد |
| RPC | batch وارونهٔ معتبر، id ناشناس/تکراری، پاسخ ناقص، 429/503، warmup، auth و خطای دائمی، سقف تلاش و پاسخ، rate gate و حالت بدون batch پاس شدند |
| resolve واقعی | خروجی canonical قبلی از ClickHouse خوانده شد؛ سه outpoint خارجی با دو txid یکتا در batch حل شدند؛ جمع دقیق ۹۰۰٬۰۰۰٬۰۰۰ ساتوشی حفظ شد |
| خطا و resume | prevout مفقود بلاک را منتشر نکرد؛ prefix سالم حفظ و پس از رفع خطا بدون تکرار ادامه داده شد |
| failover | شبکهٔ اشتباه، منبع کوتاه‌تر و fork ناسازگار رد شدند؛ منبع معتبر از checkpoint ادامه داد؛ failover وسط پنجره publication تکراری نساخت |
| follow | قطع RPC worker را خاموش نکرد؛ پس از بازیابی منبع، بدون restart به دریافت ادامه داد |
| حدود منابع | سقف تعداد prevout و بودجهٔ بایت شواهد پیش از publication خطا دادند |
| writer | دو writer هم‌زمان رد شدند؛ abort/restart ادامه داد؛ رقابت فرایند مستقل و آزادشدن قفل بعد از kill تأیید شد |
| CLI واقعی | `schema`، `warehouse-backfill` و `once` با مالک موجود یا نبود تنظیم صریح قفل، پیش از اتصال دیتابیس رد شدند |
| Docker واقعی | دو کانتینر غیر root با volume آزمایشی مشترک: مالک قفل گرفت، رقیب با `owned` و خروج ناموفق رد شد و پس از kill مالک، کانتینر تازه قفل را گرفت |
| Compose | schema و ingestion یک volume ثابت برای قفل دارند؛ volume دیتابیس قبلی حفظ شده و probe وابستگی به قفل/دیتابیس ندارد |
| Clippy | `cargo clippy --locked --offline --all-targets -- -D warnings` روی سورس نهایی پاس شد |
| Python | هر ۴۴ آزمون VM/launcher/inventory پاس شدند |

تست کانتینری قفل از fixture کوچک با همان API استاندارد Rust، UID و ساخت پوشه
استفاده کرد؛ این آزمون build کامل image release نیست. محیط Rust نسخهٔ 1.92 و
ClickHouse نسخهٔ 23.8 بود. تنظیمات و رمزهای fixture ساختگی بودند؛ فایل خصوصی
محیطی به build/test mount نشد و دیتابیس عملیاتی استفاده نشد.

## بازتولید

```bash
cd dockerizd_bitcoin
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
# Only on a fresh disposable ClickHouse, bitcoin_test / bitcoin-test-only:
BITCOIN_TEST_CLICKHOUSE_URL=http://127.0.0.1:8123 \
  cargo test --locked --all-targets -- --include-ignored
```

آزمون‌های دیتابیس از نام‌های ثابت fixture استفاده می‌کنند و به دیتابیس خالی
نیاز دارند. آن‌ها را روی سرور عملیاتی اجرا نکنید.

این شواهد شامل sync واقعی mainnet، benchmark ظرفیت، build کامل release،
استقرار پنج VM یا کنترل writerهای نسخهٔ قدیمی/میزبان مستقل نیستند.
ثبت failure، dead-letter و repair انتخابی پس از این گزارش در **ING-03** تکمیل
شدند؛ [گزارش جدید](FAILURE_VALIDATION.md). replay مستقل در **ING-04** و
freshness/heartbeat در **ING-05** باز هستند. یکسان‌سازی نهایی ساختارها و
امکانات AML هنوز تکمیل نشده است.

تنظیمات و محدودهٔ مالکیت در [README](README.md) و کارهای باقی‌مانده در
[TODO](../TODO_BITCOIN.md) آمده‌اند.
