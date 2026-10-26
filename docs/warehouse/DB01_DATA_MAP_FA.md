# DB-01 — فرهنگ داده و نقشهٔ مهاجرت — ۲۰۲۶-۱۰-۲۶

به‌روزرسانی ادامهٔ کار: [DB-03](DB03_COLUMN_CONTRACT_FA.md) با قرارداد مقدار
نسخهٔ ۲ و مسیر ارتقا تکمیل شده است؛ مرحلهٔ جاری بعدی DB-04 است. متن زیر
نتیجه و طرح نگاشت DB-01 را نگه می‌دارد.

DB-01 در محدودهٔ **شناخت ساختار، مالکیت کد و طراحی نگاشت** تکمیل شد. این خروجی
مبنای اجرای DB-03 تا DB-09 است؛ انتقال writer/reader یا یکسان‌شدن کامل دیتابیس
فعال چهار شبکه را اعلام نمی‌کند. هر شبکه همچنان ClickHouse، database و volume
مستقل روی VM خودش دارد.

برابری ساختار مشترک روی چهار database موقت و ۶۱ تست Python پاس شدند؛ دامنهٔ
دقیق و محدودیت‌ها در [گزارش اعتبارسنجی DB-01](DB01_VALIDATION.md) آمده‌اند.

## خروجی قابل بررسی

| خروجی | محتوا |
|---|---|
| [snapshot ساختار واقعی](db01-schema-snapshot.json) | نتیجهٔ `system.tables` و `system.columns` روی ClickHouse موقت 23.8.16.16؛ DDL فعلی، دو ledger ساخته‌شده در کد و baseline مشترک |
| [فرهنگ داده و نگاشت ماشینی](data-dictionary.json) | همهٔ ستون‌ها، نوع/default/codec، واحد، کلیدها، partition، index، TTL، تعریف view، وابستگی SQL، مقصد پیشنهادی و دادهٔ نیازمند حفظ |
| [TRON](dictionary/tron.md) | ۳۷ شیء و ۴۵۳ ستون قدیمی |
| [Ethereum](dictionary/ethereum.md) | ۳۹ شیء و ۵۷۵ ستون قدیمی، شامل ledger زمان اجرا |
| [BSC](dictionary/bsc.md) | ۳۰ شیء و ۵۱۱ ستون قدیمی، شامل ledger زمان اجرا |
| [Bitcoin](dictionary/bitcoin.md) | ۱۱ شیء و ۱۱۳ ستون قدیمی؛ فقط منبع backfill، نه مسیر نصب تازه |
| [قرارداد مشترک](dictionary/common.md) | ۲۹ جدول با ۳۷۶ ستون و ۲۰ view با ۲۸۵ ستون خروجی؛ وضعیت مصرف جداگانه برای هر شبکه |

در مجموع **۱۱۷ شیء و ۱۶۵۲ ستون قدیمی** نگاشت شدند. شمارش ستون viewها جدا از
جدول است؛ این اعداد تعداد ویژگی‌های مستقل یا ردیف‌های داده نیستند. snapshot
جدید جای موجودی تاریخی قبلی را بازنویسی نمی‌کند. ledgerهای قدیمی Ethereum/BSC
چهار ستون `migration_id/description/checksum/applied_at` دارند و نباید به تاریخچهٔ
`aml_schema_migrations` تبدیل یا با آن ادغام شوند.

هر ستون یکی از وضعیت‌های صریح زیر را دارد:

- `candidate_field_mapping`: مقصد معلوم است؛ تبدیل نوع، enum، دامنه و داده باید در مرحلهٔ اجرا اعتبارسنجی شود.
- `preserve_in_versioned_json`: طرح حفظ مقدار و نوع اصلی در فضای نام نسخه‌دار JSON؛ هنوز backfill جدیدی برای سه شبکه نوشته نشده است.
- `reconstruct_from_provenance`: projection باید از claim/review معتبر بازسازی شود؛ رکورد مشتق‌شده ادعای تأییدشدهٔ جدید نمی‌سازد.
- `retain_legacy_until_replacement`: مقصد بدون اتلاف در نسخهٔ ۱ وجود ندارد یا فیلد باید با معنای متفاوت بازسازی شود؛ تاریخچهٔ قدیمی تا اجرای جایگزین و تطبیق باقی می‌ماند.

در کنار نگاشت مبدأ، `required_target_inputs` فیلدهای مقصدی را نشان می‌دهد که از
ستون‌های قدیمی تأمین نمی‌شوند؛ برای مثال hash و ترتیب اصلی تراکنش در TRON،
commit/revision تازه، پوشش nullable، reviewer واقعی و revision ورودی تحلیل.
این فیلدها با صفر، رشتهٔ خالی یا مقدار ساختگی پر نمی‌شوند. نگاشت viewها، نقشهٔ
قرارداد خواندن است؛ به معنی درج در جدول view نیست.

