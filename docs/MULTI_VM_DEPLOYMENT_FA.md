# راهنمای استقرار سامانه AML روی Linux

این راهنما برای VMهای لینوکسی Main، TRON، Ethereum و BSC است. همه شبکه‌ها داخل یک مخزن Git قرار دارند؛
روی هر VM فقط سرویس‌های همان نقش اجرا می‌شوند. اسکریپت‌های اجرایی Bash هستند و PowerShell لازم نیست.

BSC نیز API تحقیق، UI و ریسک بدون ML متصل به Neo4j مرکزی دارد.
راهنمای اجرای آن، محدودیت‌ها و قالب برچسب‌ها در [تحویل BSC](../dockerizd_bsc/docs/VM_READINESS_FA.md) است.

## 1. معماری و آدرس‌دهی

```text
Browser
   |
   v
Main VM: Gateway + UI + Analytical Node + one Neo4j
   | private API + service key
   +-------------------------+
   v                         v
TRON VM                  Ethereum VM
API + ingestion          API + ingestion
metadata worker          metadata/analytics workers
ClickHouse               ClickHouse
TRON Node/RPC            Reth/RPC
```

| VM | IP نمونه | پورت دسترسی |
|---|---|---|
| Main | 10.20.0.10 | 8080 یا TLS سازمان روی 443 |
| TRON | 10.20.0.21 | 4001 فقط از Main |
| Ethereum | 10.20.0.22 | 5001 فقط از Main |
| BSC | 10.20.0.23 | 6001 فقط از Main |

این IPها نمونه‌اند؛ قبل از اجرا با IP واقعی شبکه کارفرما جایگزین شوند.
مرورگر به Main متصل می‌شود؛ Main فقط API شبکه‌ها را صدا می‌زند.
ClickHouse روی Chain VM و تنها Neo4j روی Main VM قرار دارد. پورت‌های دیتابیس روی localhost باقی می‌مانند.
راهنمای رفتار موقت/دائمی و ریسک: [Neo4j مرکزی و Export](CENTRAL_INVESTIGATIONS_FA.md).

## 2. پیش‌نیاز و دریافت پروژه

روی هر VM، Docker Engine و افزونه Compose v2 نصب و سرویس Docker روشن باشد.
برای بررسی:

```bash
docker version
docker compose version
```

روی Ubuntu/Debian ابزارهای helper را نصب کنید:

```bash
sudo apt-get update
sudo apt-get install -y bash git ca-certificates curl jq openssl coreutils
# فقط برای ساخت کاربر Basic Auth روی Main VM
sudo apt-get install -y apache2-utils
```

کاربر اجرا باید دسترسی Docker داشته باشد. تمام دستورها با همان کاربر مالک پروژه اجرا شوند.
از ساعت هماهنگ‌شده، دیسک پایدار برای volumeها و RPC قابل‌دسترسی از Chain VM مطمئن شوید.

```bash
git clone https://github.com/Mehranai/AML_Real.git "$HOME/AML_Whole"
cd "$HOME/AML_Whole"
```

برای دریافت نسخه جدید همان clone موجود، از workflow معمول Git استفاده کنید؛ clone جدید و `git init` داخل شبکه‌ها لازم نیست.
اگر پروژه را در مسیر دیگری قرار دادید، فقط دستور cd تغییر می‌کند؛ اسکریپت‌ها مسیر را نسبت به خودشان پیدا می‌کنند.

برای imageهای آماده، معماری CPU مقصد باید با imageها سازگار باشد؛ بسته فعلی ساخته‌شده برای `linux/amd64` است.
انتقال فایل‌ها از Windows نیز مجاز است؛ برای اجرا همیشه Bash لینوکس استفاده شود.

## 3. ساخت کلید سرویس

روی یک سیستم امن:

```bash
cd "$HOME/AML_Whole"
bash scripts/new-service-key.sh
```

خروجی را در `AML_SERVICE_KEY` هر سه فایل محیط قرار دهید.
کلید به مرورگر داده نمی‌شود؛ Gateway آن را در هدر `X-AML-Service-Key` اضافه می‌کند.
فایل‌های محیط باید فقط برای کاربر اجرا قابل خواندن باشند و داخل Git قرار نمی‌گیرند.

## 4. تنظیم TRON VM

