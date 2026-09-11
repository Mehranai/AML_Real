# تنظیم شبکه VMهای کارفرما

این سند فقط تنظیمات غیرمحرمانه را پوشش می‌دهد. فایل‌های .env.example با IPهای تأییدشده آماده‌اند،
اما هنوز جای فایل خصوصی مقداردهی‌شده را نمی‌گیرند. رمز دیتابیس و کلید داخلی نباید در GitHub منتشر شوند.

| VM | IP | ورودی |
|---|---|---|
| Main | 10.20.0.10 | وب روی 8080 |
| TRON | 10.20.0.21 | API روی 4001 |
| Ethereum | 10.20.0.22 | API روی 5001 |
| BSC | 10.20.0.23 | API روی 6001 |

این IPها باید واقعاً به سیستم‌عامل VMها اختصاص داشته باشند؛ env، آدرس شبکه VM را ایجاد نمی‌کند.
فقط VM اصلی Neo4j دارد. ClickHouse هر شبکه و پورت‌های مدیریت Neo4j فقط روی localhost منتشر می‌شوند.
Firewall باید API شبکه‌ها را فقط برای IP اصلی باز کند. برای استفاده عملیاتی، TLS/VPN و احراز هویت تحلیل‌گر لازم است.

## انتقال Docker به Linux

بسته فعلی برای Linux/amd64 آزموده شده است؛ معماری CPU مقصد باید سازگار باشد.
روی مقصد Docker Engine، Compose v2، Bash، jq و sha256sum لازم‌اند.
برای اجرای image آماده به Rust/Cargo روی VM مقصد نیاز نیست.

روی ماشین build که imageهای به‌روز را دارد:

```bash
bash scripts/export-images.sh --include-bsc
```

فایل‌های پروژه و پوشه deployment-artifacts را منتقل کنید؛ تنظیمات خصوصی را جدا از Git و از مسیر امن ببرید.
روی مقصد:

```bash
bash scripts/import-images.sh ./deployment-artifacts
```

ابزار، checksum همه imageها را پیش از import بررسی می‌کند. منبع بسته و manifest باید قابل اعتماد باشد.
imageها شامل داده ClickHouse یا snapshotهای Neo4j نیستند؛ انتقال داده و backup مرحله جداگانه است.
پس از تأمین تنظیمات خصوصی، scripts/vm.sh با role همان VM اجرا می‌شود.
برای image آماده --build لازم نیست؛ --pull-never مانع دانلود image در مقصد می‌شود.
در BSC، برای بسته آفلاین فعلی باید BSC_CLICKHOUSE_IMAGE=clickhouse/clickhouse-server:23.8 باشد؛
digest پیش‌فرض ممکن است در imageهای واردشده موجود نباشد.

## اتصال RPC آزمایشی

در 2026-09-11، endpointهای عمومی زیر بدون API key تست شدند:
- TRON: api.trongrid.io، پاسخ بلاک از getnowblock.
- Ethereum: ethereum-rpc.publicnode.com، chain id برابر 0x1.
- BSC: bsc-rpc.publicnode.com، chain id برابر 0x38.

این فقط تأیید اتصال در زمان تست است، نه تضمین پایداری، سرعت backfill، تاریخچه یا trace.
منابع معرفی سرویس: [Ethereum PublicNode](https://ethereum.publicnode.com)،
[BSC PublicNode](https://bsc.publicnode.com)، [TRON API](https://developers.tron.network/reference/select-network).

## volumeهای موجود

تغییر رمز در env لزوماً رمز دیتابیس دارای داده قبلی را تغییر نمی‌دهد، به‌خصوص Neo4j.
برای خطای ورود، volume را حذف نکنید؛ تنظیمات و فرآیند تغییر رمز باید با دیتابیس موجود تطبیق داده شود.
Compose project name را ثابت نگه دارید تا volume دیگری انتخاب نشود.
در این مرحله هیچ دیتابیسی بازنشانی نشده و فایل‌های خصوصی قبلی بدون تغییر حفظ شده‌اند.
