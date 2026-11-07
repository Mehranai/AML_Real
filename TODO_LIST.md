# مسیر واحد آماده‌سازی AML چهار شبکه

آخرین بازبینی: ۲۰۲۶-۱۱-۰۷. این فایل جایگزین TODO_BITCOIN و roadmapهای
پراکنده است. هدف، قابلیت‌های AML/تحقیق/KYT در سطح محصول تجاری روی همین
**TRON، Ethereum، BSC و Bitcoin** است؛ برابری فعلی با Chainalysis ادعا نمی‌شود.
[وضعیت واقعی و تصمیم‌های پاک‌سازی](docs/PROJECT_STATUS_FA.md).

## اصول ثابت

- چهار VM شبکه، هرکدام ClickHouse/database/volume مستقل؛ یک VM اصلی با
  Gateway، Analytical Node و تنها Neo4j. VMها را کاربر می‌سازد.
- RPC آماده توسط کاربر تأیید شده است؛ نصب یا sync نود کامل روی این VMها
  جزو کار نیست. توان RPC برای history/trace/prevout همچنان باید پذیرفته شود.
- یک منبع schema و مدل مشترک؛ تفاوت UTXO، EVM و TRON با fact تخصصی و
  capability بیان می‌شود. مقدار ساختگی برای یکسان نشان‌دادن شبکه‌ها ممنوع است.
- شروع از دیتابیس خالی. انتقال، نگاشت و حفظ ردیف‌های دیتابیس آزمایشی قبلی
  لازم نیست. دریافت تاریخچه از نود با ingestor اصلی همچنان قابلیت لازم است.
- هر تسک باید به writer/reader/API زنده و آزمون پذیرش متصل باشد. نصب جدول،
  plan آفلاین یا fixture به‌تنهایی «قابلیت محصول» یا «تکمیل شبکه» نیست.
- جدول/سرویس تازه فقط برای نیاز اجرایی مشخص؛ گزارش‌های مرحله‌ای و ابزارهای
  موازی ساخته نمی‌شوند. کد زنده قبل از جایگزینی معتبر حذف نمی‌شود.

## ۰. بازتنظیم و آماده‌سازی

- [x] ممیزی مسیرهای واقعی هر چهار شبکه و ثبت فاصلهٔ محصول؛ کنارگذاشتن فرض
  انتقال دادهٔ قدیمی و برنامهٔ صرفاً بیت‌کوین.
- [x] حذف ابزارهای انتقال تاریخچه/replay موازی، جدول‌های صرفاً آرشیوی و
  گزارش‌های تکراری؛ حفظ دریافت، تحلیل، تست‌های زنده و backup عملیاتی.
- [x] یکسان‌کردن متغیر `CLICKHOUSE_IMAGE` و نسخهٔ صریح در Compose چهار شبکه؛
  حفظ قرارداد عملیاتی ۱ تا ۵؛ توسعهٔ جاری و کمبودهای ترون ذیل DB-4 ثبت می‌شوند.
- [x] تهیهٔ [ساخت دستی VM](docs/VM_SETUP_FA.md)، [معماری](docs/BACKEND_ARCHITECTURE_FA.md)
  و [استفاده](docs/USAGE_FA.md). VM ساخته یا اجراشدهٔ کاربر هنوز پذیرفته نشده است.
- [x] تأیید رگرسیون پاک‌سازی طبق [TESTING](docs/TESTING.md)؛ پذیرش RPC/VM واقعی جداست.
- [ ] کاربر: ساخت پنج VM، شبکه و دیسک، اتصال RPCهای آمادهٔ تأییدشده،
  تنظیم endpointها و کلیدها طبق راهنما. کدنویسی DB روی Docker موقت منتظر VM نمی‌ماند.

## ۱. اولویت فوری: یکسان‌سازی فعال ClickHouse

ترتیب اجرا: **قرارداد writer مشترک → Ethereum → BSC → TRON → پذیرش هر چهار شبکه**.
Bitcoin مرجع آمادهٔ بخش‌هایی از مسیر مشترک است، نه تنها هدف کار.

- [x] **DB-1 — قرارداد اجرایی:** مدل مشترک block/transaction/transfer، مبلغ
  صحیح خام، زمان UTC میلی‌ثانیه، asset/token ID، موقعیت اصلی، receipt/trace/
  prevout coverage و publication/checkpoint برای writer و reader تثبیت شدند.
  اعتبارسنجی batch و تطبیق تمام ستون‌های ذخیره‌شده مشترک است؛ writer فعال
  Bitcoin به آن متصل شد. تست از صفر روی چهار database، رد دادهٔ ناسازگار
  و retry موفق است؛ پروفایل‌های نامرتبط خالی می‌مانند. [پذیرش](docs/TESTING.md).
