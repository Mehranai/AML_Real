<!-- @style: ./style.css -->
# AML Whole

سامانه تحقیق کیف پول برای TRON، Ethereum و BNB Smart Chain با یک مخزن Git و استقرار مستقل هر شبکه.
کاربر در VM اصلی شبکه و آدرس کیف پول را انتخاب می‌کند؛ Analytical Node شواهد API همان شبکه را دریافت می‌کند.
فقط VM اصلی Neo4j دارد: بررسی‌ها ابتدا موقت‌اند و Export همان snapshot را دائمی می‌کند. امتیاز ریسک بدون ML و با دلایل قابل بررسی ارائه می‌شود.

[راهنمای Neo4j مرکزی، Export و ریسک بدون ML](docs/CENTRAL_INVESTIGATIONS_FA.md)

[راهنمای BSC: اجرا روی VM، داده‌های هویتی، holdings و محدودیت‌های تحویل](dockerizd_bsc/docs/VM_READINESS_FA.md)

راهنمای مرحله‌به‌مرحله نصب، شبکه، رمزها، انتقال آفلاین و تست:
[راهنمای استقرار روی Linux](docs/MULTI_VM_DEPLOYMENT_FA.md).

## اجرای ساده روی همین کامپیوتر

برای دیدن پروژه روی ویندوز فعلی، Docker Desktop را روشن کنید و در ترمینال اجرا کنید:

```cmd
cd /d D:\Sarbazi\AML_Whole
python scripts/run_local.py
```

