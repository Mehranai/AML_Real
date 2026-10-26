# DB-03 — قرارداد ستون‌ها و مقادیر، نسخهٔ ۲

این مرحله قواعد مقدار را در schema مشترک و مسیر درج Rust اعمال می‌کند. ستون‌ها،
۲۹ جدول و ۲۰ view نسخهٔ ۱ حفظ می‌شوند؛ نسخهٔ ۲ به هر جدول یک CHECK مشترک و
نسخه‌دار اضافه می‌کند. دیتابیس و ClickHouse هر شبکه همچنان روی VM همان شبکه
مستقل است. انتقال جدول‌های قدیمی و writer/reader سه شبکه در DB-04 تا DB-08
ادامه دارد؛ نصب این ارتقا به معنی پایان آن انتقال نیست.

## منبع قرارداد و مسیر اجرا

- [تعریف پایهٔ تغییرنکرده](../../shared/warehouse/schema/0001_warehouse.json) ستون‌ها، انواع، engine، کلیدها و viewها را تعریف می‌کند.
- [تعریف قواعد نسخهٔ ۲](../../shared/warehouse/schema/0002_column_contract.json) شامل ۲۱۷ قاعدهٔ ستون و checksum پایه است.
- `current_contract()` قرارداد جاری نسخهٔ ۲ را از پایه و قواعد می‌سازد؛ `baseline_contract()` همان تعریف تاریخی نسخهٔ ۱ است. قیدهای SQL و validator ردیف Rust از همین قواعد ساخته می‌شوند.
- بیت‌کوین هر دو SQL نسخه‌دار را در `sql/migrations` embed و پیش از DDL کنترل می‌کند. schema job هر چهار adapter همان ارتقا را از بستهٔ مشترک می‌گیرد.
- درج مشترک ابتدا نوع و قواعد **تمام ردیف‌ها** را بررسی می‌کند؛ سپس اتصال/هویت شبکه و batch insert انجام می‌شوند. خطای ردیف آخر یک ورودی چند batch، قبل از درج batch اول رد می‌شود.

## قالب و معنای مشترک

| داده | قرارداد جاری |
|---|---|
| شبکه | فقط `tron:mainnet`، `eip155:1`، `eip155:56` و `bip122:000000000019d6689c085ae165831e93`؛ database به هویت نصب‌شدهٔ همان شبکه متصل است |
| hash | Bitcoin/TRON: ۶۴ رقم hex کوچک؛ Ethereum/BSC: پیشوند `0x` و ۶۴ رقم hex کوچک؛ parent خالی فقط برای genesis و checkpoint خالی فقط در حالت uninitialized مجاز است |
| ارتفاع، ترتیب و زمان | ارتفاع UInt64، ترتیب tx/event/vin/vout برابر UInt32 و زمان Unix میلی‌ثانیه UInt64؛ نوع و بازهٔ عدد کنترل می‌شود. واحد از بزرگی عدد حدس زده نمی‌شود؛ تبدیل ثانیه به میلی‌ثانیه صریح و با کنترل overflow است |
| revision و commit | revision مثبت UInt64؛ allocation انتشار همچنان از revision نوشته‌شده/منتشرشده جلوتر می‌رود. identity تازه حتی با عقب‌رفتن ساعت باید revision بزرگ‌تر از قبلی داشته باشد؛ commit خالی مجاز نیست |
| مبلغ | UInt256 در ClickHouse؛ در JSON درج Rust فقط رشتهٔ صحیح دهدهی canonical. float، notation علمی، عدد JSON برای UInt256، overflow و صفرهای ابتدایی رد می‌شوند. تبدیل واحد اعشاری کاملاً رشته‌ای و بدون گردکردن است |
| دارایی بومی | `native:TRX` با decimals=6، `native:ETH` و `native:BNB` با decimals=18 و `native:BTC` با decimals=8؛ دارایی بومی شبکهٔ دیگر یا decimals اشتباه رد می‌شود |
| توکن و NFT | استاندارد سازگار با شبکه و contract canonical؛ شناسهٔ توکن NFT در asset ID انتقال حفظ می‌شود. collection بدون token ID برای metadata قابل نمایش است و به‌عنوان انتقال یک NFT پذیرفته نمی‌شود |
| confidence و score | confidence/amount share/exposure score در بازهٔ ۰ تا ۱؛ score/contribution/threshold در بازهٔ ۰ تا ۱۰۰؛ NaN/Infinity پذیرفته نمی‌شوند. مقدار NULL فقط در ستون nullable مجاز است |
| review | `pending/approved/rejected/revoked`؛ وضعیت `verified` به approval تبدیل نمی‌شود. review غیر pending به شناسهٔ reviewer نیاز دارد. احراز و workflow آن در AML-01 تکمیل می‌شود. helper تبدیل صریح نام‌های قدیمی با validator درج canonical جدا است |
| شواهد | reference دارای namespace مانند `source:...`، `tx:<hash>` یا `outpoint:<tx_hash>:<index>`؛ hash و index outpoint معتبر و canonical، بدون control character؛ شبکه از ستون network_id و زمینهٔ ردیف می‌آید |
| payload | JSON معتبر برای ستون‌های JSON و hex کوچک با تعداد زوج رقم برای script؛ مقدار اصلی با validator بازنویسی نمی‌شود |

`AssetId::parse_legacy` تبدیل صریح قالب قدیمی را فراهم می‌کند: برای مثال
`eip155:56/native:bnb` به دارایی بومی BSC، و TRON contract با Base58Check معتبر
به hex شبکهٔ `41...`. `AssetId::parse` فقط مقدار از قبل canonical را قبول می‌کند.
ترکیب `network_id + asset_id` هویت کامل دارایی است؛ full canonical key نیز از
همین دو ساخته می‌شود. token ID صحیح UInt256 است و مقدارش با float محاسبه نمی‌شود.

