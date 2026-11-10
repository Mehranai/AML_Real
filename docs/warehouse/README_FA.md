# قرارداد مشترک ClickHouse

منبع DDL، [کتابخانهٔ Rust](../../shared/warehouse/README.md) است. قرارداد
فعلی نسخهٔ **۶** با **۴۲ جدول، ۵۶ view و ۷۳۹ ستون** است؛ نام database و
هویت شبکه متغیر deployment هستند. هر شبکه ClickHouse و volume خودش را
دارد؛ data warehouse یا ClickHouse مرکزی دیگری ایجاد نمی‌شود.

ستون، نوع، default، constraint، کلید، engine، index و SELECT کامل viewها در
[current-contract.json](current-contract.json) قرار دارند. این فایل با
`export_sql --contract-json` تولید می‌شود و source مستقل قابل ویرایش نیست.
مستندات مرحله‌ای و نگاشت دیتابیس قدیمی حذف شده‌اند.

**وضعیت فعال:** writer بلاک هر چهار شبکه از مسیر مشترک استفاده می‌کند.
readerهای ترون projection محلی روی همان قرارداد دارند؛ metadata/intelligence،
خوشه‌بندی و exposure نیز از جدول‌های مشترک استفاده می‌کنند. init/check برنامه
فقط قرارداد مشترک را می‌خواهد. فایل‌ها و mountهای قدیمی DDL حذف شده‌اند؛
[SQL تولیدشده](../../shared/warehouse/sql/README.md) فقط یک نسخهٔ مشترک دارد.
نصب واقعی چهار initializer روی چهار ClickHouse مستقل، شکل یکسان را تأیید
کرده است. پذیرش مسیر دادهٔ هر چهار شبکه با ۱۳ سناریوی ClickHouse واقعی و
RPC نمونه موفق شد؛ پذیرش RPC/VM واقعی در [TODO_LIST](../../TODO_LIST.md) باز است.

## قرارداد اجرایی writer و reader

ورودی هر بلاک، header مشترک و batch رکوردهای همان بلاک است. در
[`core::publish_observed_batch`](../../shared/warehouse/src/core.rs) ابتدا تمام
batch اعتبارسنجی می‌شود: تعداد و موقعیت تراکنش‌ها از صفر پیوسته است، شناسهٔ
ذخیره‌سازی تکراری پذیرفته نمی‌شود، هر fact به تراکنش موجود همان بلاک و
موقعیت همان تراکنش اشاره می‌کند و پروفایل تخصصی نامرتبط خالی می‌ماند.
مبلغ خام، token ID، موقعیت log/trace و coverage با قواعد مشترک بررسی می‌شوند.

سپس revision رزرو، facts نوشته و **تمام ستون‌ها** از دیتابیس دوباره خوانده و
با ورودی مقایسه می‌شوند. فقط پس از تطبیق، complete marker و checkpoint
منتشر می‌شوند؛ اختلاف مبلغ با تعداد ردیف یکسان نیز انتشار را متوقف می‌کند.
ارتفاع مشاهده‌شدهٔ RPC مستقل از آخرین بلاک دریافت‌شده حفظ می‌شود.
`core::read` با همان تعریف ستون‌ها، اعداد دقیق و checkpoint ثابت می‌خواند.

writer باید مالک انحصاری cursor باشد و revision تازه را از
`publication::next_revision` بگیرد. این کنترل‌ها جای lock نمی‌گیرند.
[writer فعال بیت‌کوین](../../dockerizd_bitcoin/src/storage/writer.rs) این مسیر را
مصرف می‌کند؛ کنترل خاص UTXO/prevout و جمع مبلغ آن نیز حفظ شده است.
BSC نیز writer، replay/repair و reader/API را به همین قرارداد متصل کرده
است؛ rollback شاخه، صف metadata و intelligence/review از مسیر مشترک
استفاده می‌کنند. انتشار بلاک TRON نیز متصل است؛ رکوردهای عملیاتی آن نیز مشترک شده‌اند. در Ethereum همین
قرارداد فعال است؛ `refresh_observed_batch` پوشش trace بلاک قدیمی همان شاخه
را بدون عقب‌بردن tip تکمیل می‌کند و تغییر header یا کاهش coverage را رد
می‌کند. facts تراکنش‌محور باید tx واقعی داشته باشند؛ withdrawal اتریوم
در شاهد خام بلاک و پاسخ `block_credits` با tx hash تهی نمایش داده می‌شود.

## شاهد تخصصی TRON

نسخهٔ ۶ روی هر چهار database نصب می‌شود؛ سه جدول جدید فقط در ترون داده
می‌گیرند. indexها همان ترتیب source هستند. در internal، آرایهٔ `callValueInfo`
بدون حذف/جابه‌جایی عناصر نگه داشته می‌شود؛ rejectedِ برنگشته NULL است.
resourceِ برنگشته هم NULL است و به صفر تبدیل نمی‌شود. شاهد اصلی receipt و
تراکنش جداگانه حفظ می‌شوند.

