# اجرای واقعی تحلیل‌ها و وضعیت تحویل

## فرمان روشن کردن کامل

ویندوز، از ریشه پروژه و با Docker Desktop روشن:

```cmd
python scripts/run_local.py up --build --with-ingestion
python scripts/run_local.py status
```

صفحه اصلی: http://127.0.0.1:8080

فقط `python scripts/run_local.py` عمداً حالت API-only است. برای دریافت بلاک و اجرای workerها باید `--with-ingestion` را بدهید. `--build` بعد از تغییر سورس لازم است؛ روشن کردن image قدیمی قابلیت جدید را اضافه نمی‌کند. توقف بدون حذف داده:

```cmd
python scripts/run_local.py stop
```

روی میزبان Linux با VMهای پروژه:

```bash
bash scripts/vm.sh tron up --build
bash scripts/vm.sh tron ps
bash scripts/vm.sh tron logs
```

برای استقرار بدون VM روی Linux:

```bash
bash scripts/aml.sh up --build --with-ingestion --with-bsc --with-bitcoin
```

## چه چیزی خودکار اجرا می‌شود؟

| شبکه | اجرای پس‌زمینه | هنگام جستجوی ولت |
| --- | --- | --- |
| TRON | ingest، metadata، worker clustering و exposure | گراف، fingerprint، holdings، شواهد و ادعاهای هویتی |
| Ethereum | ingest، metadata و worker موجود clustering/exposure | گراف و بررسی کیف پول |
| BSC | follow و metadata | گراف، exposure و خواندن برچسب‌های تأییدشده؛ کاندیداها endpoint جدا دارند |
| Bitcoin | ingest و analytics شامل clustering/exposure | تحقیق، activity، holdings، paths و دلایل risk همراه coverage |

ریسک مرکزی از شواهد بازگشتی همان بررسی محاسبه می‌شود؛ ML فعال نشده است. امتیاز، احتمال آماری یا حکم اثبات پول‌شویی نیست.

## worker ترون

فایل ورودی: `dockerizd_tron/app/src/bin/tron_analytics_worker.rs`

منطق اجرا: `dockerizd_tron/app/src/tasks/analytics.rs`

1. schema واقعی را اعتبارسنجی می‌کند؛ دیتابیس را حذف یا بازسازی نمی‌کند.
2. منبع داخلی کشف کاندیدا را فقط اگر وجود نداشته باشد با نوع HEURISTIC و اعتماد UNVERIFIED ثبت می‌کند. منبع غیرفعال‌شده توسط تحلیلگر را دوباره فعال نمی‌کند.
3. اگر checkpoint ingest وجود نداشته باشد، وضعیت WAITING_FOR_DATA می‌ماند. از صفر بودن داده نتیجه «ولت سالم است» نمی‌گیرد.
4. کشف کاندیدا را از ابتدای تاریخچه در بازه‌های ۱۰۰٬۰۰۰ بلاکی با هم‌پوشانی ۱۰٬۰۰۰ بلاک جلو می‌برد. فاصله اجرای پیش‌فرض ۶۰ ثانیه است؛ پس از رسیدن به ingest، بازه اخیر را دوباره بررسی می‌کند.
5. کاندیداها در `aml_intelligence_claims` با `claim_kind=cluster` و وضعیت pending ثبت می‌شوند؛ نتیجهٔ review در `aml_cluster_*` با seal مشترک منتشر می‌شود. هیچ کاندیدایی خودکار به صرافی شناخته‌شده یا مالکیت قطعی تبدیل نمی‌شود.
6. هر ساعت exposure مربوط به seedهای فعال اجرا می‌شود. مسیر و جمع نتیجه در `aml_exposure_paths` و `aml_address_exposures` و وضعیت اجرا در `aml_exposure_runs` است. بدون seed معتبر، exposure مفیدی تولید نمی‌شود.
7. پیشرفت کشف و زمان exposure در volume کوچک `tron_analytics_state` ذخیره می‌شود. restart از ادامه می‌رود. تغییر hash بلاک checkpoint کشف را از ابتدا آغاز می‌کند.
8. خطای query یا عبور از سقف منابع باعث شکست آشکار worker و restart توسط Docker می‌شود؛ heartbeat و وضعیت مرحله قابل مشاهده است.

این worker فقط یک replica دارد؛ آن را scale نکنید. تحلیل بازه‌ای و سقف تعداد کاندیداها، کشف جامع تمام الگوهای تاریخی را تضمین نمی‌کند. افزودن برچسب جدید برای یک صرافی قدیمی ممکن است به اجرای مجدد کشف روی تاریخچه قدیمی با ابزار `tron_discover_address_clusters` نیاز داشته باشد. بازه اخیر جایگزین بازتحلیل کامل نیست.

## دیدن وضعیت روی ویندوز

