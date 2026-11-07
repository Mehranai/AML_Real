# Ethereum AML

Rust adapter برای Ethereum mainnet با `network_id=eip155:1`. این VM مالک
API، ingestion، metadata/analytics workerها و ClickHouse محلی است. Neo4j
روی VM اصلی قرار می‌گیرد؛ VM اتریوم نود کامل بلاکچین اجرا نمی‌کند.

## مسیر دادهٔ فعال

init و check فقط قرارداد مشترک نسخهٔ ۵ را مصرف می‌کنند: ۳۹ جدول و ۵۳ view.
writer، checkpoint، investigation، holdings، intelligence، clustering، exposure
و risk مستقیماً `aml_*` را مصرف می‌کنند. projectionهای `eth_*` در SQL، CTE
محلی همان query هستند؛ جدول یا view فیزیکی جدا و کپی داده ایجاد نمی‌کنند.
منبع DDL [کتابخانهٔ مشترک](../shared/warehouse/README.md) است؛ دادهٔ دیتابیس
آزمایشی قدیمی منتقل نمی‌شود و استقرار تازه دیتابیس خالی لازم دارد.

- block/transaction، receipt/log، trace و شاهد خام اصلی حفظ می‌شوند. مبلغ
  و شناسهٔ NFT به‌صورت عدد صحیح دقیق ذخیره می‌شوند؛ تراکنش failed جریان
  موفق ایجاد نمی‌کند. ERC20/ERC721/ERC1155 به مدل دارایی مشترک نگاشت می‌شوند.
- withdrawal اعتبارسنج، رخداد بلاک است: در شاهد خام بلاک و پاسخ
  `block_credits` نمایش داده می‌شود؛ tx hash یا فرستندهٔ ساختگی ندارد.
- batch قبل از نوشتن بررسی می‌شود و همهٔ ستون‌ها پس از نوشتن تطبیق داده
  می‌شوند؛ checkpoint فقط پس از complete منتشر می‌شود. lock مشترک کانتینرها
  در `/var/lock/ethereum` جلوی دو ingestor هم‌زمان را می‌گیرد.
- دریافت مجدد با trace اجباری می‌تواند پوشش بلاک قدیمی همان شاخه را کامل
  کند و tip را نگه دارد؛ تعویض ضمنی هش یا کاهش coverage پذیرفته نمی‌شود.
  این رفتار به معنی پیاده‌سازی repair خودکار تغییر بلاک finalized نیست.
- صف metadata از discovery منتشرشده هم بازسازی می‌شود. bytecode و eth_call
  با شمارهٔ بلاک کشف خوانده می‌شوند؛ هش RPC قبل/بعد و هش canonical موقع
  ذخیره بررسی می‌شود. decimals نامعلوم، `NULL` باقی می‌ماند.
- برچسب‌ها به source و review واقعی وابسته‌اند. cluster، exposure و risk
  با source snapshot و publication مشترک منتشر می‌شوند؛ revoke خروجی قبلی
  را نامعتبر می‌کند. registry اولیه از [config](config/protocols.json) می‌آید؛
  attribution آن تنظیم نرم‌افزاری است، نه تأیید انسانی تحلیلگر.
- exposure مسیر مشاهده‌شدهٔ همان دارایی است، نه اثبات تداوم عین پول یا
  مالکیت. مسیر بعد از service شناخته‌شده ادامه نمی‌یابد؛ چند hop فقط با
  ترتیب صریح بلاک‌ها ساخته می‌شود و ادامهٔ داخل همان بلاک ادعا نمی‌شود.
  محاسبه محدود است؛ عبور از بودجهٔ گراف خطا می‌دهد و نتیجهٔ ناقص complete
  منتشر نمی‌کند. کیفیت intelligence و تنظیم policy جداگانه باید پذیرفته شود.

## اجرا

از ریشه پس از تنظیم `dockerizd_ethereum/.env`:

```bash
bash scripts/vm.sh ethereum up --build
bash scripts/vm.sh ethereum check-schema
bash scripts/vm.sh ethereum check-runtime
```

`ETH_RPC_URL`، بازه، finality/trace، رمز DB و service key را از
[نمونه](.env.example) تنظیم کنید. history metadata به RPC دارای state تاریخی
در بلاک کشف نیاز دارد؛ خطای RPC در صف retry/dead-letter ثبت می‌شود.
`up --api-only` دریافت را شروع نمی‌کند. برای تکمیل trace بازهٔ قبلی، اجرای
هم‌زمان ingestion را متوقف و CLI همان ingestor را با `ETH_TRACE_MODE=required`
برای آن بازه اجرا کنید؛ فایل lock را دستی حذف نکنید.

[ساخت VM](../docs/VM_SETUP_FA.md)، [استفاده](../docs/USAGE_FA.md)،
[قرارداد](../docs/warehouse/README_FA.md)، [آزمون‌ها](../docs/TESTING.md)،
[وضعیت کل پروژه](../docs/PROJECT_STATUS_FA.md)، [TODO_LIST](../TODO_LIST.md).
پذیرش RPC واقعی، VM کاربر و parity کامل API مرکزی همچنان تسک‌های جدا هستند.
