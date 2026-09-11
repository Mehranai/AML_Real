# تحویل BSC و آماده‌سازی VM

## معماری فعلی

مسیر تحقیق کیف پول:

```text
Browser -> Main Gateway -> Analytical Node -> BSC API -> BSC ClickHouse
                               |
                               +-> Shared Neo4j: temporary snapshot -> Export -> saved
```

VM شبکه BSC فقط ClickHouse، ingestion، API و metadata worker دارد. Neo4j روی این VM نصب نمی‌شود.
ریسک در Analytical Node با سیاست توضیح‌پذیر و بدون ML محاسبه می‌شود. عدد صفر تا صد
«امتیاز ریسک شواهد» است، نه احتمال آماری یا اثبات حقوقی پول‌شویی.

داده مورد استفاده تحقیق از viewهای canonical خوانده می‌شود: hash و revision هر fact باید
با marker کامل بلاک منطبق باشد. داده نیمه‌نوشته و orphan وارد تحقیق نمی‌شود.
metadata و holdings تنها بخش‌هایی هستند که به RPC نیاز دارند؛ تحقیق داده ذخیره‌شده
با قطع بودن RPC هم کار می‌کند.

## اجرای Linux

از ریشه مخزن:

```bash
cp -n dockerizd_bsc/.env.example dockerizd_bsc/.env
bash scripts/new-service-key.sh
```

مقادیر زیر را در فایل شبکه تنظیم کنید. کلید سرویس باید با VM اصلی یکسان باشد:

```dotenv
BSC_RPC_URL=http://YOUR_BSC_NODE:8545
BSC_CLICKHOUSE_PASSWORD=YOUR_STRONG_PASSWORD
AML_SERVICE_KEY=SAME_RANDOM_KEY_AS_MAIN_VM
BSC_API_BIND_ADDRESS=10.20.0.23
BSC_API_PORT=6001
BSC_MODE=development
BSC_TRACE_MODE=auto
# فقط در اولین اجرای ingestion:
BSC_INGEST_START_BLOCK=YOUR_START_BLOCK
```

آدرس نمونه خصوصی است و باید با IP واقعی جایگزین شود. پورت 6001 فقط از IP شبکه خصوصی VM
اصلی قابل دسترس باشد؛ پورت‌های ClickHouse روی localhost باقی بمانند.
برای production، RPC یا نود باید trace تاریخی و block receipts بدهد؛ آن‌وقت
`BSC_MODE=production`، `BSC_TRACE_MODE=required` و
`BSC_REQUIRE_BLOCK_RECEIPTS=true` را تنظیم کنید. حالت development تضمین پوشش internal transfer نیست.

برای ساخت اولیه به Rust 1.92 و دسترسی دریافت dependencyها روی ماشین build نیاز دارید:

```bash
(cd dockerizd_bsc && bash scripts/refresh-linux-vendor.sh)
bash scripts/vm.sh bsc up --build --api-only
bash scripts/vm.sh bsc check
```

schema به صورت خودکار با job کوتاه‌عمر اجرا می‌شود. تغییرات قدیمی migration و دیتای موجود پاک نمی‌شوند.
برای اجرای ingestion و worker نیز:

```bash
bash scripts/vm.sh bsc up
bash scripts/vm.sh bsc logs
```

روی VM اصلی، `AML_BSC_UPSTREAM=http://10.20.0.23:6001` و همان `AML_SERVICE_KEY` را
در `.env` تنظیم کنید و `bash scripts/vm.sh main up --build` را اجرا کنید.
ورودی کاربر صفحه اصلی روی پورت 8080 است؛ از آنجا BNB Smart Chain را انتخاب کند.
API مستقیم شبکه محافظت‌شده است؛ کلید سرویس را داخل JavaScript یا مرورگر قرار ندهید.
Basic Auth و TLS را مطابق راهنمای استقرار اصلی فعال کنید.