CHECKهای دیتابیس روی **مقدار ذخیره‌شده** کار می‌کنند. نوع اصلیِ ورودی که یک
دستور SQL دلخواه پیش‌تر cast کرده باشد قابل بازیابی نیست؛ به همین دلیل رد
float/عدد JSON برای مبلغ خام در مرز درج Rust هم اجرا می‌شود. writerهای پشتیبانی‌شده
باید از این مسیر استفاده کنند.

## Bitcoin و nullable

تراکنش چندورودی/چندخروجی Bitcoin، یک انتقال تک‌فرستنده نیست:
`from_address`، `to_address` و `amount_raw` تراکنش NULL می‌مانند. مبلغ دقیق روی
vin/vout و legهای رابطه ذخیره می‌شود؛ fee بر حسب ساتوشی است و input/output
count باید معلوم باشد. مقدار خام، fee و UTXO از سقف عرضهٔ Bitcoin عبور نمی‌کنند.

پوشش receipt و trace در Bitcoin باید NULL باشد؛ پوشش prevout در شبکه‌های
غیر UTXO باید NULL باشد. جدول‌های raw ورودی/خروجی فقط برای Bitcoin، log فقط
برای Ethereum/BSC و contract خام TRON فقط برای TRON رکورد می‌پذیرند. جدول در
schema مشترک همه نصب می‌شود و دادهٔ نامرتبط ساخته نمی‌شود. صفر و false جای
NULL مربوط به `not_applicable` را نمی‌گیرند.

## ارتقای قابل بررسی

۱. نام database، تاریخچهٔ هر دو نسخه، checksum پایه، هویت شبکه و شکل فعلی
جدول/view بررسی می‌شوند. شیء گم‌شده در baseline ثبت‌شده، با ساخت جدول خالی
جایگزین نمی‌شود. constraint اضافه یا دست‌کاری‌شده، drift است.

۲. اگر نسخهٔ ۲ ثبت نشده باشد، ردیف‌های موجود در جدول‌های مشترک با قواعد جدید
بررسی می‌شوند. سقف هر query برابر ۶۰ ثانیه و دو thread است. خطا/timeout یا دادهٔ
ناسازگار، قبل از ALTER و ثبت نسخهٔ جدید، ارتقا را متوقف می‌کند. دادهٔ قدیمی
حذف، اصلاح خودکار، صفرگذاری یا تأیید خودکار نمی‌شود؛ تبدیل/حفظ شواهد باید در
مسیر مهاجرت انجام شود.

۳. برای نصب تازه، baseline نصب و تاریخچهٔ نسخهٔ ۱ حفظ می‌شود؛ سپس ALTERهای
نسخهٔ ۲ اجرا و صحت تمام قیدها دوباره کنترل می‌شوند. رکورد migration نسخهٔ ۲
اضافه و identity با revision تازه ثبت می‌شود. ستون و view قبلی تغییر نمی‌کند.

۴. ارتقای نیمه‌تمام، فقط در صورت تطبیق قیدهای موجود با قرارداد ادامه می‌یابد.
نسخهٔ ۲ ثبت‌شده با قید حذف‌شده/متفاوت، خودکار ترمیم و معتبر اعلام نمی‌شود.
اجرای مجدد نصب کامل، قیدها یا تاریخچه را بازنویسی نمی‌کند.

`check-schema` و `--check` فقط SELECT/EXPLAIN هستند. دیتابیس نسخهٔ ۱ برای
استفاده از برنامهٔ جدید به اجرای schema job نیاز دارد؛ فرمان check خودش ارتقا
انجام نمی‌دهد. باینری قدیمی نیز نسخهٔ ۲ ناشناخته را قبول نمی‌کند؛ rollback
عملیاتی و restore همچنان در DB-08 آزمون می‌شوند.

## SQL و فرمان‌ها

```bash
# Produce/check the unchanged v1 artifact:
cargo run --locked --manifest-path shared/warehouse/Cargo.toml --example export_sql -- \
  --check dockerizd_bitcoin/sql/migrations/20261022_0001_shared_warehouse.sql
# Produce/check the v2 delta:
cargo run --locked --manifest-path shared/warehouse/Cargo.toml --example export_sql -- \
  --upgrade --check dockerizd_bitcoin/sql/migrations/20261026_0002_column_contract.sql

bitcoin_init_schema --print-sql  # v1 baseline followed by v2 ALTERs; no settings needed
bitcoin_init_schema             # under the existing shared Bitcoin writer lock
bitcoin_init_schema --check
bash scripts/vm.sh bitcoin check-schema
```

خروجی SQL برای بازبینی و بسته‌بندی است؛ اجرای دستی آن جای schema job و کنترل
ledger، identity، preflight و قفل را نمی‌گیرد. معماری و تنظیمات اتصال VMها عوض
نشده‌اند. نتیجهٔ آزمون‌ها در [DB03_VALIDATION.md](DB03_VALIDATION.md) ثبت می‌شود.

مرحلهٔ بعد DB-04 است: هماهنگ‌کردن جدول‌ها/viewهای دادهٔ پایه و مسیرهای مصرف
چهار شبکه. نگاشت و حفظ فیلدهای نامپوشش، جدول‌های تحلیلی و backfill نهایی در
DB-05 تا DB-08 باقی است. ING-04 همچنان پس از مسیر DB قرار دارد.