- [x] **DB-2 — Ethereum:** ingest/checkpoint، API تحقیق/موجودی/مسیر، metadata،
  intelligence، clustering/exposure و risk به `aml_*` متصل شدند. status،
  مبلغ/شناسهٔ NFT، موقعیت log/trace و شاهد اصلی حفظ می‌شوند؛ withdrawal در
  شاهد بلاک و `block_credits` بدون tx ساختگی نمایش داده می‌شود. صف metadata،
  lock، restart، تکمیل trace بلاک قدیمی، مرز سرویس و revoke در تست زنده با
  RPC نمونه و ClickHouse واقعی موفق شدند. DDL native و executor بی‌مصرف
  اتریوم حذف شدند؛ schema مشترک v5 تغییر نکرد. [پذیرش](docs/TESTING.md).
- [x] **DB-3 — BSC:** ingest/checkpoint، metadata، protocol/intelligence و
  API تحقیق/موجودی/مسیر به `aml_*` متصل شدند. receipt/trace اصلی، NFT ID،
  مبلغ صحیح و موقعیت batch/log/call حفظ می‌شوند. replay/repair، شش مرز
  خرابی نوشتن، rollback با ancestor و تعویض کل بازه، restart، revoke و
  API دارای auth روی ClickHouse واقعی موفق شدند. replay اجباری registry
  فعلی را می‌خواند؛ منبع غیرفعال با import فعال نمی‌شود. DDL/executor قدیمی
  حذف شدند و قفل مشترک کانتینرها تنظیم شد؛ schema v5 تغییر نکرد.
  exposure هنگام درخواست و ارزیابی ریسک مرکزی باقی‌اند؛ worker ریسک محلی
  تازه اضافه نشده است. [پذیرش](docs/TESTING.md).
- [x] **DB-4 — TRON:** دریافت و reader/workerها به `aml_*` متصل شدند؛ protobuf/contract
  index، receipt log/internal index، TRX/TRC10/TRC20/NFT و resourceهای TRON
  حفظ می‌شوند. اطلاعات JSON بدون تطبیق با شاهد اصلی به provenance معتبر تبدیل نمی‌شود.
  - [x] **DB-4.1 — قرارداد و شاهد اصلی:** تکمیل factهای تخصصی TRON در یک
    migration مشترک؛ تطبیق txID با protobuf، ترتیب/نوع/پارامتر contractها و
    هویت receipt پیش از پذیرش تراکنش؛ آزمون نصب و تکرار روی چهار database خالی.
    schema v6 اکنون ۴۲ جدول/۵۶ view دارد؛ log/internal/resource تخصصی با
    ترتیب اصلی و NULL صریح حفظ می‌شوند. بایت‌های پارامتر قرارداد پشتیبانی‌نشده
    تأییدشده علامت نمی‌خورند. قطع نصب، drift و قطع انتشار/retry با ClickHouse
    واقعی موفق شدند. اتصال زنده در DB-4.2 و DB-4.3 پذیرفته شد.
  - [x] **DB-4.2 — دریافت و انتشار:** جایگزینی batcherهای مستقل با انتشار
    یکپارچهٔ بلاک در قرارداد مشترک؛ checkpoint، retry/replay، قفل writer و
    حفظ موقعیت و مبلغ TRX/TRC10/TRC20/NFT، log/internal/resource.
    سه سناریوی دریافت زنده روی ClickHouse واقعی و آزمون شش مرز خرابی نوشتن
    موفق شدند: fact/cursor بومی نوشته نمی‌شود، بلاک ناقص منتشر نمی‌شود و
    retry/restart و replay همان شاخه tip را خراب نمی‌کنند. gap/hash conflict
    رد می‌شود؛ قفل مشترک ingestion/replay/benchmark و خواندن دادهٔ منتشرشده
    در APIهای فعلی آزموده شدند. ۸۵ تست کتابخانه و ۸ تست شاهد اصلی موفق‌اند.
    انتقال رکوردهای عملیاتی metadata/intelligence/exposure در DB-4.3 تکمیل شد.
  - [x] **DB-4.3 — مصرف‌کنندگان و پذیرش:** API، صف و مشاهدهٔ metadata،
    منبع/claim/review، clustering و exposure به همان قرارداد متصل شدند.
    init/check با نام database تنظیم‌شده از صفر کار می‌کند؛ تست زنده بدون
    ایجاد یا seed جدول native موفق شد. metadata به head واقعی solid متصل
    است و retry/backoff و تغییر head آزموده شدند. revoke/منبع غیرفعال،
    seal نتایج، ترتیب و شاهد مسیر، مرز صرافی، replay و API دارای auth پذیرفته شدند.
    snapshot/ML محلی تکراری پس از پذیرش ذخیره/بازگشایی/Export مرکزی حذف شد؛
    تازگی رکورد عملیاتی ترون در مرکز کنترل می‌شود. ۷۸ تست کتابخانه، ۸ تست
    شاهد اصلی، سناریوی زندهٔ عملیاتی، سه رگرسیون دریافت و شش مرز خرابی نوشتن
    موفق‌اند. schema v6 تغییر نکرد؛ حذف فایل/mount قدیمی SQL در DB-5 انجام شد.
