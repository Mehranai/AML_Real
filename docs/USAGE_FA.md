# اجرا و استفاده

ابتدا [VMها و تنظیمات](VM_SETUP_FA.md) را آماده کنید. این راهنما
رفتار نسخهٔ فعلی کد را شرح می‌دهد؛ یکسان‌سازی فعال ClickHouse پذیرفته شده
و پذیرش روی VM واقعی همچنان در [TODO_LIST](../TODO_LIST.md) باز است.

## راه‌اندازی

از ریشهٔ مخزن، روی هر VM فقط role همان VM را اجرا کنید:

```bash
bash scripts/vm.sh tron up --build
# روی VMهای دیگر به‌ترتیب ethereum، bsc، bitcoin و سپس main را جایگزین کنید.
```

راه‌اندازی بدون `--api-only`، دریافت بلاک و workerهای موجود همان شبکه را
نیز روشن می‌کند. پیش از آن RPC، بازهٔ شروع، رمز ClickHouse و service key
باید تنظیم شده باشند. برای بررسی اولیهٔ schema/API بدون شروع ingestion:

```bash
bash scripts/vm.sh bitcoin up --build --api-only
bash scripts/vm.sh bitcoin check-schema
bash scripts/vm.sh bitcoin check
```

برای شروع workerها بعد از پذیرش تنظیمات، `up` را بدون `--api-only` اجرا
کنید. `resume` سرویس‌ها را بدون restart دیتابیس روشن می‌کند. نمونه:

```bash
bash scripts/vm.sh bitcoin up
bash scripts/vm.sh bitcoin check-runtime
bash scripts/vm.sh bitcoin ps
bash scripts/vm.sh bitcoin logs
```

`check-schema` وجود و صحت قرارداد نصب‌شده را کنترل می‌کند؛ به‌تنهایی
نشان‌دهندهٔ استفادهٔ تمام reader/writerها از آن یا کامل‌بودن تاریخچه نیست.
`check-runtime` وضعیت workerهای موجود را بررسی می‌کند، نه صحت همهٔ تحلیل‌ها.

## دریافت پیوسته و قطع اتصال RPC

`up` عادی همان ingestor موجود هر VM را اجرا می‌کند. برای تشخیص حالت‌ها،
فرمان داخل image هر شبکه چنین است؛ پردازش دریافت دوم را هم‌زمان با سرویس
فعال اجرا نکنید:

| شبکه | حالت پیوسته |
|---|---|
| Bitcoin | `bitcoin_aml ingest`؛ `once` فقط یک اجرای محدود است |
| Ethereum | `ethereum_ingestor follow`؛ `--max-blocks N` اجرای محدود برای آزمون است |
| BSC | `bsc_ingest --mode follow`؛ `--max-blocks N` حد همان اجراست |
| TRON | `arz_axum_for_services` با `SYNC_MODE=auto` |

در دیتابیس خالی، start تنظیم‌شده مبناست؛ پس از انتشار داده، حالت عادی از
checkpoint ادامه می‌دهد. `--start-block` اتریوم override صریح است؛ برای
ادامهٔ معمول آن را ندهید. `SYNC_MODE=live` ترون روی دیتابیس خالی از نزدیکی
head شروع می‌کند و تاریخچهٔ کامل نمی‌سازد؛ `backfill` یک pass است و حلقهٔ
دریافت پیوسته نیست. بازهٔ شروع و دسترسی تاریخی RPC را پیش از استقرار تعیین کنید.

در قطع موقت RPC، حالت follow با تأخیر بین تلاش‌ها ادامه می‌دهد؛ خطای
بلوک، مبلغ یا منبع باعث جلو رفتن checkpoint نمی‌شود. Ethereum/BSC بعد از
اتمام retry درخواست با backoff یک تا ۶۰ ثانیه دوباره تلاش می‌کنند؛ ترون
همان سقف را برای حلقه دارد و hint محدود rate limit را رعایت می‌کند.
بیت‌کوین از poll و سیاست failure فعلی خودش استفاده می‌کند. restart از
آخرین prefix منتشرشده ادامه می‌دهد، حتی اگر pass قبلی نیمه‌تمام بوده باشد.

اشکال احراز هویت، هویت نادرست منبع یا تعارض شاهد را با restart مکرر حل
نکنید؛ endpoint/کلید و خطا را بررسی کنید. در اتریوم/BSC هویت زنجیره در
دریافت پیوسته دوباره کنترل می‌شود و endpoint جایگزین BSC پیش از header
خواندن بررسی می‌شود. تعارض finalized اتریوم یا solid ترون توقف امن دارد.
در هر چهار شبکه بودجهٔ failure برقرار است: دریافت خودکار dead-letter را
بی‌صدا باز نمی‌کند و repair صریح لازم است. یک قطعی طولانی ممکن است این
بودجه را مصرف کند؛ این محدودیت را با پذیرش عملیاتی RPC بررسی کنید.

تست این رفتار روی RPC نمونه و ClickHouse واقعی در [TESTING](TESTING.md)
ثبت شده است؛ پذیرش طولانی‌مدت RPC عملیاتی و VMهای کاربر همچنان باز است.

## آماده‌سازی خوراک تحریم OFAC

