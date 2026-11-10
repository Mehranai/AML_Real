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
دستورهای زیر برای Ubuntu 24.04 تازه با مخزن رسمی Docker هستند. عضویت در
گروه `docker` اختیار مدیریتی روی میزبان می‌دهد. آن را به حساب اپراتور محدود کنید.

```bash
sudo install -d -m 0755 /etc/apt/keyrings
sudo curl -fsSL https://download.docker.com/linux/ubuntu/gpg -o /etc/apt/keyrings/docker.asc
sudo chmod a+r /etc/apt/keyrings/docker.asc
codename=$( . /etc/os-release; printf '%s' "${UBUNTU_CODENAME:-$VERSION_CODENAME}" )
printf 'Types: deb\nURIs: https://download.docker.com/linux/ubuntu\nSuites: %s\nComponents: stable\nArchitectures: %s\nSigned-By: /etc/apt/keyrings/docker.asc\n' \
  "$codename" "$(dpkg --print-architecture)" | sudo tee /etc/apt/sources.list.d/docker.sources >/dev/null
sudo apt update
sudo apt install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
sudo systemctl enable --now docker
sudo usermod -aG docker "$USER"
# از SSH خارج شوید و دوباره وارد شوید
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

هش کامل commit را از نسخهٔ پذیرفته‌شده بردارید. اگر آن نسخه آخرین commit
شاخه نیست، روی هر پنج VM با `git checkout --detach COMMIT_SHA` همان نسخه
را checkout کنید و خروجی `git rev-parse HEAD` را مقایسه کنید.

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
پنج نسخهٔ برنامه را از یک commit بسازید. مرکز، ارزیابی بیت‌کوین با
سیاست ناسازگار را رد می‌کند. برای خطای
`Bitcoin persisted risk differs from the central policy or evidence`
نسخهٔ image آداپتور بیت‌کوین و Main و تازگی snapshot را بررسی کنید.

## ۴. تنظیم chain VMها

| تنظیم | TRON | Ethereum | BSC | Bitcoin |
|---|---|---|---|---|
| bind API | `API_BIND_ADDRESS` | `API_BIND_ADDRESS` | `BSC_API_BIND_ADDRESS` | `BITCOIN_API_BIND_ADDRESS` |
| RPC | `TRON_RPC_URL`، در صورت نیاز `TRON_API_KEY` | `ETH_RPC_URL` | `BSC_RPC_URL` | `BITCOIN_RPC_URL/USER/PASSWORD` |
| شروع دریافت | `TRON_START_BLOCK` | `ETH_START_BLOCK` | `BSC_INGEST_START_BLOCK` | `BITCOIN_START_BLOCK` |
| رمز DB | `CLICKHOUSE_PASSWORD` | `CLICKHOUSE_PASSWORD` | `BSC_CLICKHOUSE_PASSWORD` | `BITCOIN_CLICKHOUSE_PASSWORD` |
| DB محلی | `tron_db` | `ethereum_aml` | `bsc_aml` | `bitcoin_aml` |
| پورت API | `TRON_API_PORT=4001` | `ETHEREUM_API_PORT=5001` | `BSC_API_PORT=6001` | `BITCOIN_API_PORT=6101` |

IP bind را IP خصوصی همان VM بگذارید. bind دیتابیس در TRON/Ethereum/Bitcoin
با `DATABASE_BIND_ADDRESS=127.0.0.1` و در BSC با
`DOCKER_BIND_ADDRESS=127.0.0.1` محلی می‌ماند. Main به ClickHouse مستقیم وصل
نمی‌شود؛ upstream آن API هر شبکه است.
برای این آموزش نام‌های پیش‌فرض دیتابیس را حفظ کنید تا فرمان `vm.sh ROLE db`
به همان دیتابیس وصل شود. روی BSC خط `BSC_INGEST_START_BLOCK` در نمونه
comment است. برای تعیین تاریخچه آن را فعال و مقدار واقعی را وارد کنید.
مقدار start برای هر شبکه یک ارتفاع مستقل است.

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
برای RPC بیت‌کوین بدون احراز هویت، `BITCOIN_RPC_USER` و
`BITCOIN_RPC_PASSWORD` را هر دو خالی بگذارید. برای RPC احراز هویت‌شده هر دو را
از ارائه‌دهنده بگیرید. تنظیم سخت‌گیرانهٔ BSC با `BSC_MODE=production`،
`BSC_TRACE_MODE=required` و `BSC_REQUIRE_BLOCK_RECEIPTS=true` به RPC دارای
این قابلیت‌ها نیاز دارد. Ethereum با `ETH_FINALITY_TAG=finalized` به
پشتیبانی این tag نیاز دارد. probe را با URL عملیاتی هر نقش اجرا کنید.

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
نکنید. حساب پنل را بسازید. رمز به شکل تعاملی پرسیده می‌شود.

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
قاعدهٔ helper روی رابطی اعمال می‌شود که مسیر شبکهٔ Main از آن می‌گذرد.
اگر میزبان از رابط دیگری به API قابل دسترسی است، کنترل همان مسیر را هم
با فایروال یا ACL شبکه و آزمون دسترسی بررسی کنید.
[جزئیات فایروال Docker](https://docs.docker.com/engine/network/packet-filtering-firewalls/)

نمونهٔ واحد systemd برای backend مبتنی بر iptables. بعد از آزمون اجرای
دستی helper، آن را روی همان VM شبکه نصب کنید.

```ini
# /etc/systemd/system/aml-vm-firewall.service
[Unit]
Description=Restrict published AML chain API to Main
Wants=network-online.target
Requires=docker.service
After=docker.service network-online.target
PartOf=docker.service

