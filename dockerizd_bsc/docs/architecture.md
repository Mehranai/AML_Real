# BSC architecture

RPC آماده → دریافت و اعتبارسنجی BSC → facts مشترک ClickHouse روی VM BSC →
API شبکه → Analytical Node و Neo4j روی Main. نود کامل یا Neo4j محلی BSC
وجود ندارد. دیتابیس هنگام استقرار تازه خالی است.

`ingestion/rpc.rs` درخواست‌های محدود، receipt fallback، trace و پاسخ خام
را می‌گیرد؛ `model.rs` header/position/status و UInt256 را بررسی می‌کند.
`transfers.rs` انتقال‌های مشاهده‌شده و `semantic.rs` تفسیرهای نسخه‌دار را
می‌سازند. `ingestion/mapping.rs` این شواهد را مستقیم به قرارداد مشترک
می‌برد؛ pipeline انتقال دیتابیس قدیمی وجود ندارد.

`store.rs` از publisher مشترک با تطبیق کامل facts و checkpoint استفاده
می‌کند. kernel lock میان ingest/replay/repair مشترک است. rollback شاخه،
بلاک‌ها و facts قبلی را از canonical خارج می‌کند؛ restart بازهٔ شروع را
حفظ می‌کند. failure و benchmark هم در namespace مشترک قرار دارند.

`db/read.rs` CTEهای مصرف‌کنندگان فعلی API را تعریف می‌کند؛ compatibility
DDL یا کپی داده نیستند. `db/records.rs` اطلاعات operational را به ستون‌های
مشترک می‌نویسد. schema/init/check از `shared/warehouse` می‌آیند.

metadata از state همان بلاک کشف خوانده می‌شود؛ holdings از finalized فعلی.
claim/review و exposure هنگام درخواست از دادهٔ canonical خوانده می‌شوند.
قواعد ترتیب تراکنش، همان دارایی و مرز سرویس حفظ شده‌اند. risk policy
محلی جدید اضافه نشده و API نیاز به ارزیابی مرکزی را اعلام می‌کند.

[نگاشت داده](data-dictionary.md)، [ساخت VM](../../docs/VM_SETUP_FA.md)،
[وضعیت کل پروژه](../../docs/PROJECT_STATUS_FA.md)،
[پذیرش و محدودیت تست](../../docs/TESTING.md).