```bash
cd "$HOME/AML_Whole"
umask 077
cp -n dockerizd_tron/app/.env.example dockerizd_tron/app/.env
chmod 600 dockerizd_tron/app/.env
nano dockerizd_tron/app/.env
```

مقادیر اصلی را تنظیم کنید:

```dotenv
API_BIND_ADDRESS=10.20.0.21
DATABASE_BIND_ADDRESS=127.0.0.1
TRON_API_PORT=4001
AML_SERVICE_AUTH_REQUIRED=true
AML_SERVICE_KEY=PASTE_SHARED_KEY
CLICKHOUSE_USER=admin
CLICKHOUSE_PASSWORD=CHOOSE_DATABASE_PASSWORD
TRON_RPC_URL=http://PRIVATE_TRON_NODE:8090
TRON_API_KEY=
SYNC_MODE=auto
TRON_START_BLOCK=0
```

RPC را با نود یا provider واقعی جایگزین کنید. برای Node داخل container، آدرس localhost به همان container اشاره می‌کند؛
از DNS سرویس در شبکه مشترک Docker یا IP قابل‌دسترسی نود استفاده کنید.

```bash
bash scripts/vm.sh tron up --build
bash scripts/vm.sh tron check
```

برای شروع فقط API و دیتابیس:

```bash
bash scripts/vm.sh tron up --build --api-only
```

## 5. تنظیم Ethereum VM

```bash
cd "$HOME/AML_Whole"
umask 077
cp -n dockerizd_ethereum/.env.example dockerizd_ethereum/.env
chmod 600 dockerizd_ethereum/.env
nano dockerizd_ethereum/.env
```

```dotenv
API_BIND_ADDRESS=10.20.0.22
DATABASE_BIND_ADDRESS=127.0.0.1
ETHEREUM_API_PORT=5001
AML_SERVICE_AUTH_REQUIRED=true
AML_SERVICE_KEY=PASTE_SHARED_KEY
CLICKHOUSE_USER=admin
CLICKHOUSE_PASSWORD=CHOOSE_DATABASE_PASSWORD
ETH_RPC_URL=http://PRIVATE_RETH_NODE:8545
ETH_EXPECTED_CHAIN_ID=1
ETH_NETWORK_ID=eip155:1
ETH_TRACE_MODE=auto
ETH_RISK_ENGINE_ENABLED=false
```

در صورت نیاز به trace کامل، نود باید trace را ارائه کند و `ETH_TRACE_MODE=required` تنظیم شود.

```bash
bash scripts/vm.sh ethereum up --build
bash scripts/vm.sh ethereum check
# یا فقط API و دیتابیس:
bash scripts/vm.sh ethereum up --build --api-only
```

## 6. تنظیم Main VM

```bash
cd "$HOME/AML_Whole"
umask 077
cp -n .env.example .env
chmod 600 .env
nano .env
```

```dotenv
NEO4J_PASSWORD=CHOOSE_CENTRAL_NEO4J_PASSWORD
AML_GRAPH_TTL_HOURS=24
AML_BIND_ADDRESS=10.20.0.10
AML_PORT=8080
AML_TRON_UPSTREAM=http://10.20.0.21:4001
AML_ETHEREUM_UPSTREAM=http://10.20.0.22:5001
AML_SERVICE_KEY=PASTE_SHARED_KEY
AML_BASIC_AUTH_REALM=off
AML_HTPASSWD_FILE=./gateway/auth/disabled.htpasswd
```

مقدار upstream فقط origin است: بدون path، slash انتهایی یا username/password.
برای تست اتصال از Main:

```bash
curl --fail --show-error --connect-timeout 5 http://10.20.0.21:4001/ready
curl --fail --show-error --connect-timeout 5 http://10.20.0.22:5001/ready
bash scripts/vm.sh main up --build
bash scripts/vm.sh main check
```

صفحه سامانه: `http://10.20.0.10:8080`.
در اجرای تک‌میزبانی Linux نیز باید APIها روی IP قابل دسترسی از container Gateway bind شوند؛
`127.0.0.1` میزبان از داخل container، آدرس همان میزبان نیست.

## 7. رمز ورود UI

پس از نصب `apache2-utils` روی Main:

```bash
cd "$HOME/AML_Whole"
mkdir -p secrets
htpasswd -cB secrets/aml.htpasswd analyst
chmod 644 secrets/aml.htpasswd
```