[Service]
Type=oneshot
ExecStart=/usr/local/sbin/aml-vm-firewall
RemainAfterExit=yes

[Install]
WantedBy=docker.service
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now aml-vm-firewall.service
sudo systemctl status aml-vm-firewall.service
```

## ۷. ترتیب اجرا و تحویل

### ۷.۱ نصب خالی، API و آزمون RPC

روی هر VM شبکه فقط یکی از نام‌های `tron`، `ethereum`، `bsc` یا `bitcoin`
را انتخاب کنید. چهار بار روی چهار میزبان اجرا می‌شود.

```bash
cd "$HOME/AML_Real"
role=tron  # مقدار درست همین VM را بگذارید
bash scripts/vm.sh "$role" up --build --api-only
bash scripts/vm.sh "$role" ps
bash scripts/vm.sh "$role" check-schema
bash scripts/vm.sh "$role" check
bash scripts/vm.sh "$role" probe
bash scripts/vm.sh "$role" db --query 'SELECT network_id, schema_version, checksum FROM aml_warehouse_identity FINAL'
```

خروجی صحیح شامل یک هویت شبکه و schema version برابر ۶ است. در نصب
تازه، API ممکن است سالم و checkpoint هنوز خالی باشد. اگر schema job موفق
نشده، پیش از شروع worker علت را از `logs` پیدا کنید. probe را برای هر
شبکه جداگانه با آدرس RPC نهایی موفق کنید.

### ۷.۲ شروع دریافت و سنجش پیشرفت

```bash
bash scripts/vm.sh "$role" up
bash scripts/vm.sh "$role" check-runtime
bash scripts/vm.sh "$role" db --query 'SELECT network_id, initialized, start_block, last_block_number, observed_rpc_height, revision FROM aml_sync_state_current'
bash scripts/vm.sh "$role" db --query 'SELECT block_number, status, attempts, error_code FROM aml_ingestion_failures_current ORDER BY last_failed_at_unix_ms DESC LIMIT 10'
```

`check-runtime` را پس از سالم‌شدن کانتینرها اجرا کنید. برای صحت داده،
چند بار فاصلهٔ head مشاهده‌شده و آخرین بلاک منتشرشده را ثبت کنید و
نمونهٔ تراکنش را با RPC مقایسه کنید. فاصله در شروع catch-up طبیعی است.
توقف ارتفاع همراه با retry یا dead-letter نیاز به بررسی RPC و log دارد.
دستورهای توقف و ادامهٔ worker، `pause-ingestion` و `resume` هستند.
[عیب‌یابی ingest و repair](USAGE_FA.md)

### ۷.۳ راه‌اندازی Main و آزمون کاربر

ابتدا از Main، `/ready` هر چهار API را روی IP خصوصی خودش امتحان کنید.
در صورت استفاده از IP نمونه:

```bash
for origin in http://10.20.0.21:4001 http://10.20.0.22:5001 http://10.20.0.23:6001 http://10.20.0.24:6101; do
  curl --fail --silent --show-error --connect-timeout 5 "$origin/ready"
  printf '\n'
