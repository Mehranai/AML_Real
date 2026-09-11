# AML Whole

سامانه تحقیق کیف پول برای TRON، Ethereum و BNB Smart Chain با یک مخزن Git و استقرار مستقل هر شبکه.
کاربر در VM اصلی شبکه و آدرس کیف پول را انتخاب می‌کند؛ Analytical Node شواهد API همان شبکه را دریافت می‌کند.
فقط VM اصلی Neo4j دارد: بررسی‌ها ابتدا موقت‌اند و Export همان snapshot را دائمی می‌کند. امتیاز ریسک بدون ML و با دلایل قابل بررسی ارائه می‌شود.

[راهنمای Neo4j مرکزی، Export و ریسک بدون ML](docs/CENTRAL_INVESTIGATIONS_FA.md)

[راهنمای BSC: اجرا روی VM، داده‌های هویتی، holdings و محدودیت‌های تحویل](dockerizd_bsc/docs/VM_READINESS_FA.md)

راهنمای مرحله‌به‌مرحله نصب، شبکه، رمزها، انتقال آفلاین و تست:
[راهنمای استقرار روی Linux](docs/MULTI_VM_DEPLOYMENT_FA.md).

## اجرای Linux روی چند VM

پیش‌نیاز: Linux، Bash نسخه 4 یا بالاتر، Docker Engine و افزونه Docker Compose v2.
برای helperهای تست و انتقال، `curl`، `jq`، `openssl` و `sha256sum` لازم‌اند.
تمام دستورات زیر از ریشه مخزن اجرا می‌شوند. فقط role همان VM را اجرا کنید.

```bash
git clone https://github.com/Mehranai/AML_Real.git "$HOME/AML_Whole"
cd "$HOME/AML_Whole"
```

روی TRON VM:

```bash
umask 077
cp -n dockerizd_tron/app/.env.example dockerizd_tron/app/.env
nano dockerizd_tron/app/.env
bash scripts/vm.sh tron up --build
bash scripts/vm.sh tron check
```

روی Ethereum VM:

```bash
umask 077
cp -n dockerizd_ethereum/.env.example dockerizd_ethereum/.env
nano dockerizd_ethereum/.env
bash scripts/vm.sh ethereum up --build
bash scripts/vm.sh ethereum check
```

روی Main VM:

```bash
umask 077
cp -n .env.example .env
nano .env
bash scripts/vm.sh main up --build
bash scripts/vm.sh main check
bash scripts/smoke-test.sh --main-url http://127.0.0.1:8080
```

قبل از up، RPC و رمز ClickHouse شبکه‌ها و رمز Neo4j فقط در `.env` اصلی را تنظیم کنید. `API_BIND_ADDRESS` هر Chain VM باید IP خصوصی واقعی آن VM باشد؛
`AML_TRON_UPSTREAM` و `AML_ETHEREUM_UPSTREAM` در Main VM باید همان IPها را نشان دهند.
`DATABASE_BIND_ADDRESS=127.0.0.1` باقی می‌ماند.

یک کلید با `bash scripts/new-service-key.sh` بسازید و مقدار یکسان آن را در `AML_SERVICE_KEY` هر سه VM قرار دهید.
در Chain VMها `AML_SERVICE_AUTH_REQUIRED=true` تنظیم شود.
راهنمای کامل، فعال‌کردن رمز UI و تنظیم TLS را توضیح می‌دهد.

صفحه اصلی روی `http://MAIN_VM_IP:8080` در دسترس است؛ برای دسترسی از شبکه، `AML_BIND_ADDRESS` در Main VM را
به IP مناسب تغییر دهید. مقدار پیش‌فرض آن فقط localhost است.

## دستورهای روزانه

```bash
# API و دیتابیس، بدون شروع ingestion جدید
bash scripts/vm.sh tron up --build --api-only
bash scripts/vm.sh ethereum up --build --api-only

# مشاهده وضعیت، log و توقف بدون حذف volume
bash scripts/vm.sh tron ps
bash scripts/vm.sh ethereum logs
bash scripts/vm.sh main check
bash scripts/vm.sh tron down
```

اجرای API-only، workerهایی را که قبلاً روشن بوده‌اند متوقف نمی‌کند.
نام Compose پروژه‌ها در launcher چند VM به‌ترتیب `aml-main`، `aml-tron` و `aml-ethereum` است.
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
bash scripts/export-images.sh --output ./deployment-artifacts
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
  dockerizd_bsc/                 ingestion/replay/schema؛ investigation هنوز آماده نیست
```

```text
Browser: network + wallet
  -> Main VM / Gateway
  -> Chain VM / Rust API
  -> ClickHouse / evidence and graph queries
  -> JSON / browser graph and evidence panels

Explicit POST /neo4j/import -> Neo4j projection on the same Chain VM
```

GETهای investigation و paths داده ذخیره‌شده را می‌خوانند؛ projection پایدار Neo4j با POST انجام می‌شود.
HTML هر شبکه هنگام build Gateway از سورس همان شبکه کپی می‌شود. پس از تغییر UI، imageهای Gateway و API مربوط را بازسازی کنید.
این استقرار تطبیق bridge بین شبکه‌ها یا گراف مشترک چندزنجیره‌ای اضافه نمی‌کند.

## پورت‌ها

| سرویس | TRON | Ethereum |
|---|---|---|
| API | 4001 | 5001 |
| ClickHouse HTTP | 18123 | 28123 |
| ClickHouse Native | 19000 | 29000 |
| Neo4j Browser | 18474 | 28474 |
| Neo4j Bolt | 17687 | 27687 |

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