```cmd
docker compose -p app --env-file dockerizd_tron/app/.env -f dockerizd_tron/app/docker-compose.yml logs --tail 100 tron-analytics
docker compose -p app --env-file dockerizd_tron/app/.env -f dockerizd_tron/app/docker-compose.yml exec -T tron-analytics tron_analytics_worker --status
```

روی VM ترون از ریشه پروژه، نام project پیش‌فرض `aml-tron` است، نه `app`. روی همان VM از `bash scripts/vm.sh tron logs` استفاده کنید.

وضعیت‌های مهم: WAITING_FOR_DATA، CLUSTERING، EXPOSURE، IDLE و FAILED. IDLE یعنی چرخه فعلی تمام شده، نه اینکه کل شبکه یا تمامی تحلیل‌ها کامل است. سالم بودن کانتینر به تنهایی کیفیت تشخیص را ثابت نمی‌کند.

مشاهده نتایج در ClickHouse:

```sql
SELECT address, entity_type AS cluster_type, address_role, confidence, review_status,
       evidence_json
FROM aml_intelligence_claims_current
WHERE network_id='tron:mainnet' AND claim_kind='cluster' AND review_status='pending'
ORDER BY created_at_unix_ms DESC LIMIT 20;

SELECT seed_id, status, max_hops, path_count, completed_at_unix_ms
FROM aml_exposure_runs_visible WHERE network_id='tron:mainnet' LIMIT 20;
```

## اصلاحات ایمنی exposure

- seed غیرفعال در نتیجه بررسی ولت استفاده نمی‌شود.
- انتشار خودکار در صرافی شناخته‌شده متوقف می‌شود؛ خروجی تجمیع‌شده صرافی به پول یک کاربر نسبت داده نمی‌شود.
- مسیر چندمرحله‌ای همان دارایی را حفظ می‌کند و ترتیب `(block_number, transaction_index)` باید افزایشی باشد؛ ادامهٔ زنجیره‌سازی داخل همان تراکنش انجام نمی‌شود. relationship ID و txهای واقعی هر مسیر حفظ می‌شوند.
- این مسیر، شواهد ارتباط است؛ هنوز اثبات نمی‌کند همان واحدهای پول از همه واسطه‌ها عبور کرده‌اند. عبور از swap/bridge در این الگوریتم دنبال نمی‌شود.
- عبور از سقف یال/حالت‌های جستجو خطا می‌دهد؛ خروجی ناقص با وضعیت COMPLETE منتشر نمی‌شود.
- حین refresh، snapshot قدیمی آن seed دیگر به عنوان نتیجه کامل جاری مصرف نمی‌شود؛ خروجی فقط پس از کنترل snapshot منبع، شمارش و ثبت seal مشترک قابل مشاهده است؛ شکست یا قطع اجرا، خروجی ناقص معتبر ایجاد نمی‌کند.

## چه چیزی هنوز شرط تحویل است؟

راه‌اندازی خودکار یک نقص واقعی را رفع می‌کند، ولی برابری با Chainalysis ایجاد نمی‌کند. اتصال اثبات‌شده دو طرف بریج، پوشش همه decoderها، داده‌های هویتی معتبر، بازتحلیل پس از تغییر برچسب/تاریخچه، نمایش freshness در UI و آزمون بار و بازیابی روی داده واقعی هنوز نیاز به تکمیل و پذیرش جداگانه دارند.

import برچسب، تأیید انسانی، replay تخریبی و benchmark عمداً به صورت خودکار با بالا آمدن سیستم اجرا نمی‌شوند. این ابزارها وظیفه عملیاتی مشخص دارند؛ اجرا نکردن خودکارشان نقص نیست.

## تست قابل تکرار

تست `dockerizd_tron/app/tests/analytics_pipeline.rs` روی ClickHouse موقت با
حساب آزمایشی اجرا می‌شود و دیتابیس یکتا را فقط از قرارداد مشترک می‌سازد.
بلاک‌ها از RPC نمونه به ingestor اصلی داده می‌شوند؛ جدول بومی seed نمی‌شود.

```bash
WAREHOUSE_TEST_URL=http://127.0.0.1:18141 \
  cargo test --manifest-path dockerizd_tron/app/Cargo.toml --test analytics_pipeline \
  -- --ignored --nocapture --test-threads=1
```

آزمون، review و لغو منبع/برچسب، خوشهٔ pending و reviewed، ترتیب و شاهد مسیر،
مرز صرافی، retry metadata و وضعیت مشاهدهٔ solid، API دارای auth و تغییر
snapshot پس از replay را بررسی می‌کند. پذیرش RPC/VM واقعی جداست.

نتایج جاری و محدودیت‌های پذیرش در [TESTING](TESTING.md) ثبت می‌شوند.