برای اجرای همه شبکه‌ها روی یک ماشین آزمایشی:

```bash
bash scripts/aml.sh up --with-bsc
# ingestion فقط با درخواست صریح:
bash scripts/aml.sh up --with-bsc --with-ingestion
```

## انتقال آفلاین

```bash
bash scripts/export-images.sh --include-bsc
# روی ماشین مقصد:
bash scripts/import-images.sh
```

فایل‌های مخزن، envهای محلی و imageهای exportشده لازم‌اند. archive وابستگی Rust فقط برای
build از source لازم است، نه اجرای image آماده. برای image آماده ClickHouse واردشده از
این بسته، در env شبکه `BSC_CLICKHOUSE_IMAGE=clickhouse/clickhouse-server:23.8` بگذارید؛
این alias همان image بسته‌بندی‌شده و checksum-بررسی‌شده است. سپس:

```bash
bash scripts/vm.sh bsc up --api-only --pull-never
```

نام Compose project را بدون برنامه تغییر ندهید؛ volume به نام project وابسته است.
دستور down عادی volume را حذف نمی‌کند. از `down -v` روی داده واقعی استفاده نکنید.

## خروجی‌ها و محدودیت‌های دقیق

| بخش | منبع و رفتار |
|---|---|
| Graph | relationshipهای canonical، پیش‌فرض 750 edge؛ depth تا 3 و سقف 5000 edge |
| Fingerprint | شمارش کامل در بازه query، جهت جریان، counterparties، تراکنش ناموفق، قرارداد و تعداد semantic events |
| Asset flows | مبلغ خام UInt256 ورودی و خروجی جداگانه برای هر دارایی؛ موجودی نامیده نمی‌شود |
| Holdings | خواندن BNB و دارایی‌های کشف‌شده از finalized RPC؛ حداکثر 100 دارایی غیر native؛ خطا برابر صفر نیست |
| NFT | ownerOf برای ERC-721 و balanceOf(address,id) برای ERC-1155 |
| Metadata | worker محدودشده، retry تا 10 تلاش؛ override دستی تأییدشده نسبت به RPC اولویت دارد |
| Paths | تا 10 hop؛ حداکثر 100 آدرس گسترش‌یافته، 10000 حالت، 25 نتیجه پیش‌فرض |
| Directed paths | یک دارایی و ترتیب تراکنش‌های رو به جلو یا عقب؛ حالت both فقط اتصال گرافی است |
| Exposure | تا 3 hop، حداکثر 32 آدرس گسترش‌یافته و 100 edge برای هر آدرس؛ دو جهت مستقل |
| Service boundary | صرافی، bridge، custodian و DEX بازبینی‌شده ادامه traversal را قطع می‌کنند |
| Cluster leads | مقصد مشترک با 3 تا 20 فرستنده؛ فقط سرنخ، نه ادعای مالکیت مشترک |
| Export | همان تحقیق موقت در Neo4j اصلی ذخیره دائمی می‌شود، با network_id=eip155:56 |

همه سقف‌ها با truncation و محدودیت پوشش گزارش می‌شوند. مسیر حساب‌محور اثبات نمی‌کند
دقیقاً همان پول جابه‌جا شده است. تبدیل دارایی در swap یا ادامه مسیر خودکار bridge
بین شبکه‌ها هنوز انجام نمی‌شود؛ فقط evidence شامل شبکه مقصد، receiver و message ثبت و نمایش داده می‌شود.

Holdings یک live read جداگانه است؛ در snapshot تحقیق/Export نگهداری نمی‌شود و در UI هم
این تفاوت ذکر شده است. موجودی توکن‌هایی که هیچ انتقالی از آن‌ها در داده موجود نداریم کشف نمی‌شود.
مقادیر خام‌اند؛ decimals در metadata معنی واحد نمایش را مشخص می‌کند.

## وارد کردن اطلاعات