سپس [صفحه اصلی](http://127.0.0.1:8080) را باز کنید. این دستور VM نمی‌سازد و به Multipass نیاز ندارد؛ از imageهای آماده و دیتابیس‌های موجود استفاده می‌کند.
تنظیمات اتصال محلی را فقط برای فرایند Docker اعمال می‌کند؛ فایل‌های `.env` مربوط به VMها تغییر نمی‌کنند.
اگر کانتینر دیتابیس از قبل موجود باشد، رمز همان کانتینر برای حفظ دسترسی استفاده می‌شود. رمزها نمایش داده نمی‌شوند.
برای بررسی سلامت `python scripts/run_local.py check`، برای وضعیت `python scripts/run_local.py status` و برای توقف بدون حذف داده `python scripts/run_local.py stop` را اجرا کنید.
دستور بالا فقط APIها را راه‌اندازی می‌کند؛ دریافت بلاک‌های جدید شروع نمی‌شود و workerهایی که از قبل روشن باشند نیز خاموش نمی‌شوند.

برای اجرای کامل هر سه شبکه، شامل ingest، metadata و تحلیل‌های خودکار موجود، از این فرمان استفاده کنید:

```cmd
python scripts/run_local.py up --build --with-ingestion
```

این فرمان imageهای جدید را می‌سازد و worker ترون (`tron-analytics`) و اتریوم (`ethereum-analytics`) را هم اجرا می‌کند. تحلیل exposure در BSC هنگام درخواست بررسی ولت انجام می‌شود؛ ابزارهای import و تأیید برچسب عمداً خودکار اجرا نمی‌شوند.
اجرای کامل، RPC و منابع کافی می‌خواهد و به معنی تکمیل تاریخچه شبکه یا تأیید صحت تمام تحلیل‌ها نیست.
[جزئیات اجرای تحلیل‌ها، مشاهده خروجی و محدودیت‌های باقی‌مانده](docs/AUTOMATIC_ANALYTICS_FA.md).
این میان‌بر فعلاً برای Docker Desktop ویندوز است؛ برای استقرار Linux از بخش‌های بعدی استفاده کنید.

## ساخت خودکار VMها روی یک کامپیوتر

اگر هنوز VM نساخته‌اید، این مسیر را استفاده کنید. پس از نصب یک‌باره Python 3.10+ و Multipass روی میزبان، پروژه چهار VM اوبونتو را می‌سازد و Docker، تنظیم IP، انتقال imageها و اجرای سرویس‌ها را انجام می‌دهد:

```powershell
cd D:\Sarbazi\AML_Whole
python scripts/provision_vms.py doctor
python scripts/provision_vms.py up
```

روی Linux به جای `python` از `python3` استفاده کنید. URL صفحه اصلی در پایان چاپ می‌شود.
imageهای برنامه باید آماده باشند؛ برای ساخت از سورس `up --build` و برای تحویل image آماده `up --bundle PATH` را استفاده کنید.
به طور پیش‌فرض ingestion خاموش است. اجرای دوباره VM تکراری نمی‌سازد و `stop` داده‌ها را حذف نمی‌کند.
این مسیر VMها را خودکار می‌سازد، اما نصب اولیه ابزار مجازی‌سازی میزبان و فعال بودن آن پیش‌نیاز است.

**قبل از اجرا [راهنمای ساخت خودکار VMها](docs/AUTO_VMS_FA.md) را بخوانید؛ حداقل RAM و فضای آزاد بررسی می‌شود.**

## اجرای Linux روی VMهای از قبل آماده

این بخش مسیر جایگزین برای چهار VM موجود است؛ بعد از اجرای سازنده خودکار بالا، این دستورها را دوباره اجرا نکنید.

پیش‌نیاز: Linux، Bash نسخه 4 یا بالاتر، Docker Engine و افزونه Docker Compose v2.
برای helperهای تست و انتقال، `curl`، `jq`، `openssl` و `sha256sum` لازم‌اند.
تمام دستورات زیر از ریشه مخزن اجرا می‌شوند. فقط role همان VM را اجرا کنید.

```bash
git clone https://github.com/Mehranai/AML_Real.git "$HOME/AML_Whole"
cd "$HOME/AML_Whole"
```

فایل‌های `.env` از قبل مقداردهی شده‌اند؛ نیازی به کپی `.env.example`، تولید کلید یا تنظیم متغیر در ترمینال نیست:

| VM | IP | فایل تنظیمات |
|---|---|---|
| Main | `10.20.0.10` | `.env` |
| TRON | `10.20.0.21` | `dockerizd_tron/app/.env` |
| Ethereum | `10.20.0.22` | `dockerizd_ethereum/.env` |
| BSC | `10.20.0.23` | `dockerizd_bsc/.env` |

این IPها باید واقعاً روی VMها تنظیم شده باشند؛ فایل env شبکه سیستم‌عامل را تنظیم نمی‌کند.
مقادیر مشترک، رمزهای آزمایشی شناخته‌شده‌اند و برای شبکه آزمایشی ایزوله هستند، نه استقرار عمومی production.
روی ماشین سازنده ابتدا imageهای به‌روز را با `bash scripts/export-images.sh --include-bsc` بسته‌بندی کنید.
پوشه `deployment-artifacts` را به هر VM منتقل کرده و پیش از دستورهای زیر اجرا کنید:

```bash
bash scripts/import-images.sh ./deployment-artifacts
```

روی TRON VM:

```bash
bash scripts/vm.sh tron up --api-only --pull-never
bash scripts/vm.sh tron check
```

روی Ethereum VM:

```bash
bash scripts/vm.sh ethereum up --api-only --pull-never
bash scripts/vm.sh ethereum check
```

روی BSC VM:

```bash
bash scripts/vm.sh bsc up --api-only --pull-never
bash scripts/vm.sh bsc check
```

روی Main VM:

```bash
bash scripts/vm.sh main up --pull-never
bash scripts/vm.sh main check
bash scripts/smoke-test.sh --main-url http://10.20.0.10:8080
```

launcher فایل `.env` همان role را خودکار می‌خواند. RPC، رمزها و کلید مشترک در فایل‌ها آماده‌اند؛
RPCهای فعلی عمومی و بدون کلید خصوصی‌اند و محدودیت نرخ، تاریخچه و trace دارند.
API شبکه‌ها در firewall فقط برای VM اصلی باز باشد؛ پورت دیتابیس‌ها روی localhost باقی می‌ماند.
صفحه اصلی: `http://10.20.0.10:8080`. احراز هویت UI در تنظیم آزمایشی خاموش است.
اگر IPها تغییر کنند، bind و upstreamهای متناظر را در همین فایل‌ها تغییر دهید.
تغییر رمز env به معنی تغییر رمز دیتابیس دارای volume قبلی نیست؛ برای رفع خطای ورود volume را حذف نکنید.

دستورهای بالا فقط API و دیتابیس را راه می‌اندازند. برای شروع ingestion و workerها، روی VM همان شبکه اجرا کنید:

```bash
bash scripts/vm.sh tron up --pull-never
# روی VM اتریوم:
bash scripts/vm.sh ethereum up --pull-never
# روی VM BSC:
bash scripts/vm.sh bsc up --pull-never
```

در دیتابیس خالی، تنظیم فعلی دریافت را از بلاک صفر آغاز می‌کند؛ آماده بودن API به معنی آماده بودن تاریخچه نیست.

## دستورهای روزانه

```bash
# API و دیتابیس، بدون شروع ingestion جدید
bash scripts/vm.sh tron up --api-only --pull-never
bash scripts/vm.sh ethereum up --api-only --pull-never
bash scripts/vm.sh bsc up --api-only --pull-never

# مشاهده وضعیت، log و توقف بدون حذف volume
bash scripts/vm.sh tron ps
bash scripts/vm.sh ethereum logs
bash scripts/vm.sh main check
bash scripts/vm.sh tron down
```

اجرای API-only، workerهایی را که قبلاً روشن بوده‌اند متوقف نمی‌کند.
نام Compose پروژه‌ها در launcher چند VM به‌ترتیب `aml-main`، `aml-tron`، `aml-ethereum` و `aml-bsc` است.
برای داده‌های قدیمی، نام پروژه موجود را با `docker compose ls -a` بررسی و در همه دستورات همان `--project NAME` را بدهید؛
تغییر نام پروژه باعث انتخاب volume دیگری می‌شود.

## اجرای همه سرویس‌ها روی یک میزبان Linux

`aml.sh` برای آزمایش تک‌میزبانی است. فایل `.env` ریشه و هر دو شبکه لازم‌اند.
روی Linux، Gateway داخل container نمی‌تواند به سرویس bindشده فقط روی localhost میزبان وصل شود.
IP خصوصی قابل‌دسترسی میزبان را در `API_BIND_ADDRESS` هر دو شبکه و upstreamهای ریشه بنویسید.

```bash
# پیش‌فرض: APIها و دیتابیس‌ها
bash scripts/aml.sh up --build
# به‌همراه ingestion و workerها
bash scripts/aml.sh up --build --with-ingestion
bash scripts/aml.sh ps
bash scripts/aml.sh down
```

این launcher نام‌های قدیمی `app`، `dockerizd_ethereum` و `aml-whole` را حفظ می‌کند.
برای استفاده از volumeهای launcher چند VM، صریحاً گزینه‌های
`--tron-project aml-tron --ethereum-project aml-ethereum --main-project aml-main` را بدهید.

## انتقال imageهای آماده

```bash
bash scripts/export-images.sh --include-bsc --output ./deployment-artifacts
# پوشه deployment-artifacts را به VM مقصد منتقل کنید.
bash scripts/import-images.sh ./deployment-artifacts
bash scripts/vm.sh tron up --pull-never
```

روی هر VM فقط role همان VM را اجرا کنید. بدون `--build`، build خودکار انجام نمی‌شود؛
`--pull-never` نیز دانلود image در مقصد را غیرفعال می‌کند.
manifest و SHA-256 تمام imageها پیش از import بررسی می‌شوند.
بسته شامل imageهای برنامه، ClickHouse و Neo4j است؛ داده دیتابیس و فایل‌های `.env` داخل آن نیست.

## ساختار پروژه

```text
AML_Whole/
  .git/                         تنها مخزن Git
  compose.yaml                  Gateway
  scripts/                      launcher، کلید، انتقال و smoke-test با Bash
  gateway/                      Nginx، انتخاب شبکه و تست‌های UI
  contracts/                    قرارداد API بین VMها
  docs/                         راهنمای استقرار
  dockerizd_tron/app/            Rust، SQL، UI و Compose ترون
  dockerizd_ethereum/            Rust، SQL، UI و Compose اتریوم
  dockerizd_bsc/                 ingestion/replay/schema، API تحقیق، metadata و intelligence
```

```text
Browser: network + wallet
  -> Main VM / Gateway
  -> Main VM / Analytical Node
  -> Chain VM / Rust API
  -> ClickHouse / evidence and graph queries
  -> Main VM / Analytical Node / central Neo4j temporary snapshot
  -> JSON / browser graph and evidence panels

Explicit Export -> persistent investigation snapshot in central Neo4j
```

API شبکه داده ذخیره‌شده همان شبکه را می‌خواند؛ در مسیر مرکزی، snapshot تحقیق موقت است و با Export صریح دائمی می‌شود. در این استقرار فقط VM اصلی Neo4j دارد.
HTML هر شبکه هنگام build Gateway از سورس همان شبکه کپی می‌شود. پس از تغییر UI، imageهای Gateway و API مربوط را بازسازی کنید.
این استقرار تطبیق bridge بین شبکه‌ها یا گراف مشترک چندزنجیره‌ای اضافه نمی‌کند.

## پورت‌ها

| سرویس | TRON | Ethereum | BSC |
|---|---|---|---|
| API | 4001 | 5001 | 6001 |
| ClickHouse HTTP | 18123 | 28123 | 38123 |
| ClickHouse Native | 19000 | 29000 | 39000 |

Neo4j فقط در VM اصلی است؛ پورت پیش‌فرض Browser برابر 7474 و Bolt برابر 7687 است و روی localhost منتشر می‌شوند. مقادیر `.env` می‌توانند این پیش‌فرض‌ها را تغییر دهند.

پورت‌ها قابل تنظیم‌اند. دیتابیس‌ها فقط روی localhost همان VM منتشر می‌شوند.

## تست

```bash
# تست قرارداد launcherها، خطاها و checksum، بدون دیتابیس یا RPC واقعی
bash scripts/tests/linux-cli.sh

# تست واقعی image آماده Gateway با پورت و کانتینر موقت
bash scripts/tests/gateway-docker.sh

# تست آدرس و مسیردهی
npm ci
npm test
# تست مرورگر؛ Docker روشن و image Gateway ساخته‌شده لازم است
npx playwright install chromium
npm run test:browser
```

تست مرورگر از پاسخ‌های کنترل‌شده استفاده می‌کند. برای تست داده واقعی، آدرس ingestشده در ClickHouse را به
`scripts/smoke-test.sh` بدهید. `ready` فقط آمادگی وابستگی‌هاست و به معنی کامل‌بودن تاریخچه شبکه نیست.

فایل‌های `.sh` در Git با LF و مجوز اجرا نگهداری می‌شوند. در صورت انتقال با ZIP یا از فایل‌سیستم بدون مجوز Unix،
اجرای `bash scripts/vm.sh ...` به executable bit وابسته نیست.

## کنترل روزمره شبکه‌ها و دیتابیس

بعد از رسیدن به سر زنجیره، ingestion به‌صورت خودکار بلاک‌های نهایی‌شده جدید را دنبال می‌کند؛ cron روزانه لازم نیست. با تنظیم فعلی فاصله بررسی TRON و BSC سه ثانیه و Ethereum دوازده ثانیه است.

روی میزبان Linux، از ریشه پروژه و پس از ساخت VMها:

```bash
# Stop only Ethereum applications; its VM and ClickHouse stay running.
python3 scripts/provision_vms.py pause ethereum
python3 scripts/provision_vms.py ps ethereum
python3 scripts/provision_vms.py db ethereum --query "SHOW TABLES"
python3 scripts/provision_vms.py db ethereum --query "SELECT * FROM transactions_canonical LIMIT 10 FORMAT Vertical"
python3 scripts/provision_vms.py resume ethereum
```

نام `ethereum` را برای شبکه‌های دیگر با `tron` یا `bsc` جایگزین کنید. `pause-ingestion` فقط workerها را متوقف می‌کند و API را دست‌نخورده نگه می‌دارد؛ `pause` API را هم متوقف می‌کند. `resume` سرویس‌های همان شبکه را با image موجود و بدون restart دیتابیس اجرا می‌کند. `db` فقط‌خواندنی است و رمز را داخل کانتینر می‌خواند. اجرای `up` کلی می‌تواند توقف دستی سرویس‌ها را لغو کند.

[راهنمای کامل: sync خودکار، کنترل مستقل شبکه‌ها، ClickHouse و checkpoint](docs/CHAIN_OPERATIONS_FA.md)

## بررسی قبل از تحویل سرور

[چک‌لیست فعلی تحویل، چرخه واقعی داده، اصلاحات و محدودیت‌های تأییدشده](docs/RELEASE_READINESS_FA.md)

ingest ابتدا «سه ستون» را پر نمی‌کند؛ تراکنش، انتقال و رویداد هنگام ingest تولید می‌شوند، metadata/analytics در worker و بخشی از تحلیل‌ها هنگام جستجوی ولت اجرا می‌شوند. برچسب معتبر صرافی و seed ریسک نیازمند منبع و review هستند. [جزئیات اجرای خودکار تحلیل‌ها](docs/AUTOMATIC_ANALYTICS_FA.md).

برای کنترل API و همه workerهای لازم، داخل VM مربوط از `sudo bash scripts/vm.sh tron check-runtime` استفاده کنید؛ نام شبکه قابل تغییر است. استقرار کامل با `provision_vms.py up --build --with-ingestion` این کنترل را اجرا می‌کند. سلامت فرایند به معنی کامل یا تازه بودن تاریخچه نیست.

ساخت عادی BSC دیگر به archive خارج از Git وابسته نیست. ساخت آفلاین قبلی در `dockerizd_bsc/Dockerfile.offline` حفظ شده است. فایل‌های `.env` شبکه‌ها باید جداگانه و خصوصی منتقل شوند؛ وجود `.env` روی کامپیوتر شما تضمین نمی‌کند در clone سرور هم باشد.
