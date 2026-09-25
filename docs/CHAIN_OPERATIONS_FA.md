# راهنمای کار روزمره شبکه‌ها و ClickHouse

این راهنما برای استقرار چهار VM با `scripts/provision_vms.py` است. فرمان‌های Python را روی میزبان Linux، از ریشه پروژه اجرا کنید، نه داخل VM اصلی:

```bash
cd "$HOME/AML_Whole"
```

نام شبکه یکی از `tron`، `ethereum` یا `bsc` است. ابزار نام VM را از تنظیمات پروژه می‌گیرد؛ لازم نیست IP یا رمز را در ترمینال وارد کنید. VM انتخاب‌شده باید قبلاً ساخته شده و Running باشد. این فرمان‌های کنترلی VM جدید نمی‌سازند و Docker میزبان را لازم ندارند؛ از Docker داخل VM استفاده می‌کنند.

## ۱. پس از همگام‌سازی چه اتفاقی می‌افتد؟

حالت عادی، دنبال‌کردن پیوسته بلاک‌های نهایی‌شده است، نه اجرای یک‌باره و نه وظیفه زمان‌بندی‌شده روزانه:

1. در شروع، checkpoint از ClickHouse خوانده می‌شود.
2. بلاک‌های عقب‌مانده تا سر نهایی‌شده زنجیره دریافت و ذخیره می‌شوند.
3. پس از رسیدن به سر زنجیره، حلقه زنده می‌ماند؛ با تنظیم فعلی TRON و BSC هر ۳ ثانیه و Ethereum هر ۱۲ ثانیه دوباره بررسی می‌شوند.
4. بلاک‌های جدید از ادامه دریافت می‌شوند. نیازی نیست هر روز برنامه را دستی اجرا کنید.

این فاصله‌ها زمان بررسی در حالت رسیدن به سر زنجیره‌اند، نه تضمین تأخیر ثابت؛ سرعت RPC، حجم بلاک و ظرفیت دیتابیس مؤثر است. سرویس‌ها باید روشن و RPC در دسترس باشد. اگر RPC یا ingestion خطا داشته باشد، صرف روشن‌بودن کانتینر یا پاسخ ready، کامل‌بودن داده را ثابت نمی‌کند؛ لاگ و checkpoint را هم بررسی کنید.

فایل‌های مربوط:

- TRON: `dockerizd_tron/app/src/tasks/fetch_loop.rs`، حالت `SYNC_MODE=auto` و `TRON_POLL_INTERVAL_SECONDS=3`.
- Ethereum: `dockerizd_ethereum/src/ethereum/ingestion.rs`، تابع `follow_finalized` و `ETH_POLL_INTERVAL_SECONDS=12`.
- BSC: `dockerizd_bsc/src/ingestion/service.rs`، تابع `follow`، حالت `BSC_SYNC_MODE=auto` و `BSC_POLL_INTERVAL_SECONDS=3`.

در Ethereum یک اشکال اصلاح شد: دیتابیس خالی اکنون واقعاً از `ETH_START_BLOCK` شروع می‌کند، نه از آخرین بلاک شبکه. اگر checkpoint موجود باشد، همان ادامه پیدا می‌کند؛ `--start-block` صریح در ابزار تخصصی همچنان اولویت دارد. این اصلاح، بازه تاریخی جاافتاده در دیتابیس قبلی را خودکار پر نمی‌کند. برای آن باید بازه جاافتاده جداگانه و کنترل‌شده backfill شود؛ checkpoint را دستی پاک نکنید.

اجرای اولیه همراه ingestion هر سه شبکه، در صورتی که هنوز استقرار انجام نشده:

```bash
python3 scripts/provision_vms.py up --build --with-ingestion
```

این کار ممکن است فضای زیادی مصرف کند. برای کنترل یک شبکه در استقرار موجود، به‌جای `up` کلی از دستورهای بخش بعد استفاده کنید.

## ۲. توقف و ادامه فقط یک شبکه

مثال برای Ethereum:

```bash
# Stop the API and background workers, but keep the VM and ClickHouse running.
python3 scripts/provision_vms.py pause ethereum

# Inspect container states on this VM.
python3 scripts/provision_vms.py ps ethereum

# Show recent container logs without following indefinitely.
python3 scripts/provision_vms.py logs ethereum

# Resume the API and background workers from the stored checkpoints.
python3 scripts/provision_vms.py resume ethereum
```

با `pause`، VM اصلی و TRON و BSC تغییر نمی‌کنند. فقط API، ingestion و workerهای معمول Ethereum متوقف می‌شوند. جستجوی Ethereum در پنل اصلی تا `resume` در دسترس نیست؛ جستجوی شبکه‌های دیگر باقی می‌ماند.

برای دو شبکه دیگر:

```bash
python3 scripts/provision_vms.py pause tron
python3 scripts/provision_vms.py resume tron

python3 scripts/provision_vms.py pause bsc
python3 scripts/provision_vms.py resume bsc
```