done
bash scripts/vm.sh main up --build
bash scripts/vm.sh main check-runtime
curl --fail --silent --show-error http://10.20.0.10:8080/health
curl --fail --silent --show-error --user amladmin http://10.20.0.10:8080/ready
```

مرورگر را با کاربر Basic Auth باز کنید. در هر شبکه یک آدرس با دادهٔ
واقعاً منتشرشده انتخاب کنید. تحقیق Wallet و Paths را اجرا کنید،
شبکه و ارتفاع شاهد را ببینید، Export و سپس بازگشایی را امتحان کنید.
یک آدرس یا تراکنش آزمون را به Watchlist اضافه کنید و زمان بررسی بعدی
و نتیجهٔ آن را ببینید. یک پروندهٔ آزمایشی با تحقیق ذخیره‌شده بسازید و
گردش بازبینی و دانلود گزارش را با دو حساب جدا بررسی کنید.
برای بازبین دوم، فرمان زیر را روی Main اجرا کنید. `-c` را فقط هنگام
ایجاد اولیهٔ فایل به کار ببرید. وضعیت تحویل webhook فقط در صورت تنظیم
گیرندهٔ عملیاتی آزموده می‌شود.

```bash
sudo htpasswd -B gateway/auth/users.htpasswd amlreviewer
sudo chown root:101 gateway/auth/users.htpasswd
sudo chmod 640 gateway/auth/users.htpasswd
# پس از به‌روزرسانی فایل mount شده، Gateway را با فایل جدید بسازید
docker compose --project-name aml-main --env-file .env up -d --no-deps --force-recreate gateway
```

### ۷.۴ توقف، به‌روزرسانی و معیار تحویل

```bash
bash scripts/vm.sh "$role" pause-ingestion  # API و DB برقرار می‌مانند
bash scripts/vm.sh "$role" resume           # ادامه از checkpoint فعلی
bash scripts/vm.sh "$role" logs
bash scripts/vm.sh "$role" ps
```

برای توقف کامل سرویس‌های یک role، `bash scripts/vm.sh ROLE down` را اجرا
کنید. volume با این فرمان حفظ می‌شود. برای انتشار بعدی یک commit مشخص
را در هر پنج VM هماهنگ کنید، قبل از تغییر نسخه پذیرش schema و بازیابی
را روی محیط جدا بسنجید و همین ترتیب اجرا را تکرار کنید.

| نشانه | بررسی اول |
| --- | --- |
| Compose متغیر الزامی می‌خواهد | فایل `.env` درست همان role و مقدار رمز یا service key را بررسی کنید |
| API سالم است ولی داده خالی است | `initialized`، start، فعال‌بودن worker و log خطای RPC را ببینید |
| Main از VM شبکه پاسخ نمی‌گیرد | IP و پورت bind، مسیر خصوصی، firewall و نشانی upstream را کنترل کنید |
| پاسخ 401 یا 403 | احراز هویت پنل را از کلید سرویس بین VMها جدا بررسی کنید. پنج `AML_SERVICE_KEY` باید یکسان باشند |
| خطای هویت یا checksum | با `check-schema` نسخه، نام دیتابیس و تعلق volume به همان شبکه را بررسی کنید |
| ریسک `UNKNOWN` یا trace غایب | دامنهٔ دادهٔ منتشرشده، اعتبار منبع و قابلیت RPC را کنترل کنید |

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
آزموده شده است. تحویل VM شامل آزمون روی RPC و دادهٔ هدف، آزمون بازیابی،
TLS و کنترل نقش‌ها، ثبت رویداد عملیاتی، پایش lag و ظرفیت و تأیید بازبین است.
وضعیت هر مورد در [فهرست پذیرش](../TODO_LIST.md) آمده است.
[راهنمای استفاده پس از بالا آمدن](USAGE_FA.md)
