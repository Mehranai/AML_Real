# Bitcoin reviewed address attribution

این مرحله مسیر عملی **منبع → import برچسب pending → review → تحقیق آدرس → سیاست
ریسک مرکزی → پنل** را به جدول‌های موجود نسخهٔ ۵ وصل می‌کند. schema، نوع داده و
سیاست امتیازدهی جدیدی اضافه نشده است؛ ClickHouse بیت‌کوین در VM خودش و Neo4j در
VM اصلی باقی می‌مانند. exposure چندمرحله‌ای، clustering و detectorها هنوز بازند.

## داده و تصمیم

- `aml_intelligence_sources`: منبع، reference، trust tier اعلامی، مجوز استفاده و
  وضعیت فعال. فعال‌بودن منبع به معنی صحت خودکار ادعا یا ریسکی‌بودن آدرس نیست.
- `aml_intelligence_claims`: آدرس mainnet معتبر، entity/exchange، confidence،
  reference، شواهد، ثبت‌کننده، بازهٔ اعتبار و نسخهٔ جاری برچسب.
- `aml_intelligence_reviews`: تصمیم‌های جداگانهٔ approved/rejected/revoked،
  reviewer، دلیل، شواهد و نسخهٔ دقیق subject. هر تصمیم شناسهٔ تازه دارد و قبل و
  بعدِ ادعا و منبع هنگام review در `evidence_json` آن نگهداری می‌شوند.

import همیشه pending است و فیلدهای تأیید را از فایل ورودی قبول نمی‌کند. منبع باید
قبلاً ثبت و فعال باشد. تغییر claim موجود با `expected_revision` انجام می‌شود و
تأیید قبلی را کنار می‌گذارد؛ ویرایش هم دوباره pending است. ثبت source موجود نیز
نسخهٔ مورد انتظار لازم دارد. شناسهٔ شبکه در همهٔ فایل‌ها اجباری است.

review باید claim_id، نسخهٔ فعلی به‌صورت **رشتهٔ صحیح ده‌دهی**، تصمیم، نام reviewer،
دلیل و حداقل یک evidence ref داشته باشد. نمونهٔ ref معتبر `case:123`،
`tx:<64 lowercase hex>` یا `outpoint:<txid>:<index>` است. فایل‌های نمونه در
`docs/intelligence-*.example.*` صرفاً قالب‌اند و دادهٔ واقعی یا اتهام نیستند؛ خودکار
وارد یا تأیید نمی‌شوند. ثبت reviewer در این ابزار، ثبت نام اعلامی اپراتور مجاز VM
است؛ سامانهٔ هویت یا تفکیک نقش سازمانی جدیدی در این مرحله اضافه نشده است.

Claim فقط با منبع فعال، review تأییدشدهٔ منطبق با revision و بازهٔ اعتبار فعلی در
`entities` تحقیق وارد می‌شود. review ابتدا و claim بعد از آن درج می‌شود. اگر فرایند
بین این دو قطع شود، reader اختصاصی بیت‌کوین عدم تطبیق نسخه را می‌بیند و approval
قبلی را فعال نشان نمی‌دهد. retry کور انجام ندهید؛ `claims` و `history` را ببینید و
نسخهٔ فعلی را برای تصمیم بعدی به کار ببرید.

## اجرا

فرمان‌های mutation فقط از CLI روی VM مجاز اجرا می‌شوند. endpoint نوشتنی یا تأیید
خودکار روی وب اضافه نشده است. این سرویس ابزار به RPC و Bitcoin Core نیاز ندارد.
همهٔ mutationها از قفل مشترک همان VM استفاده می‌کنند؛ قفل intelligence از ingestion
جداست تا ثبت برچسب، دریافت بلاک را متوقف نکند. اجرای native نیازمند
`BITCOIN_CLICKHOUSE_*` و `BITCOIN_WRITER_LOCK_DIR` همان VM است.

```bash
bitcoin_aml intelligence --help
bitcoin_aml intelligence source source.json --dry-run
bitcoin_aml intelligence source source.json
bitcoin_aml intelligence import claims.jsonl --dry-run
bitcoin_aml intelligence import claims.jsonl
bitcoin_aml intelligence claims 1BoatSLRHtKNngkdXEeobR76b53LETtpyT
# expected_revision فایل review را از خروجی بالا بردارید و تصمیم واقعی را وارد کنید.
bitcoin_aml intelligence review review.json --dry-run
bitcoin_aml intelligence review review.json
bitcoin_aml intelligence history example-claim
```

`--dry-run` ساختار و پیش‌شرط‌ها را کنترل می‌کند و هیچ ردیفی درج نمی‌کند. خروجی آن
revision را رزرو نمی‌کند؛ نسخهٔ قطعی را از اجرای واقعی بگیرید. ورودی حداکثر ۸ MiB
و هر import حداکثر ۱۰۰ ادعا دارد؛ کل batch پیش از اولین درج اعتبارسنجی می‌شود.
برای update، `expected_revision` رکورد جاری را به فایل اضافه کنید. برای غیرفعال‌کردن
منبع، همان source را با `is_active:false` و نسخهٔ جاری ثبت کنید. فعال‌کردن دوبارهٔ
منبع می‌تواند ادعاهای تأییدشده و هنوز معتبر آن را دوباره فعال کند؛ برای پس‌گرفتن
خود ادعا از review با تصمیم `revoked` استفاده کنید.

