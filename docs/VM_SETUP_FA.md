# ساخت دستی پنج VM و آماده‌سازی استقرار

VMها را خودتان در Proxmox، VMware، VirtualBox یا hypervisor موجود می‌سازید.
این مخزن دیگر VMساز خودکار ندارد. نام‌ها و IPهای زیر نمونه‌اند؛ شبکهٔ
واقعی سازمان و منابع نهایی را جایگزین کنید.

**انتخاب تأییدشدهٔ شما: RPC آماده است.** روی این پنج VM نود کامل بلاکچین
نصب یا sync نمی‌شود؛ چهار VM شبکه برای backend و ClickHouse هستند.

## ۱. نقش‌ها و منابع اولیه

| VM | IP خصوصی نمونه | سرویس‌ها | API |
|---|---|---|---:|
| Main | `10.20.0.10` | Gateway، Analytical Node، تنها Neo4j | 8080 |
| TRON | `10.20.0.21` | Rust API/ingest/workers، ClickHouse مستقل | 4001 |
| Ethereum | `10.20.0.22` | Rust API/ingest/workers، ClickHouse مستقل | 5001 |
| BSC | `10.20.0.23` | Rust API/ingest/metadata، ClickHouse مستقل | 6001 |
| Bitcoin | `10.20.0.24` | Rust API/ingest/analytics، ClickHouse مستقل | 6101 |

برای پایلوت محدودِ **بک‌اند با RPC آماده**، نقطهٔ شروع پیشنهادی Main با
۲ vCPU، ۴ GiB RAM و ۴۰ GiB SSD و هر VM شبکه با ۲ تا ۴ vCPU، ۴ تا ۸ GiB RAM
و ۸۰ تا ۱۶۰ GiB SSD است. این اندازه‌گذاری پیشنهادی آزمون است؛ تضمین ظرفیت
یا اندازهٔ تاریخچهٔ کامل mainnet نیست. حدود ۲۰ تا ۳۶ GiB RAM برای مجموع
guestها و حافظهٔ جدا برای میزبان در نظر بگیرید. رشد دیسک باید پایش شود.

منابع full node/archival node جزو این استقرار نیست. وجود یک URL RPC هم تضمین archive، trace،
محدودیت نرخ مناسب یا امکان خواندن کل تاریخچه نیست.
برای TRON، پاسخ `wallet/getblockbynum` باید علاوه بر `raw_data`، بایت اصلی
`raw_data_hex` و `txID` را برگرداند؛ `wallet/gettransactioninfobyid` نیز
باید id و blockNumber سازگار داشته باشد. نبود receipt قابل retry است،
اما اختلاف protobuf/JSON یا هویت receipt خطای اعتبارسنجی است و نباید با
ساخت بایت یا مقدار فرضی دور زده شود. پشتیبانی این فیلدها هنگام پذیرش RPC
واقعی بررسی می‌شود.
برای TransferAssetContract، دسترسی `wallet/getassetissuebyname` هم لازم
است تا نام/شناسهٔ TRC10 بدون ابهام به ID عددی مشترک وصل شود. پاسخ catalog
همراه با زمان مشاهده ثبت می‌شود؛ دادهٔ مبهم یا ناقص باعث توقف انتشار می‌شود.
worker متادیتا به `walletsolidity/getnowblock` و
`walletsolidity/triggerconstantcontract` هم نیاز دارد. head مشاهده باید در
تاریخچهٔ دریافت‌شده باشد و حین تماس‌ها ثابت بماند؛ تغییر head یا عقب‌ماندن
ingest باعث retry می‌شود، نه نسبت‌دادن پاسخ به یک بلاک تاریخی فرضی.


## ۲. ساخت VM و شبکه