- [x] **DB-5 — حذف دوباره‌کاری:** SQL بومی و mount ترون، DTOهای ذخیره‌سازی
  بی‌مصرف و پنج کپی migration بیت‌کوین حذف شدند. هر شش فاز SQL فقط در
  `shared/warehouse/sql/migrations` از همان قرارداد تولید و از نظر stale یا
  فایل اضافه کنترل می‌شوند؛ runtime چهار شبکه مستقیماً از کتابخانه نصب می‌کند.
  job ترون مانند سه شبکهٔ دیگر پیش از API/workerها اجرا می‌شود. initializer
  واقعی هر چهار شبکه روی چهار ClickHouse مستقل و خالی موفق شد: ۴۲ جدول،
  ۵۶ view و ۷۳۹ ستون با engine/view/key/index یکسان؛ check و نصب مجدد،
  schema/ledger را تغییر ندادند. قرارداد v6 و checksumهای آن تغییر نکردند.
- [x] **پذیرش DB در سطح کد:** روی چهار دیتابیس خالی، شکل table/view/column/index و
  image یکسان باشد؛ ingest، restart، retry، reorg و query تحقیق هر چهار شبکه
  از مسیر مشترک موفق شوند. هیچ writer/reader/worker فعال جدول native قبلی را
  مصرف نکند. این معیار بسته‌شدن یکسان‌سازی است.
  - [x] نصب چهار initializer روی سرورهای مستقل و مقایسهٔ کامل ساختار در DB-5.
  - [x] اجرای دوبارهٔ مسیرهای داده و مصرف‌کنندگان هر چهار adapter روی v6؛
    آزمون انتشار ناقص و بازیابی بدون تکرار و بدون پیش‌افتادن checkpoint.
  - [x] بازیابی reorg در Bitcoin/BSC؛ توقف امن در تعارض هش finalized اتریوم
    و solid ترون، شامل restart و poll بدون بلاک جدید؛ سپس retry با منبع اصلاح‌شده.
  - [x] ممیزی وابستگی فعال به جدول‌های بومی و ثبت نتیجهٔ نهایی پذیرش کد؛
    پذیرش RPC/VM واقعی همچنان در بخش‌های ۲ و ۶ جداست.

  نتیجهٔ ۲۰۲۶-۱۱-۰۷: هر ۱۳ سناریوی ClickHouse واقعی موفق شدند. بررسی هش
  checkpoint در poll/restart، بستن مرحله‌ای خطاهای ترون و retry محدود خواندن
  قرارداد در قطع اتصال اصلاح شدند؛ [نتایج و فرمان‌ها](docs/TESTING.md).

## ۲. دریافت و قرارداد API قابل استفاده

- [x] **پذیرش محدود RPC عمومی واقعی:** Bitcoin از PublicNode با انتخاب کاربر،
  Ethereum/BSC از PublicNode و TRON از TronGrid نمونه؛ ۵ بلاک و ۸۷۹۱ تراکنش
  روی چهار ClickHouse مستقل دریافت و با RPC دوباره تطبیق شدند. restart بدون
  تکرار، checkpoint، API تحقیق دارای auth و گراف غیرخالی هر چهار شبکه موفق‌اند.
  کارمزد blob، دریافت بدون Basic Auth برای BTC و خواندن صفحه‌ای شاهد بلاک بزرگ
  اصلاح شدند؛ cache فقط ساخت تعریف ثابت قرارداد را نگه می‌دارد.
  trace عمومی EVM و state تاریخی/موجودی پایدار تأیید نشدند؛ این آزمون جای
  پذیرش RPC عملیاتی، follow طولانی یا مسیر مرکزی را نمی‌گیرد. [جزئیات](docs/TESTING.md).
- [ ] بازهٔ شروع و replay از RPC، catch-up و follow هر چهار شبکه؛ RPC
  capability/finality، reconnect، prevout/trace، gap، dead-letter و repair
  محدود آزموده شوند. history ناقص در پاسخ‌ها صریح بماند.
- [ ] قرارداد API و خطا، auth، pagination، range/time/asset filters، graph،
  activity، holdings، paths، attribution و source snapshot در چهار شبکه
  یکدست شود؛ تفاوت معنایی UTXO حفظ شود.