این دستورها volume یا checkpoint را حذف نمی‌کنند. `resume` فقط سرویس‌های برنامه را با image موجود اجرا می‌کند، بدون build، pull یا restart دیتابیس. ClickHouse باید از قبل روشن باشد. اگر پردازش یک بلاک هنگام توقف نیمه‌تمام بوده باشد، ممکن است همان بلاک دوباره بررسی شود؛ این با شروع مجدد کل تاریخچه فرق دارد.

`pause` نام فرمان مدیریتی ماست و از `docker compose stop` استفاده می‌کند، نه `docker pause`. قبل از بررسی ثابت داده منتظر پایان موفق فرمان بمانید. jobهای دستی replay/import که جداگانه اجرا کرده‌اید جزو سرویس‌های معمول نیستند و باید جداگانه متوقف شوند. خود ClickHouse ممکن است mergeهای پس‌زمینه را ادامه دهد؛ لازم نیست موتور دیتابیس متوقف شود.

## ۳. توقف ingest با حفظ API

اگر می‌خواهید نمودار داده‌های قبلی همچنان قابل مشاهده باشد:

```bash
python3 scripts/provision_vms.py pause-ingestion ethereum
```

ingestion و workerهای پس‌زمینه همان شبکه متوقف می‌شوند؛ API و ClickHouse دست‌نخورده می‌مانند. این دستور API ازقبل‌متوقف‌شده را روشن نمی‌کند. در این حالت داده جدید ingest نمی‌شود، اما API ممکن است هنوز snapshot یا خروجی تحلیلی بنویسد؛ برای توقف همه سرویس‌های معمول نویسنده از `pause` کامل استفاده کنید.

ادامه:

```bash
python3 scripts/provision_vms.py resume ethereum
```

`resume` همه سرویس‌های معمول این شبکه، از جمله ingestion را فعال می‌کند، حتی اگر استقرار اولیه API-only بوده باشد.

توقف Docker با سیاست `unless-stopped` حفظ می‌شود، اما اجرای دوباره `up` کلی یا `vm.sh ... up` یک دستور صریح شروع سرویس‌هاست و می‌تواند توقف دستی را لغو کند. توقف جداگانه، تنظیم دائمی «این شبکه هرگز اجرا نشود» نیست. وقتی در حال بررسی شبکه متوقف‌شده هستید `up --with-ingestion` کلی نزنید.

## ۴. اتصال به ClickHouse بدون نوشتن رمز

برای یک query از خود میزبان:

```bash
python3 scripts/provision_vms.py db tron --query "SHOW TABLES"
python3 scripts/provision_vms.py db ethereum --query "SHOW TABLES"
python3 scripts/provision_vms.py db bsc --query "SHOW TABLES"
```

کلاینت داخل کانتینر همان ClickHouse اجرا می‌شود و رمز را از environment همان کانتینر می‌گیرد؛ پورت دیتابیس به اینترنت باز نمی‌شود و رمز در فرمان میزبان قرار نمی‌گیرد. نام دیتابیس به‌صورت خودکار انتخاب می‌شود:

| شبکه | دیتابیس |
|---|---|
| TRON | `tron_db` |
| Ethereum | `ethereum_aml` |
| BSC | `bsc_aml` |

برای محیط تعاملی SQL:

```bash
python3 scripts/provision_vms.py db ethereum
```

بعد از ورود، queryها را با `;` تمام کنید و با `exit` خارج شوید. اگر ترمینال SSH شما حالت تعاملی Multipass را مناسب نمایش نداد، از `--query` استفاده کنید یا ابتدا `multipass shell aml-auto-ethereum` بزنید و در VM اجرا کنید:

```bash
sudo bash /home/ubuntu/AML_Whole/scripts/vm.sh ethereum db
```

این ابزار عمداً `readonly=1` است؛ برای دیدن داده طراحی شده، نه INSERT، DELETE، TRUNCATE یا تغییر schema. سقف زمان query برابر ۳۰ ثانیه و سقف نتیجه تنظیم‌شده ۱۰۰۰ ردیف است. این محدودیت‌ها جای queryهای هدفمند و حساب کاربری read-only مستقل در استقرار production را نمی‌گیرند. برای گرفتن خروجی کامل تاریخچه از این مسیر استفاده نکنید.

## ۵. queryهای کاربردی

در محیط SQL، یا با قرار دادن query داخل `--query`:

```sql
SELECT currentDatabase();
SHOW TABLES;
DESCRIBE TABLE transactions;
SHOW CREATE TABLE transactions;
SELECT * FROM transactions_canonical LIMIT 10 FORMAT Vertical;
SELECT * FROM address_relationships_canonical LIMIT 10 FORMAT Vertical;
```

جدول خام برای بررسی اشکال ingest مفید است؛ viewهای `*_canonical` برای مشاهده خروجی معتبر تحقیق مناسب‌ترند. جدول خام ممکن است نسخه‌های replay و ردیف‌های هنوز نهایی‌نشده داشته باشد. `LIMIT 10` نمونه می‌دهد، نه الزاماً آخرین ده تراکنش.

آخرین checkpoint در هر شبکه:

```bash
python3 scripts/provision_vms.py db tron --query "SELECT * FROM sync_state FINAL WHERE chain = 'tron' FORMAT Vertical"
python3 scripts/provision_vms.py db ethereum --query "SELECT * FROM sync_state FINAL WHERE network_id = 'eip155:1' FORMAT Vertical"
python3 scripts/provision_vms.py db bsc --query "SELECT * FROM sync_state_current WHERE network_id = 'eip155:56' FORMAT Vertical"
```

در TRON و Ethereum، `last_synced_block` آخرین بلاک checkpoint است. در BSC، `next_block` نقطه ادامه است؛ `last_finalized_block` بلاک متناظر checkpoint را نشان می‌دهد. خروجی خالی یعنی هنوز checkpoint ثبت نشده، نه اینکه بلاک صفر حتماً کامل شده است. بالا بودن شماره checkpoint به‌تنهایی پوشش تاریخچه از genesis را اثبات نمی‌کند.

حجم جدول‌ها و تعداد ردیف‌های فیزیکی بدون COUNT روی کل تاریخچه:

```sql
SELECT
    table,
    sum(rows) AS physical_rows,
    formatReadableSize(sum(bytes_on_disk)) AS disk_size
FROM system.parts
WHERE active AND database = currentDatabase()
GROUP BY table
ORDER BY sum(bytes_on_disk) DESC;
```

`physical_rows` شمارش یکتای تراکنش یا تعداد ردیف canonical نیست؛ نسخه‌های چندگانه قبل از merge هم ممکن است در آن باشند. این query حجم partهای فعال را نشان می‌دهد، نه فضای آزاد دیسک VM. برای دیسک:

```bash
multipass exec aml-auto-ethereum -- df -h /
```

نمونه تراکنش‌های یک بازه مشخص، با جایگزینی شماره بلاک‌ها:

```sql
SELECT *
FROM transactions_canonical
WHERE block_number BETWEEN 2000 AND 2094
LIMIT 20 FORMAT Vertical;
```

از `SELECT * FROM transactions` بدون LIMIT و COUNTهای سراسری مکرر روی دیتابیس بزرگ خودداری کنید. برای مقایسه پیشرفت، checkpoint را در دو زمان بخوانید و لاگ همان شبکه را بررسی کنید.

## ۶. ترتیب پیشنهادی برای بررسی بدون توقف بقیه شبکه‌ها

```bash
python3 scripts/provision_vms.py pause ethereum
python3 scripts/provision_vms.py ps ethereum
python3 scripts/provision_vms.py db ethereum --query "SELECT * FROM sync_state FINAL FORMAT Vertical"
python3 scripts/provision_vms.py db ethereum --query "SELECT * FROM transactions_canonical LIMIT 10 FORMAT Vertical"
python3 scripts/provision_vms.py resume ethereum
python3 scripts/provision_vms.py ps ethereum
python3 scripts/provision_vms.py logs ethereum
```

`stop` در ابزار Python، کل VMها را خاموش می‌کند؛ برای این سناریو از آن استفاده نکنید. `down` در launcher هر VM، دیتابیس همان VM را هم متوقف می‌کند؛ این هم با `pause` فرق دارد. هیچ‌کدام از دستورهای این راهنما VM یا volume حذف نمی‌کنند.

فرمان‌های کنترلی، نسخه فعلی `scripts/vm.sh` را قبل از اجرا به VM مربوط منتقل می‌کنند؛ برای اضافه‌شدن این کنترل‌ها به VM قبلی نیازی به ساخت مجدد image نیست. اصلاح کد Rust اتریوم نیازمند image جدید است؛ صرف `resume` سورس جدید را کامپایل نمی‌کند. استقرار اولیه روی سرور تازه با `up --build --with-ingestion` این اصلاح را شامل می‌شود.

## وضعیت آزمون

به‌روزرسانی 2026-09-25: نقص preflight زیر رفع شد و ۳۷ تست Python پاس شد. کنترل `check-runtime` نیز با تست‌های CLI اضافه شده است. گزارش فعلی: [چک‌لیست تحویل](RELEASE_READINESS_FA.md). پاراگراف بعدی سابقه آزمون نسخه قبلی است.

شش تست جدید کنترل VM، همه تست‌های قرارداد CLI در Bash و ۳۵ تست کتابخانه Ethereum پاس شدند. در کل مجموعه provisioner، ۲۹ تست پاس شد و یک تست قدیمی بررسی منابع شکست داشت؛ همین شکست قبل از تغییر نیز ثبت شد، چون بررسی فضای دیسک در نسخه فعلی حذف شده است. کنترل‌های جدید آن بخش را تغییر نداده‌اند.

تست‌های کنترلی با فرمان‌های جداشده از Docker واقعی و با VM شبیه‌سازی‌شده اجرا می‌شوند و ماشین واقعی نمی‌سازند. Docker Engine محلی هنگام توسعه این تغییر در دسترس نبود؛ تست زنده توقف سرویس، احراز هویت ClickHouse و resume روی سرور مقصد همچنان لازم است.

منابع رفتار زیرساخت: [توقف و شروع Compose](https://docs.docker.com/compose/support-and-feedback/faq/)، [اجرای سرویس بدون وابستگی‌ها](https://docs.docker.com/reference/cli/docker/compose/up/).