۱. ISO رسمی Ubuntu Server 24.04 LTS با معماری amd64 را از
[Ubuntu](https://ubuntu.com/download/server) بگیرید و پنج VM بسازید.
برای هر VM یک دیسک سیستم و در صورت امکان یک دیسک دادهٔ قابل توسعه برای
`/var/lib/docker` یا mount داده در نظر بگیرید. دیسک‌ها نباید بین ClickHouseها
مشترک باشند.

۲. شبکهٔ خصوصی قابل دسترسی از Main به چهار chain VM بسازید. دسترسی خروجی
اینترنت/RPC و DNS نیز لازم است؛ می‌تواند از NIC دوم/NAT یا شبکهٔ سازمان
باشد. روی Main مسیر دسترسی کاربران مجاز را فراهم کنید. hostnameها را
`aml-main`، `aml-tron`، `aml-ethereum`، `aml-bsc` و `aml-bitcoin` بگذارید.

۳. IP ثابت را هنگام نصب Ubuntu یا با Netplan تنظیم کنید. قبل از ویرایش
Netplan نام NIC را با `ip -br address` و gateway واقعی را با `ip route`
ببینید؛ فایل نمونه با نام NIC حدسی یا gateway ساختگی کپی نکنید. برای
اعمال تغییر شبکه از `sudo netplan try` استفاده کنید تا در صورت قطع ارتباط
امکان rollback داشته باشید.

۴. SSH با کلید، کاربر sudo، ساعت همگام‌شده و فضای دیسک را آماده کنید:

```bash
sudo apt update
sudo apt install -y ca-certificates curl git jq openssl apache2-utils
sudo timedatectl set-ntp true
ip -br address
ip route
timedatectl status
df -h
```

۵. Docker Engine، Buildx و Compose v2 را طبق
[راهنمای رسمی Ubuntu در Docker](https://docs.docker.com/engine/install/ubuntu/)
از مخزن رسمی نصب کنید. استفاده از package نصب‌شدهٔ قدیمی Compose v1 کافی
نیست؛ Dockerfileها از additional build contexts استفاده می‌کنند.
اگر کاربر عضو گروه `docker` شد، یک بار از SSH خارج و دوباره وارد شوید.

```bash
docker version
docker compose version
docker buildx version
```

## ۳. دریافت یک نسخهٔ یکسان کد

روی هر پنج VM یک commit/release یکسان از مخزن قرار دهید:

```bash
git clone https://github.com/Mehranai/AML_Real.git "$HOME/AML_Real"
cd "$HOME/AML_Real"
git rev-parse HEAD
```

فقط env مربوط به role همان VM را از نمونه کپی کنید؛ فایل نمونه را به‌جای
تنظیمات واقعی استفاده نکنید. برای نمونه روی Ethereum:

```bash
cp dockerizd_ethereum/.env.example dockerizd_ethereum/.env
chmod 600 dockerizd_ethereum/.env
```

| role | فایل نمونه → فایل خصوصی |
|---|---|
| main | `.env.example` → `.env` |
| tron | `dockerizd_tron/app/.env.example` → `dockerizd_tron/app/.env` |
| ethereum | `dockerizd_ethereum/.env.example` → `dockerizd_ethereum/.env` |
| bsc | `dockerizd_bsc/.env.example` → `dockerizd_bsc/.env` |
| bitcoin | `dockerizd_bitcoin/.env.example` → `dockerizd_bitcoin/.env` |

برای ساخت service key می‌توانید `openssl rand -hex 32` اجرا کنید. همان
`AML_SERVICE_KEY` را در پنج env وارد کنید. رمز ClickHouse هر VM و رمز Neo4j
مستقل باشند. نمونه‌های `change-me` یا `replace-with...` را باقی نگذارید.

## ۴. تنظیم chain VMها

| تنظیم | TRON | Ethereum | BSC | Bitcoin |
|---|---|---|---|---|
| bind API | `API_BIND_ADDRESS` | `API_BIND_ADDRESS` | `BSC_API_BIND_ADDRESS` | `BITCOIN_API_BIND_ADDRESS` |
| RPC | `TRON_RPC_URL`، در صورت نیاز `TRON_API_KEY` | `ETH_RPC_URL` | `BSC_RPC_URL` | `BITCOIN_RPC_URL/USER/PASSWORD` |
| شروع دریافت | `TRON_START_BLOCK` | `ETH_START_BLOCK` | `BSC_INGEST_START_BLOCK` | `BITCOIN_START_BLOCK` |
| رمز DB | `CLICKHOUSE_PASSWORD` | `CLICKHOUSE_PASSWORD` | `BSC_CLICKHOUSE_PASSWORD` | `BITCOIN_CLICKHOUSE_PASSWORD` |
| DB محلی | `tron_db` | `ethereum_aml` | `bsc_aml` | `bitcoin_aml` |

IP bind را IP خصوصی همان VM بگذارید. bind دیتابیس در TRON/Ethereum/Bitcoin
با `DATABASE_BIND_ADDRESS=127.0.0.1` و در BSC با
`DOCKER_BIND_ADDRESS=127.0.0.1` محلی می‌ماند. Main به ClickHouse مستقیم وصل
نمی‌شود؛ upstream آن API هر شبکه است.

برای عملیات ING-2، image همین نسخه را با `bash scripts/vm.sh ROLE up --build`
بسازید؛ binary قدیمی فرمان‌های maintenance جدید را ندارد. `ROLE` نام همان
شبکه است. start تاریخچه را آگاهانه تنظیم و پیش از دریافت `probe` را اجرا کنید؛
RPC باید همان start و قابلیت receipt/trace/prevout موردنیاز شبکه را فراهم کند.
موفقیت نمونهٔ probe تضمین archive کامل نیست. بودجهٔ failure جدید اتریوم و
ترون در `.env.example` با پیش‌فرض ۱۰ آمده؛ روش بازپردازش محدود و توقف writer
در [راهنمای استفاده](USAGE_FA.md) است. روی VMها نود کامل نصب نمی‌کنیم.

در TRON و Ethereum مقدار `AML_SERVICE_AUTH_REQUIRED=true` و در همهٔ شبکه‌ها
service key صحیح را تنظیم کنید؛ BSC و Bitcoin نیز API محافظت‌شده دارند.
همهٔ Composeها از `CLICKHOUSE_IMAGE` مشترک با پیش‌فرض نسخهٔ دقیق
`clickhouse/clickhouse-server:23.8.16.16` استفاده می‌کنند. این نسخه مبنای
آزمون فعلی است؛ انتخاب و پذیرش نسخهٔ پشتیبانی‌شدهٔ انتشار production جزو
مرحلهٔ عملیات در TODO است. فقط یک VM را به image متفاوت ارتقا ندهید.

«دیتابیس خالی» با «فقط بلاک جدید» یکی نیست. start=0 درخواست تاریخچه از
genesis است و زمان/فضای زیادی می‌خواهد. برای پایلوت بازهٔ شروع محدود و
یکسان با هدف آزمون تعیین کنید؛ برای محصول، بازه و coverage واقعی ثبت شود.
تاریخچه از RPC و ingestor اصلی دریافت می‌شود، نه از دیتابیس آزمایشی قدیمی.

Bitcoin به RPC اصلی unpruned و امکان resolve prevout نیاز دارد؛ verbosity=3
از undo data استفاده می‌کند و fallback `getrawtransaction` می‌تواند به
`txindex=1` نیاز داشته باشد. Ethereum/BSC برای internal flow به trace مناسب
نیاز دارند؛ `auto` به معنی پوشش کامل نیست. جزئیات اختصاصی در README هر شبکه
و محدودیت‌های فعلی در [وضعیت پروژه](PROJECT_STATUS_FA.md) آمده‌اند.

## ۵. تنظیم VM اصلی

در `.env` این موارد را وارد کنید:

```dotenv
AML_BIND_ADDRESS=10.20.0.10
AML_PORT=8080
AML_TRON_UPSTREAM=http://10.20.0.21:4001
AML_ETHEREUM_UPSTREAM=http://10.20.0.22:5001
AML_BSC_UPSTREAM=http://10.20.0.23:6001
AML_BITCOIN_UPSTREAM=http://10.20.0.24:6101
AML_BASIC_AUTH_REALM=AML-Restricted
AML_HTPASSWD_FILE=./gateway/auth/users.htpasswd
```

`AML_SERVICE_KEY` مشترک و `NEO4J_PASSWORD` قوی را هم اضافه کنید؛ مقدارهای
بالا را در فایل env موجود ویرایش کنید، فایل را با این snippet ناقص جایگزین
نکنید. حساب پنل را بسازید؛ password تعاملی پرسیده می‌شود:

```bash
htpasswd -cB gateway/auth/users.htpasswd amladmin
sudo chown root:101 gateway/auth/users.htpasswd
sudo chmod 640 gateway/auth/users.htpasswd
chmod 600 .env
```

تنظیمات Neo4j مرکزی در Compose است. هیچ Neo4j دیگری روی چهار chain VM
راه‌اندازی نکنید. برای دسترسی کاربران خارج از شبکهٔ خصوصی، TLS و کنترل
دسترسی ورودی باید در پذیرش انتشار تکمیل شوند.

## ۶. محدودکردن ورودی شبکه

SSH را فقط از شبکهٔ مدیریت و API هر chain را فقط از Main مجاز کنید.
پورت ClickHouse و Neo4j نباید عمومی باشند. Docker port publishing ممکن
است از قواعد معمول UFW عبور کند؛ فقط به روشن‌بودن UFW اکتفا نکنید.

در backend مبتنی بر iptables می‌توانید helper موجود
`deployment/vm-firewall.sh` را روی **هر chain VM** نصب کنید. نمونهٔ TRON:

```bash
sudo install -m 0755 deployment/vm-firewall.sh /usr/local/sbin/aml-vm-firewall
sudo sh -c 'printf "MAIN_IP=10.20.0.10\nAPI_PORT=4001\n" > /etc/aml-vm-firewall.env'
sudo /usr/local/sbin/aml-vm-firewall
```

روی سه VM دیگر API_PORT را 5001، 6001 و 6101 قرار دهید. این helper فقط chain
اختصاصی پروژه را در DOCKER-USER مدیریت می‌کند؛ برای ماندگاری آن پس از
restart Docker/VM یک systemd oneshot با `After=docker.service network-online.target`
و `ExecStart=/usr/local/sbin/aml-vm-firewall` نصب کنید. اگر backend شما nftables
مستقیم است، قواعد معادل را طبق سیاست شبکه اجرا و از یک میزبان غیرمجاز
ردشدن دسترسی را آزمون کنید. تغییر firewall را با دسترسی console/SSH مدیریت
انجام دهید.

## ۷. ترتیب اجرا و تحویل

ابتدا چهار chain VM را با `--api-only` بالا بیاورید و schema/API را بررسی
کنید؛ سپس ingest/workers را روشن کنید و در آخر Main را بالا بیاورید:

```bash
# روی هر VM، role خودش را جایگزین کنید.
bash scripts/vm.sh tron up --build --api-only
bash scripts/vm.sh tron check-schema
bash scripts/vm.sh tron check
bash scripts/vm.sh tron up
bash scripts/vm.sh tron check-runtime
# پس از آماده‌شدن چهار شبکه، روی Main:
bash scripts/vm.sh main up --build
bash scripts/vm.sh main check-runtime
```

اگر build را روی یک ماشین سازنده انجام می‌دهید، هر پنج image برنامه را
بسازید، سپس `scripts/export-images.sh --include-bsc --include-bitcoin` و روی
VMها `scripts/import-images.sh DIRECTORY` را اجرا کنید. بستهٔ image باید
از همان commit و نسخهٔ ClickHouse باشد؛ سپس `up --pull-never` قابل استفاده
است. این مسیر VM نمی‌سازد.

برای تست fresh-install از volume تازه استفاده کنید. volume آزمایشی v6–v10
به قرارداد عملیاتی فعلی downgrade نمی‌شود؛ اتصال تصادفی نسخهٔ جدید به volume قدیمی راه‌اندازی
از صفر نیست. هیچ فرمان حذف عمومی volume در این راهنما وجود ندارد.

هر چهار Compose یک schema job دارند: `tron-schema`، `ethereum-schema`،
`bsc-schema` و `bitcoin-schema`. ابتدا ClickHouse سالم می‌شود، سپس نصب
قرارداد مشترک باید موفق شود تا API و workerها شروع شوند. SQL بومی ترون
دیگر mount نمی‌شود. روی استقرار تازه فقط `aml_*` ایجاد می‌شود؛ دیتابیس،
هویت شبکه و volume هر VM مستقل‌اند. نصب از صفر و شکل یکسان در Docker
آزموده شده است؛ پذیرش مسیر داده و VM واقعی در [TODO_LIST](../TODO_LIST.md) باقی است.
راهنمای استفاده پس از بالا آمدن: [USAGE_FA](USAGE_FA.md).
