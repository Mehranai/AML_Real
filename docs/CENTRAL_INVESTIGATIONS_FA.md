# Neo4j مرکزی، بررسی کیف پول و ریسک بدون ML

## معماری فعلی

```text
Browser: choose network + wallet / source + target
    |
Main VM: Nginx Gateway
    |---- Analytical Node (Rust)
    |         |---- one central Neo4j
    |         |---- TRON API ---- TRON ClickHouse
    |         |---- Ethereum API ---- Ethereum ClickHouse
    |
Browser renders the returned graph

Each chain VM: own node/RPC -> ingestion -> canonical ClickHouse evidence
```

فقط VM اصلی Neo4j دارد. شبکه‌ها داده مرجع را در ClickHouse خود نگه می‌دارند و JSON گراف، fingerprint،
برچسب‌ها و exposure را از همان داده می‌سازند. رسم نمودار در مرورگر انجام می‌شود، نه داخل خود Neo4j.
سرویس مرکزی قبل از تحویل پاسخ به مرورگر، یک snapshot از خروجی شبکه و گراف آن در Neo4j ثبت می‌کند.
اگر ذخیره مرکزی شکست بخورد، درخواست موفق اعلام نمی‌شود؛ گراف قبلی در صفحه باقی می‌ماند.

TRON، Ethereum و BSC به این مسیر متصل‌اند. BSC با network_id برابر eip155:56 و upstream مستقل
روی پورت 6001 کار می‌کند؛ راهنمای آن در dockerizd_bsc/docs/VM_READINESS_FA.md است.
یکی بودن Neo4j به‌خودی‌خود به معنی تشخیص خودکار ارتباط یک bridge بین دو شبکه نیست.
برای آن، اثبات تطابق رویداد مبدأ و مقصد و decoder هر bridge همچنان لازم است.

## چرخه بررسی

1. کاربر شبکه و کیف پول را انتخاب می‌کند.
2. Gateway درخواست investigation یا paths را به Analytical Node می‌دهد.
3. Analytical Node فقط API همان شبکه را با کلید داخلی فراخوانی می‌کند؛ URL دلخواه کاربر پذیرفته نمی‌شود.
4. API شبکه داده canonical را از ClickHouse می‌خواند. در این مرحله هیچ Neo4j محلی لازم نیست.
5. برای بررسی یک کیف پول، سیاست ریسک مرکزی بر شواهد برگشتی اجرا می‌شود؛ برای جست‌وجوی مسیر، ریسک کیف پول محاسبه نمی‌شود.
6. پاسخ کامل و گراف آن با شناسه تصادفی تحقیق و network_id در یک تراکنش مرکزی ذخیره می‌شوند.
7. صفحه گراف و شواهد را نشان می‌دهد و وضعیت Temporary دارد.
8. Export فقط همان شناسه تحقیق را به حالت saved تغییر می‌دهد؛ نه داده جدید می‌خواند و نه ریسک را دوباره حساب می‌کند.
9. از منوی Saved investigations می‌توان آخرین ۱۰۰ مورد ذخیره‌شده همان کاربر را باز کرد.

Export در اینجا یعنی دائمی کردن snapshot در Neo4j، نه دانلود فایل و نه خروجی کامل تاریخچه شبکه.
محدودیت عمق، تعداد یال و بازه جست‌وجوی همان بررسی برقرار می‌ماند. فیلترهای نمایشی صفحه، snapshot اصلی را تغییر نمی‌دهند.
منوی ذخیره‌شده‌ها بررسی‌های همان شبکه را نشان می‌دهد. API فهرست، پارامتر before برای صفحه‌های قدیمی‌تر دارد.

## موقت و دائمی