htpasswd رمز را تعاملی می‌گیرد. فایل شامل hash است و باید برای کاربر غیر root کانتینر Nginx قابل خواندن باشد.
برای افزودن کاربر بعدی، `-c` را حذف کنید تا فایل قبلی بازنویسی نشود.

در `.env` ریشه:

```dotenv
AML_BASIC_AUTH_REALM=AML-Restricted
AML_HTPASSWD_FILE=./secrets/aml.htpasswd
```

```bash
bash scripts/vm.sh main up
```

برای ورود واقعی کاربران، TLS سازمان یا ارتباط VPN محافظت‌شده لازم است؛ Basic Auth خودش ترافیک را رمز نمی‌کند.
SSO/OIDC سازمان می‌تواند جلوی Gateway قرار بگیرد. `/health` برای probe بدون رمز باقی می‌ماند.

## 8. تست سیستم

```bash
bash scripts/smoke-test.sh --main-url http://10.20.0.10:8080
# با ورود UI؛ رمز به‌صورت تعاملی پرسیده می‌شود:
bash scripts/smoke-test.sh --main-url http://10.20.0.10:8080 --user analyst
```

برای بررسی تحقیق و مسیر دو آدرس، آدرس‌هایی را انتخاب کنید که داده‌شان ingest شده است:

```bash
bash scripts/smoke-test.sh \
  --main-url http://10.20.0.10:8080 \
  --user analyst \
  --tron-address SOURCE_TRON \
  --tron-path-target TARGET_TRON \
  --ethereum-address SOURCE_ETH \
  --ethereum-path-target TARGET_ETH
```

اسکریپت health، readiness، تطابق آدرس investigation و قالب paths را بررسی می‌کند.
عمق جست‌وجوی مسیر ۱۰ است. وجود paths خالی یعنی در داده و محدوده جست‌وجو مسیری پیدا نشده است؛
این اسکریپت وجود مسیر ده‌مرحله‌ای یا تکمیل تاریخچه را اثبات نمی‌کند.

تست مستقل اسکریپت‌ها، بدون دیتابیس و RPC:

```bash
bash scripts/tests/linux-cli.sh
```

برای تست واقعی image آماده Gateway با پورت موقت و پاک‌سازی خودکار کانتینر:

```bash
bash scripts/tests/gateway-docker.sh
```

## 9. ClickHouse و Neo4j

API شبکه‌ها داده ClickHouse را می‌خوانند. در مسیر VM اصلی هر investigation یا paths ابتدا به صورت موقت در Neo4j مرکزی ثبت می‌شود.
دکمه Export همان snapshot را دائمی می‌کند و network_id روی تحقیق، node و edge ثبت شده است.

```text
POST /api/investigations/{id}/export
GET /api/investigations
GET /api/investigations/{id}
```

مسیرهای قدیمی neo4j/import دیگر فعال نیستند و پاسخ 410 می‌دهند.
[راهنمای کامل ذخیره مرکزی و سیاست ریسک](CENTRAL_INVESTIGATIONS_FA.md).

روی TRON VM، برای مشاهده دیتابیس همان پروژه اجراشده:

```bash
docker compose --project-name aml-tron \
  --project-directory dockerizd_tron/app \
  --env-file dockerizd_tron/app/.env \
  --file dockerizd_tron/app/docker-compose.yml \
  exec clickhouse sh -lc 'clickhouse-client --user "$CLICKHOUSE_USER" --password "$CLICKHOUSE_PASSWORD" --database tron_db'
```

```sql
SHOW TABLES;
SELECT count() FROM transactions_canonical;
SELECT count() FROM address_relationships_canonical;
```

## 10. انتقال آفلاین

روی سیستم build، ابتدا imageهای هر سه نقش را بسازید. سپس:

```bash
bash scripts/export-images.sh --output ./deployment-artifacts
```

پنج image برنامه و دیتابیس همراه `manifest.json` و SHA-256 صادر می‌شوند.
این بسته داده‌های ClickHouse/Neo4j یا فایل‌های `.env` را شامل نمی‌شود؛ برای نمایش داده قبلی باید backup دیتابیس را
جداگانه انتقال دهید یا در مقصد ingestion را اجرا کنید.

بسته را روی هر VM مقصد منتقل و import کنید:

```bash
bash scripts/import-images.sh /path/to/deployment-artifacts
# بعد از تنظیم .env، فقط role همان VM:
bash scripts/vm.sh tron up --pull-never
```

برای Main و Ethereum، role را عوض کنید. `--pull-never` دانلود را ممنوع می‌کند و نبود image خطاست.
فرمت manifest با بسته‌ای که قبلاً صادر شده سازگار است. پس از هر تغییر backend/UI باید image مربوط دوباره ساخته و export شود.

برای build آفلاین Ethereum از سورس روی سیستم دارای Rust و دسترسی dependency:

```bash
bash dockerizd_ethereum/scripts/refresh-linux-cache.sh
bash scripts/vm.sh ethereum up --offline --api-only
```

فایل `cargo-registry-linux.tar.gz` به‌همراه checksum آن منتقل شود.
این گزینه dependencyهای Cargo را آفلاین می‌کند؛ base imageهای Docker و frontend مربوط به Dockerfile نیز باید قبلاً موجود باشند.
برای محیط بدون اینترنت، انتقال image نهایی مطمئن‌تر است.

BSC برای build سورس از `bash dockerizd_bsc/scripts/refresh-linux-vendor.sh` استفاده می‌کند؛
role مستقل آن `bash scripts/vm.sh bsc up --build --api-only` است؛ برای ingestion گزینه api-only را حذف کنید.

## 11. عملیات و حفظ داده

```bash
bash scripts/vm.sh tron ps
bash scripts/vm.sh ethereum logs
bash scripts/vm.sh main check
bash scripts/vm.sh tron down
```

نام‌های Compose: `aml-main`، `aml-tron`، `aml-ethereum` و `aml-bsc`.
برای استفاده از volumeهای قبلی، قبل از up نام پروژه موجود را با `docker compose ls -a` بررسی کنید:

```bash
bash scripts/vm.sh tron up --project app
bash scripts/vm.sh tron check --project app
```

همان نام را در up/check/logs/down تکرار کنید. هیچ اسکریپتی volume را حذف نمی‌کند.
`down -v` دستور حذف داده است و برای عملیات روزانه استفاده نمی‌شود.

## 12. شبکه و backup

فقط کاربران مجاز به Main و فقط Main به پورت API شبکه‌ها دسترسی داشته باشند.
پورت‌های دیتابیس روی localhost باقی بمانند. قوانین firewall باید ترافیک منتشرشده Docker را نیز پوشش دهند.

ClickHouse و Neo4j باید backup سازگار با نسخه و روی مقصد جداگانه داشته باشند.
فایل‌های محیط و کلیدها در محل امن سازمان نگهداری شوند. image export جایگزین backup دیتابیس نیست.

## 13. خطاهای رایج

- `Missing .../.env`: فایل نمونه همان role را کپی و تنظیم کنید.
- `Permission denied` برای shell: دستور را با `bash scripts/vm.sh ...` اجرا کنید یا `chmod +x scripts/*.sh` بزنید.
- `bash\r` یا `$'\r'`: فایل با CRLF منتقل شده؛ clone جدید از Git با قانون LF پروژه بگیرید.
- `401 unauthorized service request`: کلید Main و Chain VMها یکسان نیست.
- `503 selected network API is unavailable`: bind، IP، firewall یا API شبکه را بررسی کنید.
- health موفق و investigation خالی: ingestion هنوز داده آدرس را پوشش نداده است.
- `No such image`: image را build یا import کنید؛ up عادی build خودکار ندارد.
- `Cannot connect to the Docker daemon`: وضعیت `systemctl status docker` و دسترسی کاربر را بررسی کنید.
- شروع دیتابیس خالی پس از تغییر launcher: نام Compose پروژه و volume انتخاب‌شده را بررسی کنید.

## 14. روز ارائه

1. IPها، RPC و ساعت هر سه VM صحیح باشند.
2. Chain VMها را روشن و check کنید؛ سپس Main را روشن کنید.
3. smoke-test را با آدرس ingestشده هر شبکه اجرا کنید.
4. جست‌وجوی کیف پول و مسیر دو آدرس را در UI نمایش دهید.
5. محدوده تاریخچه ingest و محدودیت تعداد edge/hop را همراه خروجی توضیح دهید.
6. بسته image، backup داده نمونه و راهنمای حاضر را همراه داشته باشید.
