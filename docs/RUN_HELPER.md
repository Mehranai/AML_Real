
برای فعال‌کردن ingestion در استقرار VMها باید صریحاً اجرا کنی:

```bash
python3 scripts/provision_vms.py up --with-ingestion
```

این کار هر سه شبکه را شروع می‌کند و ممکن است از بلاک صفر شروع به دریافت و ذخیره‌سازی کند.

## Second Section (Turn Off and On)

برای خاموش‌کردن کامل اجرای محلی ویندوز:

```cmd
cd /d D:\Sarbazi\AML_Whole
python scripts/run_local.py stop
```

این دستور کانتینرها را متوقف می‌کند و داده‌های ClickHouse را حذف نمی‌کند.

برای روشن‌کردن دوباره:

```cmd
python scripts/run_local.py
```

اما اجرای محلی فعلی ingestion ندارد.

اگر روی VMها ingestion را با این دستور فعال کرده بودی:

```bash
python3 scripts/provision_vms.py up --with-ingestion
```

خاموش‌کردن:

```bash
python3 scripts/provision_vms.py stop
```

روشن‌کردن دوباره و ادامه از checkpoint:

```bash
python3 scripts/provision_vms.py up
```

از `down -v` استفاده نکن؛ چون ممکن است volumeهای دیتابیس و checkpointها حذف شوند.