- [ ] پذیرش انتهابه‌انتها از RPC واقعی → ClickHouse محلی → API شبکه →
  Analytical Node → پنل/Neo4j برای هر چهار شبکه، با مبلغ و تعداد قابل تطبیق.

## ۳. دادهٔ intelligence و ریسک معتبر

- [ ] خوراک معتبر/مجاز entity، sanctions و منابع ریسک: import، provenance،
  نسخه، refresh، review، revoke و تغییر برچسب؛ دیتاست و کیفیت آن جدا از کد پذیرفته شود.
- [ ] clustering/entity و exposure مستقیم/غیرمستقیم: مدل مشترک و اجرای معتبر
  چهار شبکه، جلوگیری از ادعای مالکیت بی‌شاهد و کنترل CoinJoin/ابهام.
- [ ] یک سیاست توضیح‌پذیر و نسخه‌دار ریسک در مرکز و APIها؛ تازگی شواهد،
  امتیاز هر دلیل، UNKNOWN و تست false positive/negative روی مجموعهٔ برچسب‌دار.
  ML صرفاً پس از داشتن داده و ارزیابی مشخص؛ پیش‌نیاز ساخت جداول بیشتر نیست.

## ۴. KYT، هشدار و پرونده

- [ ] watchlist و ثبت آدرس/تراکنش تحت پایش با شناسهٔ مشتری/سامانهٔ متصل؛
  قواعد و thresholdهای قابل تنظیم، بررسی اولیه و بازبینی پس از تغییر intelligence.
- [ ] alert پایدار و بدون تکرار، severity، acknowledgement، assignment،
  وضعیت رسیدگی، webhook/API و retry قابل مشاهده؛ محدودیت نرخ و backlog کنترل شود.
- [ ] پروندهٔ تحقیق: یادداشت، timeline، شواهد، ارتباط alert/transaction،
  workflow بازبینی و گزارش قابل اشتراک؛ snapshot فعلی مبنای توسعه است.

## ۵. ردیابی پیشرفتهٔ موردنیاز چهار شبکه

- [ ] تکمیل پوشش نسخه‌دار DeFi، swaps، mixers، NFT و رفتارهای obfuscation؛
  registry هویت و شواهد واقعی از حدس selector/flow تفکیک شوند.
- [ ] cross-chain tracing بین شبکه‌های تحت پوشش: تطبیق دو سمت bridge،
  amount/token/time و شناسهٔ عملیات، نمایش confidence و شاهد اتصال در گراف مرکزی.
  وجود correlation key یا استفاده از bridge به‌تنهایی اثبات انتقال یا پول‌شویی نیست.
- [ ] مجموعهٔ ارزیابی معتبر و تست عدم رگرسیون؛ ادعای پوشش هر detector و
  منابع attribution با نتیجهٔ قابل سنجش ثبت شود.

## ۶. انتشار و بهره‌برداری روی VMها

- [ ] نسخهٔ پشتیبانی‌شده و یکسان ClickHouse/Neo4j، imageهای release یکسان،
  schema check، restart policy و ترتیب startup؛ سازگاری نسخهٔ جدید قبل از تغییر داده آزموده شود.
- [ ] TLS، RBAC تحلیلگر/بازبین/مدیر، audit عملیات، مدیریت secret و محدودیت
  دسترسی شبکه؛ ClickHouseها محلی و Neo4j فقط روی Main بمانند.
- [ ] پایش lag، نرخ ingest/query، CPU/RAM/disk، خطا و پوشش؛ alert عملیاتی،
  backup زمان‌بندی‌شده و restore واقعی مستقل برای هر شبکه.
- [ ] benchmark با بار هدف روی RPC و VM واقعی، سیاست retention و اندازهٔ
  history، بازیابی بعد از outage/reorg و تعیین منابع نهایی.
- [ ] راهنمای اجرا/استفاده با release پذیرفته‌شده تطبیق و walkthrough کاربر
  روی هر چهار شبکه انجام شود. تست fixture جای پذیرش VM را نمی‌گیرد.

**یکسان‌سازی DB در سطح کد پذیرفته شد. مرحلهٔ بعد بخش ۲، «دریافت و قرارداد API
قابل استفاده» است: یکدست‌کردن پاسخ/خطا، pagination و فیلترها با حفظ تفاوت
UTXO/EVM/TRON، سپس پذیرش کامل RPC عملیاتی و مسیر کامل روی VMهای واقعی.
آزمون محدود RPC عمومی پیش از این ادامه انجام و با محدودیت‌هایش ثبت شد.** معیار قابلیت‌های هدف و وضعیت موجود در
[PROJECT_STATUS](docs/PROJECT_STATUS_FA.md) بر پایهٔ منابع رسمی Chainalysis آمده است.