از ریشهٔ مخزن در VM بیت‌کوین، بعد از build image جدید:

```bash
docker compose --project-directory dockerizd_bitcoin --project-name aml-bitcoin \
  --env-file dockerizd_bitcoin/.env -f dockerizd_bitcoin/docker-compose.yml \
  --profile tools run --rm -T --no-deps bitcoin-intelligence \
  bitcoin_aml intelligence source - < source.json
docker compose --project-directory dockerizd_bitcoin --project-name aml-bitcoin \
  --env-file dockerizd_bitcoin/.env -f dockerizd_bitcoin/docker-compose.yml \
  --profile tools run --rm -T --no-deps bitcoin-intelligence \
  bitcoin_aml intelligence import - < claims.jsonl
docker compose --project-directory dockerizd_bitcoin --project-name aml-bitcoin \
  --env-file dockerizd_bitcoin/.env -f dockerizd_bitcoin/docker-compose.yml \
  --profile tools run --rm -T --no-deps bitcoin-intelligence \
  bitcoin_aml intelligence review - < review.json
```

project name باید همان استقرار موجود باشد. ابزار از image فعلی `bitcoin_aml`
استفاده می‌کند. فایل‌های source/claims/review واقعی را در پوشهٔ خصوصی
`dockerizd_bitcoin/intelligence-data/` نگه دارید که در Git نادیده گرفته می‌شود؛
نام فایل‌های فرمان‌های بالا را متناسب با مسیر خود عوض کنید.

## تحقیق، ریسک و نتیجهٔ ذخیره‌شده

تحقیق آدرس `entities` فعال، ادعاهای جاری و sourceهای مربوط را همراه با
`intelligence_snapshot` برمی‌گرداند. endpointهای فقط‌خواندنی زیر همان service key و
`Cache-Control: no-store` مسیرهای موجود را دارند:

```text
GET /api/bitcoin/wallet/{address}/intelligence
GET /api/bitcoin/wallet/{address}/intelligence/snapshot
```

هش snapshot به آدرس، شبکه، ادعاهای جاری، sourceها، آخرین reviewهای مربوط و وضعیت
فعال در زمان خواندن وابسته است. شروع/پایان اعتبار نیز می‌تواند آن را تغییر دهد.
تحقیق تغییر snapshot بین ابتدا و انتهای query را رد می‌کند. برای هر آدرس بیش از
۱۰۰ ادعای جاری به‌جای پاسخ ناقص، خطای روشن داده می‌شود.

Analytical Node همان سیاست `aml_evidence_v2_verified_inputs` را اجرا می‌کند؛
برچسب تأییدشده با designation ریسک صریح می‌تواند signal موجود
`APPROVED_RISK_LABEL` بسازد. صرف exchange بودن، pending بودن، نداشتن شواهد یا
زیادبودن تراکنش امتیاز ریسک تولید نمی‌کند. نبود signal در بیت‌کوین همچنان UNKNOWN
است. وزن یا مدل ML تازه‌ای در این مرحله اضافه نشده است.

مرکز هنگام ایجاد، reopen و Export علاوه بر snapshot زنجیره، snapshot برچسب همین
آدرس را از adapter بررسی می‌کند. تغییر برچسب یا منبع نتیجهٔ قبلی را stale و قطع
دسترسی/هویت نامعتبر آن را unverified می‌کند؛ امتیاز جاری UNKNOWN می‌شود و امتیاز
تاریخی برای ممیزی حفظ می‌شود. پنل برچسب‌ها و وضعیت فعال/غیرفعال و reviewer را نشان
می‌دهد. payload تاریخی بدون rerun، با برچسب تازه جایگزین نمی‌شود.

قابلیت جدید باید هم روی adapter و هم Analytical Node مستقر شود؛ ابتدا مرکز و سپس
adapter بیت‌کوین را ارتقا دهید تا کنترل تازگی برچسب از ابتدا موجود باشد. اتصال این مسیر
در تست‌های موقت بررسی شده؛ روی VM واقعی اجرا یا برچسب واقعی وارد نشده است.

## مرز تاریخچه

جدول claim از ReplacingMergeTree استفاده می‌کند؛ نگهداری تمام نسخه‌های فیزیکی آن
بعد از merge تضمین نمی‌شود. مرجع ماندگار **تصمیم‌های review و snapshot شواهد داخل
هر تصمیم** است. فرمان history آخرین ۱۰۰ تصمیم را با `truncated` و claim جاری نشان
می‌دهد؛ رکوردهای قدیمی‌تر در جدول review باقی می‌مانند. تاریخچهٔ تمام ویرایش‌های
pending و تمام تغییرهای registry منبع به‌عنوان event log مستقل پیاده نشده است.
مقادیر source هنگام هر review در شواهد همان تصمیم حفظ می‌شوند.

shared view عمومی `aml_intelligence_claims_active` عوض نشده است؛ مسیر فعال بیت‌کوین
برای کنترل انتشار ناقص review از reader این ماژول استفاده می‌کند. workerهای آینده
بیت‌کوین باید همین کنترل و snapshot را مصرف کنند.

[نتایج اعتبارسنجی](INTELLIGENCE_VALIDATION.md)، [کارهای باز](../TODO_BITCOIN.md).
