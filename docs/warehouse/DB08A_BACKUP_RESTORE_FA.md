# DB-08A؛ پشتیبان بومی و بازیابی قابل‌تطبیق

این زیرگام، مسیر برگشت پیش از مهاجرت داده را فراهم می‌کند. ابزار
`scripts/warehouse_backup.py` از **کل database همان شبکه**، شامل جدول‌های قدیمی،
جدول‌های `aml_*`، viewها، checkpoint و ledger، پشتیبان بومی ClickHouse می‌گیرد.
سپس بازیابی را در database تازه روی مقصد مستقل با منبع تطبیق می‌دهد.
schema جاری همچنان نسخهٔ **۵** است: **۳۹ جدول، ۵۳ view و ۶۸۲ ستون جدول**.
دیتابیس‌ها و دیسک‌های چهار VM مستقل‌اند؛ این ابزار ClickHouse مرکزی ایجاد نمی‌کند.

DB-08A کامل است؛ **DB-08 هنوز باز است**. این کار نگاشت یا backfill انجام نمی‌دهد
و reader/writer، worker، ML یا cache شبکه‌های قدیمی را تغییر نمی‌دهد.
خروجی `verify` همیشه `cutover_allowed: false` دارد. ادامه به‌ترتیب DB-08B
(نگاشت و تطبیق)، DB-08C (backfill و مسیرهای فعال) و DB-08D (تحلیل و پذیرش) است.

## آماده‌سازی هر VM

ابزار Python 3.10+ فقط از کتابخانهٔ استاندارد استفاده می‌کند؛ به پکیج Python
یا Rust binary جدید نیاز ندارد. هر دو provisioner فایل ابزار و دو فایل زیر
را فقط در بستهٔ VMهای شبکه قرار می‌دهند؛ overlay خودکار فعال نمی‌شود.

- `shared/clickhouse/backup_disk.xml`: دیسک محلی مجاز با نام `aml_backups`.
- `shared/clickhouse/backup.compose.yml`: overlay اختیاری با volume وابسته به
  Compose project همان شبکه؛ نام global مشترک برای volume ندارد.

در پنجرهٔ نگهداری، همهٔ برنامه‌ها و workerهای نویسندهٔ همان شبکه و هر نویسندهٔ
خارجی را متوقف کنید. `bash scripts/vm.sh bitcoin pause` برنامه‌های پروژه را
متوقف و ClickHouse و volume آن را حفظ می‌کند. `pause-ingestion` برای این مرحله
کافی نیست، چون workerهای دیگر ممکن است بنویسند.

مثال فعال‌سازی دیسک **در VM بیت‌کوین**، از ریشهٔ مخزن:

```bash
bash scripts/vm.sh bitcoin pause
export AML_CLICKHOUSE_BACKUP_CONFIG="$PWD/shared/clickhouse/backup_disk.xml"
docker compose --project-directory dockerizd_bitcoin --project-name aml-bitcoin \
  --env-file dockerizd_bitcoin/.env -f dockerizd_bitcoin/docker-compose.yml \
  -f shared/clickhouse/backup.compose.yml up -d --no-deps clickhouse
```

این فرمان برای افزودن config، کانتینر ClickHouse همان project را بازسازی می‌کند؛
دادهٔ volume قبلی حفظ می‌شود. نام project باید **همان نام استقرار موجود** باشد؛
اگر قبلاً `--project` سفارشی داشتید همان را به کار ببرید. مسیرها برای TRON،
Ethereum و BSC به‌ترتیب `dockerizd_tron/app`، `dockerizd_ethereum` و
`dockerizd_bsc` هستند؛ نام‌های پیش‌فرض project برابر `aml-tron`، `aml-ethereum`
و `aml-bsc` است. در فرمان‌های بعدی مدیریت ClickHouse نیز overlay را نگه دارید.
هیچ فرمان `down -v` یا پاک‌کردن database جزئی از این روند نیست.