## مسیرهای واقعی نوشتن، خواندن و worker

مرجع هر یافته در فایل‌های فرهنگ داده، با path، line و عبارت قابل کنترل آمده
است. یافتن نام جدول در enum، allowlist یا struct، writer محسوب نمی‌شود. چهار
نوع مسیر جدا ثبت شده‌اند: SQL مستقیم، فراخوانی helper درج، binding و dispatch
عمومی، و وابستگی/trigger در DDL.

| خانواده | TRON | Ethereum | BSC | Bitcoin |
|---|---|---|---|---|
| block/fact/checkpoint | batcher binding → `GenericBatcher`؛ `ingestion_state` و `db/sync_state`؛ readerهای canonical | `EthereumStore` و helper `insert_rows`؛ investigation/holdings | `ClickHouseIngestionStore`؛ ingestion service و investigation | `map_evidence` → writer → publication مشترک؛ queryهای canonical |
| metadata/job/discovery | batcher discovery و metadata worker | extractor/store و metadata worker که store را صدا می‌زند | store، `metadata::worker_once` و binary دوره‌ای | جدول مشترک نصب است؛ worker و metadata واقعی BTC باز است |
| intelligence/review/entity | ثبت source/label/review و projection در `entity_intelligence` | CSV import، review و projection در `intelligence` | import claim نسخه‌دار؛ reader از `intelligence_active` | writer/reader تحلیلی باز است |
| cluster | candidate discovery + review/membership/version؛ analytics worker با cursor/status فایلی | `clustering`، ذخیرهٔ claim/membership/run و analytics worker | candidate هنگام درخواست؛ جدول/run پایدار cluster ندارد | writer/worker باز است |
| exposure | propagation از canonical؛ ذخیرهٔ aggregate و run توسط task؛ analytics worker | path/run پایدار، مرز سرویس و worker | exposure هنگام درخواست محاسبه می‌شود؛ path/run پایدار ندارد | writer/worker باز است |
| risk/snapshot | snapshot/evidence/subject در Analytical Node؛ inference قدیمی ML جدا | سیاست/قاعده در DDL، خواندن و ذخیرهٔ signal/assessment هنگام ارزیابی | evidence تحقیق و policy مرکزی؛ جدول خروجی ریسک محلی در DDL ندارد | شواهد واقعی AML و ارزیابی باز است |
| holdings | materialized viewها روی raw relationships، reader موجودی از canonical؛ اعتبار پس از reorg باید تطبیق شود | canonical relationships + metadata و RPC | metadata + RPC در investigation | query ورودی/خروجی canonical؛ ترتیب وقوع BIP30 حفظ می‌شود |
| failure/benchmark | ingestion state و ابزار performance | failure در store؛ benchmark فقط DDL، writer عملیاتی در source یافت نشد | store و ابزار benchmark | failure پایدار فعال؛ benchmark باز است |

این ممیزی مسیرهای کد را بررسی می‌کند؛ روشن‌بودن worker روی VM، کامل‌بودن
تاریخچه و صحت عددی خروجی زنده از وجود source نتیجه گرفته نشده‌اند. Neo4j
مصرف‌کنندهٔ گراف تحقیق در معماری فعلی می‌ماند و محل ذخیرهٔ fact چهار شبکه نیست.

## تصمیم‌های نگاشت و شکاف‌های مانع cutover