- موقت به معنی داده واقعاً ثبت‌شده روی دیسک با زمان انقضا است، نه صرفاً داده داخل RAM.
- پیش‌فرض انقضا ۲۴ ساعت است؛ AML_GRAPH_TTL_HOURS بین ۱ تا ۱۶۸ ساعت تنظیم می‌شود.
- کار پاک‌سازی هر دقیقه حداکثر ۱۰ تحقیق منقضی را حذف می‌کند؛ هنگام backlog حذف فیزیکی ممکن است دیرتر انجام شود.
- تحقیق منقضی بلافاصله از API قابل خواندن یا Export نیست، حتی اگر حذف فیزیکی هنوز نوبتش نرسیده باشد.
- Export زمان انقضا را حذف می‌کند. Export تکراری زمان ذخیره اول را تغییر نمی‌دهد.
- قفل روی رکورد تحقیق و بررسی مجدد وضعیت، رقابت Export با پاک‌سازی را کنترل می‌کند.
- saved از restart سرویس باقی می‌ماند، ولی همچنان به volume سالم و backup نیاز دارد. حذف volume داده را از بین می‌برد.
- هیچ داده قدیمی ClickHouse یا Neo4j شبکه‌ها در این تغییر خودکار حذف یا به DB مرکزی منتقل نشده است.

## ساختار Neo4j

| جزء | اطلاعات و کاربرد |
|---|---|
| Investigation | id، network_id، network، mode، address، target، state، created_at_unix_ms، expires_at_unix_ms یا saved_at_unix_ms |
| owner | هش هویت کاربر یا session؛ برای محدود کردن خواندن و Export به صاحب تحقیق |
| payload_json | خروجی کامل همان لحظه، از جمله شواهد، پوشش داده و نتیجه سیاست ریسک |
| snapshot_hash | SHA-256 پاسخ ذخیره‌شده، برای مقایسه اینکه شواهد همان snapshot هستند؛ امضای دیجیتال نیست |
| lock_version | کنترل هم‌زمانی Export و پاک‌سازی؛ کاربرد تحلیلی ندارد |
| InvestigationWallet | key اختصاصی تحقیق، address، subject_key، network_id و evidence_json |
| CONTAINS | عضویت node در یک تحقیق، مبنای پاک‌سازی ایزوله |
| TRANSFER | شناسه یال، network_id، شناسه تحقیق، مبدأ و مقصد، tx_hash، asset_id، amount به صورت رشته و شواهد یال |

کلید کیف پول در این گراف شامل investigation_id + network_id + address است.
در نتیجه یک آدرس در چند تحقیق nodeهای مستقل دارد و پاک‌سازی یکی روی دیگری اثر نمی‌گذارد.
subject_key شامل network_id + address است تا بتوان همان کیف پول را بین snapshotها پیدا کرد.
مقادیر بزرگ توکن به float تبدیل نمی‌شوند. JSON کامل کنار projection گراف برای بازنمایی ثابت شواهد نگه داشته می‌شود؛
این کپی محدودِ نتیجه بررسی است، نه کپی کل ClickHouse. سقف هر پاسخ ۱۶ MiB و هر گراف ۲۰ هزار node/edge است.

برای مشاهده گراف ذخیره‌شده در Neo4j Browser با دسترسی مدیر:

```cypher
MATCH (i:Investigation {state:'saved'})
RETURN i.id, i.network_id, i.address, i.saved_at_unix_ms
ORDER BY i.saved_at_unix_ms DESC LIMIT 100;
```

```cypher
MATCH (i:Investigation {id:$investigation_id})-[:CONTAINS]->(w:InvestigationWallet)
OPTIONAL MATCH (w)-[r:TRANSFER]->(other:InvestigationWallet)
WHERE other.investigation_id = i.id
RETURN i, w, r, other;
```

## ریسک بدون ML

ML موجود دست‌نخورده و غیرفعال باقی مانده است. امتیاز اصلی صفحه از analytical_node/src/risk.rs می‌آید.
این روش یک سیاست شواهدی نسخه‌دار به نام aml_evidence_v1 است؛ وزن‌ها انتخاب کارشناسی اولیه‌اند،
نه پارامترهای آموزش‌دیده و نه احتمال آماری پول‌شویی. خروجی همیشه probability_claimed=false دارد.