اطلاعات را مستقیم با UPDATE در view وارد نکنید. فایل JSONL به معنی یک JSON object در هر خط است.
قالب‌ها در `intelligence.example.jsonl` و `metadata.example.jsonl` همین پوشه هستند؛
صرفاً مثال ساختاری‌اند و هیچ هویت واقعی را تأیید نمی‌کنند.

```bash
cd dockerizd_bsc
cargo run --locked --bin bsc_intelligence -- import-labels labels.jsonl --dry-run
cargo run --locked --bin bsc_intelligence -- import-labels labels.jsonl
cargo run --locked --bin bsc_intelligence -- claims 0xWALLET
cargo run --locked --bin bsc_intelligence -- candidates 0xWALLET
cargo run --locked --bin bsc_intelligence -- import-metadata tokens.jsonl --dry-run
cargo run --locked --bin bsc_intelligence -- import-metadata tokens.jsonl
```

در Docker ابزار import همان تنظیمات دیتابیس داخلی را استفاده می‌کند:

```bash
docker compose -p aml-bsc --profile tools run --rm -v "$PWD/labels.jsonl:/input/labels.jsonl:ro" bsc-intelligence bsc_intelligence import-labels /input/labels.jsonl
```

نام `aml-bsc` متعلق به اجرای `vm.sh` است؛ اگر با `aml.sh` یا `--project` اجرا کرده‌اید،
همان نام project واقعی را در دستور import قرار دهید تا به دیتابیس دیگری متصل نشود.

برای تأیید یا رد claim، همان claim_id را با review_status جدید و reviewed_by معتبر
دوباره import کنید. تاریخچه قبلی append-only می‌ماند؛ آخرین revision تصمیم جاری است.
برای clustering مقدار claim_kind را cluster قرار دهید. cluster هیچ‌وقت exposure seed نمی‌شود.
برچسب seed باید صریحاً risk_level، seed_category، منبع و evidence داشته باشد.
فقط یک job/operator همزمان import انجام دهد؛ نوشتن concurrent برای یک claim پشتیبانی نمی‌شود.
منبع اولیه را خودتان از اطلاعات قابل اتکا و مجاز تأمین کنید؛ نام صرافی از روی حدس تولید نمی‌شود.

## مسیر کد

| فایل/پوشه | مسئولیت |
|---|---|
| src/bin/bsc_api.rs | ورودی HTTP service |
| src/investigation/api.rs | routing، service-key، اعتبارسنجی، سقف همزمانی و shutdown |
| src/db/warehouse.rs | query پارامتری ClickHouse، محدودیت زمان/حافظه/حجم پاسخ |
| src/investigation/wallet.rs | تجمیع investigation، fingerprint، دارایی، رویداد و پوشش |
| src/investigation/graph.rs | edge loader، wallet graph و path search محدود |
| src/intelligence.rs | import/review، active labels، cluster leads و exposure evidence |
| src/metadata.rs | metadata RPC، override و holdings finalized با عدد 256 بیتی |
| src/bin/bsc_token_metadata_worker.rs | اجرای یک batch یا worker دائمی |
| src/bin/bsc_intelligence.rs | ابزار CLI import و بازبینی claim |
| web/index.html | رسم canvas و پنل شواهد، holdings و paths |
| ../analytical_node/src/main.rs | دریافت خروجی BSC و ذخیره snapshot در Neo4j اصلی |
| ../analytical_node/src/risk.rs | امتیاز شواهد مشترک؛ بدون ML |

## مرز تحویل

این تحویل برای استقرار و تست یکپارچه VM است، نه اعلام پایان ممیزی production.
پیش از استفاده عملیاتی: داده/برچسب واقعی، reconcile روی نود واقعی، benchmark حداقل دو برابر
نرخ زنجیره، تست 24 ساعته، تست بار query، backup/restore، کنترل دسترسی و dependency audit
باید روی زیرساخت مقصد انجام شوند. تست‌های fixture جای این موارد را نمی‌گیرند.
