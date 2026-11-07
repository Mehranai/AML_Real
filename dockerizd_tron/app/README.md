# TRON Rust app

VM ترون برای API، دریافت داده و ClickHouse مستقل است؛ Neo4j روی VM اصلی
قرار دارد. [اجرا](../../docs/USAGE_FA.md)، [ساخت VM](../../docs/VM_SETUP_FA.md)
و [کارهای باقی‌مانده](../../TODO_LIST.md).

`src/services/tron/fetcher.rs` تراکنش‌ها را از RPC آماده دریافت و شاهد اصلی
را کنترل می‌کند. `publication_store.rs` بلاک کامل را به قرارداد مشترک v6
منتشر می‌کند؛ تراکنش/انتقال، contract/log/internal/resource، feature، شاهد
خام و metadata discovery همگی با یک checkpoint دیده می‌شوند. batcherهای
مستقل و نوشتن مستقل sync state حذف شده‌اند.

`src/db/read.rs` به DTOهای قبلی از روی viewهای canonical مشترک شکل می‌دهد؛
کپی داده یا table تازه‌ای ایجاد نمی‌کند. metadata، برچسب/بازبینی، خوشه‌بندی و exposure نیز
از همان جدول‌های مشترک استفاده می‌کنند. init/check برنامه فقط قرارداد مشترک
را نصب/کنترل می‌کند. SQL بومی و mount آن حذف شده‌اند؛ Compose ابتدا
`tron-schema` را اجرا می‌کند و API/workerها منتظر موفقیت آن می‌مانند.
[SQL قابل بازبینی](../../shared/warehouse/sql/README.md) یک نسخهٔ مشترک دارد.

در اجرای CLI، `TRON_WRITER_LOCK_DIR` بین ingestion/replay/benchmark باید
مشترک باشد. Compose آن را روی volume مشترک `/var/lib/tron-writer` قرار
می‌دهد. replay فقط بازهٔ قبلاً منتشرشدهٔ همان شاخه را refresh می‌کند؛ برای
دریافت بلاک‌های جدید از ingest استفاده کنید. تنظیمات قدیمی اندازه/فاصلهٔ
flush batch حذف شده‌اند؛ مرز انتشار همیشه بلاک کامل است.

NFT نوع TRC721 با token ID دقیق و مقدار ۱ ثبت می‌شود. TransferAssetContract
با [lookup بدون ابهام](https://github.com/tronprotocol/java-tron/blob/develop/framework/src/main/java/org/tron/core/Wallet.java#L1588) در `wallet/getassetissuebyname` به ID عددی نگاشت و شاهد
catalog با زمان مشاهده نگه‌داری می‌شود. این catalog وضعیت تاریخی دارایی
را اثبات نمی‌کند؛ پاسخ مبهم یا فاقد شناسه انتشار را متوقف می‌کند.

API holdings، **netflow تاریخچهٔ دریافت‌شده** را نشان می‌دهد؛ موجودی فعلی
RPC نیست. مقدار خام دقیق و علامت ناقص‌بودن حفظ می‌شود؛ decimals ناشناخته
و مقدار انسانی وابسته به آن NULL هستند. [تست‌ها](../../docs/TESTING.md).

metadata از `walletsolidity/triggerconstantcontract` خوانده می‌شود. سرآیند solid
قبل و بعد از تماس‌ها باید ثابت باشد و همان بلاک در تاریخچهٔ canonical دریافت
شده باشد. زمان/بلاک مشاهدهٔ واقعی، پاسخ خام و وضعیت retry/dead_letter حفظ
می‌شوند؛ metadata به بلاک کشف قدیمی نسبت داده نمی‌شود. محدودیت معنایی این
RPC در [مستند TRON](https://developers.tron.network/reference/triggerconstantcontract-1)
آمده است؛ این مشاهده ادعای اثبات تاریخی مستقل یا verified بودن دارایی نیست.

کاندیداهای ساختاری در `aml_intelligence_claims` برای review می‌مانند؛ تنها
پس از review، خروجی مشتق‌شده در خانوادهٔ `aml_cluster_*` منتشر می‌شود.
exposure نیز مسیرهای مشاهده‌شده با relationship/tx اصلی، ترتیب تراکنش و
دارایی ثابت را نگه می‌دارد و از خروجی تجمیعی صرافی عبور نمی‌کند. نتیجه،
تخصیص قطعی همان وجوه یا اثبات مالکیت مشترک نیست. تغییر source/review یا
revision زنجیره sealهای وابسته را نامعتبر می‌کند؛ retry خروجی تازه می‌سازد.

API تحقیق `source_snapshot` و `operational_snapshot` دارد. مسیر اصلی ذخیرهٔ
تحلیل و گراف همان `GET /api/tron/wallet/{address}/investigation` روی Main است. endpoint قدیمی
`/api/analysis/tron/wallet/{address}` پاسخ 410 و مسیر سرویس مرکزی را می‌دهد؛
جدول‌های snapshot و executor/آموزش ML محلی ترون کنار گذاشته شده‌اند.
API قدیمی ai-risk همچنان DISABLED/UNKNOWN است؛ سیاست توضیح‌پذیر مرکزی
از شواهد تحقیق استفاده می‌کند. بازگشایی/Export در مرکز، تغییر دادهٔ زنجیره
یا رکوردهای عملیاتی ترون را بررسی می‌کند و ریسک تاریخی را current نشان نمی‌دهد.
