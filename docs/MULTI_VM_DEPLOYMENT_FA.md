# راهنمای استقرار چند-VM سامانه AML

این سند روش انتقال، اجرا و تست نسخه فعلی سامانه روی سه ماشین مستقل را توضیح می‌دهد:

- VM اصلی: رابط کاربری و Gateway
- VM شبکه TRON: API، ingestion، ClickHouse و Neo4j ترون
- VM شبکه Ethereum: API، ingestion، ClickHouse و Neo4j اتریوم

BSC در نسخه فعلی schema، ingestion، replay و repair دارد، اما هنوز API تحقیق کیف پول، Neo4j و UI آن کامل نیست؛
بنابراین تا تکمیل این اجزا در Gateway اصلی فعال نشده است.

## 1. معماری اجرایی

```text
Browser
  |
  | HTTP/HTTPS فقط به VM اصلی
  v
Main VM: Nginx Gateway + Unified UI
  |                         |
  | private API + key       | private API + key
  v                         v
TRON VM                  Ethereum VM
  |- tron-api              |- ethereum-api
  |- ingestion             |- ingestion
  |- metadata worker       |- metadata/analytics workers
  |- ClickHouse            |- ClickHouse
  |- Neo4j                 |- Neo4j
  `- TRON node/RPC         `- Reth/RPC
```

مرورگر و VM اصلی نباید مستقیماً به ClickHouse یا Neo4j شبکه‌ها متصل شوند. تنها Chain API هر شبکه از طریق
شبکه خصوصی برای VM اصلی قابل دسترسی است.

## 2. نمونه آدرس‌دهی

در این راهنما از IPهای نمونه زیر استفاده می‌شود. آن‌ها را با IP واقعی شبکه خصوصی کارفرما عوض کنید:

| نقش | IP خصوصی | پورت قابل دسترسی |
|---|---:|---:|
| Main VM | `10.20.0.10` | `8080` یا reverse proxy روی `443` |
| TRON VM | `10.20.0.21` | `4001` فقط از Main VM |
| Ethereum VM | `10.20.0.22` | `5001` فقط از Main VM |

پورت‌های ClickHouse و Neo4j روی `127.0.0.1` همان VM باقی می‌مانند و نباید در شبکه عمومی یا خصوصی باز شوند.

## 3. پیش‌نیازها

- Docker Engine یا Docker Desktop
- Docker Compose v2
- ساعت هماهنگ‌شده با NTP روی هر سه VM
- ارتباط IP خصوصی یا VPN بین Main VM و Chain VMها
- فضای پایدار Docker volume برای ClickHouse و Neo4j
- RPC یا Full Node قابل دسترسی از VM همان شبکه

برای ساده‌ترین انتقال، repository کامل را روی هر VM قرار دهید و فقط role همان VM را اجرا کنید. وجود سورس سایر
شبکه‌ها باعث اجرای آن‌ها نمی‌شود.

## 4. ساخت کلید داخلی مشترک

روی یک سیستم امن از ریشه پروژه اجرا کنید:

```powershell
cd D:\Sarbazi\AML_Whole
$serviceKey = .\scripts\new-service-key.ps1
$serviceKey
```

این مقدار باید دقیقاً در فایل `.env` هر سه VM یکسان باشد. آن را در Git، screenshot یا پیام عمومی قرار ندهید.

## 5. تنظیم TRON VM

```powershell
cd D:\Sarbazi\AML_Whole\dockerizd_tron\app
Copy-Item .env.example .env
```

مقادیر اصلی `.env`:

```dotenv
API_BIND_ADDRESS=10.20.0.21
DATABASE_BIND_ADDRESS=127.0.0.1
TRON_API_PORT=4001

AML_SERVICE_AUTH_REQUIRED=true
AML_SERVICE_KEY=PASTE_THE_SHARED_SERVICE_KEY

CLICKHOUSE_USER=admin
CLICKHOUSE_PASSWORD=USE_A_RANDOM_DATABASE_PASSWORD
NEO4J_PASSWORD=USE_ANOTHER_RANDOM_PASSWORD

TRON_RPC_URL=http://PRIVATE_TRON_NODE:8090
TRON_API_KEY=
SYNC_MODE=auto
TRON_START_BLOCK=0
```

