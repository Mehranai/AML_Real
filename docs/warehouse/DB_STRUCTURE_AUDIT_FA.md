# اصلاح ترتیب DB و ساختار بیت‌کوین — ۲۰۲۶-۱۰-۲۳

به‌روزرسانی ۲۰۲۶-۱۰-۲۶: این سند نتیجهٔ بازبینی ساختاری قبلی است. ممیزی کامل
ستون‌ها، خانواده‌های تحلیلی و مسیرهای worker اکنون در
[خروجی DB-01](DB01_DATA_MAP_FA.md) تکمیل شده است. اکنون
[DB-03 و ارتقای نسخهٔ ۲](DB03_COLUMN_CONTRACT_FA.md) و
[قرارداد پایه DB-04 نسخهٔ ۳](DB04_CORE_CONTRACT_FA.md) نیز تکمیل شده‌اند. [DB-05 نسخهٔ ۴](DB05_ANALYTICS_CONTRACT_FA.md) قرارداد تحلیلی/تخصصی را اضافه می‌کند؛ قدم بعد DB-06 است. وضعیت‌های
باز DB-01 در متن تاریخی زیر با این خروجی جایگزین می‌شوند.

اعتراض دربارهٔ عبور زودهنگام از مراحل DB درست بود. پیش از این، بیت‌کوین فایل‌های
تخت `db.rs`، `warehouse.rs` و یک SQL قدیمی داشت؛ SQL نصب فعال از قرارداد داخل
بستهٔ مشترک ساخته می‌شد و در پوشه SQL بیت‌کوین دیده نمی‌شد. این چیدمان به
ساختار ماژولی Ethereum/BSC نزدیک نبود. تکمیل ING-01 تا ING-03، به معنی تکمیل
DB-01 و DB-03 تا DB-09 نیست. ING-04 در این بازبینی جلو نرفته است.

## تغییر این بازبینی

```text
dockerizd_bitcoin/
  src/
    bin/bitcoin_init_schema.rs
    db/
      mod.rs          # Database/SyncState و مرز ماژول‌ها
      client.rs       # اتصال HTTP و adapter بستهٔ مشترک
      migrations.rs   # نصب، checksum، identity، drift و schema CLI
      sql.rs          # SQL نصب و کنترل انطباق با قرارداد
      queries.rs      # checkpoint، publication و status
      warehouse.rs    # نگاشت قرارداد و انتقال تاریخچهٔ قدیمی
    bitcoin/
      rpc.rs, ingestion.rs, fetch.rs, resolver.rs, probe.rs
      failures.rs, repair.rs
    domain/model.rs   # آدرس، مقدار و شواهد UTXO
    storage/
      writer.rs       # reserve → facts → verify → complete → publish
      writer_lock.rs
  sql/
    migrations/20261022_0001_shared_warehouse.sql
    legacy/001_core.sql
```

`lib.rs` مسیرهای عمومی قبلی را re-export می‌کند؛ API و ابزارهای قبلی به تغییر
import نیاز ندارند. binary مستقل schema در image کپی و در Compose و
`vm.sh bitcoin check-schema` استفاده می‌شود. `bitcoin_aml schema` نیز همان
مسیر نصب را اجرا می‌کند.

SQL داخل migrations واقعاً در نصب مصرف می‌شود و از یک قرارداد مشترک تولید
می‌شود؛ فایل نمایشی یا schema مستقل نیست. انطباق کامل پیش از DDL بررسی
می‌شود. سایر adapterها همان DDL را از قرارداد مشترک می‌گیرند. این تغییر schema
نسخهٔ ۲ یا migration تازه‌ای ایجاد نکرده و نسخهٔ ۱ را تغییر نداده است.

## مسیرهای فعال چهار شبکه

