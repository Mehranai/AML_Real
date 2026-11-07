# Bitcoin SQL

نصب schema از [قرارداد مشترک](../../shared/warehouse/README.md) انجام
می‌شود. SQL قابل بازبینی هر شش فاز فقط در
[پوشهٔ مشترک](../../shared/warehouse/sql/README.md) نگه‌داری می‌شود؛ کپی‌های
قبلی `migrations` بیت‌کوین حذف شده‌اند. `queries/activity.sql` و
`queries/holdings.sql` queryهای فعال و مخصوص معنای UTXO هستند و schema
مستقل ایجاد نمی‌کنند. query روابط از کتابخانهٔ مشترک استفاده می‌کند.

`bitcoin_init_schema` قرارداد جاری v6 با ۴۲ جدول، ۵۶ view و ۷۳۹ ستون را
روی ClickHouse مستقل VM بیت‌کوین نصب می‌کند. `--check` فقط بررسی می‌کند و
`--print-sql` خروجی هر شش فاز را بدون نیاز به اتصال دیتابیس نشان می‌دهد.
Compose، job schema را پیش از API/workerها اجرا می‌کند؛ دیتابیس استقرار
باید خالی باشد. نصب توسط binary، هویت شبکه، drift و ledger را نیز کنترل
می‌کند؛ اجرای دستی قالب SQL جای آن را نمی‌گیرد.

[شرح جدول‌ها و تفاوت‌های شبکه‌ها](../../docs/warehouse/README_FA.md).
