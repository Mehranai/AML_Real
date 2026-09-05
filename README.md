# AML Whole

ورودی مشترک سامانه تحقیق کیف پول برای TRON و Ethereum.
این پوشه دو سرویس مستقل را کنار هم نگه می‌دارد؛ داده‌ها و منطق شبکه‌ها با هم ادغام نمی‌شوند.

راهنمای کامل استقرار روی VM اصلی و VM مستقل هر شبکه:
[`docs/MULTI_VM_DEPLOYMENT_FA.md`](docs/MULTI_VM_DEPLOYMENT_FA.md)

## اجرا در Windows / Docker Desktop

از ریشه همین پوشه اجرا کنید:

```powershell
cd D:\Sarbazi\AML_Whole
.\scripts\aml.ps1 up
```

آدرس صفحه اصلی: http://127.0.0.1:8080

شبکه را انتخاب کنید، آدرس همان شبکه را وارد کنید و Investigate را بزنید.
صفحه کامل تحقیق آن شبکه باز می‌شود و آدرس به صورت خودکار جست‌وجو می‌شود.
پیوند Networks در بالای صفحه شما را به انتخاب شبکه برمی‌گرداند.
Open console نیز بدون واردکردن آدرس، صفحه همان شبکه را برای جست‌وجوی مسیر دو کیف پول باز می‌کند.

پیش‌نیازها: Docker Desktop با Linux containers و تنظیمات موجود هر شبکه:

- `dockerizd_tron/app/.env`
- `dockerizd_ethereum/.env`

کلید RPC و رمز دیتابیس فقط در فایل مربوط به همان شبکه باقی می‌ماند.
فایل `.env` ریشه اختیاری است؛ برای تغییر پورت 8080 یا آدرس APIها از `.env.example` همین پوشه استفاده کنید.
اگر PowerShell اجرای اسکریپت را محدود کرده است، دستور معادل:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\aml.ps1 up
```

اجرای پیش‌فرض فقط APIها و وابستگی‌های دیتابیس را بالا می‌آورد. Ingestion جدید شروع نمی‌شود و از سهمیه RPC مصرف نمی‌کند.
کارگرهایی که از قبل در حال اجرا بوده‌اند متوقف نمی‌شوند.

```powershell
# اجرای ingestion و workerها همراه رابط
.\scripts\aml.ps1 up -WithIngestion
# بازسازی تصاویر Rust پس از تغییر کد backend
.\scripts\aml.ps1 up -Build -WithIngestion
# وضعیت و آخرین log هر سه stack
.\scripts\aml.ps1 ps
.\scripts\aml.ps1 logs
# توقف بدون حذف volumeهای دیتابیس
.\scripts\aml.ps1 down
```

## ساختار و مسیر درخواست

```text
AML_Whole/
  compose.yaml                  فقط gateway مشترک
  scripts/aml.ps1                اجرای stackها از مسیر نسبی جدید
  gateway/
    Dockerfile                  تصویر سبک Nginx و UIها
    nginx.conf                  تنظیمات عمومی و access log بدون آدرس کیف پول
    default.conf.template       مسیردهی ثابت به API هر شبکه
    web/                        فرم انتخاب شبکه، آدرس و وضعیت سرویس‌ها
    tests/                      تست آدرس، مسیردهی و جریان مرورگر
  dockerizd_tron/app/            سرویس Rust، UI، SQL و Compose موجود TRON
  dockerizd_ethereum/            سرویس Rust، UI، SQL و Compose موجود Ethereum
  dockerizd_bsc/                 schema/ingestion/replay آماده؛ investigation API و UI هنوز فعال نیست
  contracts/                     قرارداد versioned بین Gateway و Chain VMها
