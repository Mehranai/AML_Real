# BSC AML

Rust adapter برای BNB Smart Chain mainnet با chain ID=56 و
`network_id=eip155:56`. API روی 6001 است؛ ClickHouse/database/volume مستقل
روی VM شبکه و Neo4j تحقیق فقط روی Main قرار می‌گیرد. RPC آماده را کاربر
تأمین می‌کند؛ VM شبکه نود کامل اجرا نمی‌کند.

## مسیر دادهٔ فعال

init/check، ingest/range/follow، replay/repair، metadata، protocol/intelligence
و API تحقیق/موجودی/مسیر از قرارداد مشترک `aml_*` استفاده می‌کنند. DDL native
قبلی نصب نمی‌شود. schema همچنان v5 با ۳۹ جدول، ۵۳ view و ۶۸۲ ستون است؛
projectionهای `bsc_*` فقط CTE هستند و داده یا view فیزیکی جدا نمی‌سازند.

- block/transaction/receipt/trace اصلی RPC، از جمله فیلدهای اضافی پاسخ،
  حفظ می‌شوند. مبلغ خام و شناسهٔ NFT دقیق‌اند؛ log index، sub-index دستهٔ
  ERC1155 و مسیر callTracer نگه داشته می‌شوند. تراکنش failed انتقال موفق
  ایجاد نمی‌کند. پروفایل‌های Bitcoin و TRON خالی می‌مانند.
- writer مشترک کل batch را بررسی و همهٔ ستون‌های ذخیره‌شده را تطبیق می‌دهد؛
  سپس complete و checkpoint منتشر می‌شوند. lock مشترک نقش‌های Compose در
  `/var/lock/bsc` از دو ingestor هم‌زمان جلوگیری می‌کند؛ فایل lock حذف نشود.
- replay همان شاخه می‌تواند پوشش trace/تحلیل را تازه کند و tip را نگه دارد.
  reorg با rollback مشترک و tombstone بالای مرز انتشار انجام می‌شود؛
  retry و restart نیز همان cursor را مصرف می‌کنند. replay اجباری registry
  فعلی را دوباره می‌خواند تا تغییر review/source در تفسیر بازه اعمال شود. در تعویض کل بازه، شروع
  بازهٔ قبلی حفظ می‌شود. repair حداکثر ۱۰هزار بلاک و reorg عمق تنظیم‌شده دارد.
- صف metadata از discovery canonical مصرف می‌شود. RPC state به بلاک کشف
  متصل و هش قبل/بعد و هنگام ذخیره بررسی می‌شود. state تاریخی باید در RPC
  موجود باشد؛ decimals ناشناخته NULL است. metadata دستی به نام بازبین واقعی
  وابسته است و شمارهٔ بلاک ساختگی نمی‌گیرد.
- source/claim/review پروتکل و intelligence در جدول‌های مشترک ذخیره می‌شوند؛
  تصمیم برچسب به بازبین و دلیل نیاز دارد و revoke آن را از active خارج
  می‌کند. import به‌خودی‌خود منبع را VERIFIED یا منبع غیرفعال را فعال نمی‌کند.
- تحلیل entity/cluster candidate و exposure همچنان هنگام درخواست انجام و
  در snapshot مرکزی ذخیره می‌شود. BSC worker یا امتیاز ریسک محلی تازه ندارد؛
  `risk_engine` نیاز به ارزیابی مرکزی را اعلام می‌کند. مرز سرویس و ترتیب
  تراکنش همان دارایی حفظ می‌شوند؛ مسیر، اثبات مالکیت یا تداوم عین پول نیست.
- پاسخ تحقیق، snapshot مشترک بلاک و نسخهٔ intelligence را کنترل می‌کند.
  holdings از RPC finalized خوانده می‌شود؛ خطا/شبکهٔ غلط موجودی صفر نیست و
  endpoint مستقیم holdings برای unavailable پاسخ 503 می‌دهد.

## اجرا

پس از تنظیم `.env` از [.env.example](.env.example)، از ریشه روی VM BSC:

```bash
bash scripts/vm.sh bsc up --build
bash scripts/vm.sh bsc check-schema
bash scripts/vm.sh bsc check-runtime
```

بازه در `BSC_INGEST_START_BLOCK/BSC_INGEST_END_BLOCK` و رفتار دریافت در
`BSC_SYNC_MODE` است. بازهٔ منتشرشده باید پیوسته باشد؛ شروع تازه می‌تواند از
بلاک انتخابی باشد. برای دریافت، replay یا repair از ingestorهای خود پروژه
استفاده کنید. اجرای هم‌زمانشان روی یک database با lock رد می‌شود.

[VM](../docs/VM_SETUP_FA.md)، [استفاده](../docs/USAGE_FA.md)،
[معماری BSC](docs/architecture.md)، [نگاشت داده](docs/data-dictionary.md)،
[آزمون‌ها](../docs/TESTING.md)، [TODO_LIST](../TODO_LIST.md).
پذیرش RPC/VM واقعی و مسیر کامل پنل/Neo4j مرکزی همچنان جدا هستند.
