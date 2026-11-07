# BSC SQL

نصب schema از [قرارداد مشترک](../../shared/warehouse/README.md) انجام
می‌شود. SQL قابل بازبینی هر شش فاز فقط در
[پوشهٔ مشترک](../../shared/warehouse/sql/README.md) نگه‌داری می‌شود؛ کپی
مستقل برای شبکه نگه‌داری نمی‌شود.

`bsc_schema` قرارداد جاری v6 با ۴۲ جدول، ۵۶ view و ۷۳۹ ستون را روی
ClickHouse مستقل VM شبکه نصب می‌کند؛ `bsc_schema --check` فقط بررسی
می‌کند. Compose ابتدا job schema را اجرا می‌کند و API/workerها پس از
موفقیت آن شروع می‌شوند. دیتابیس استقرار باید خالی باشد.

projectionهای [read.rs](../src/db/read.rs) فقط CTE هر query هستند و شیء
فیزیکی دیگری ایجاد نمی‌کنند. [شرح جدول‌ها و تفاوت‌های شبکه‌ها](../../docs/warehouse/README_FA.md).
