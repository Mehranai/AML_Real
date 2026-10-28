# DB-05 — قرارداد تحلیلی و داده‌های تخصصی

تعریف این گام در [نسخهٔ ۴ ثابت](analytics-contract-v4.json) حفظ شده است؛
قرارداد جاری با [DB-07](DB07_VISIBILITY_FA.md) به نسخهٔ ۵ ارتقا یافته است.

نسخهٔ ۴، ساختار مقصد مشترک را به **۳۸ جدول، ۴۰ view و ۶۶۸ ستون جدول** می‌رساند: ۹ جدول جدید، ۱۱۰ ستون تکمیلی در ۱۶ جدول موجود و ۱۹ view جدید. تمام تعریف‌ها در [migration مشترک](../../shared/warehouse/schema/0004_analytics_specialized.json) هستند؛ [SQL قابل بررسی بیت‌کوین](../../dockerizd_bitcoin/sql/migrations/20261027_0004_analytics_specialized.sql) از همان تعریف تولید و پیش از هر DDL تطبیق داده می‌شود. هر چهار شبکه همان قرارداد را در schema job مصرف می‌کنند و ClickHouse/database/volume هر VM مستقل می‌ماند.

## خانواده‌های تکمیل‌شده در schema

| خانواده | تغییر |
|---|---|
| دارایی و job | supply دقیق UInt256، code hash، وضعیت metadata، verification و زمان‌های nullable؛ کشف دارایی در `aml_metadata_discoveries` از صف job جداست و به commit معتبر وابسته است |
| protocol registry | `aml_protocol_registry` با source/reference، confidence، review، decoder config و هویت شبکهٔ دور؛ config قدیمی مانند method/event/topic index در JSON اصلی حفظ می‌شود |
| intelligence | source metadata و شواهد اصلی، source record، case، submitter، supersession، زمان review و revision موضوع review |
| entity و exchange | `aml_entities` و `aml_entity_addresses`؛ projection تأییدشده و view صرافی از claim تأییدشدهٔ فعال ساخته می‌شوند؛ نبود provenance با کپی projection قدیمی جبران نمی‌شود |
| clustering | هر دو سمت pair claim و same_entity nullable، run، reviewer/source و supersession؛ جزئیات membership و catalog مستقل `aml_cluster_versions` |
| exposure | seed provenance و snapshot مجموعهٔ seedها، پارامترها و محدودیت run، time weight، service boundary، continuity و allocation evidence؛ `aml_address_exposures` aggregate قدیمی را جدا نگه می‌دارد و مسیر یا مبلغ ساختگی نمی‌سازد |
| risk | تنظیمات policy/rule، strength سیگنال، reasons/evidence، زمان as-of و روش scoring؛ احتمال با score یکی نمی‌شود |
| تخصصی شبکه | `aml_bitcoin_scripts` کنار input/output/outpoint موجود؛ `aml_evm_traces` با trace path مرتب؛ `aml_raw_transactions` برای envelope اصلی و receipt nullable؛ log و contract خام موجود حفظ می‌شوند |

در تمام جدول‌های تکمیل‌شده، `raw_evidence_json` محل حفظ بدون اتلافِ نام/نوع/مقدار اصلی است؛ JSON قبلی باید داخل فضای نام نسخه‌دار نگه داشته شود. پیش‌فرض `{}` نشانهٔ بازیابی شواهد گذشته نیست. verification، confidence، زمان‌ها و flagهایی که قبلاً وجود نداشتند nullable باقی می‌مانند؛ schema اطلاعات نامعلوم را صفر یا تأییدشده نمی‌کند.

Bitcoin script و EVM trace در کل بستهٔ schema حضور دارند، ولی constraint شبکه درج دادهٔ نامرتبط را رد می‌کند. برای بیت‌کوین trace یا receipt ساختگی تولید نمی‌شود. این گام قرارداد ذخیره و خواندن است؛ producer جدید script/trace و تحلیل‌های AML در مراحل بعدی پیاده می‌شوند.

## قرارداد خواندن و منشأ تأیید