| شبکه | init/check واقعی | writer دادهٔ پایه | reader واقعی | نتیجه |
|---|---|---|---|---|
| TRON | `src/db/tron_schema.rs`؛ DDL قدیمی + baseline مشترک | `src/services/tron/batcher/impls.rs`؛ جدول‌های `transactions`، relationships و semantic قدیمی | سرویس‌های investigation/analytical/exposure از viewهای بدون `aml_` | انتقال runtime به قرارداد مشترک باز است |
| Ethereum | `src/db/migrations.rs`؛ migrationهای قدیمی + baseline مشترک | `src/storage/writer.rs`؛ `transactions`، `ingested_blocks` و semantic قدیمی | `src/investigation.rs` و holdings؛ viewهای قدیمی | انتقال runtime باز است؛ تغییرات آزمایشی قبلی همچنان خارج از مسیر فعال‌اند |
| BSC | `src/db/migrations.rs`؛ migrationهای قدیمی + baseline مشترک | `src/ingestion/store.rs`؛ جدول‌های قدیمی | `src/investigation/wallet.rs` و ingestion service؛ `transactions_canonical` قدیمی | انتقال runtime باز است |
| Bitcoin | `src/db/migrations.rs`؛ SQL فعال از قرارداد مشترک | `src/storage/writer.rs`؛ facts و fence مشترک `aml_*` | `src/db/queries.rs`، `src/db/warehouse.rs` و API؛ viewهای `aml_*_canonical` | دادهٔ پایه و failure منتقل شده‌اند؛ جدول‌های تحلیلی هنوز writer کامل ندارند |

مرجع جزئیات ستون/engine/view قدیمی، [موجودی DDL](legacy-inventory.json) است.
نام شبکه در جدول بالا نسبت به پوشهٔ همان adapter است. این بررسی مسیرهای دادهٔ
پایه را تأیید می‌کند؛ ممیزی تمام queryهای ساخته‌شده پویا و همه workerها برای
بستن کامل DB-01 باقی است.

## چه چیزی واقعاً یکسان است؟

۲۹ جدول و ۲۰ view در namespace مشترک `aml_*`، انواع، کلیدها، partition، index
و checksum تعریف مشترک دارند. تست مشترک شکل ستون‌های هر چهار دیتابیس موقت
را مقایسه می‌کند و drift را رد می‌کند. این با یکسان‌بودن **تمام اشیای فعلی**
چهار دیتابیس فرق دارد: سه adapter هنوز اشیای قدیمی خود را نصب و مصرف می‌کنند.
بنابراین schema کامل و مسیر فعال هر چهار شبکه هنوز یکسان نشده است.

مسیر JSON قرارداد و checksum ثابت است:
`shared/warehouse/schema/0001_warehouse.json`؛
`147ccdadef5b7c195af6dff3cc1d576fbc882a9f6912d99862e3d6919c1f6224`.
هر ClickHouse/database/volume همچنان روی VM مستقل همان شبکه است.

## ترتیب اصلاح‌شدهٔ ادامه

۱. DB-01: تکمیل فرهنگ داده و مالکیت writer/reader برای همه خانواده‌ها؛ مشخص‌کردن
نگاشت هر schema قدیمی به قرارداد هدف و نیازهای نامپوشش.

۲. DB-03 تا DB-07: استانداردکردن نوع/واحد/معنا و قرارداد canonical در مسیرهای
فعال؛ مدل UTXO باید چندورودی/چندخروجی را با شواهد outpoint حفظ کند.

۳. DB-08 و DB-09: backfill قابل‌ادامه، مقایسهٔ شمارش/مبالغ/hash/revision و
نتیجهٔ query، آزمون restore، سپس تغییر reader/writer و تنظیمات نگهداری. مسیر
قدیمی و volume پیش از اثبات تطبیق بازنشسته نمی‌شوند.

۴. پس از معیار پذیرش P0، ادامهٔ ING-04 و زنجیرهٔ AML. صرف ایجاد جدول، تغییر
نام پوشه یا فایل SQL، هیچ‌یک از مراحل بالا را به‌تنهایی نمی‌بندد.

وضعیت اجرای آزمون همین اصلاح در [DB_STRUCTURE_VALIDATION.md](DB_STRUCTURE_VALIDATION.md)
ثبت می‌شود؛ این بازبینی rollout یا migration دیتابیس مستقر نیست.