اگر فعلاً از provider استفاده می‌شود، `TRON_RPC_URL` و در صورت نیاز `TRON_API_KEY` را با اطلاعات provider پر کنید.
با `SYNC_MODE=auto` ingestion از checkpoint ذخیره‌شده ادامه پیدا می‌کند.

اجرای کامل:

```powershell
cd D:\Sarbazi\AML_Whole
.\scripts\vm.ps1 -Role tron -Action up -Build
.\scripts\vm.ps1 -Role tron -Action check
```

برای تست API بدون شروع ingestion:

```powershell
.\scripts\vm.ps1 -Role tron -Action up -Build -ApiOnly
```

## 6. تنظیم Ethereum VM

```powershell
cd D:\Sarbazi\AML_Whole\dockerizd_ethereum
Copy-Item .env.example .env
```

مقادیر اصلی `.env`:

```dotenv
API_BIND_ADDRESS=10.20.0.22
DATABASE_BIND_ADDRESS=127.0.0.1
ETHEREUM_API_PORT=5001

AML_SERVICE_AUTH_REQUIRED=true
AML_SERVICE_KEY=PASTE_THE_SHARED_SERVICE_KEY

CLICKHOUSE_USER=admin
CLICKHOUSE_PASSWORD=USE_A_RANDOM_DATABASE_PASSWORD
NEO4J_PASSWORD=USE_ANOTHER_RANDOM_PASSWORD

ETH_RPC_URL=http://PRIVATE_RETH_NODE:8545
ETH_EXPECTED_CHAIN_ID=1
ETH_NETWORK_ID=eip155:1
ETH_TRACE_MODE=auto
ETH_RISK_ENGINE_ENABLED=false
```

اجرای کامل:

```powershell
cd D:\Sarbazi\AML_Whole
.\scripts\vm.ps1 -Role ethereum -Action up -Build
.\scripts\vm.ps1 -Role ethereum -Action check
```

## 7. تنظیم Main VM

```powershell
cd D:\Sarbazi\AML_Whole
Copy-Item .env.example .env
```

مقادیر اصلی `.env`:

```dotenv
AML_BIND_ADDRESS=0.0.0.0
AML_PORT=8080
AML_TRON_UPSTREAM=http://10.20.0.21:4001
AML_ETHEREUM_UPSTREAM=http://10.20.0.22:5001
AML_SERVICE_KEY=PASTE_THE_SHARED_SERVICE_KEY

# فقط برای تست اولیه داخلی؛ قبل از دسترسی کاربران احراز هویت را فعال کنید.
AML_BASIC_AUTH_REALM=off
AML_HTPASSWD_FILE=./gateway/auth/disabled.htpasswd
```

قبل از start، اتصال شبکه را بررسی کنید:

```powershell
Test-NetConnection 10.20.0.21 -Port 4001
Test-NetConnection 10.20.0.22 -Port 5001
```

سپس Gateway را اجرا کنید:

```powershell
.\scripts\vm.ps1 -Role main -Action up -Build
.\scripts\vm.ps1 -Role main -Action check
```

صفحه سامانه روی `http://10.20.0.10:8080` در دسترس است.

## 8. فعال‌کردن رمز ورود UI

یک فایل محلی و ignored برای کاربر تحلیلگر بسازید:

```powershell
New-Item -ItemType Directory -Force .\secrets | Out-Null
docker run --rm httpd:2.4-alpine htpasswd -nbB analyst 'A_STRONG_PASSWORD' |
  Set-Content -Encoding ascii .\secrets\aml.htpasswd
```

سپس `.env` در Main VM را تغییر دهید:

```dotenv
AML_BASIC_AUTH_REALM=AML-Restricted
AML_HTPASSWD_FILE=./secrets/aml.htpasswd
```

Gateway را بازسازی کنید:

```powershell
.\scripts\vm.ps1 -Role main -Action up -Build
```

Basic Auth برای شبکه داخلی و ارائه کافی است. برای دسترسی سازمانی چندکاربره، Nginx باید پشت OIDC/SSO و TLS سازمان قرار گیرد.

## 9. تست کامل از Main VM

بدون Basic Auth:

```powershell
.\scripts\smoke-test.ps1 `
  -MainUrl http://127.0.0.1:8080 `
  -TronAddress TGX6tRfV4CcUH4hbsujqhcL8omACeGu4kQ `
  -EthereumAddress 0x238a4d9fb5337fa220f98f2829d9d903664b337b
```

با Basic Auth:

```powershell
$credential = Get-Credential analyst
.\scripts\smoke-test.ps1 -MainUrl http://127.0.0.1:8080 -Credential $credential
```

برای تست مسیر تا 10 hop، targetها را نیز اضافه کنید:

```powershell
.\scripts\smoke-test.ps1 `
  -MainUrl http://127.0.0.1:8080 `
  -TronAddress SOURCE_TRON `
  -TronPathTarget TARGET_TRON `
  -EthereumAddress SOURCE_ETH `
  -EthereumPathTarget TARGET_ETH
```

آدرس تست باید در ClickHouse همان شبکه ingest شده باشد. پاسخ خالی برای آدرس خارج از محدوده ingest الزاماً خطای سیستم نیست.

## 10. رفتار ClickHouse و Neo4j

Endpointهای `GET investigation` و `GET paths` فقط از داده‌های ذخیره‌شده می‌خوانند و UI را تغذیه می‌کنند.
رسم canvas در مرورگر از JSON پاسخ انجام می‌شود و نیاز ندارد مرورگر به Neo4j متصل شود.

Projection پایدار در Neo4j یک command صریح است:

```text
POST /api/tron/wallet/{address}/neo4j/import
POST /api/tron/wallet/{source}/paths/{target}/neo4j/import
POST /api/ethereum/wallet/{address}/neo4j/import
POST /api/ethereum/wallet/{source}/paths/{target}/neo4j/import
```

Gateway هدر `X-AML-Service-Key` را خودش اضافه می‌کند. کاربران نباید service key را در مرورگر وارد کنند.

## 11. انتقال به محیط بدون اینترنت

بهترین روش این است که imageها روی سیستم build متصل ساخته و سپس منتقل شوند. پس از build موفق:

```powershell
.\scripts\export-images.ps1
```

پوشه ignored به نام `deployment-artifacts` شامل imageهای Gateway، سرویس‌های TRON و Ethereum، ClickHouse و
Neo4j به‌همراه نام و checksum آن‌ها می‌شود. آن را خارج از Git به محیط کارفرما منتقل کنید. روی VM مقصد:

```powershell
.\scripts\import-images.ps1 -InputDirectory D:\Transfer\deployment-artifacts
.\scripts\vm.ps1 -Role tron -Action up
# یا Roleهای ethereum و main روی VM مربوط به خودشان
```

چون `-Build` استفاده نشده، Compose از image واردشده استفاده می‌کند.

برای build آفلاین Ethereum از source، ابتدا روی سیستم متصل cache لینوکسی checksumدار را بسازید:

```powershell
cd .\dockerizd_ethereum
.\scripts\refresh-linux-cache.ps1
cd ..
.\scripts\vm.ps1 -Role ethereum -Action up -Offline
```

`cargo-registry-linux.tar.gz` حجیم است و در Git نگهداری نمی‌شود. آن را باید همراه artifactهای deployment انتقال دهید.

## 12. Firewall

قواعد لازم:

- کاربران مجاز -> Main VM: پورت `8080` یا ترجیحاً `443`
- Main VM -> TRON VM: پورت `4001`
- Main VM -> Ethereum VM: پورت `5001`
- هیچ client دیگری -> Chain APIها: مسدود
- تمام ماشین‌های دیگر -> پورت‌های ClickHouse و Neo4j: مسدود

حتی با وجود firewall، `DATABASE_BIND_ADDRESS=127.0.0.1` را تغییر ندهید. برای production بهتر است روی Chain VMها
پورت‌های database از Compose نیز کاملاً حذف شوند؛ loopback فعلی برای مشاهده و عیب‌یابی محلی در نظر گرفته شده است.

## 13. عملیات روزانه

```powershell
# وضعیت
.\scripts\vm.ps1 -Role tron -Action ps

# آخرین logها
.\scripts\vm.ps1 -Role ethereum -Action logs

# readiness
.\scripts\vm.ps1 -Role main -Action check

# توقف بدون حذف volume
.\scripts\vm.ps1 -Role tron -Action down
```

هیچ‌گاه برای عملیات عادی `docker compose down -v` اجرا نکنید؛ گزینه `-v` volumeهای ClickHouse و Neo4j را حذف می‌کند.

## 14. Backup

- ClickHouse باید با روش backup سازگار با نسخه ClickHouse و مقصد جداگانه snapshot شود.
- Neo4j Community باید با توقف هماهنگ‌شده یا روش dump/backup مورد تأیید نسخه آن نگهداری شود.
- فایل‌های `.env` باید در password manager یا secret manager سازمان نگهداری شوند.
- backup روی همان دیسک VM، backup واقعی محسوب نمی‌شود.

## 15. خطاهای متداول

`401 unauthorized service request`:
کلید `AML_SERVICE_KEY` در Main VM و Chain VM یکسان نیست یا auth فقط در Chain VM فعال شده است.

`503 selected network API is unavailable`:
Chain API خاموش است، `API_BIND_ADDRESS` اشتباه است یا firewall ارتباط Main VM را بسته است.

`ready` موفق ولی داده کیف پول خالی است:
دیتابیس‌ها سالم‌اند، اما ingestion هنوز به بلاک مربوط به آن آدرس نرسیده است. endpoint ingestion/status و log worker را بررسی کنید.

خطای bind:
IP نوشته‌شده در `API_BIND_ADDRESS` باید واقعاً روی کارت شبکه همان VM وجود داشته باشد و پورت آزاد باشد.

خطای Docker build در crates.io:
دوباره build کنید تا cache ادامه پیدا کند، یا از image exportشده/روش offline استفاده کنید.

## 16. افزودن شبکه بعدی

هر شبکه جدید باید قبل از ثبت در Gateway این قرارداد را پاس کند:

- API versioned مطابق `contracts/chain-api-v1.openapi.yaml`
- liveness و readiness واقعی
- investigation read-only
- path search محدود و دارای `truncated`/coverage
- projection صریح Neo4j با POST
- service authentication
- ClickHouse و Neo4j محلی و غیرقابل دسترسی از بیرون VM
- تست contract و smoke-test

پس از آن network registry، Nginx upstream و UI برای همان شبکه اضافه می‌شوند.

## 17. چک‌لیست روز ارائه

1. هر سه VM ساعت و ارتباط شبکه صحیح داشته باشند.
2. Chain VMها زودتر از Main VM start شوند.
3. `vm.ps1 -Action check` روی هر سه VM پاس شود.
4. از Main VM، پورت‌های 4001 و 5001 قابل دسترسی باشند.
5. `smoke-test.ps1` با حداقل یک آدرس ingestشده از هر شبکه پاس شود.
6. یک جست‌وجوی wallet و یک path search در UI نمایش داده شود.
7. خاموش‌کردن آزمایشی یک Chain API فقط همان شبکه را unavailable نشان دهد.
8. `.env`، service key و رمز دیتابیس در Git یا صفحه ارائه نمایش داده نشوند.