| شاهد | سهم سیاست اولیه |
|---|---|
| برچسب پرریسک فعال و تأییدشده | حداکثر ۷۰، متناسب با درجه ریسک ثبت‌شده |
| دریافت مستقیم از seed پرریسک | حداکثر ۵۵، ضرب در قدرت exposure |
| ارسال مستقیم به seed پرریسک | حداکثر ۴۵، ضرب در قدرت exposure؛ در حال حاضر از مسیر exposure اتریوم |
| exposure غیرمستقیم | حداکثر ۲۵، ضرب در قدرت exposure |
| رویداد mixer با confidence حداقل ۰٫۸ | ۱۵؛ به‌تنهایی اثبات جرم نیست |
| حداقل سه receipt و ارسال نزدیکِ هم‌دارایی و تقریباً هم‌اندازه | ۵، برای آدرس فاقد انتساب سرویس شناخته‌شده |

فقط قوی‌ترین exposure امتیاز می‌گیرد تا مسیرهای تکراری چند رأی مستقل تلقی نشوند.
exposure عبوری از سرویس در این سیاست امتیاز نمی‌گیرد و صرفاً زمینه بررسی است.
استفاده معمول از swap، bridge یا exchange به‌تنهایی امتیاز ندارد.
در الگوی ارسال سریع، فاصله زمانی حداکثر ۱۰ دقیقه، اختلاف مقدار حداکثر ۱۰ درصد و تراکنش‌ها متمایز هستند.
این الگو اثبات نمی‌کند همان واحدهای دریافتی خرج شده‌اند؛ موجودی قبلی و فعالیت تجاری ممکن است توضیح دیگری باشند.

جمع سهم‌ها حداکثر ۱۰۰ است. ۶۰ به بالا HIGH، ۲۵ تا کمتر از ۶۰ MEDIUM، مثبت کمتر از ۲۵ LOW است.
اگر هیچ سیگنالی نباشد و حداقل سه انتقال دیده شده باشد، خروجی NO_SIGNALS است، نه «کیف پول قطعاً پاک».
اگر شاهد کافی نباشد، risk_score=null و UNKNOWN برمی‌گردد، نه صفر.
پنل Risk دلیل، سهم، جزئیات شاهد و محدودیت پوشش داده را نشان می‌دهد.

نکته مهم: امتیاز بر اساس شواهد بازگردانده‌شده در بررسی است، نه تضمین تحلیل تاریخچه کامل شبکه.
TRON فعلاً exposure دریافتی منتشرشده از seed را به صورت خلاصه می‌دهد؛ همه مسیرهای کامل ارسالی/دریافتی را به اندازه اتریوم ندارد.
برچسب‌های معتبر، تکمیل ingestion، propagation به‌روز و بررسی انسانی برای کیفیت نتیجه ضروری‌اند.
وزن‌ها و آستانه‌ها باید با پرونده‌های واقعی و نظر تیم compliance بازبینی شوند؛ تغییر سیاست نیاز به نسخه و تست جدید دارد.

## اجرای Linux

از ریشه مخزن، فایل محیط هر VM را از نمونه خودش بسازید.
در VMهای شبکه، رمز ClickHouse، RPC، API_BIND_ADDRESS، AML_SERVICE_AUTH_REQUIRED=true و کلید مشترک AML_SERVICE_KEY تنظیم شوند.
در VM اصلی:

```dotenv
AML_BIND_ADDRESS=10.20.0.10
AML_PORT=8080
AML_TRON_UPSTREAM=http://10.20.0.21:4001
AML_ETHEREUM_UPSTREAM=http://10.20.0.22:5001
AML_SERVICE_KEY=REPLACE_WITH_AT_LEAST_32_RANDOM_CHARACTERS
NEO4J_PASSWORD=REPLACE_WITH_A_STRONG_PASSWORD
AML_GRAPH_TTL_HOURS=24
```

```bash
# هر دستور روی VM مربوط به خودش
bash scripts/vm.sh tron up --build
bash scripts/vm.sh ethereum up --build
bash scripts/vm.sh main up --build
bash scripts/vm.sh main check
bash scripts/smoke-test.sh --main-url http://10.20.0.10:8080
```

سپس صفحه VM اصلی را باز کنید، شبکه و آدرس را وارد کنید، Risk را ببینید و Export را بزنید.
برای ارزیابی readiness مرکزی، /ready را استفاده کنید؛ /health فقط زنده بودن Gateway است.
برای انتقال آفلاین، image جدید aml-analytical-node:local هم باید همراه سایر imageها صادر و وارد شود؛
scripts/export-images.sh آن را شامل می‌شود. archive قدیمی deployment-artifacts خودکار به‌روز نمی‌شود.