| بخش | تصمیم مشترک / کار لازم در مراحل بعد |
|---|---|
| هویت و زمان | `network_id` از هویت تأییدشدهٔ همان adapter؛ TRON timestamp میلی‌ثانیه است، Bitcoin block time ثانیه و با کنترل overflow ×۱۰۰۰ می‌شود؛ DateTime قدیمی با timezone/precision خودش حفظ شود. زمان درج، زمان بلاک و زمان انتشار جای هم قرار نگیرند. |
| مقدار | مبلغ خام UInt256 و مقدار صحیح ساتوشی؛ decimals از دارایی معتبر. `balance_decimal` شناور TRON فقط خروجی نمایش است. `output_sats` تراکنش BTC در evidence حفظ می‌شود و به `amount_raw` انتقال واحد تبدیل نمی‌شود؛ مقدار در legهای ورودی/خروجی است. |
| header و انتشار | block hash، ترتیب tx/event، commit و revision مشترک لازم است. `state_revision`، timestamp جایگزینی و revision قدیمی قابل کپی مستقیم به publication مشترک نیستند. header یا ترتیب مفقود باید با شواهد معتبر تکمیل شود. |
| raw و روابط | `token_id`، `event_sub_index`، trace position، شناسهٔ log و جزئیات finality/provider باید بدون اتلاف حفظ شوند؛ baseline ستون typed کافی برای همهٔ آن‌ها ندارد. دادهٔ خام TRON contract در DDL قدیمی کامل موجود نیست؛ payload کامل از ستون‌های خلاصه جعل نمی‌شود. |
| feature/semantic | featureهای scalar در JSON نسخه‌دار با نام/نوع اصلی؛ detector و protocol/counterparty/bridge evidence حفظ شوند. nested JSON قدیمی بازنویسی نشود. block fence و نسخهٔ detector باید reader را کنترل کنند. |
| discovery/registry | discovery با hash/revision/tx evidence، queue job نیست. قرارداد مستقل برای evidence کشف و registry پروتکلِ دارای source/review/revision لازم است؛ `aml_metadata_jobs` یا raw log جایگزین آن‌ها نیستند. |
| claim/review/entity | supersession، case/submitter، source record، review history و projectionهای entity/exchange حفظ شوند؛ `is_verified` metadata تأیید analyst نیست. نسخهٔ ۱ همهٔ ستون‌های provenance را پوشش نمی‌دهد. |
| cluster | pair claim اتریوم به membership تک‌آدرس تقلیل پیدا نکند؛ هر دو endpoint و `same_entity` در evidence حفظ شوند. catalog نسخهٔ خوشه، confidence/review و source claim عضویت به گسترش قرارداد نیاز دارند. common-input در BTC فقط candidate است. |
| exposure | aggregate TRON مسیر قابل ممیزی نیست. time weight، continuity، service boundary، allocation، محدودیت run و seed provenance باید در قرارداد هدف حفظ شوند؛ نبود path خام با ساختن مسیر جایگزین نمی‌شود. |
| risk | score از probability جدا بماند؛ policy/version، strength، reasons، evidence، as-of time و روش scoring حفظ شوند. baseline برای برخی از این موارد ستون کافی ندارد. نبود شواهد، سالم‌بودن یا امتیاز صفر نیست. |
| snapshot/ML/holdings | snapshot/subject/evidence و تاریخچهٔ ML موجود TRON در baseline مقصد کامل ندارند؛ تا تعیین قرارداد سازگار حفظ شوند. یکسانی DDL به معنی اضافه‌کردن ML به BTC نیست. delta/MV و cacheهای holdings باید از canonical با کنترل reorg بازسازی و تطبیق شوند. |
| عملیات و retention | شمارنده‌ها، زمان‌های lifecycle خطا، metrics benchmark و وضعیت retry/dead-letter حفظ شوند. TTL قدیمی یکسان نیست؛ shared v1 هیچ TTL ندارد. سیاست مشترک در DB-09 تعیین و backup/restore در DB-08 آزمون می‌شود. |

یک ناسازگاری قدیمیِ جدا هم ثبت شد: exporter آموزش TRON برای
`wallet_ml_training_runs` و ستون‌هایی از model registry/deployment، SQL می‌سازد
که در DDL فعلی موجود نیستند. SQL تولیدی اجرا نشد، آموزش اجرا نشد و runtime
TRON تغییر نکرد. جزئیات ستون‌ها در `source_schema_conflicts` ثبت شده است؛ این
موارد در طراحی حفظ تاریخچهٔ DB-05/DB-08 لحاظ می‌شوند.

**نسخهٔ اعمال‌شدهٔ ۱ تغییر نکرده است.** بستن شکاف‌ها به قرارداد/ارتقای بعدی
مشترک و سپس adapter/backfill/readers نیاز دارد. JSONهای فرهنگ داده، schema
اجرایی دوم یا اجازهٔ حذف فیلدهای بدون مقصد نیستند.

## بازتولید و کنترل تغییر

فرمان روزمره بدون شبکه یا Docker، به‌روز بودن snapshot، DDL، نگاشت، source،
anchorهای dispatch و فایل‌های تولیدشده را بررسی می‌کند:

```powershell
python scripts/warehouse_data_dictionary.py --check
python -m unittest discover -s scripts/tests -p test_warehouse_data_dictionary.py -v
```

برای تغییر صرفاً مستندات نگاشت، generator بدون `--check` اجرا و diff بازبینی
می‌شود. اگر DDL، فهرست migrationها یا تعریف ledger تغییر کرده باشد، snapshot
باید روی ClickHouse **موقت تازه و خالی** بازتولید شود؛ ابزار دیتابیس غیرخالی را
رد می‌کند، `.env` نمی‌خواند و هیچ database یا volumeای را حذف نمی‌کند:

```powershell
python scripts/warehouse_data_dictionary.py --refresh --disposable --url http://127.0.0.1:PORT
```

بعد از بازبینی و بستن DB-01، **مرحلهٔ بعد DB-03** است: تثبیت و اعمال قرارداد
ستون/نوع/واحد با پوشش شکاف‌های بالا و حفظ baseline قبلی؛ سپس DB-04 تا DB-09 و
پذیرش P0. ادامهٔ ING-04 همچنان بعد از این مسیر است.