backup volume روی همان VM برای خرابی کل VM کافی نیست. archive را به میزبان/فضای
پشتیبان مستقل منتقل کنید و restore آزمایشی را روی ClickHouse مقصد جدا اجرا کنید.
دیسک `aml_backups` در مقصد نیز باید پیکربندی شده باشد. مبنای SQL بومی:
[راهنمای رسمی Backup/Restore در ClickHouse](https://clickhouse.com/docs/operations/backup).

## گرفتن پشتیبان و ادامهٔ job

اتصال HTTP با نسخهٔ آزموده‌شدهٔ **23.8** است. URL فقط origin است و credential،
path یا query نمی‌پذیرد. نام database واقعی همان VM و پورت واقعی را وارد کنید؛
پورت نمونهٔ بیت‌کوین در فایل تنظیمات نمونه `48123` است.

```bash
export AML_BACKUP_URL=http://127.0.0.1:48123
export AML_BACKUP_USER=your_clickhouse_user
read -rsp 'ClickHouse password: ' AML_BACKUP_PASSWORD
export AML_BACKUP_PASSWORD
python3 scripts/warehouse_backup.py --url "$AML_BACKUP_URL" --timeout 300 backup \
  --network bitcoin --database bitcoin_aml \
  --manifest backups/bitcoin-before-db08.json --writers-paused
python3 scripts/warehouse_backup.py --url "$AML_BACKUP_URL" --timeout 300 status \
  --manifest backups/bitcoin-before-db08.json
```

ابزار `.env` را نمی‌خواند. نام متغیرهای credential با `--user-env` و
`--password-env` قابل تنظیم است. credential به manifest یا خروجی عمومی نمی‌رود؛
خطای ClickHouse فقط با کد عددی گزارش می‌شود. proxy محیط و redirect غیرفعال‌اند.

خروجی‌های `running` و `dispatched` با exit code **۳** به معنی انتظار برای job
هستند؛ `verified` کد **۰** و `rejected` کد **۲** دارد. `status` را تا نتیجهٔ نهایی
ادامه دهید. برای ادامهٔ backup همان فرمان اولیه را با `--resume` تکرار کنید.
UUID و وضعیت dispatch پیش از درخواست ذخیره می‌شوند؛ قطع ارتباط باعث ارسال
دوبارهٔ backup یا درج دوبارهٔ restore نمی‌شود. ادامه به سابقهٔ `system.backups`
همان سرور وابسته است؛ اگر job پس از restart در دسترس نباشد ابزار موفقیت را حدس
نمی‌زند و job نامعلوم را دوباره ارسال نمی‌کند. یک تلاش مستقل تازه لازم است.

manifest موجود بدون `--resume` بازنویسی نمی‌شود. تا پایان اثبات backup، نویسنده‌ها
متوقف بمانند. fingerprint قطعه‌ها و mutationها پیش/پس از خواندن و backup کنترل
می‌شود؛ merge یا تغییر منبع تلاش را رد می‌کند. در منبع دارای merge فعال، در همان
پنجرهٔ نگهداری merge جدول‌های منبع موردنظر را نیز متوقف کنید و بعد از backup
آن‌ها را از سر بگیرید. ابزار هیچ تنظیم یا جدول منبع را تغییر نمی‌دهد.

## انتقال و بازیابی آزمایشی

archive در `/var/lib/clickhouse/backups/<network>-<UUID>.zip` روی سرور منبع است؛
manifest محلی مسیر و UUID آن را ثبت می‌کند. archive و manifest تأییدشده را با هم
به مقصد مستقل ببرید؛ archive را در دیسک `aml_backups` مقصد با همان نام و مجوز
خواندن کاربر ClickHouse قرار دهید. manifest شامل DDL، hash و اطلاعات منبع است و
خصوصی محسوب می‌شود. پوشه‌های `backups/` و archiveها در Git نادیده گرفته می‌شوند.
فقط receipt پاک‌سازی‌شدهٔ fixtureها داخل `docs/warehouse` قرار دارد.

```bash
export AML_RESTORE_URL=http://127.0.0.1:48123
# این endpoint متعلق به VM آزمایشی مستقل است؛ credential مقصد را تنظیم کنید.
python3 scripts/warehouse_backup.py --url "$AML_RESTORE_URL" --timeout 300 restore \
  --backup-manifest backups/bitcoin-before-db08.json \
  --target-database bitcoin_restore_db08a \
  --manifest backups/bitcoin-restore-db08a.json
python3 scripts/warehouse_backup.py --url "$AML_RESTORE_URL" --timeout 300 status \
  --manifest backups/bitcoin-restore-db08a.json
python3 scripts/warehouse_backup.py verify \
  --backup-manifest backups/bitcoin-before-db08.json \
  --restore-manifest backups/bitcoin-restore-db08a.json
```

نام database مقصد **نباید وجود داشته باشد**. نسخهٔ کامل ClickHouse و timezone
باید با منبع یکسان باشند. restore دو UUID و دو فاز دارد: ساختار خالی، سپس داده.
قبل از فاز داده، engine، ستون، default، codec و DDL تمام جدول‌های فیزیکی و خالی‌بودن
آن‌ها بررسی و merge فقط در همین مقصد تازه متوقف می‌شود.

در نسخهٔ آزموده‌شده، `RESTORE DATABASE ... AS ...` بعضی aliasهای view را از دست
می‌دهد. ابزار viewهای مقصد تازه و خالی را از DDL ثبت‌شده، به ترتیب وابستگی بازسازی
و دوباره دقیقاً تطبیق می‌دهد. MV فقط با `TO` صریح پذیرفته می‌شود؛ بازسازی metadata
آن در مقصد خالی انجام می‌گیرد. هیچ جدول فیزیکی یا شیء منبعی حذف/جایگزین نمی‌شود.
فاز داده از `allow_non_empty_tables=false` و `allow_different_table_def=true`
استفاده می‌کند؛ دومی فقط پس از کنترل مستقل و دقیق schema برای حفظ alias اصلاح‌شده
است و به معنی پذیرش drift جدول نیست. نتیجهٔ داده و schema بعد از restore مجدداً
با منبع ثبت‌شده مقایسه می‌شود.

در صورت شکست، database آزمایشی برای بررسی باقی می‌ماند و مقصد فعال نمی‌شود.
`restore ... --resume` همان job ثبت‌شده را دنبال می‌کند. ابزار database شکست‌خورده
را پاک نمی‌کند. بعد از `verified`، mergeهای مقصد هنوز متوقف‌اند؛ تنها پس از ثبت
اثبات و بررسی نتیجه می‌توان آن‌ها را آزاد کرد:

```bash
python3 scripts/warehouse_backup.py --url "$AML_RESTORE_URL" --timeout 300 release-merges \
  --manifest backups/bitcoin-restore-db08a.json
```

این فرمان بار دیگر محتوای مقصد را کنترل می‌کند. شروع merge می‌تواند نسخه‌های قدیمی
ReplacingMergeTree را ادغام کند؛ receipt، اثباتِ زمان بررسی است و پایش دائمی داده
نیست. هیچ‌یک از این فرمان‌ها runtime برنامه را به مقصد جدید وصل نمی‌کنند.

## محتوای تطبیق و محدودیت‌ها

همهٔ ردیف‌های فیزیکی، شامل نسخه‌های قبلی، orphan و attempt منتشرنشده، بدون `FINAL`
خوانده می‌شوند. SHA-256 روی نمایش مرتب و طول‌دار **تمام ستون‌های ذخیره‌شده** محاسبه
می‌شود. نوع و تعریف ستون هم جداگانه در fingerprint است. String/FixedString با hex،
float با بیت‌های مقدار، زمان با epoch دقیق، Array با ترتیب اصلی و NULL جدا از صفر
و رشتهٔ خالی مقایسه می‌شوند. جمع ستون‌های مبلغ صحیح با integer نامحدود Python است؛
حتی مجموع بزرگ‌تر از UInt256 سرریز نمی‌کند. NULL ناشناخته صفر محسوب نمی‌شود.

نوع‌های scalar/array موجود مخزن پوشش داده شده‌اند؛ نوع ناشناخته و engine خارج از
View/MaterializedView/MergeTree/ReplacingMergeTree/SummingMergeTree رد می‌شود.
ReplicatedMergeTree/Keeper، storage policy سفارشی، رمزگذاری backup، رمزگذاری volume،
بار تولید و قطع/وصل خود سرور در دامنهٔ پذیرش فعلی نیستند. cap metadata برابر
۳۲ MiB، هر ردیف ۱۲۸ MiB، حافظهٔ query برابر ۵۱۲ MiB و spill sort برابر ۶۴ MiB
است؛ timeout قابل تنظیم از ۱ تا ۳۶۰۰ ثانیه است. حجم/بار واقعی باید جدا ارزیابی شود.
هویت شبکه با ledger مشترک یا ستون‌های شبکهٔ legacy کنترل می‌شود؛ legacy بدون
چنین شاهدی صریحاً `unverified_legacy` ثبت می‌شود و اثبات هویت شبکه محسوب نمی‌شود.

آزمون‌ها روی دو ClickHouse موقت مستقل اجرا شدند و به VM یا دیتابیس واقعی دست نزدند.
[گزارش پذیرش](DB08A_VALIDATION.md)، [receipt چهار شبکه](db08a-acceptance.json).