[اعتبارسنجی source](../../dockerizd_tron/app/src/services/tron/source_evidence.rs)
در مسیر فعلی ingest، پیش از انتشار هر بلاک اجرا می‌شود: SHA256 بایت `raw_data_hex`
با `txID`، تعداد/ترتیب/نوع contract و فیلدهای مالی TransferContract،
TransferAssetContract و TriggerSmartContract با protobuf تطبیق داده می‌شوند.
پارامتر contractهای دیگر صرفاً حفظ می‌شود و پرچم تأیید پارامترشان صفر است.
این کنترل تأیید امضا، عضویت در بلاک یا اجماع نیست؛ اتصال receipt با id،
ارتفاع و درصورت وجود زمان، فقط سازگاری پاسخ RPC را بررسی می‌کند.
تعریف بایت‌ها از [پروتکل رسمی](https://github.com/tronprotocol/protocol/blob/master/core/Tron.proto)
و رابطهٔ hash از [مستند رسمی encoding](https://developers.tron.network/docs/encoding) است.

**وضعیت ترون پس از DB-4.2:** بلاک کامل ابتدا در حافظه آماده و بعد از
اعتبارسنجی، با facts و checkpoint مشترک منتشر می‌شود. batcher مستقل حذف
شده است. replay فقط شواهد بازهٔ منتشرشدهٔ همان شاخه را تازه می‌کند و tip
را جابه‌جا نمی‌کند؛ hash conflict یا gap با cursor فرضی دور زده نمی‌شود.
قفل فایل مشترک بین ingestion/replay/benchmark برقرار است.

`db/read.rs` فقط projection محلی query برای DTOهای فعلی می‌سازد و دادهٔ
دومی ذخیره نمی‌کند. APIهای تحقیق از همین facts canonical می‌خوانند.
نام‌های قدیمی در queryهای این adapterها CTE محلی روی `aml_*` هستند؛ جدول
یا view بومی جداگانه‌ای با آن نام‌ها در ClickHouse ساخته نمی‌شود.
موجودی فعلی RPC ادعا نمی‌شود: holdings ترون فعلاً netflow دقیق تاریخچهٔ
دریافت‌شده است، با `balance_incomplete=true` و decimals/مقدار انسانی NULL
درصورت ناشناخته‌بودن. metadata، صف retry/dead_letter، منبع و review برچسب،
خوشه‌بندی و exposure از جدول‌های مشترک استفاده می‌کنند. کاندیدای ساختاری
در `aml_intelligence_claims` با `claim_kind=cluster` می‌ماند؛ نتیجهٔ بازبینی‌شده
با snapshot منبع و seal در `aml_cluster_*` منتشر می‌شود. مسیر exposure
relationship/tx اصلی را حفظ می‌کند؛ `fund_continuity_claimed=0` است. تغییر
منبع/review یا revision زنجیره، نتایج وابسته را نامعتبر می‌کند. حذف فایل‌های
DDL قدیمی ترون انجام شده است؛ job مشترک schema پیش از runtime اجرا می‌شود.

## جدول‌ها

| جدول | نقش |
|---|---|
| `aml_schema_migrations` | نسخه و checksum فازهای DDL نصب‌شده |
| `aml_warehouse_identity` | اتصال این database به هویت شبکه و نسخهٔ schema |
| `aml_sync_state` | checkpoint انتشار و بازهٔ داده |
| `aml_ingested_blocks` | header، وضعیت دریافت، شمارش facts و complete marker |
| `aml_transactions` | دادهٔ پایهٔ تراکنش و وضعیت اجرای آن |
| `aml_address_relationships` | جریان مشاهده‌شده یا leg نوع‌دار UTXO با منشأ و مبلغ |
| `aml_transaction_features` | feature/classification نسخه‌دار هر تراکنش |
| `aml_semantic_aml_events` | تفسیر decoder با confidence و شواهد قابل بررسی |
| `aml_asset_metadata` | نام، نماد، decimals و provenance دارایی |
| `aml_metadata_jobs` | صف و وضعیت دریافت metadata |
| `aml_metadata_discoveries` | مشاهدهٔ دارایی از شاهد اصلی بلاک/تراکنش |
| `aml_ingestion_failures` | تلاش‌ها، خطا، retry/dead-letter و نتیجهٔ repair |
| `aml_ingestion_benchmarks` | سنجش نرخ و هزینهٔ دریافت |
| `aml_intelligence_sources` | منبع اطلاعات هویتی/ریسک و وضعیت آن |
| `aml_intelligence_claims` | ادعای برچسب یا انتساب با source و نسخه |
| `aml_intelligence_reviews` | تصمیم review/revoke و شواهد بازبینی |
| `aml_entities` | کاتالوگ entity؛ نام entity اثبات مالکیت نیست |
| `aml_entity_addresses` | رابطهٔ آدرس با entity و claim پشتیبان |
| `aml_protocol_registry` | هویت و decoder/version قراردادهای پروتکل |
| `aml_cluster_claims` | کاندیدای رابطهٔ خوشه با شواهد |
| `aml_cluster_memberships` | عضویت وابسته به claim/run معتبر |
| `aml_cluster_runs` | وضعیت و محدودهٔ اجرای detector خوشه |
| `aml_cluster_versions` | نسخه و منشأ تعریف خوشه |
| `aml_exposure_seeds` | seed ریسک وابسته به intelligence معتبر |
| `aml_exposure_runs` | پارامتر و snapshot محاسبهٔ exposure |
| `aml_exposure_paths` | مسیر، تخصیص مبلغ و شواهد exposure |
| `aml_address_exposures` | جمع‌بندی آدرس وابسته به run و مسیرها |
| `aml_risk_policies` | تعریف نسخهٔ policy امتیازدهی |
| `aml_risk_policy_rules` | قواعد و وزن‌های همان policy |
| `aml_wallet_risk_signals` | دلیل مستقل ارزیابی ریسک و reference شاهد |
| `aml_wallet_risk_assessments` | نتیجهٔ ارزیابی، policy و snapshot ورودی |
| `aml_bitcoin_inputs` | vin، prevout و ارزش صحیح ورودی Bitcoin |
| `aml_bitcoin_outputs` | vout، ارزش، coinbase و script مقصد |
| `aml_bitcoin_scripts` | اطلاعات تخصصی script و منشأ آدرس نامعلوم |
| `aml_evm_logs` | log اصلی Ethereum/BSC با index/topic/data |
| `aml_evm_traces` | شاهد trace و مسیر فراخوانی EVM |
| `aml_tron_contracts` | نوع/ترتیب contract، بایت اصلی Any و وضعیت محدود تطبیق protobuf/JSON |
| `aml_tron_logs` | emitter، topic/data و موقعیت واقعی log در receipt ترون |
| `aml_tron_internals` | موقعیت internal، caller/target، rejected و آرایهٔ اصلی callValueInfo |
| `aml_tron_resources` | هزینه و مصرف Energy/Bandwidth؛ NULL برای فیلد برنگشته |
| `aml_raw_transactions` | payload/receipt اصلی برای audit و decoder |
| `aml_analysis_publications` | seal انتشار خروجی تحلیلی و وابستگی به source |

جدول تخصصی نامرتبط برای یک شبکه خالی است. Bitcoin sender واحد ساختگی،
مبلغ جمعی مبهم یا یال دلخواه input→output نمی‌گیرد. Ethereum withdrawal
بلاک‌محور در شاهد بلاک حفظ شده است؛ تفاوت‌های خاص TRON نیز باید در اتصال
آن صریحاً پوشش داده شوند. گپ با tx hash یا position فرضی پنهان نمی‌شود.

## قواعد داده و view

شناسهٔ شبکه ثابت، زمان UTC میلی‌ثانیه، hash و آدرس canonical، مبلغ خام صحیح
و token ID دقیق هستند. مبلغ در API با رشته منتقل می‌شود تا JavaScript
precision را از دست ندهد. BTC/ساتوشی decimals=8، ETH/BNB=18 و TRX=6 است؛
decimals دارایی ناشناخته حدس زده نمی‌شود.

fact خام ممکن است unpublished باشد. reader عادی از viewهای canonical/
current/visible/active و snapshot معتبر می‌خواند؛ `ReplacingMergeTree` به
تنهایی مرز انتشار بلاک یا تازگی تحلیل را تضمین نمی‌کند. آخرین revision و
review پیش از فیلتر approval انتخاب می‌شوند تا revoke پنهان نشود.

نسخه‌های DDL ۱ تا ۵ و checksum آن‌ها ثابت مانده‌اند. migration تازهٔ
`20261106_0006_tron_evidence` فقط کمبود تخصصی ترون را تکمیل می‌کند؛ با
نسخه‌های ۶ تا ۱۰ِ حذف‌شدهٔ مخصوص تاریخچهٔ قبلی متفاوت است. database آزمایشی قدیمی
downgrade نمی‌شود؛ نصب تازه volume تازه می‌خواهد. backup دادهٔ تولیدشده
پس از راه‌اندازی قابلیت عملیاتی جدا و حفظ‌شده است.

برای هر role: `bash scripts/vm.sh ROLE check-schema`.
ساخت موفق schema به معنی انتقال کامل IO یا پذیرش AML نیست.
[ممیزی](../PROJECT_STATUS_FA.md)، [VM](../VM_SETUP_FA.md)، [تست](../TESTING.md).

[آموزش خواندن داده و Neo4j](../DATABASE_GUIDE_FA.md) · [فرهنگ همهٔ ستون‌ها و viewها](DATA_DICTIONARY_FA.md)
