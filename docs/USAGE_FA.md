# اجرا و استفاده

ابتدا [VMها و تنظیمات](VM_SETUP_FA.md) را آماده کنید. این راهنما
رفتار نسخهٔ فعلی کد را شرح می‌دهد؛ تکمیل همسان‌سازی فعال TRON و پذیرش
روی VM واقعی همچنان در [TODO_LIST](../TODO_LIST.md) باز است.

## راه‌اندازی

از ریشهٔ مخزن، روی هر VM فقط role همان VM را اجرا کنید:

```bash
bash scripts/vm.sh tron up --build
# روی VMهای دیگر به‌ترتیب ethereum، bsc، bitcoin و سپس main را جایگزین کنید.
```

راه‌اندازی بدون `--api-only`، دریافت بلاک و workerهای موجود همان شبکه را
نیز روشن می‌کند. پیش از آن RPC، بازهٔ شروع، رمز ClickHouse و service key
باید تنظیم شده باشند. برای بررسی اولیهٔ schema/API بدون شروع ingestion:

```bash
bash scripts/vm.sh bitcoin up --build --api-only
bash scripts/vm.sh bitcoin check-schema
bash scripts/vm.sh bitcoin check
```

برای شروع workerها بعد از پذیرش تنظیمات، `up` را بدون `--api-only` اجرا
کنید. `resume` سرویس‌ها را بدون restart دیتابیس روشن می‌کند. نمونه:

```bash
bash scripts/vm.sh bitcoin up
bash scripts/vm.sh bitcoin check-runtime
bash scripts/vm.sh bitcoin ps
bash scripts/vm.sh bitcoin logs
```

`check-schema` وجود و صحت قرارداد نصب‌شده را کنترل می‌کند؛ به‌تنهایی
نشان‌دهندهٔ استفادهٔ تمام reader/writerها از آن یا کامل‌بودن تاریخچه نیست.
`check-runtime` وضعیت workerهای موجود را بررسی می‌کند، نه صحت همهٔ تحلیل‌ها.

## تحقیق

۱. روی VM اصلی، آدرس `http://10.20.0.10:8080` را باز کنید؛ IP را با IP واقعی
Main جایگزین کنید. اگر Basic Auth فعال کرده‌اید، با حساب ساخته‌شده وارد شوید.

۲. شبکه را انتخاب و آدرس همان شبکه را وارد کنید. آدرس معتبر در یک شبکه
به‌معنی هویت همان شخص در شبکهٔ دیگر نیست.

۳. graph، activity، holdings، برچسب‌ها، exposure، رویدادها و دلایل risk را
همراه با coverage و محدودیت‌ها بخوانید. نبود نتیجه یا `UNKNOWN` به معنای
بی‌خطر بودن کیف پول نیست. مقدار خام دارایی‌ها واحد صحیح و بدون گردکردن دارد.

۴. نتیجهٔ تحقیق ابتدا snapshot موقت در Neo4j مرکزی است. `Export` همان
snapshot را ماندگار می‌کند؛ تغییر داده یا برچسب، مدرک ذخیره‌شده را بی‌صدا
بازنویسی نمی‌کند. نتیجهٔ قدیمی ممکن است historical/unverified شود.

API مرکزی تحقیق:

```text
GET  /api/{network}/wallet/{address}/investigation
GET  /api/investigations
GET  /api/investigations/{id}
POST /api/investigations/{id}/export
```

احراز هویت درخواست کاربر در Gateway و service key بین سرویس‌ها کنترل
می‌شود. قرارداد chain API در [OpenAPI](../contracts/chain-api-v1.openapi.yaml)
است؛ parity کامل چهار شبکه هنوز باید پذیرفته شود.

## عملیات روزمره

```bash
bash scripts/vm.sh ethereum pause-ingestion
bash scripts/vm.sh ethereum db --query 'SELECT count() FROM aml_transactions_canonical'
bash scripts/vm.sh ethereum resume
```

در هر چهار شبکه، `aml_transactions_canonical` از مسیر فعال ingest پر می‌شود.
در ترون هم facts و checkpoint مشترک‌اند؛
رکوردهای عملیاتی metadata/intelligence/exposure ترون نیز در `aml_*` هستند؛
SQL بومی و mountها حذف شده‌اند و هر چهار شبکه schema job مشترک دارند؛
پذیرش مسیر داده با RPC نمونه و ClickHouse واقعی موفق شده است؛ پذیرش روی
RPC و VMهای واقعی در ادامهٔ برنامه است.

توقف برنامه‌ها با حفظ ClickHouse: `pause`. توقف stack بدون حذف volume:
`down`. برای لاگ‌ها و سلامت از `logs`، `ps` و `check` استفاده کنید. برای
تغییر بازه و تنظیمات worker، `up` را دوباره اجرا کنید؛ `resume` کانتینر را
با تنظیمات قبلی روشن می‌کند. از `down -v` برای توقف روزمره استفاده نکنید.

ابزارهای Bitcoin موجود: `probe`، `failures`، `scan-gaps` و `repair`؛ برای
repair ابتدا ingestion را متوقف کنید. این ابزارها در حال حاضر در
`vm.sh` برای سه شبکهٔ دیگر پیاده نشده‌اند؛ parity عملیات جزو TODO است.

در restart و poll اتریوم/ترون، هش checkpoint از RPC دوباره کنترل می‌شود؛
حتی وقتی بلاک جدیدی نرسیده است. تعارض دادهٔ finalized اتریوم یا solid ترون
دریافت را متوقف می‌کند و checkpoint قبلی حفظ می‌شود. ابتدا هویت، تازگی و
درستی RPC را بررسی کنید؛ پس از اصلاح منبع، همان بازه/اجرای follow را دوباره
اجرا کنید. replay مجوز جایگزینی خودکار دادهٔ نهایی‌شده نیست.
در Bitcoin/BSC، مسیر reorg با ancestor و انتشار شاخهٔ جدید جداگانه است.

شرح workerها: [تحلیل خودکار](AUTOMATIC_ANALYTICS_FA.md).
backup/restore و پذیرش انتشار: [آزمون‌ها](TESTING.md).