اگر نسخه قبلی را اجرا می‌کردید، کانتینر Neo4j قدیمی هر Chain VM را با نام دقیق آن متوقف کنید.
با docker ps -a و docker inspect مالکیت Compose آن را بررسی کنید؛ volume قدیمی را تا backup و تصمیم درباره انتقال داده نگه دارید.
سرویس‌های جدید شبکه Neo4j ایجاد نمی‌کنند. endpointهای قدیمی neo4j/import با HTTP 410 به مسیر مرکزی ارجاع می‌دهند.
کد و ابزارهای legacy داخل شبکه‌ها برای سازگاری باقی‌اند ولی بخشی از مسیر عملیاتی جدید نیستند.

## دسترسی و نگهداری

برای محیط کارفرما، Basic Auth با حساب مجزای هر تحلیل‌گر و TLS در ورودی Main فعال باشد.
Gateway هدر هویت کاربر را خودش تولید می‌کند و هدر جعلی مرورگر را جایگزین می‌کند.
بدون Basic Auth، مالکیت به cookie محلی مرورگر وابسته است؛ پاک کردن cookie یا مرورگر دیگر، تحقیق قبلی را در UI در دسترس قرار نمی‌دهد.
این حالت فقط برای آزمایش مناسب است. کلید داخلی و رمز Neo4j نباید به کاربر مرورگر داده شوند.
API شبکه‌ها فقط از IP خصوصی VM اصلی قابل دسترسی باشند. پورت‌های Neo4j مرکزی فقط روی localhost منتشر می‌شوند.
برای ارتباط بین VMها از شبکه خصوصی قابل‌اعتماد یا TLS/VPN استفاده کنید؛ key روی HTTP رمزنگاری نمی‌شود.

Backup جداگانه از volume مرکزی، مانیتورینگ فضای دیسک و خطاهای پاک‌سازی لازم است.
این پیاده‌سازی تک‌نمونه‌ای است؛ HA، SSO/RBAC سازمانی، امضای شواهد و audit log سازمانی هنوز تکمیل نشده‌اند.

## محل کد و API

- analytical_node/src/main.rs: اعتبارسنجی، احراز هویت، دریافت پاسخ شبکه، normalize گراف، endpointها و worker پاک‌سازی.
- analytical_node/src/store.rs: schema/indexهای Neo4j، ذخیره اتمیک، Export، خواندن و پاک‌سازی.
- analytical_node/src/risk.rs: سیاست ریسک بدون ML و تست‌های تصمیم‌گیری.
- gateway/default.conf.template: routing به سرویس مرکزی و APIهای ClickHouse شبکه‌ها.
- gateway/web/assets/investigations.js: وضعیت snapshot، Export، فهرست ذخیره‌شده‌ها و شواهد ریسک.
- dockerizd_tron/app/src/services/tron/wallet_investigation.rs: گردآوری شواهد TRON و خروجی مستقل exposure.
- dockerizd_ethereum/src/investigation.rs: ساخت بررسی و مسیرهای اتریوم از ClickHouse.
- compose.yaml: فقط در VM اصلی، Gateway + Analytical Node + Neo4j.
- gateway/tests/browser.mjs: تست یکپارچه با Neo4j واقعی Docker و شواهد ساختگی مشخص.

```text
GET  /api/{tron|ethereum|bsc}/wallet/{address}/investigation
GET  /api/{tron|ethereum|bsc}/wallet/{source}/paths/{target}
GET  /api/investigations?before=<timestamp>
GET  /api/investigations/{id}
POST /api/investigations/{id}/export
```

Export به Content-Type: application/json نیاز دارد. شناسه متعلق به کاربر دیگر یا منقضی پاسخ 404 می‌گیرد.
قابلیت ۱۰ hop در جست‌وجوی مسیر حفظ شده، ولی حدود ایمنی جست‌وجو همچنان برقرار است.

مراجع فنی: [Neo4j Query API](https://neo4j.com/docs/query-api/current/query/) و
[کنترل هم‌زمانی Neo4j](https://neo4j.com/docs/operations-manual/current/database-internals/concurrent-data-access/).