ابزار `scripts/prepare_ofac_sdn.py` فایل محلی `SDN.XML` را به ادعاهای در انتظار
بازبینی تبدیل می‌کند. فایل و خروجی را در `intelligence-data/` نگه دارید.
SHA-256 مربوط به همان نسخه را از صفحهٔ دانلود بردارید و با `--expected-sha256`
مقایسه کنید. مقدار `--license` را طبق مجوز استفادهٔ ثبت‌شدهٔ سازمان وارد کنید.
[صفحهٔ رسمی OFAC](https://ofac.treasury.gov/sanctions-list-service).

```bash
python3 scripts/prepare_ofac_sdn.py \
  --input intelligence-data/ofac/sdn.xml \
  --output-dir intelligence-data/ofac/2026-11-08 \
  --source-url 'https://sanctionslistservice.ofac.treas.gov/api/PublicationPreview/exports/SDN.XML' \
  --expected-sha256 '<SHA-256 رسمی همان فایل>' \
  --operator aml-operator --license '<مرجع مجوز استفاده>'
```

پوشهٔ مقصد باید تازه باشد. `manifest.json` در پایان نوشته می‌شود و شمارش
شبکه‌ها، هش فایل و وضعیت تطبیق آن را دارد. هر ادعا UID و نماد منتشرشده را
نگه می‌دارد. قالب‌های ناسازگار با شبکهٔ مقصد در `excluded-addresses.jsonl`
می‌آیند. آدرس BNB با قالب EVM صرفاً کاندیدای BSC است و بازبین باید شبکه را
تأیید کند. نماد ETH فقط به فایل Ethereum می‌رود.

فایل‌های خروجی به‌ترتیب `bitcoin-source.json`/`bitcoin-claims.jsonl`،
`ethereum-labels.csv`، `bsc-source.json`/`bsc-claims.jsonl` و
`tron-source.json`/`tron-labels.jsonl` هستند. وقتی بیت‌کوین بیش از ۱۰۰ ادعا
داشته باشد، فایل‌ها با شماره‌هایی مثل `bitcoin-claims-001.jsonl` تولید می‌شوند.
اگر شمارش یک شبکه صفر است، import همان شبکه را رد کنید. نمونهٔ اعتبارسنجی BSC:

```bash
bsc_intelligence source intelligence-data/ofac/2026-11-08/bsc-source.json --dry-run
bsc_intelligence import-labels intelligence-data/ofac/2026-11-08/bsc-claims.jsonl --dry-run
```

برای import اولیه، BSC و Bitcoin ابتدا source و سپس claim را با CLI ثبت
می‌کنند. در TRON از `tron_register_intelligence_source` و سپس
`tron_ingest_entity_labels` استفاده کنید. در Ethereum مقدارهای source و
`--license` را از manifest به `ethereum_ingest_entity_labels` بدهید و
`--source-type REGULATORY --trust-tier HIGH` را تعیین کنید. وضعیت pending تا
ثبت تصمیم صریح بازبین پابرجاست.

برای بررسی نسخهٔ بعدی، به همان فرمان `--previous-input مسیر-نسخه-قبلی` و
`--previous-sha256 هش-نسخه-قبلی` اضافه کنید. هر دو XML باید سربرگ کامل و
هش مورد انتظار داشته باشند. برای دو فایل متفاوت، تاریخ انتشار نسخهٔ تازه
باید بعد از نسخهٔ قبلی باشد. فایل یکسان صف خالی می‌دهد. برای نسخه‌های
متفاوتِ منتشرشده در یک روز، بررسی دستی ترتیب انتشار لازم است.

`review-delta.jsonl` پیش و پس هر ادعا را ثبت می‌کند. `added` وارد فایل
pending شبکه می‌شود. `changed` و `removed` برای تصمیم بازبین در صف می‌مانند.
`unresolved` یعنی نسخهٔ تازه برای همان UID و شبکه آدرس ناسازگار دارد و
برای تصمیم دربارهٔ حذف، شاهد بیشتری لازم است. بازبین وضعیت جاری ادعا را
با شناسه یا `source_record_id` تطبیق می‌دهد و دلیل و هش نسخهٔ مربوط را در
تصمیم ثبت می‌کند. تغییر ادعا را جداگانه به‌صورت pending ثبت و دوباره
بازبینی کنید. برای این کار خروجی نسخهٔ تازه را در پوشهٔ جدا آماده کنید و
فقط ردیف منطبق با شناسهٔ ادعا را بردارید. در Bitcoin revision جاری را
به‌عنوان `expected_revision` به ردیف تغییر اضافه کنید.

برای لغو، در Bitcoin از `bitcoin_aml intelligence review` با تصمیم
`revoked` و revision جاری استفاده کنید. در BSC فایل تک‌رکوردیِ همان claim
را با `review_status=revoked`، بازبین و دلیل به `bsc_intelligence import-labels`
بدهید. در TRON از `tron_review_intelligence ENTITY_LABEL ID REVOKED REVIEWER REASON`
استفاده کنید و هش نسخه را در دلیل بنویسید.

در Ethereum ردیف CSV همان ادعا را با `review_status=REVOKED`، بازبین، دلیل،
مراجع شاهد و ستون `expected_revision` به `ethereum_ingest_entity_labels`
بدهید. مقدار revision را به‌صورت رشتهٔ ده‌دهی از view جاری بخوانید.
برای یافتن claim از `source_id` و `source_record_id` موجود در صف استفاده کنید.
فرمان تنها ادعای تأییدشدهٔ همان نسخه را لغو می‌کند. برای بررسی نتیجه،
ردیف جاری و تاریخچهٔ تصمیم را با `aml_intelligence_claims_current` و
`aml_intelligence_reviews` بخوانید.

## تحقیق

۱. روی VM اصلی، آدرس `http://10.20.0.10:8080` را باز کنید؛ IP را با IP واقعی
Main جایگزین کنید. اگر Basic Auth فعال کرده‌اید، با حساب ساخته‌شده وارد شوید.

۲. شبکه را انتخاب و آدرس همان شبکه را وارد کنید. آدرس معتبر در یک شبکه
به‌معنی هویت همان شخص در شبکهٔ دیگر نیست.

۳. graph، activity، holdings، برچسب‌ها، exposure، رویدادها و دلایل risk را
همراه با coverage و محدودیت‌ها بخوانید. نبود نتیجه یا `UNKNOWN` به معنای
بی‌خطر بودن کیف پول نیست. مقدار خام دارایی‌ها واحد صحیح و بدون گردکردن دارد.

بخش Evidence پنل، سرنخ‌های خوشه‌بندی را جدا از ارتباط‌های بازبینی‌شده
می‌شمارد. در Bitcoin حتی سرنخ بازبینی‌شده در دستهٔ سرنخ می‌ماند. BSC از
مقصد مشترک در نمونهٔ محدود همان دارایی و بازه، سرنخ pending می‌سازد.
`analysis_evidence` در API شمار موارد مستقیم و چندمرحله‌ایِ برگشتی را با
جهت، نوع شاهد و ارجاع به رکورد اصلی برمی‌گرداند. در Bitcoin شاهد از مسیر
UTXO با کران محافظه‌کارانه می‌آید. در Ethereum و BSC از مسیر انتقال مشاهده‌شده
و در TRON از خلاصهٔ انتشار مواجهه می‌آید. وجود واسطهٔ خدماتی در نتیجه مشخص است.
این شمارش به بازه و شواهد همان تحقیق محدود است.

۴. نتیجهٔ تحقیق ابتدا snapshot موقت در Neo4j مرکزی است. `Export` همان
snapshot را ماندگار می‌کند؛ تغییر داده یا برچسب، مدرک ذخیره‌شده را بی‌صدا
بازنویسی نمی‌کند. نتیجهٔ قدیمی ممکن است historical/unverified شود.

API مرکزی تحقیق:

```text
GET  /api/{network}/wallet/{address}/investigation
GET  /api/investigations
GET  /api/investigations/{id}
POST /api/investigations/{id}/export
```

احراز هویت درخواست کاربر در Gateway و service key بین سرویس‌ها کنترل
می‌شود. قرارداد chain API در [OpenAPI](../contracts/chain-api-v1.openapi.yaml)
است؛ قرارداد پاسخ در سطح کد پذیرفته شده و پذیرش RPC/VM جداست.

## فهرست انتقال‌ها با فیلتر و صفحه‌بندی

هر چهار VM مسیر زیر را دارند؛ Gateway نیز آن را به همان VM شبکه می‌فرستد:

```text
GET /api/{network}/wallet/{address}/transfers
```

`network` یکی از `bitcoin`، `ethereum`، `bsc` یا `tron` است. این مسیر فقط
شواهد انتقال ذخیره‌شده را می‌خواند. تحقیق کامل و risk، موجودی و جست‌وجوی
چند hop محدودهٔ پاسخ خودشان را دارند؛ فیلتر این لیست به آن‌ها منتقل نمی‌شود.

| گزینه | کاربرد |
|---|---|
| `limit` | تعداد ردیف صفحه، ۱ تا ۱۰۰؛ پیش‌فرض ۵۰ |
| `from_block`, `to_block` | محدودهٔ بستهٔ ارتفاع بلاک |
| `from_unix_ms`, `to_unix_ms` | محدودهٔ بستهٔ زمان UTC به میلی‌ثانیه |
| `asset_id` | دارایی همان شبکه؛ شناسهٔ collection برای همهٔ NFTهای آن، یا collection/token ID برای یک توکن |
| `direction` | `incoming`، `outgoing` یا `both`؛ پیش‌فرض `both` |
| `cursor` | مقدار `pagination.next_cursor` صفحهٔ قبلی، همراه همان limit و فیلترها |

دارایی بومی به‌ترتیب `native:BTC`، `native:ETH`، `native:BNB` و `native:TRX`
است. دارایی شبکهٔ دیگر یا نوع پشتیبانی‌نشده خطای ۴۰۰ می‌دهد. مقدار و token ID
رشتهٔ عدد صحیح‌اند؛ آن‌ها را به float تبدیل نکنید. در بیت‌کوین، outgoing
ورودی خرج‌شدهٔ آدرس و incoming خروجی به آدرس است؛ ارتباط address→transaction
یا transaction→address حفظ می‌شود و پرداخت دوتایی میان آدرس‌ها استنتاج نمی‌شود.

مثال روی شبکهٔ خصوصی از Main به API بیت‌کوین؛ متغیرها را قبلاً تنظیم کنید:

```bash
curl --get "$CHAIN_API/api/bitcoin/wallet/$ADDRESS/transfers" \
  -H "X-AML-Service-Key: $AML_SERVICE_KEY" \
  --data-urlencode 'limit=2' \
  --data-urlencode 'asset_id=native:BTC' \
  --data-urlencode 'direction=both'
```

اگر `has_more=true` بود، همین درخواست را با `cursor` برگشتی تکرار کنید.
وقتی `next_cursor=null` است، در snapshot و فیلتر فعلی صفحهٔ دیگری نیست؛
این نتیجه، تاریخچهٔ خارج از بازهٔ ذخیره‌شده را پوشش نمی‌دهد.

ترتیب نزولی بلاک، transaction index، event index، sub-index و شناسهٔ رابطه
ثابت است. تغییر limit/فیلتر/آدرس همراه cursor خطای ۴۰۰ دارد. اگر هنگام
خواندن یا بین دو صفحه منبع منتشرشده تغییر کند، پاسخ **۴۰۹ با code برابر
`snapshot_changed`** می‌آید؛ cursor را حذف و از صفحهٔ اول شروع کنید.
این سیاست شامل رسیدن بلاک جدید، replay/reorg و تغییر frame منبع نیز هست؛
صفحهٔ تاریخی پس از تغییر منبع نگه‌داری نمی‌شود.

## خواندن پاسخ مشترک تحقیق و مسیر

پاسخ‌های `investigation` و `paths` هر چهار شبکه دارای
`response_contract: chain_response_v1` هستند. فیلدهای بومی برای جزئیات و
شواهد قبلی حفظ شده‌اند؛ بخش `evidence` نمایش مشترک را می‌دهد:

| بخش | استفاده |
|---|---|
| `graph` | گراف در ریشهٔ پاسخ؛ node با `subject_type` و edge با مبلغ صحیح `amount_raw`، دارایی/token استاندارد و موقعیت اصلی |
| `evidence.activity` | تعداد دریافت/ارسال و تراکنش در بازه‌های زمانیِ ذخیره‌شده؛ اثبات تاریخچهٔ کامل نیست |
| `evidence.holdings` | دارایی و مبلغ خام، وضعیت دسترس‌پذیری و مدل موجودی |
| `evidence.paths` | ارجاع به node/edge همان گراف؛ جست‌وجوی محدود، بدون ادعای پیوستگی پول یا مالکیت |
| `evidence.attribution` | claim منبع با وضعیت review و reference؛ نمایش، آن را تأیید نمی‌کند |

بخش درخواست‌نشده `not_requested` و دادهٔ در دسترس‌نبودنی `unavailable`
است؛ `null` موجودی به صفر تبدیل نمی‌شود. مدل موجودی Bitcoin برابر
`indexed_utxo`، اتریوم/BSC برابر `finalized_rpc` و ترون برابر
`indexed_netflow` است. مدل آخر خالص انتقال‌های ثبت‌شده است و ممکن است منفی
باشد؛ موجودی فعلی حساب از RPC نیست. endpoint مستقل `holdings` همچنان
پاسخ بومی خود را دارد؛ نمایش مشترک داخل پاسخ تحقیق است.

`response_capabilities` تفاوت قابل‌اعمال‌بودن log/trace/prevout/internal
را نشان می‌دهد. وضعیت واقعی پوشش را از `coverage` و `data_coverage`
بخوانید؛ قابل‌اعمال‌بودن یک قابلیت، تأیید کامل‌بودن دادهٔ آن نیست.

در پنل، بخش **Evidence** شبکه، بلاک، پوشش تاریخچه، مدل موجودی، تعداد
شواهد و شناسهٔ snapshot را نشان می‌دهد. مرکز قرارداد را پیش از ذخیره
کنترل می‌کند؛ بازگشایی و Export تازگی منبع را دوباره بررسی می‌کنند.
اگر بلاک‌ها یا شواهد عملیاتی وابسته تغییر کرده باشند، `source_validity`
برابر `stale` است؛ در قطع/ناتوانی بررسی `unverified` است. شواهد تاریخی
حفظ می‌شوند و ریسک جاری `UNKNOWN` می‌شود؛ برای تصمیم جاری تحقیق تازه بگیرید.
در نمای Wallet، `risk_engine.policy_version` و `risk_policy_hash` سیاست
ثبت‌شده را مشخص می‌کنند و هر سیگنال سهم و شاهد خودش را دارد. برای استفاده
از امتیاز جاری، `source_validity.current_risk_usable` باید `true` باشد.
اگر شواهد کافی نباشد یا سیاست و نتیجهٔ ذخیره‌شده قابل تأیید نباشند، امتیاز
`null` و سطح `UNKNOWN` است. تحقیق قدیمی بدون هش برای مرور تاریخی در دسترس
است و برای امتیاز جاری باید دوباره اجرا شود. امتیاز صفر با `NO_SIGNALS`
فقط بیانگر نبود سیگنال در شواهد بررسی‌شده است.
قرارداد کامل در [OpenAPI](../contracts/chain-api-v1.openapi.yaml) است.

## Watchlist و بررسی KYT

روی Gateway اصلی با همان حساب کاربری وارد شوید. برای اتصال سامانهٔ بیرونی،
شناسهٔ مبهم مشتری را در `customer_id` و شناسهٔ سامانه را در `system_id`
بفرستید. این دو شناسه به VM شبکه‌ها ارسال نمی‌شوند. نمونهٔ ثبت یک آدرس:

```bash
curl --user analyst -H 'Content-Type: application/json' \
  --data '{"network":"ethereum","kind":"address","value":"0x0000000000000000000000000000000000000001","system_id":"payments","customer_id":"customer-123","rule":"evidence_score","threshold":25}' \
  https://aml.example/api/kyt/watchlist
```

برای تراکنش، `kind` را `transaction` بگذارید و هش را در `value` بدهید.
نام‌های شبکه `bitcoin`، `ethereum`، `bsc` و `tron` هستند.
`evidence_score` از ۱ تا ۱۰۰ امتیاز جاری را می‌سنجد و `approved_label`
از ۱ تا ۷۰ فقط سهم برچسب تأییدشده را می‌سنجد. پیش‌فرض `evidence_score`
با آستانهٔ ۲۵ است. ارسال مجدد همان مورد و همان تنظیمات نتیجه را حفظ
می‌کند. تغییر قاعده یا آستانه، نتیجهٔ قبلی را پاک و بررسی تازه را صف می‌کند.

- `GET /api/kyt/watchlist` فهرست فعال را صفحه‌بندی می‌کند. برای صفحهٔ
  بعد، `next_after` را به صورت `?after=...` بفرستید.
- `GET /api/kyt/watchlist/{id}` آخرین نتیجه و زمان بررسی را می‌دهد.
  `result: null` یعنی بررسی اولیه در صف است.
- `DELETE /api/kyt/watchlist/{id}` پایش همان مورد را متوقف می‌کند.

worker مرکزی هر پنج ثانیه یک موردِ آماده را برمی‌دارد و پس از بررسی
موفق، آن را برای ۱۵ دقیقه بعد در صف می‌گذارد. خطای منبع معمولاً یک دقیقه
بعد دوباره بررسی می‌شود. تغییر اطلاعات و برچسب‌ها در بررسی بعدی لحاظ
می‌شود. زمان اجرای واقعی به طول صف و پاسخ VM شبکه بستگی دارد.

`matched` یعنی قاعده برای یک شاهد قابل‌استفاده برقرار بوده است.
`no_match` فقط به دادهٔ بررسی‌شده مربوط است. `unknown` یعنی پوشش یا
تازگی برای تصمیم کافی نبود. `not_indexed` یعنی تراکنش در تاریخچهٔ canonical
ایندکس‌شده مشاهده نشد. بررسی تراکنش وضعیت جاری حداکثر چهار آدرسِ مشاهده‌شده
را می‌سنجد. اگر سقف یا کمبود شواهد اجازهٔ تکمیل ندهد، نتیجهٔ منفی صادر
نمی‌شود. در بیت‌کوین، ورودی و خروجی فقط آدرس مشاهده‌شده را مشخص می‌کنند.

### هشدارهای KYT

اولین `matched` یک هشدار در Neo4j مرکزی می‌سازد. تا وقتی همان شرط پابرجاست،
حتی پس از بستن هشدار، بررسی‌های بعدی هشدار تکراری نمی‌سازند. `unknown`
و `not_indexed` این دوره را قطع نمی‌کنند. پس از `no_match` یا تغییر تنظیم
پایش، تطبیق بعدی یک هشدار تازه می‌سازد. `match_alert_id` در Watchlist شناسهٔ
هشدار این دوره است. شدت `low`، `medium` یا `high` از بیشترین امتیاز ریسک
آدرس‌هایی می‌آید که با قاعده تطبیق کرده‌اند. شواهد اولین بررسی همراه هشدار
حفظ می‌شوند و نتیجهٔ بررسی‌های تازه در Watchlist می‌آید.

- `GET /api/kyt/alerts` فهرست جدیدترین هشدارها را برمی‌گرداند. فیلترهای
  `status` و `severity` اختیاری‌اند. برای صفحهٔ بعد همان فیلترها را نگه
  دارید و `next_after` را در `after` بفرستید.
- `GET /api/kyt/alerts/{id}` شواهد اولیه، وضعیت رسیدگی و وضعیت ارسال را می‌دهد.
- `PATCH /api/kyt/alerts/{id}` با بدنهٔ JSON یکی از اقدام‌های زیر را انجام می‌دهد.
  مالک هشدار می‌تواند آن را واگذار یا پس بگیرد. مسئول واگذارشده با حساب
  خودش می‌تواند هشدار را بخواند، تأیید کند و ببندد.

```json
{"action":"assign","assignee":"analyst-2"}
```

`{"action":"unassign"}` واگذاری را لغو می‌کند. `{"action":"acknowledge"}`
وضعیت را از `open` به `acknowledged` می‌برد و
`{"action":"resolve"}` سپس آن را `resolved` می‌کند.

برای فعال‌کردن webhook، در فایل محیط VM اصلی `AML_KYT_WEBHOOK_URL` را
روی مقصد HTTPS مورد اعتماد برای هشدارهای همهٔ شبکه‌ها بگذارید و در صورت
نیاز `AML_KYT_WEBHOOK_TOKEN` را برای Bearer تنظیم کنید. پس از راه‌اندازی
مجدد نود تحلیلی، هشدارهای تازه وارد صف ارسال می‌شوند. هشدارهای ایجادشده
در حالت غیرفعال با وضعیت `disabled` ثبت می‌شوند. اقلام صف با مقصدی
ارسال می‌شوند که در زمان اجرای worker تنظیم شده است.

رویداد `kyt.alert.created` با `Idempotency-Key` برابر شناسهٔ هشدار فرستاده
می‌شود. گیرنده باید پردازش تکراری همین کلید را بی‌اثر کند. worker محلی
هر پنج ثانیه حداکثر یک ارسال را شروع می‌کند. خطای اتصال، ۴۰۸، ۴۲۵، ۴۲۹
و خطاهای ۵xx تا سقف شش تلاش تکرار می‌شوند. اقلام تحویل‌نشده پس از ۲۴ ساعت
با وضعیت `failed` در سابقه می‌مانند. جزئیات هر ارسال در هشدار و خلاصهٔ
صف هشدارهای خودتان در `GET /api/kyt/alerts/backlog` در دسترس است.

### پروندهٔ تحقیق

پرونده از API مرکزی و با حساب تحلیلگر ساخته می‌شود. درخواست زیر یک پرونده می‌سازد.

```bash
curl --user analyst -H 'Content-Type: application/json' \
  -H 'Idempotency-Key: case-2026-0001' \
  --data '{"title":"بررسی تراکنش مشتری"}' https://aml.example/api/cases
```

تغییرات را با `POST /api/cases/{id}/events` و یک کلید یکتا برای همان عمل
بفرستید. برای تکرار درخواست پس از خطا همان کلید و همان بدنه را بفرستید.

| بدنهٔ JSON | کاربرد |
| --- | --- |
| `{"action":"add_note","text":"شرح بررسی"}` | ثبت یادداشت با نام نویسنده و زمان |
| `{"action":"attach_investigation","investigation_id":"..."}` | اتصال تحقیق مالک که قبلاً Export شده است |
| `{"action":"attach_alert","alert_id":"..."}` | اتصال هشدار KYT متعلق به مالک پرونده |
| `{"action":"attach_transaction","network":"bitcoin","hash":"..."}` | اتصال تراکنش دارای یک وقوع canonical در ClickHouse همان شبکه |
| `{"action":"assign_reviewer","reviewer":"analyst-2"}` | دسترسی بازبینی با نام دقیق حساب کاربری دیگر |
| `{"action":"submit_review"}` | ارسال پروندهٔ دارای شاهد و بازبین |
| `{"action":"withdraw_review"}` | بازگرداندن پروندهٔ در انتظار بازبینی به پیش‌نویس توسط مالک |
| `{"action":"request_changes","reason":"شرح اصلاح"}` | بازگرداندن پرونده برای اصلاح توسط بازبین |
| `{"action":"approve","comment":"بررسی شد"}` | تأیید و بستن توسط بازبین |

مالک در پیش‌نویس و پس از درخواست اصلاح، شاهد و یادداشت اضافه می‌کند.
بازبین در زمان بازبینی می‌تواند یادداشت اضافه کند. پس از تأیید، پرونده برای
مطالعه و دانلود در دسترس می‌ماند. ثبت دوبارهٔ یک شاهد یکسان با کلید تازه
خطای تعارض می‌دهد. اگر رخداد تراکنش مبهم یا خارج از دادهٔ ایندکس‌شده باشد،
اتصال آن پذیرفته نمی‌شود.

- `GET /api/cases` پرونده‌های متعلق به حساب یا واگذارشده برای بازبینی را فهرست می‌کند.
- `GET /api/cases/{id}` تاریخچهٔ مرتب پرونده را می‌دهد. برای صفحهٔ بعد
  مقدار `next_after` را در `after` بفرستید.
- `GET /api/cases/{id}/evidence/{event_id}` شاهد ضمیمه را می‌خواند. در مورد
  تحقیق ذخیره‌شده، snapshot تاریخی پس از بررسی هش برگردانده می‌شود. این
  مسیر دسترسی بازبین به همان شاهد را می‌دهد و دسترسی جداگانه به تحقیق ایجاد نمی‌کند.
- `GET /api/cases/{id}/report` گزارش متنی UTF-8 قابل دانلود می‌دهد. گزارش
  حداکثر ۲۵۰ رویداد آخر و خلاصهٔ شواهد آن‌ها را دارد و اگر رویدادهای قدیمی‌تری
  وجود داشته باشند، آن را ذکر می‌کند. گزارش را مالک یا بازبین دانلود و
  از مسیر مورد اعتماد خود به اشتراک می‌گذارد.

شواهد با وضعیت ثبت‌شده در زمان اتصال نگه داشته می‌شوند. امتیاز یا وقوع
تراکنش در گزارش پرونده باید در همان زمینهٔ تاریخی خوانده شود.

### شواهد DeFi، NFT و الگوهای رفتاری

در تحقیق Wallet، پنل **Protocol and NFT evidence** و فیلد
`semantic_evidence` شواهد برگشتی را با شناسهٔ snapshot نمایش می‌دهند.
`decoded_receipt` یعنی topic، ساختار ABI و جایگاه log بررسی شده‌اند.
`protocol_identity` جداگانه وضعیت تطبیق آدرس با registry هنگام
طبقه‌بندی را مشخص می‌کند. `candidate` برای سرنخ‌های حاصل از
selector، جریان یا لاگِ بدون اثبات ABI است. پاسخ Paths این بخش را درخواست نمی‌کند.

| شبکه | محدودهٔ شواهد |
| --- | --- |
| Ethereum | ABIهای شناخته‌شدهٔ سواپ و نقدینگی، میکسر Tornado با registry و انتقال ERC‑721/1155 در گراف |
| BSC | ABIهای شناخته‌شدهٔ سواپ و نقدینگی در registry بازبینی‌شده، میکسر با ABI تعریف‌شده و انتقال ERC‑721/1155 در گراف |
| TRON | سواپ سازگار با AMM V2 در صورت تطابق مبلغ لاگ با انتقال‌های canonical، سرنخ‌های طبقه‌بندی و انتقال TRC‑721 در گراف |
| Bitcoin | سرنخ‌های UTXO مانند CoinJoin احتمالی و peel chain. بخش NFT و receipt پروتکل برای مدل بومی UTXO قابل اعمال نیست |

مقادیر NFT به‌صورت رشتهٔ دقیق برمی‌گردند. مبلغ سواپ فقط وقتی در بخش
مشترک نمایش داده می‌شود که از کلمات ABI یا تطابق مستقیم با انتقال‌های
canonical آمده باشد. جمع انتقال‌های کل تراکنش در payload بومی برای
بررسی باقی می‌ماند. در اتریوم و BSC، گیرندهٔ برداشت Tornado از بدنهٔ
رویداد خوانده می‌شود و relayer جداگانه در شاهد ثبت می‌شود. [ABI قرارداد Tornado](https://github.com/tornadocash/tornado-core/blob/master/contracts/Tornado.sol)

پوشش از روی پاسخ محدودِ تحقیق گزارش می‌شود. فروش یا wash trading در NFT،
میکسر TRON، و انتساب مالکیت یا استمرار وجوه در الگوهای بیت‌کوین خارج از
پوشش این قرارداد هستند. سیاست ریسک `aml_evidence_v3_receipt_proof`
برای سیگنال میکسر به هر دو شاهد ABI و registry نیاز دارد. برای بهره‌برداری،
نسخهٔ نود تحلیلی و آداپتور بیت‌کوین باید هماهنگ به‌روزرسانی شوند.
برای تولید شاهد ABI روی بلاک‌هایی که با decoder قدیمی ایندکس شده‌اند،
replay همان بازه لازم است.

### شواهد بازار NFT در اتریوم

در تحقیق کیف پول اتریوم، در **Evidence** بخش **NFT marketplace · Seaport 1.6**
را باز کنید. سامانه برای حداکثر ۲۴ تراکنش دارای انتقال NFT در گراف
برگشتی، لاگ `OrderFulfilled` را از ClickHouse اتریوم می‌خواند. سفارش
وقتی نمایش داده می‌شود که انتقال ERC721 یا ERC1155 همان تراکنش با
دارایی، مقدار و طرف مشخص‌شده در رسید سازگار باشد. شناسهٔ سفارش، اقلام
پیشنهاد و اقلام دریافتی، دریافت‌کننده‌ها و شمارهٔ لاگ در شاهد ثبت می‌شوند.
شناسه‌های NFT و مقدارها رشته‌های ده‌دهی دقیق هستند.
[استقرار Seaport](https://github.com/ProjectOpenSea/seaport#deployments) و
[تعریف رخداد](https://github.com/ProjectOpenSea/seaport-types/blob/main/src/interfaces/ConsiderationEventsAndErrors.sol)

این بخش تا ۱۲ رسید را از تاریخچهٔ موجود بررسی می‌کند. مقدارها اقلام
اعلام‌شده در سفارش‌اند. تأیید تسویه، قیمت فروش و تشخیص wash trading
در این مرحله پوشش ندارند. پاسخ خالی فقط به محدودهٔ جست‌وجوی همان
تحقیق مربوط است.

### ردیابی عملیات cBridge بین اتریوم و BSC

در تحقیق اتریوم یا BSC، بخش **Cross-chain bridge evidence** را باز کنید
و هش تراکنشی را که در گراف یا شواهد پروتکل همان تحقیق دیده می‌شود وارد
کنید. اگر تراکنش چند رخداد cBridge دارد، شمارهٔ دقیق لاگ را هم بدهید.
می‌توانید از تراکنش ارسال یا Relay شروع کنید. پوشش فعلی شامل USDC و USDT
در هر دو جهت و قراردادهای pool اصلی است.
[مسیر cBridge](https://cbridge.celer.network/bridge/ethereum-binance/) و
[آدرس قراردادها](https://cbridge-docs.celer.network/reference/contract-addresses.html)

سرویس شناسهٔ یکسان عملیات، جهت و دو آدرس، قرارداد و اعشار توکن، اختلاف
مقدار حداکثر ۵٪ و فاصلهٔ زمانی حداکثر ۴۸ ساعت را روی رسیدهای canonical
تطبیق می‌دهد. در صورت تطبیق، پنل دو تراکنش، شمارهٔ لاگ، اطمینان اتصال و
رسیدها را نمایش می‌دهد. اتصال `BRIDGE_OPERATION` در همان تحقیق ذخیره
می‌شود و با بازکردن دوبارهٔ آن قابل مشاهده است. هر تحقیق حداکثر ۳۲ اتصال
ذخیره می‌کند.

جست‌وجو به تاریخچهٔ ثبت‌شده در ClickHouse هر شبکه محدود است. پاسخ خالی
می‌تواند ناشی از پوشش ناقص تاریخچه یا خارج بودن از بازه باشد. تطبیق شاهد
تاریخی عملیات cBridge است. وضعیت تسویه، قطعیت بلاک و تداوم وجوه خارج
از دامنهٔ این شاهد هستند.
[معنای رخدادهای قرارداد cBridge](https://github.com/celer-network/sgn-v2-contracts/blob/main/contracts/liquidity-bridge/Bridge.sol)

### ردیابی Allbridge Core بین BSC و ترون

در همان بخش **Cross-chain bridge evidence** در تحقیق BSC یا ترون، هش
تراکنش موجود در تحقیق و در صورت نیاز شمارهٔ لاگ را وارد کنید. پوشش
شامل قراردادهای اصلی Allbridge Core، پیام‌رسان بومی آن و مقصد USDT
در هر دو جهت است. می‌توانید از رسید ارسال یا دریافت شروع کنید. جست‌وجو
روی تاریخچهٔ ذخیره‌شده در ClickHouse هر شبکه و پنجرهٔ ۴۸ ساعت انجام می‌شود.
[قراردادهای Allbridge Core](https://docs-core.allbridge.io/product/how-does-allbridge-core-work/allbridge-core-contracts)

سامانه هش پیام را با مبلغ vUSD رخداد مبدأ، گیرنده، شبکه‌ها، توکن مقصد،
nonce، پیام‌رسان و قرارداد مبدأ بازسازی می‌کند. رسید مقصد فقط با هش
یکسان پذیرفته می‌شود. رسید ناموفق ترون وارد تطبیق نمی‌شود. پنل مقدار
vUSD مبدأ و مقدار USDT مقصد را با واحدهای جدا نشان می‌دهد. اتصال و
رسیدها به‌عنوان شاهد تاریخی عملیات در Neo4j همان تحقیق ذخیره می‌شوند.
[منطق قرارداد](https://github.com/allbridge-io/allbridge-core-evm-contracts/blob/master/contracts/Bridge.sol) و
[محاسبهٔ هش](https://github.com/allbridge-io/allbridge-core-evm-contracts/blob/master/contracts/libraries/HashUtils.sol)

### تطبیق سپردهٔ tBTC v2 از بیت‌کوین به اتریوم

در تحقیق بیت‌کوین، بخش **Cross-chain bridge evidence** را باز کنید و هش
تراکنش موجود در تحقیق را وارد کنید. اگر چند خروجی ممکن وجود دارد،
شمارهٔ **Output index (vout)** را هم وارد کنید. سامانه در تاریخچهٔ
نمایه‌شدهٔ اتریوم تا ۴۸ ساعت قبل و بعد از زمان بلاک بیت‌کوین دنبال
رویداد `DepositRevealed` قرارداد اصلی می‌گردد.
[آدرس و ABI قرارداد](https://github.com/keep-network/tbtc-v2/blob/main/solidity/deployments/mainnet/Bridge.json)

تطبیق مستلزم یکسان بودن هش تراکنش، شمارهٔ خروجی، مبلغ ساتوشی و اسکریپت
P2SH یا P2WSH سپردهٔ استاندارد است. شاهد تاریخی شامل `vout` بیت‌کوین
و شمارهٔ لاگ اتریوم در Neo4j همان تحقیق ذخیره می‌شود.
[قواعد اسکریپت سپرده](https://github.com/keep-network/tbtc-v2/blob/main/solidity/contracts/bridge/Deposit.sol)

برای تأیید جمع‌آوری همان سپرده، در همان فرم **Verify deposit sweep**
را انتخاب کنید. سامانه خرج خروجی را از بلاک سپرده تا ۴٬۳۲۰ بلاک بعد
جست‌وجو می‌کند. سپس رویداد موفق `DepositsSwept` را تا هفت روز پس از
زمان بلاک خرج روی اتریوم می‌جوید. جست‌وجوی اتریوم برای اختلاف ساعت
بلاک‌ها تا دو ساعت قبل از زمان بلاک خرج را هم در نظر می‌گیرد، با این
شرط که رویداد sweep پس از رویداد ثبت سپرده باشد.
[فرایند اثبات جمع‌آوری](https://docs.threshold.network/app-development/tbtc-contracts-api/tbtc-v2-api/depositsweep)

نتیجهٔ sweep جداگانه ذخیره می‌شود و شامل خروجی سپرده، رسید ثبت آن،
ورودی خرج‌کننده (`vin`) و رسید اثبات اتریوم است. جست‌وجو به تاریخچهٔ
نمایه‌شده محدود است.

برای بررسی صدور، **Verify optimistic mint** را در همان فرم انتخاب
کنید. سامانه از هش تراکنش و شمارهٔ خروجی بیت‌کوین، کلید سپرده را
محاسبه می‌کند. سپس تا هفت روز بعد از رویداد ثبت سپرده، در تاریخچهٔ
نمایه‌شدهٔ اتریوم دنبال `OptimisticMintingFinalized` در خزانهٔ رسمی
tBTC می‌گردد. برای تأیید، لاگ `Minted` و انتقال توکن از آدرس صفر به
سپرده‌گذار باید در همان رسید موفق به هم بخورند. مبلغ دریافتی با دقت
۱۸ رقم اعشار نمایش داده می‌شود و صدور کارمزد، در صورت وجود، جدا ثبت
می‌شود. شاهد صدور و شاهد جمع‌آوری سپرده دو نتیجهٔ جداگانه‌اند.
[قواعد قرارداد صدور](https://docs.threshold.network/app-development/tbtc-contracts-api/tbtc-v2-api/tbtcoptimisticminting)

صدور عادی پس از جمع‌آوری، بازخرید و سپرده‌های دارای دادهٔ اضافه در
فهرست کارهای باقی‌مانده‌اند.

## عملیات روزمره

```bash
bash scripts/vm.sh ethereum pause-ingestion
bash scripts/vm.sh ethereum db --query 'SELECT count() FROM aml_transactions_canonical'
bash scripts/vm.sh ethereum resume
```

در هر چهار شبکه، `aml_transactions_canonical` از مسیر فعال ingest پر می‌شود.
در ترون هم facts و checkpoint مشترک‌اند؛
رکوردهای عملیاتی metadata/intelligence/exposure ترون نیز در `aml_*` هستند؛
SQL بومی و mountها حذف شده‌اند و هر چهار شبکه schema job مشترک دارند؛
پذیرش مسیر داده با RPC نمونه و ClickHouse واقعی موفق شده است؛ پذیرش کامل
RPC عملیاتی و VMهای واقعی در ادامهٔ برنامه است. آزمون محدود RPC عمومی
چهار شبکه نیز جداگانه در [TESTING](TESTING.md) ثبت شده است.

توقف برنامه‌ها با حفظ ClickHouse: `pause`. توقف stack بدون حذف volume:
`down`. برای لاگ‌ها و سلامت از `logs`، `ps` و `check` استفاده کنید. برای
تغییر بازه و تنظیمات worker، `up` را دوباره اجرا کنید؛ `resume` کانتینر را
با تنظیمات قبلی روشن می‌کند. از `down -v` برای توقف روزمره استفاده نکنید.

فرمان‌های عملیاتی چهار شبکه از همان image و ingestor اصلی اجرا می‌شوند؛
سرویس یا دیتابیس تازه لازم نیست. روی VM همان شبکه، `role` را با
`bitcoin`، `ethereum`، `bsc` یا `tron` جایگزین کنید:

```bash
bash scripts/vm.sh ethereum probe
bash scripts/vm.sh ethereum failures --limit 50
bash scripts/vm.sh ethereum scan-gaps --from-block 100 --to-block 109 --max-blocks 10
bash scripts/vm.sh ethereum pause-ingestion
bash scripts/vm.sh ethereum replay --from-block 100 --to-block 109 --max-blocks 10
bash scripts/vm.sh ethereum repair --from-block 105 --to-block 105 --max-blocks 1
# فقط پس از بررسی علت خطای دائمی و اصلاح منبع:
bash scripts/vm.sh ethereum repair --from-block 110 --to-block 110 --max-blocks 1 --include-dead
bash scripts/vm.sh ethereum resume
```

اعداد مثال‌اند؛ بازه را داخل تاریخچهٔ منتشرشدهٔ همان DB انتخاب کنید.
`probe` بدون اتصال به DB، هویت شبکه، head/finality و دسترسی به start تنظیم‌شده
و نمونهٔ head را بررسی می‌کند؛ در EVM receipt/trace، در TRON شاهد receipt
و در Bitcoin محدودیت pruning/prevout نیز گزارش می‌شوند. نمونهٔ ترون حداکثر
۸ receipt در هر بلاک است؛ genesis ترون با مسیر ویژهٔ بدون runtime receipt
اعتبارسنجی می‌شود. سقف زمان بررسی نمونهٔ چهار شبکه برابر ۱۲۰ ثانیه است.
وضعیت غیر `ready` خروج ناموفق دارد؛ موفقیت نمونه به معنی دسترسی به تمام
archive یا تأیید تمام تاریخچه نیست (`full_history_verified=false`).

`failures` صف فعلی و `scan-gaps` شمارش بلاک/تراکنش، پیوستگی و coverage لازم
در بازه را بررسی می‌کنند؛ این scan همهٔ مبلغ‌ها را دوباره با RPC تطبیق نمی‌دهد.
scan سه شبکهٔ account فقط خواندنی است؛ فرمان قدیمی scan بیت‌کوین همچنان
رکورد gap را با قفل writer ثبت می‌کند. برای صفحهٔ بعد failureهای account،
`--cursor` برگشتی را بدون تغییر بدهید؛ بیت‌کوین `--after-block` دارد.
این صفحات صف جاری‌اند و audit ثابت و تغییرناپذیر نیستند.

پیش از `replay`، `repair` یا scan بیت‌کوین ingestion را متوقف کنید؛ قفل مشترک
از دو writer هم‌زمان جلوگیری می‌کند. بازه حداکثر ۱۰۰۰ بلاک و `--max-blocks`
اجباری است. replay دادهٔ بازهٔ منتشرشده را از RPC معتبر دوباره می‌خواند؛
repair شکاف/خطای موجود یا اولین بلاک ناموفق بعد از tip را بازپردازش می‌کند.
بازپردازش تاریخی tip را عقب نمی‌برد و تعارض hash را نمی‌پذیرد. حذف کامل
نشانگر بلاک تاریخی با rewind خودکار ترمیم نمی‌شود؛ باید backup را برگردانید
یا DB خالی را از RPC بازسازی کنید. dead-letter فقط با repair صریح و
`--include-dead` باز می‌شود؛ شکست repair آن را بی‌صدا برای follow باز نمی‌کند.
Bitcoin روش قبلی انتخاب `--failure-id` را هم نگه می‌دارد. قواعد این بخش برای
فرمان‌های مشترک `vm.sh` است؛ ابزارهای تخصصی قدیمی replay/repair ممکن است
سقف و گزینه‌های متفاوت داشته باشند؛ برای عملیات روزمره از همین launcher استفاده کنید.

بودجهٔ failure در Ethereum با `ETHEREUM_FAILURE_MAX_ATTEMPTS` و در TRON با
`TRON_FAILURE_MAX_ATTEMPTS` تنظیم می‌شود (پیش‌فرض ۱۰، محدودهٔ ۱ تا ۱۰۰۰).
این تعداد pass ناموفق است، نه retry کوتاه HTTP؛ خطای شاهد دائمی ترون می‌تواند
از اولین بار dead-letter شود. تنظیمات budget قبلی Bitcoin/BSC برقرارند.

در restart و poll اتریوم/ترون، هش checkpoint از RPC دوباره کنترل می‌شود؛
حتی وقتی بلاک جدیدی نرسیده است. تعارض دادهٔ finalized اتریوم یا solid ترون
دریافت را متوقف می‌کند و checkpoint قبلی حفظ می‌شود. ابتدا هویت، تازگی و
درستی RPC را بررسی کنید؛ پس از اصلاح منبع، همان بازه/اجرای follow را دوباره
اجرا کنید. replay مجوز جایگزینی خودکار دادهٔ نهایی‌شده نیست.
در Bitcoin/BSC، مسیر reorg با ancestor و انتشار شاخهٔ جدید جداگانه است.

شرح workerها: [تحلیل خودکار](AUTOMATIC_ANALYTICS_FA.md).
backup/restore و پذیرش انتشار: [آزمون‌ها](TESTING.md).