```

```text
Browser: network + wallet
  -> /networks/tron/?address=...       یا /networks/ethereum/?address=...
  -> همان UI موجود شبکه، با ارسال خودکار فرم
  -> /api/tron/...                    یا /api/ethereum/...
  -> Nginx -> API Rust همان شبکه
  -> ClickHouse همان شبکه -> تحلیل/گراف موجود -> Neo4j طبق جریان همان شبکه
  -> JSON -> نمودار و پنل شواهد همان UI
```

Gateway موتور تحلیل یا دیتابیس جدیدی ندارد. فایل‌های HTML از مسیر اصلی شبکه‌ها در زمان build کپی می‌شوند؛
دو نسخه مستقل از UI در کد نگهداری نمی‌شود. بعد از تغییر UI، gateway را دوباره build کنید.
صفحات مستقیم قبلی روی 4001 و 5001 همچنان قابل استفاده‌اند؛ برای به‌روزرسانی HTML تعبیه‌شده در Rust باید تصویر همان شبکه نیز build شود.

مسیرهای investigation، holdings/fingerprint موجود TRON، جست‌وجوی مسیر تا سقف قبلی 10 hop،
ارسال POST به Neo4j و snapshotهای TRON بدون تغییر قرارداد backend عبور داده می‌شوند.
درگاه قابلیت تازه‌ای برای تطبیق bridge بین شبکه‌ها اضافه نمی‌کند؛ مسیر بین TRON و Ethereum هنوز یک گراف مشترک نیست.
هیچ ML یا محاسبه ریسک جدیدی به این تغییر اضافه نشده است.

## پورت‌ها و حفظ داده بعد از جابه‌جایی

| سرویس | TRON | Ethereum |
|---|---|---|
| API | 4001 | 5001 |
| ClickHouse HTTP | 18123 | 28123 |
| ClickHouse Native | 19000 | 29000 |
| Neo4j Browser | 18474 | 28474 |
| Neo4j Bolt | 17687 | 27687 |

پورت‌ها defaults هستند و ممکن است در `.env` شبکه تغییر کرده باشند. در آن صورت upstream ریشه را نیز هماهنگ کنید.
اسکریپت نام پروژه Compose قبلی را حفظ می‌کند: TRON با `app` و Ethereum با `dockerizd_ethereum`.
بنابراین همان named volumeهای قبلی استفاده می‌شوند. اگر قبلاً نام دیگری داشته‌اید، قبل از up از
`docker compose ls -a` کمک بگیرید و پارامترهای `-TronProject` و `-EthereumProject` را تنظیم کنید.
صرفاً start کردن کانتینر قدیمی کافی نیست: bind mount آن ممکن است هنوز به پوشه قبلی اشاره کند.
اجرای compose up از مسیر جدید، mount فایل SQL ترون را با مسیر جدید بازسازی می‌کند.
هیچ دستور `down -v` یا حذف داده‌ای در اسکریپت وجود ندارد.

## استقرار شبکه‌ها روی سیستم‌های جدا

هر شبکه می‌تواند Compose خودش را روی ماشین مستقل اجرا کند. روی ماشین gateway فقط این دستور کافی است:

```powershell
docker compose up -d --build
# یا
.\scripts\aml.ps1 up -GatewayOnly
```

در `.env` ریشه آدرس خصوصی APIها را تنظیم کنید:

```dotenv
AML_TRON_UPSTREAM=http://tron-api.internal:4001
AML_ETHEREUM_UPSTREAM=http://ethereum-api.internal:5001
```

مقدار upstream باید فقط origin باشد: بدون path، slash انتهایی یا username/password.
TLS برای upstreamهای HTTPS بررسی می‌شود. مرورگر فقط با gateway ارتباط دارد؛ CORS بین پورت‌های شبکه لازم نیست.
روی Linux، پیش‌فرض `host.docker.internal` به gateway میزبان resolve می‌شود ولی سرویس bindشده فقط به 127.0.0.1
ممکن است از کانتینر قابل دسترس نباشد. از شبکه Docker مشترک یا IP خصوصی قابل دسترس استفاده کنید.
برای ماشین‌های مجزا پورت API باید با firewall فقط برای gateway قابل دسترس شود؛ دیتابیس‌ها را عمومی نکنید.

اجرای role محلی هر VM با اسکریپت واحد:

```powershell
.\scripts\vm.ps1 -Role tron -Action up -Build
.\scripts\vm.ps1 -Role ethereum -Action up -Build
.\scripts\vm.ps1 -Role main -Action up -Build
```

در deployment چند-VM هر دستور فقط روی VM مربوط به همان role اجرا می‌شود. راهنمای جزئی تنظیم IP، کلید سرویس،
Basic Auth، firewall، تست و انتقال آفلاین در `docs/MULTI_VM_DEPLOYMENT_FA.md` قرار دارد.

این ورودی به‌صورت پیش‌فرض روی localhost منتشر می‌شود. Basic Auth اختیاری برای ارائه و شبکه داخلی پیاده‌سازی شده است؛
برای دسترسی عمومی یا چندکاربره، Gateway را پشت OIDC/SSO و TLS سازمان قرار دهید.

## وضعیت و تست

`/health` فقط سلامت خود gateway است. وضعیت ClickHouse و Neo4j در صفحه اصلی از `/ready` هر شبکه خوانده می‌شود.
Ready به معنی تکمیل تاریخچه ingestion نیست. Unknown یعنی API پاسخ معتبر نداده و وضعیت دیتابیس معلوم نیست.
آفلاین‌بودن یک شبکه مانع استفاده از شبکه دیگر نیست. درخواست‌های 502/504 اتصال با خطای JSON و وضعیت 503 نمایش داده می‌شوند.

```powershell
npm test
npm ci
npx playwright install chromium
npm run test:browser
```

Node فقط برای تست توسعه است؛ در محیط اجرایی gateway به Node یا Python نیاز ندارد.
تست مرورگر به Docker روشن و تصویر ساخته‌شده gateway نیاز دارد. یک stack موقت مستقل و دو API ساختگی ایجاد می‌کند
و در پایان آن‌ها را جمع می‌کند؛ به دیتابیس واقعی داده تستی اضافه نمی‌کند.
تست مرورگر از پاسخ‌های کنترل‌شده استفاده می‌کند و صحت مسیردهی/UI را می‌سنجد، نه صحت داده زنده زنجیره.
برای تست زنده، آدرسی از داده‌های ingestشده ClickHouse همان شبکه جست‌وجو کنید.

### نتیجه بررسی این نسخه

در 2026-09-03، تست‌های آدرس، Nginx، جداسازی درخواست شبکه‌ها، حفظ query و POST، قطع یک API،
جست‌وجوی خودکار و نمایش نمودار/مسیر در مرورگر پاس شدند. نماهای 390، 768 و 1440 پیکسل بررسی شدند.
بررسی زنده از درگاه نیز برای این آدرس‌های موجود در دیتابیس پاسخ 200 داد:

- TRON: `TGX6tRfV4CcUH4hbsujqhcL8omACeGu4kQ`، گراف محدود به 25 انتقال با 26 node.
- Ethereum: `0x238a4d9fb5337fa220f98f2829d9d903664b337b`، گراف با 1 انتقال و 2 node.

برای هر دو جفت آدرس ذخیره‌شده، جست‌وجوی مسیر با سقف 10 hop نیز پاسخ داد. این تست به معنی کشف مسیر 10 مرحله‌ای نیست؛
مسیر پیدا‌شده مستقیم بود. جست‌وجوی TRON با `per_address_limit=10` مقدار `truncated=true` داشت و ادعای کامل‌بودن نمی‌کند.
تغییرات این مرحله schema را تغییر نمی‌دهند و داده‌های قبلی حذف نشده‌اند.

مرجع تنظیمات پروکسی: [مستندات رسمی Nginx](https://nginx.org/en/docs/http/ngx_http_proxy_module.html).
