# TRON SQL

نصب schema از [قرارداد مشترک](../../../shared/warehouse/README.md) انجام
می‌شود. SQL قابل بازبینیِ هر شش فاز فقط در
[پوشهٔ مشترک](../../../shared/warehouse/sql/README.md) نگه‌داری می‌شود؛ کپی
اختصاصی ترون و `init_database_tron.sql` حذف شده‌اند.

`tron_schema` قرارداد جاری v6 با ۴۲ جدول، ۵۶ view و ۷۳۹ ستون را روی
ClickHouse مستقل VM ترون نصب می‌کند؛ `tron_schema --check` فقط بررسی
می‌کند. Compose ابتدا job یک‌بارهٔ schema را اجرا می‌کند و API/workerها
بعد از موفقیت آن شروع می‌شوند. دیتابیس استقرار باید خالی باشد.

projectionهای [read.rs](../src/db/read.rs) فقط CTE هر query هستند و جدول
یا view فیزیکی دیگری ایجاد نمی‌کنند. شرح جدول‌ها و تفاوت شبکه‌ها در
[راهنما](../../../docs/warehouse/README_FA.md) آمده است.