`shared/warehouse/src/analytics.rs` reader محدود با `network_id` اجباری، مقدار UInt256 دقیق و انتخاب صریح `Current` یا projection پشتیبانی‌شدهٔ `Approved` دارد. درج عمومی فقط جدول‌های تحلیلی را می‌پذیرد؛ factهای تخصصی باید از مسیر انتشار بلاک عبور کنند.

viewهای current برای review، membership، path، policy/rule و signal اضافه شدند. current فقط آخرین رکورد ذخیره‌شده است؛ تازگی run و اعتبار snapshot پس از reorg در DB-07 کنترل می‌شود. وجود current به معنی معتبر یا تازه‌بودن تحلیل نیست.

viewهای تأییدشدهٔ claim، protocol و entity/exchange، بعد از انتخاب آخرین revision فیلتر می‌کنند. لغو تأیید، غیرفعال‌شدن source یا پایان بازهٔ اعتبار claim نباید نسخهٔ قدیمیِ تأییدشده را زنده کند. metadata verification جای تأیید analyst نیست. تاریخچهٔ review و supersession مستقل حفظ می‌شود.

این ترتیب در viewهای تأییدشده با `argMax(tuple(...), revision)` پیش از فیلتر وضعیت برقرار شده است. تکیهٔ صرف بر `FINAL` در این joinها کافی نبود؛ تست ClickHouse 23.8 بازگشت رکورد قدیمی پس از غیرفعال‌شدن source را آشکار کرد و با این ترتیب رفع شد.

## ارتقا و نگاشت تاریخچه

نسخه‌های ۱، ۲ و ۳ و SQL/checksum آن‌ها دست‌نخورده‌اند. نصب تازه و ارتقا به‌ترتیب هر چهار ledger را ثبت می‌کند. قطع میانهٔ افزودن ستون، ساخت جدول و نصب view قابل ادامه است؛ شکل ناقص یا تغییریافتهٔ جدول جدید هم مانند drift جدول قدیمی رد می‌شود. هیچ DROP/TRUNCATE، پاک‌کردن volume یا تبدیل خودکار دادهٔ قدیمی وجود ندارد.

فرهنگ داده، snapshot تاریخی نسخهٔ ۱ و تعریف ثابت نسخهٔ ۳ را جدا نگه می‌دارد. در [data-dictionary.json](data-dictionary.json)، `target_analytics_contract` و `target_analytics_mapping` هدف نسخهٔ ۴ و برنامهٔ حفظ فیلدهای اصلی را نشان می‌دهند. ستون مبدأ بدون candidate typed هم در JSON اصلی حفظ می‌شود؛ این برنامه، backfill اجراشده نیست. hash/ترتیب/commit یا reviewer مفقود باید از شواهد معتبر تأمین شوند و تا آن زمان source قدیمی باقی می‌ماند.

ساختارهای مرکزی Analytical Node و تاریخچهٔ ML قدیمی TRON در محل فعلی حفظ می‌شوند؛ این گام ML به بیت‌کوین اضافه نمی‌کند. اختلاف‌های exporter قدیمی همچنان در `source_schema_conflicts` ثبت‌اند و نباید با اجرای SQL ناسازگار پنهان شوند.

## محدوده و ادامه

DB-05 در محدودهٔ **قرارداد مشترک تحلیلی/تخصصی، migration و readerهای سازگار** است. فعال‌کردن تمام writer/readerهای legacy، انتقال تاریخچه و restore در DB-08 و worker/APIهای واقعی AML بیت‌کوین در AML-01 تا AML-08 باقی‌اند. هیچ ادعای برابری کامل دیتابیس فعال یا پایان P0 نمی‌شود.

مرحلهٔ بعد **DB-06: معنای رابطه‌های مشترک و UTXO** است. canonical/invalidation در DB-07 و مهاجرت نهایی در DB-08 دنبال می‌شوند؛ ING-04 هنوز جلو نرفته است. دامنه و نتیجهٔ آزمون‌ها در [DB05_VALIDATION.md](DB05_VALIDATION.md) ثبت می‌شود.
