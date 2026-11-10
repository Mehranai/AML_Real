# آزمون و پذیرش

فقط یک مرجع جاری برای آزمون‌ها نگه‌داری می‌شود؛ گزارش‌های DB مرحله‌ای
حذف شده‌اند. نتیجهٔ کد، صحت دیتاست، پوشش mainnet و پذیرش VM را جدا ببینید.

## بررسی کد

از ریشهٔ مخزن با Rust مطابق Dockerfileها (اکنون 1.92) و Python:

```bash
python -m unittest discover -s scripts/tests -p 'test_*.py'
cargo test --manifest-path shared/warehouse/Cargo.toml
cargo test --manifest-path dockerizd_bitcoin/Cargo.toml --no-run
cargo test --manifest-path dockerizd_bitcoin/Cargo.toml --lib
cargo test --manifest-path dockerizd_ethereum/Cargo.toml --lib
cargo test --manifest-path dockerizd_tron/app/Cargo.toml --lib
cargo test --manifest-path dockerizd_bsc/Cargo.toml --lib
bash scripts/tests/linux-cli.sh
```

روی Linux تست‌های Bash را نیز اجرا کنید؛ روی Windows بخشی skip می‌شوند.
native build ترون به `pkg-config` و OpenSSL development headers نیاز دارد؛
Dockerfile پروژه آن‌ها را نصب می‌کند. برای محیط‌های آفلاین پس از آماده‌بودن
cache می‌توان `--offline` داد. build cache جزو source قابل push نیست.

صحت خروجی SQL و snapshot قرارداد با exporter کنترل می‌شود؛ برای هر فایل
گزینهٔ مربوط (`--upgrade/--core/--analytics/--visibility/--tron-evidence`) را بدهید:

```bash
cargo run --manifest-path shared/warehouse/Cargo.toml --example export_sql -- \
  --check --contract-json docs/warehouse/current-contract.json
cargo run --manifest-path shared/warehouse/Cargo.toml --example export_sql -- \
  --check --migrations shared/warehouse/sql/migrations
```

## ClickHouse واقعی موقت

تست‌های `--ignored` schema و داده می‌نویسند و برای آزمون drift عمداً بعضی
اشیای fixture را تغییر می‌دهند. آن‌ها را فقط روی کانتینر موقت تازه اجرا کنید؛
هرگز URL دیتابیس کاربر/VM مستقر ندهید. نمونه با حساب صرفاً آزمایشی:

```bash
docker run -d --name aml-schema-test -p 127.0.0.1:18141:8123 \
  -e CLICKHOUSE_USER=warehouse_test -e CLICKHOUSE_PASSWORD=warehouse-test-only \
  clickhouse/clickhouse-server:23.8.16.16
WAREHOUSE_TEST_URL=http://127.0.0.1:18141 \
  cargo test --manifest-path shared/warehouse/Cargo.toml --test clickhouse_contract \
  -- --ignored --test-threads=1
docker rm --force --volumes aml-schema-test
```

تست چهار شبکه، شکل یکسان جدول/ستون، هویت، نصب مجدد، قطع init، drift،
checkpoint و پنهان‌بودن facts نیمه‌تمام را کنترل می‌کند. آزمون pipeline
بیت‌کوین نیز از دیتابیس خالی و RPC نمونه شروع می‌شود و restart، prevout،
graph/path، مقدار دقیق و reorg را بررسی می‌کند؛ به دیتابیس قدیمی نیاز ندارد.
آن تست حساب `bitcoin_test` با رمز `bitcoin-test-only` و database تازهٔ
`bitcoin_aml` می‌خواهد و متغیر `BITCOIN_TEST_CLICKHOUSE_URL` را می‌خواند.
برای آن کانتینر تازهٔ جدا بسازید؛ حساب فوق به‌صورت خودکار توسط image تنظیم
می‌شود و نیازی به CREATE USER در کانتینر آزمون مشترک نیست:

```bash
docker run -d --name aml-bitcoin-test -p 127.0.0.1:18142:8123 \
  -e CLICKHOUSE_USER=bitcoin_test -e CLICKHOUSE_PASSWORD=bitcoin-test-only \
  clickhouse/clickhouse-server:23.8.16.16
# پس از پاسخ موفق /ping با همان حساب آزمایشی:
BITCOIN_TEST_CLICKHOUSE_URL=http://127.0.0.1:18142 \
  cargo test --manifest-path dockerizd_bitcoin/Cargo.toml --test clickhouse_pipeline \
  -- --ignored --test-threads=1
docker rm --force --volumes aml-bitcoin-test
```

پیش از هر دو فرمان cargo، آماده‌شدن `/ping` را با `curl --fail --user`
و حساب همان کانتینر بررسی کنید؛ وجود کانتینر به‌تنهایی آماده‌بودن سرور نیست.

## پذیرش release

پاس‌شدن تست کتابخانه یا fixture شرط کافی تحویل نیست. روی پنج VM واقعی
باید موارد زیر پذیرفته شوند:

- دریافت مستقیم RPC هر چهار شبکه و تطبیق hash، مبلغ، تعداد، coverage و lag.
- عدم ایجاد/مصرف جدول native قبلی بعد از اتصال کامل writer/readerها.
- API و پنل و Neo4j مرکزی، auth، snapshot، تغییر source و reorg.
- worker، retry، outage، disk pressure، backup و restore مستقل و ظرفیت هدف.
- intelligence معتبر و ارزیابی detector/risk؛ هیچ نتیجهٔ سالم‌بودن از نبود داده ساخته نشود.

`scripts/warehouse_backup.py` و overlay دیسک backup حفظ شده‌اند. دستورهای
آن را با `--help` ببینید؛ backup نیازمند توقف writer و restore روی مقصد
جداست. scheduler/retention و پذیرش restore روی VM در TODO عملیات بازند.

نتایج DB-1 تا DB-3 زیر، در زمان قرارداد v5 ثبت شده‌اند؛ قرارداد جاری v6
و پذیرش بخش نخست ترون در پایان همین فایل آمده است.

## پذیرش قرارداد اجرایی DB-1

تست [`core_contract`](../shared/warehouse/tests/core_contract.rs) از همان
کانتینر موقت بالا و چهار database خالی استفاده می‌کند:

```bash
WAREHOUSE_TEST_URL=http://127.0.0.1:18141 \
  cargo test --manifest-path shared/warehouse/Cargo.toml --test core_contract \
  -- --ignored --test-threads=1
```

این تست مبلغ UInt256 و token ID حداکثری، موقعیت trace، raw payload،
فیلدهای nullable و رفتار مشترک reader را بررسی می‌کند. در هر شبکه یک
مبلغ هنگام insert عمداً تغییر می‌کند؛ با همان تعداد ردیف، انتشار باید
رد شود، checkpoint قبلی ثابت بماند و retry با revision تازه موفق شود.

تست عادی `live_batch_integrity_is_checked_before_any_io` نیز روی هر چهار
شبکه، شناسهٔ تکراری، موقعیت غلط، fact بدون تراکنش، پروفایل نامرتبط،
coverage نامعتبر، مبلغ عددی به‌جای رشته و endpoint ساختگی را پیش از هر
query/insert رد می‌کند. pipeline واقعی Bitcoin پس از اتصال writer آن به
همین مسیر، restart و reorg را دوباره کنترل می‌کند.

نتیجهٔ DB-1 در ۲۰۲۶-۱۱-۰۶: تست چهار دیتابیس خالی با writer و reader مشترک
و سناریوی تغییر مبلغ/retry موفق شد. تست جدید preflight روی چهار شبکه و
تست RPC نمونه → writer فعال Bitcoin → ClickHouse با restart/reorg نیز
موفق شدند. در reader، تبدیل عدد Float64 طبق نوع ستون یکدست شد؛ مقدار
UInt256 و token ID به رشتهٔ صحیح خام باقی می‌مانند. ساختار نسخهٔ ۵ با
۳۹ جدول/۵۳ view و snapshot آن تغییر نکرد. suite عادی shared و تست جدید
preflight موفق‌اند؛ تمام targetهای Bitcoin کامپایل و ۴۱ تست کتابخانهٔ آن
نیز پس از تغییر writer موفق شدند. پذیرش RPC/VM واقعی همچنان جداست.

## پذیرش اتصال زندهٔ Ethereum ـ DB-2

آزمون [shared_pipeline](../dockerizd_ethereum/tests/shared_pipeline.rs) با RPC
مصنوعی داخل خود تست و ClickHouse واقعی اجرا می‌شود. دیتابیس ثابت
`ethereum_aml_db2` باید از ابتدا وجود نداشته باشد؛ تست در غیر این صورت متوقف
می‌شود. URL خصوصی پروژه یا VM واقعی را برای fixture استفاده نکنید.

```bash
docker run -d --name aml-ethereum-test -p 127.0.0.1:18146:8123 \
  -e CLICKHOUSE_USER=warehouse_test -e CLICKHOUSE_PASSWORD=warehouse-test-only \
  clickhouse/clickhouse-server:23.8.16.16
# پس از پاسخ موفق /ping با همان حساب آزمایشی:
ETHEREUM_TEST_CLICKHOUSE_URL=http://127.0.0.1:18146 \
AML_SERVICE_AUTH_REQUIRED=true \
AML_SERVICE_KEY=ethereum-fixture-service-key-0123456789abcdef \
  cargo test --manifest-path dockerizd_ethereum/Cargo.toml --test shared_pipeline \
  -- --ignored --nocapture --test-threads=1
docker rm --force --volumes aml-ethereum-test
```

این تست از init تازه و init مجدد شروع می‌شود و ingest واقعی adapter،
checkpoint، جلوگیری از دو writer، تراکنش failed، UInt256 و token ID
حداکثری، log index، شاهد اصلی receipt/trace و withdrawal بدون tx ساختگی
را کنترل می‌کند. metadata از صف واقعی با state بلاک ثابت خوانده می‌شود؛
API دارای auth، investigation و holdings نیز مصرف‌کنندهٔ همین داده هستند.
restart و تکمیل trace بلاک قدیمی بدون عقب‌بردن tip، رد کاهش coverage و
تعویض هش، CSV با reviewer واقعی fixture، خوشه‌بندی، exposure چند hop و
مرز سرویس، risk و حذف خروجی معتبر پس از revoke نیز در همان مسیرند.
تست نبودِ هرگونه شیء native در دیتابیس تازه را بررسی می‌کند.

پذیرش RPC mainnet، VM واقعی، Neo4j مرکزی، ظرفیت و parity کامل API چهار
شبکه جزو این fixture نیستند. schema مشترک همچنان v5 با ۳۹ جدول، ۵۳ view
و ۶۸۲ ستون است؛ پوشش جدید با نگاشت writer/reader حاصل می‌شود.

نتیجهٔ DB-2 در ۲۰۲۶-۱۱-۰۶: سناریوی کامل بالا روی ClickHouse واقعی
`23.8.16.16` موفق شد؛ ۳۱ تست کتابخانه و یک تست هویت شبکهٔ اتریوم موفق
شدند و همهٔ binary/test targetهای آن کامپایل شدند. ۲۴ تست عادی قرارداد
مشترک و تست واقعی core روی چهار دیتابیس خالی نیز موفق شدند. Compose اتریوم
با env نمونه و مقادیر مصنوعی معتبر بود؛ اسکن وابستگی، مصرف جدول native
قدیمی در کد فعال آن پیدا نکرد. کانتینر/volume آزمون جدا از stack پروژه
بودند. این نتایج پذیرش RPC واقعی و VM کاربر محسوب نمی‌شوند.

## پذیرش اتصال زندهٔ BSC ـ DB-3

پنج سناریوی ignored در
[service.rs](../dockerizd_bsc/src/ingestion/service.rs) و
[سناریوی کامل API/NFT](../dockerizd_bsc/tests/support/shared_pipeline.rs)
از RPC مصنوعی داخل خود تست و databaseهای تازه با نام یکتا استفاده می‌کنند.
فایل دوم داخل ماژول تست service کامپایل می‌شود و binary موازی نیست.

```bash
docker run -d --name aml-bsc-test -p 127.0.0.1:18147:8123 \
  -e CLICKHOUSE_USER=warehouse_test -e CLICKHOUSE_PASSWORD=warehouse-test-only \
  clickhouse/clickhouse-server:23.8.16.16
# پس از پاسخ موفق /ping با همان حساب آزمایشی:
BSC_TEST_CLICKHOUSE_URL=http://127.0.0.1:18147 \
BSC_TEST_CLICKHOUSE_USER=warehouse_test \
BSC_TEST_CLICKHOUSE_PASSWORD=warehouse-test-only \
  cargo test --manifest-path dockerizd_bsc/Cargo.toml --lib \
  -- --ignored --nocapture --test-threads=1
docker rm --force --volumes aml-bsc-test
```

این تست‌ها موارد زیر را بررسی می‌کنند:

- دریافت از RPC اصلی adapter تا writer و canonical reader مشترک؛ receipt
  fallback، status تراکنش failed، creation و نگه‌داشتن پاسخ خام اصلی.
- توقف عمدی پس از transaction/log/relationship/discovery/semantic/complete؛
  checkpoint قبل از انتشار ثابت می‌ماند و retry با revision تازه موفق است.
- replay با تعداد ثابت، refresh پوشش trace، جلوگیری از دو writer، restart،
  dead-letter و requeue صریح، reorg با ancestor و بازیابی کل بازهٔ غیر genesis.
- ERC721 و ERC1155 batch با token ID و مبلغ UInt256 حداکثری، sub-index واقعی،
  metadata از بلاک ثابت و نامعتبرشدن metadata شاخهٔ قدیمی پس از reorg.
- import/review/revoke با actor و دلیل، exposure هنگام درخواست، source snapshot،
  API دارای auth، status، investigation، paths و موجودی دقیق native/NFT.
- منبع غیرفعال با import دوباره فعال نمی‌شود؛ replay اجباری registry فعلی
  را می‌خواند و تفسیر قبلی بدون منبع فعال را دوباره منتشر نمی‌کند.

نتیجهٔ DB-3 در ۲۰۲۶-۱۱-۰۶: هر پنج سناریوی واقعی ClickHouse موفق شدند؛
سناریوی replay بعد از افزودن کنترل منبع غیرفعال نیز دوباره موفق شد.
۴۳ تست عادی موفق‌اند و هر پنج ignored جداگانه اجرا شده‌اند. همهٔ binary/test
targetهای BSC در نسخهٔ نهایی کامپایل شدند. تست مستقل HTTP نشان داد RPC
شبکهٔ اشتباه، holdings unavailable با پاسخ 503 و فهرست دارایی خالی می‌دهد؛
موجودی صفر فرض نمی‌شود.

binary واقعی `bsc_schema` روی دیتابیس موقت تازه، v5 با ۳۹ جدول/۵۳ view نصب
کرد. fixture موجود مرورگر به `aml_*` منتقل و seed آن روی همان schema واقعی
بررسی شد: ۱۵ تراکنش، ۶ رابطهٔ کیف پول نمونه، ۴ log توکن و پنهان‌ماندن ردیف
منتشرنشده صحیح بودند. syntax فایل Node، Compose با env مصنوعی، قالب Rust و
ارجاع‌های محلی راهنماها نیز بررسی شدند. snapshot قرارداد ۶۸۲ ستون تغییر نکرد.

مرورگر/Neo4j مرکزی، release image، RPC mainnet، VM کاربر و benchmark بار
در این مرحله اجرا نشده‌اند. پذیرش کد با این موارد یکسان نیست. تست مرورگر
همچنان در [investigation.mjs](../dockerizd_bsc/tests/investigation.mjs) باقی است؛
fixture آن برای UI/paths مصنوعی است و جای تست زندهٔ ingestion Rust را نمی‌گیرد.

## نتیجهٔ ممیزی پاک‌سازی ۲۰۲۶-۱۱-۰۶

| بررسی | نتیجه |
|---|---|
| Rust قرارداد مشترک | ۲۳ تست موفق |
| Rust Bitcoin | کامپایل تمام targetها؛ ۴۱ تست کتابخانه موفق |
| Rust Ethereum | ۳۵ موفق، یک تست اختیاری اجرا نشد |
| Rust TRON | ۸۲ موفق |
| Rust BSC | ۴۴ موفق، پنج تست اختیاری اجرا نشد |
| Python روی Linux | هر ۴۲ تست موفق؛ همان suite روی Windows پنج skip دارد |
| CLI لینوکس | تمام گروه‌ها موفق؛ Docker/HTTP/Cargo در این suite mock هستند |
| ClickHouse واقعی `23.8.16.16` | دو تست قرارداد چهار شبکه و یک pipeline بیت‌کوین موفق |
| تولید قرارداد | پنج migration SQL، query روابط و JSON جاری مطابق exporter |
| پیکربندی | Compose هر پنج role معتبر؛ image دیتابیس چهار شبکه یکسان |

در تست واقعی ClickHouse، نصب از صفر، هویت و شکل یکسان، init مجدد/قطع‌شده،
رد drift و مرز انتشار چهار شبکه بررسی شدند. pipeline بیت‌کوین از RPC نمونه
به storage واقعی رسید و restart/prevout/graph/path/reorg را آزمود. این تست
با RPC واقعی mainnet یا روی VM کاربر اجرا نشده است. سایر integrationهای
اختیاری، browser/Neo4j انتهابه‌انتها، backup/restore واقعی و benchmark بار
در این نوبت اجرا نشدند و موفق فرض نمی‌شوند.

## پذیرش قرارداد و شاهد اصلی TRON — DB-4.1

قرارداد جاری v6 با **۴۲ جدول، ۵۶ view و ۷۳۹ ستون** است. migration جدید
فقط contract/protobuf، log، internal و resource ترون را تکمیل می‌کند؛
checksum و SQL فازهای ۱ تا ۵ ثابت مانده‌اند. snapshot JSON و SQL frozen
با exporter کنترل می‌شوند.

روی ClickHouse موقت واقعی `23.8.16.16` این آزمون‌ها موفق شدند:

- `clickhouse_contract`: نصب و تکرار روی چهار database خالی با شکل یکسان،
  هویت شبکه، قطع init، کنترل drift و مرز انتشار (۲ تست).
- `core_contract`: رفت‌وبرگشت تمام ستون‌ها، پروفایل‌های تخصصی از جمله سه
  جدول جدید، پنهان‌ماندن facts ناقص و retry در چهار شبکه (۱ تست).
- `visibility_contract::interrupted_tron_v6_resumes_and_rejects_applied_drift`:
  قطع پس از اضافه‌شدن بخشی از ستون‌ها، ledger قبلی، ادامهٔ نصب، init مجدد،
  رد ادعای بی‌زمینهٔ verification و رد تغییر schema اعمال‌شده (۱ تست).
- `dockerizd_tron/app/tests/shared_warehouse.rs`: native driver، حساب
  read-only و RPC نمونه → تطبیق بایت‌ها → facts → انتشار مشترک؛ قطع پس از
  نوشتن facts، نامرئی‌بودن آن‌ها، retry با revision تازه و تطبیق دقیق تمام
  ستون‌های خوانده‌شده (۲ تست).

```bash
WAREHOUSE_TEST_URL=http://127.0.0.1:18141 \
  cargo test --manifest-path shared/warehouse/Cargo.toml --test visibility_contract \
  interrupted_tron_v6_resumes_and_rejects_applied_drift -- --ignored --test-threads=1
WAREHOUSE_TEST_URL=http://127.0.0.1:18141 \
  cargo test --manifest-path dockerizd_tron/app/Cargo.toml --test shared_warehouse \
  -- --ignored --test-threads=1
cargo test --manifest-path dockerizd_tron/app/Cargo.toml --test tron_source_evidence
```

تست‌های بالا باید روی fixture تازه اجرا شوند؛ بعضی عمداً view را حذف
می‌کنند تا drift را تشخیص دهند. برای تکرار از کانتینر آزمایشی تازه استفاده کنید.

۸۲ تست کتابخانهٔ قبلی ترون و ۸ تست جدید source موفق شدند. تست‌های جدید
شامل بردار بایت مستقل مستند رسمی، تغییر مبلغ/آدرس/calldata/ترتیب/type URL،
receipt تراکنش یا بلاک دیگر، منفی/اعشاری/overflow، protobuf خراب یا مبهم،
صفر صریح در برابر NULL، log با مقدار/شناسهٔ حداکثری، internal ردشده و ترتیب
callValueInfo، عدم تأیید پارامتر پشتیبانی‌نشده و نبود receipt ساختگی genesis
هستند. خطای صریح `ret` یا `TransactionInfo.result` بر نتیجهٔ موفق مقدم است.
۲۵ تست عادی shared، ۴۱ تست کتابخانهٔ Bitcoin و ۳۲ تست ابزار backup نیز
روی قرارداد جاری موفق شدند.

در زمان پایان DB-4.1، کنترل source در ingest فعال بود، اما اتصال publisher
به دریافت زنده و انتقال مصرف‌کنندگان هنوز باقی بود. نتیجهٔ اتصال دریافت
در DB-4.2 در بخش بعد آمده است؛ پذیرش mainnet/VM یا decoder کامل NFT
از آزمون قرارداد به‌تنهایی نتیجه نمی‌شود.

## پذیرش دریافت و انتشار زندهٔ TRON — DB-4.2

این مرحله schema تازه‌ای اضافه نمی‌کند؛ قرارداد همان v6 با ۴۲ جدول،
۵۶ view و ۷۳۹ ستون است. batcherهای مستقل حذف شده‌اند و Loader واقعی،
facts هر بلاک و checkpoint را از publisher مشترک منتشر می‌کند. APIهای
دادهٔ زنجیره از projection خواندنی روی `aml_*` استفاده می‌کنند؛ جدول یا
کپی موازی برای سازگاری ساخته نمی‌شود. رکوردهای عملیاتی metadata،
intelligence، clustering/exposure و وابستگی تحلیل محلی در پایان DB-4.2
باقی بودند و موضوع DB-4.3 شدند.

در ۲۰۲۶-۱۱-۰۷ روی ClickHouse واقعی `23.8.16.16` و RPC مصنوعی درون تست،
هر سه سناریوی [committed_visibility](../dockerizd_tron/app/tests/committed_visibility.rs)
موفق شدند:

- genesis با فیلدهای پیش‌فرض protobuf، بلاک خالی و TRC10 قدیمی با lookup
  هویت یکتای دارایی؛ پاسخ اصلی catalog و زمان مشاهده حفظ می‌شوند.
- دادهٔ تراکنش یا receipt نامعتبر قبل از انتشار رد می‌شود؛ هیچ بلاک ناقص
  یا cursor جلو‌رفته‌ای دیده نمی‌شود و retry با دادهٔ اصلاح‌شده موفق است.
- دریافت چندتراکنشی با پاسخ‌های RPC خارج از ترتیب؛ مبلغ UInt256 و NFT ID
  حداکثری، موقعیت contract/log/internal و call value اصلی، تراکنش failed،
  semantic event و صف کشف metadata دقیق باقی می‌مانند. investigation، graph،
  activity، fingerprint، holdings و ingestion health همان داده را می‌خوانند.
  restart دوباره receipt نمی‌گیرد؛ replay بلاک قدیمی tip را عقب نمی‌برد و
  benchmark در بازهٔ منتشرشده نتیجهٔ NOOP را در قرارداد مشترک ثبت می‌کند.

در سناریوی آخر، جدول‌های فیزیکی قدیمی fact، cursor، failure و discovery
خالی می‌مانند. holdings، جمع دقیق netflow تاریخچهٔ دریافت‌شده است؛ موجودی
RPC فرض نمی‌شود. تاریخچه ناقص و decimals نامعلوم صریح‌اند و مقدار انسانی
نامعلوم NULL می‌ماند.

آزمون [شش مرز خرابی](../dockerizd_tron/app/src/services/tron/publication_store.rs)
نیز روی ClickHouse واقعی موفق شد: قبل/بعد از reserve، پس از اولین fact،
قبل/بعد از complete و قبل از checkpoint. در هر مورد، وضعیت منتشرشده ثابت
می‌ماند؛ Loader تازه retry می‌کند، ردیف تکراری قابل مشاهده ساخته نمی‌شود و
failure حل‌شده ثبت می‌شود. رقابت دو پردازه برای قفل writer نیز آزموده شده است.
Compose برای ingestion/replay/benchmark از volume مشترک قفل استفاده می‌کند.

```bash
# روی ClickHouse آزمایشی تازه با حساب warehouse_test / warehouse-test-only:
WAREHOUSE_TEST_URL=http://127.0.0.1:18141 \
  cargo test --manifest-path dockerizd_tron/app/Cargo.toml --test committed_visibility \
  -- --ignored --test-threads=1
WAREHOUSE_TEST_URL=http://127.0.0.1:18141 \
  cargo test --manifest-path dockerizd_tron/app/Cargo.toml --lib \
  shared_writer_recovers_at_six_physical_write_boundaries -- --ignored --test-threads=1
cargo check --manifest-path dockerizd_tron/app/Cargo.toml --all-targets
cargo test --manifest-path dockerizd_tron/app/Cargo.toml --lib --test tron_source_evidence
```

این fixture، پذیرش RPC mainnet، throughput واقعی، VM یا Neo4j مرکزی نیست.
replay این مرحله فقط بازخوانی همان شاخهٔ منتشرشده است؛ تعویض خودکار شاخهٔ
solid یا decoder کامل تمام استانداردهای NFT ادعا نمی‌شود. تست‌های قدیمی
`analytics_pipeline` و `tron_stack_smoke` در پایان DB-4.2 هنوز جدول بومی را
مستقیم seed می‌کردند؛ جایگزینی آن‌ها با مسیر زنده جزو پذیرش DB-4.3 است.

بررسی نهایی همان نسخه: تمام targetها کامپایل شدند؛ ۸۵ تست کتابخانه و
۸ تست شاهد اصلی موفق شدند. تنها ignored کتابخانه همان آزمون شش مرز خرابی
است که جداگانه روی ClickHouse اجرا و موفق شد. قالب Rust، Compose با env
نمونه، syntax اسکریپت پنل و ارجاع‌های محلی راهنماها نیز بررسی شدند.

## پذیرش مصرف‌کنندگان و رکوردهای عملیاتی TRON — DB-4.3

قرارداد همچنان v6 با ۴۲ جدول، ۵۶ view و ۷۳۹ ستون است؛ جدول یا سرویس
موازی برای snapshot/ML محلی ساخته نشد. writer و reader عملیاتی از جدول‌های
مشترک موجود استفاده می‌کنند. در پایان DB-4.3، فایل/mount قدیمی SQL ترون
برای DB-5 باقی بود؛ init/check کد فقط قرارداد مشترک را نصب و کنترل می‌کند.

در ۲۰۲۶-۱۱-۰۷، تست بازنویسی‌شدهٔ
[analytics_pipeline](../dockerizd_tron/app/tests/analytics_pipeline.rs) روی ClickHouse
واقعی `23.8.16.16` موفق شد. دیتابیس fixture فقط `aml_*` دارد و تراکنش‌ها
از RPC مصنوعی با protobuf واقعی به Loader اصلی داده می‌شوند؛ جدول بومی
seed یا برای سازگاری ایجاد نمی‌شود. موارد پذیرفته‌شده:

- CLI واقعی init با نام database تنظیم‌شده و دیتابیسِ هنوز ساخته‌نشده؛
  سپس API واقعی با auth، پاسخ 401 بدون کلید و تحقیق/وضعیت دارای snapshot.
- ثبت منبع و claim، pending بودن قبل از review، تأیید، revoke، غیرفعال‌کردن
  منبع و ورود مجدد بدون reset کردن claim تأییدشده. کاندیدای ساختاری تا review
  عضویت فعال نمی‌سازد؛ خروجی cluster با snapshot و seal مشترک منتشر می‌شود.
- exposure دارای relationship/tx اصلی، ترتیب تراکنش در بلاک و توقف در
  صرافی؛ مسیر عقب‌گرد پذیرفته نمی‌شود. بازاجرای scan تکرار قابل مشاهده
  ایجاد نمی‌کند. مسیر مشاهده‌شده ادعای تخصیص قطعی همان وجوه ندارد.
- worker metadata با خطای RPC، backoff، retry موفق، head واقعی مشاهده و
  تغییر head بین تماس‌ها. metadata verified فرض نمی‌شود و به زمان کشف
  قدیمی دارایی نسبت داده نمی‌شود. replay، metadata و نتایج مشتق‌شدهٔ وابسته
  به revision قبلی را نامرئی می‌کند و بازتولید موفق است.

۷۸ تست کتابخانه و ۸ تست شاهد اصلی موفق شدند. سه سناریوی
`committed_visibility` نیز روی همان schema مشترک دوباره موفق شدند؛ تعداد
تست‌های قدیمی با حذف executor/ذخیرهٔ ML غیرفعال و parser مربوط به DDL بومی
کاهش یافته است. تست `tron_stack_smoke` که به جدول‌های قدیمی و Neo4j محلی
وابسته بود با پذیرش مسیر زندهٔ بالا جایگزین شد.

[آزمون مرکزی](../analytical_node/src/tron_operations_tests.rs) خروجی واقعی همین
fixture را مصرف می‌کند: ساخت تحقیق، ذخیرهٔ payload/گراف، بازگشایی و Export،
تشخیص تغییر رکورد عملیاتی بدون بلاک جدید و UNKNOWN پس از تغییر/عدم تأیید
منبع موفق‌اند. هشت تست عادی سرویس مرکزی و این سناریوی جداگانه موفق شدند.
سمت storage مرکزی در این تست، fixture پروتکل HTTP Neo4j است؛ اجرای واقعی
Neo4j/VM مرکزی ادعا نمی‌شود. endpoint قدیمی snapshot محلی پاسخ 410 می‌دهد
و مسیر `GET /api/tron/wallet/{address}/investigation` روی Main را معرفی می‌کند؛ ML محلی همچنان
DISABLED است و ریسک از سیاست مرکزی موجود می‌آید.

```bash
WAREHOUSE_TEST_URL=http://127.0.0.1:18141 \
TRON_OPERATIONS_PROOF=/tmp/tron-operations-proof.json \
  cargo test --manifest-path dockerizd_tron/app/Cargo.toml --test analytics_pipeline \
  -- --ignored --nocapture --test-threads=1
TRON_OPERATIONS_PROOF=/tmp/tron-operations-proof.json \
  cargo test --manifest-path analytical_node/Cargo.toml \
  shared_tron_evidence_reaches_central_create_reopen_and_export -- --ignored
```

پذیرش RPC mainnet، ظرفیت/تازگی زیر بار واقعی، VMهای کاربر و Neo4j واقعی
همچنان در مراحل بعدی‌اند. آزمون مشترک شش مرز خرابی نوشتن نیز روی fixture
فاقد جدول بومی دوباره موفق شد. Compose با env نمونه، قالب Rust و ارجاع‌های
محلی مستندات بررسی شدند؛ این نتایج جایگزین پذیرش استقرار کاربر نیستند.

## پذیرش نصب و SQL مشترک — DB-5

در ۲۰۲۶-۱۱-۰۷، initializer واقعی هر چهار شبکه روی **چهار سرور مستقل و
خالی** با image یکسان `clickhouse/clickhouse-server:23.8.16.16` اجرا شد.
تست [schema_installation.py](../scripts/tests/schema_installation.py) هر
binary را از پوشهٔ موقت و با تنظیمات صریح آزمایشی اجرا می‌کند؛ فایل `.env`
پروژه را نمی‌خواند. در هر شبکه init، `--check` و init مجدد موفق شدند.

هر دیتابیس دقیقاً **۴۲ جدول، ۵۶ view و ۷۳۹ ستون فیزیکی** داشت. تعریف کامل
engine، partition/sort/primary key، view، ستون/default و skipping index
پس از یکسان‌کردن فقط qualifier نام database برابر بود. نام/هویت database
مجزاست؛ checksum قرارداد یکسان است. شیء native یا compatibility اضافه
ایجاد نشد و check/re-init، شکل schema و ledger را تغییر ندادند.

SQL شش فاز فقط در [بستهٔ مشترک](../shared/warehouse/sql/README.md) نگه‌داری
می‌شود؛ پنج کپی بیت‌کوین و SQL بومی ترون حذف شدند. exporter و تست
`sql_bundle` تطابق کامل فایل‌ها با منبع را کنترل می‌کنند. تغییر عمدی یک
فایل و افزودن SQL اضافی به کپی موقت هر دو با `--check --migrations` رد شدند؛
`bitcoin_init_schema --print-sql` دقیقاً همان شش فاز را برگرداند. snapshot
قرارداد و checksumهای v1–v6 تغییر نکردند.

هفت تست کتابخانهٔ مشترک و یک تست بستهٔ SQL، ۴۰ تست کتابخانهٔ بیت‌کوین،
هر ۴۲ تست Python و تمام گروه‌های قرارداد CLI روی Linux موفق شدند. چهار
Compose با env نمونه parse شدند، image مشترک و نبود mount SQL قدیمی کنترل
شد و وابستگی تمام runtimeهای ترون به موفقیت `tron-schema` تأیید شد.
Docker/HTTP/Cargo در suite CLI، mock هستند؛ آزمون چهار initializer بالا
با binary و ClickHouse واقعی انجام شده است.

برای تکرار، ابتدا binaryهای schema را بسازید و چهار ClickHouse آزمایشی
تازه با حساب `warehouse_test` / `warehouse-test-only` و endpointهای متفاوت
آماده کنید. تست دیتابیس دارای جدول را نمی‌پذیرد و هیچ دیتابیسی را drop نمی‌کند:

```bash
python3 scripts/tests/schema_installation.py \
  --binary tron="$PWD/dockerizd_tron/app/target/debug/tron_schema" \
  --binary ethereum="$PWD/dockerizd_ethereum/target/debug/ethereum_init_schema" \
  --binary bsc="$PWD/dockerizd_bsc/target/debug/bsc_schema" \
  --binary bitcoin="$PWD/dockerizd_bitcoin/target/debug/bitcoin_init_schema" \
  --url tron=http://127.0.0.1:18141 \
  --url ethereum=http://127.0.0.1:18142 \
  --url bsc=http://127.0.0.1:18143 \
  --url bitcoin=http://127.0.0.1:18144
```

این مرحله پذیرش schema/بسته‌بندی است؛ ماتریس یکپارچهٔ دریافت و بازیابی
چهار adapter، ساخت release image و پذیرش VM/RPC واقعی هنوز جدا هستند.

## ماتریس پذیرش مسیر دادهٔ چهار شبکه — قرارداد v6

این تست‌ها از دیتابیس خالی، RPC نمونه و مسیر واقعی ingestor، writer و
reader/API استفاده می‌کنند. هر شبکه سرور ClickHouse مستقل با همان image
`23.8.16.16` دارد؛ source، مبلغ، موقعیت و finality متناسب با شبکه‌اند.
پذیرش ساختار ۴۲ جدول/۵۶ view/۷۳۹ ستون در DB-5، مبنای همین اجراهاست.

| شبکه | سناریوهای زنده | پوشش |
|---|---|---|
| Bitcoin | ۱ در [clickhouse_pipeline](../dockerizd_bitcoin/tests/clickhouse_pipeline.rs) | UTXO/prevout، مبلغ دقیق، انتشار ناقص، restart/retry، موجودی/مسیر، حذف شاخهٔ orphan و دریافت شاخهٔ جدید |
| Ethereum | ۲ در [shared_pipeline](../dockerizd_ethereum/tests/shared_pipeline.rs) | دریافت، metadata/intelligence/cluster/exposure/risk و API؛ قطع واقعی نوشتن قبل از complete marker، restart، بستن خطا پس از موفقیت، trace refresh، تعارض هش tip/بلاک قدیمی و عقب‌رفتن finalized head |
| BSC | ۵ در [service](../dockerizd_bsc/src/ingestion/service.rs) و [API](../dockerizd_bsc/tests/support/shared_pipeline.rs) | شش مرز خرابی، retry/replay، dead-letter/requeue، reorg با ancestor و تعویض بازه، NFT/metadata، revoke و query/API دارای auth |
| TRON | ۵: سه [دریافت](../dockerizd_tron/app/tests/committed_visibility.rs)، یک [مرزهای خرابی](../dockerizd_tron/app/src/services/tron/publication_store.rs)، یک [عملیاتی/API](../dockerizd_tron/app/tests/analytics_pipeline.rs) | شاهد اصلی و موقعیت/مبلغ، شش مرز خرابی، restart/replay، توقف در تعارض solid و poll خالی، retry واقعی receipt، metadata، review/revoke، cluster/exposure و API |

اتریوم و ترون checkpoint را در poll بدون بلاک جدید نیز بررسی می‌کنند؛
بلاک کامل/کش‌شده از تطبیق هش معاف نیست. تعارض دادهٔ finalized/solid توقف
می‌دهد و state منتشرشده ثابت می‌ماند؛ جایگزینی ضمنی شاخه انجام نمی‌شود.
بازیابی شاخه در Bitcoin/BSC با مسیر rollback/ancestor آزموده می‌شود.

در ترون، بررسی header یا skip فقط خطاهای همان مراحل را resolved می‌کند؛
خطای receipt/transaction تا replay موفق باز می‌ماند. تست عمداً receipt را
خراب می‌کند و ثابت می‌کند poll خالی/skip، خطا را پنهان یا receipt را دوباره
پردازش نمی‌کند؛ replay با شاهد اصلاح‌شده آن را می‌بندد.

دو اجرای اولیهٔ ترون با قطع اتصال HTTP شکست خوردند؛ backtrace اجرای دوم،
خواندن ledger در `ReadFence::capture` را نشان داد. کانتینر/سرور OOM یا exit
نداشت. خواندن‌های قرارداد/فنس ترون اکنون با `readonly=2` فقط برای
`Error::Network` حداکثر سه تلاش با فاصلهٔ ۵۰/۱۰۰ میلی‌ثانیه دارند. نوشتن‌ها
و خطای SQL خودکار تکرار نمی‌شوند و کنترل ثبات snapshot باقی است.
سه تست TCP قطعی در [client](../dockerizd_tron/app/src/db/client.rs)، قطع
اتصال پس از دریافت درخواست، توقف پس از سه تلاش و یک‌بار اجراشدن خطای SQL
را کنترل می‌کنند؛ این تست‌ها در اجرای عادی کتابخانه نیز هستند.

فرمان‌های Bitcoin/Ethereum/BSC در بخش‌های بالاتر هستند. برای TRON، روی
سرور موقت تازه و با `WAREHOUSE_TEST_URL` همان fixture:

```bash
cargo test --locked --manifest-path dockerizd_tron/app/Cargo.toml --lib \
  -- --include-ignored --test-threads=1
cargo test --locked --manifest-path dockerizd_tron/app/Cargo.toml --test tron_source_evidence
cargo test --locked --manifest-path dockerizd_tron/app/Cargo.toml --test committed_visibility \
  -- --ignored --nocapture --test-threads=1
cargo test --locked --manifest-path dockerizd_tron/app/Cargo.toml --test analytics_pipeline \
  -- --ignored --nocapture --test-threads=1
```

نتیجهٔ نهایی ۲۰۲۶-۱۱-۰۷: هر **۱۳ سناریوی زنده** ماتریس بالا موفق شدند؛
پس از اصلاح قطع خواندن، هر سه تست دریافت ترون، تست عملیاتی/API و کتابخانهٔ
ترون از build نهایی دوباره اجرا و موفق شدند. ترون ۸۱ تست عادی و یک سناریوی
شش مرز خرابی داشت (۸۲ موفق با `--include-ignored`)؛ هشت تست شاهد اصلی
هم موفق‌اند. ۳۱ تست کتابخانهٔ Ethereum و ۴۳ تست عادی BSC نیز موفق شدند.
قالب Rust و لینک‌های راهنماها بررسی شدند؛ schema/checksumهای v6 تغییر نکردند.

ممیزی مسیرهای فعال و fixtureهای فاقد شیء native، استفاده از قرارداد مشترک
را تأیید کردند. این نتیجه معیار بسته‌شدن یکسان‌سازی **کد** است. RPC mainnet،
VMهای کاربر، release image، مسیر کامل سرویس مرکزی/Neo4j و ظرفیت زیر بار
واقعی همچنان در بخش‌های ۲ و ۶ TODO باقی‌اند.

## پذیرش محدود RPC عمومی واقعی — ۲۰۲۶-۱۱-۰۷

این اجرا جدا از fixtureهای بالاست: binary اصلی هر شبکه از RPC عمومی mainnet
داده گرفت و روی ClickHouse مستقل و خالی با image `23.8.16.16` ذخیره کرد.
Ethereum/BSC از PublicNode و TRON از TronGrid فایل‌های نمونه استفاده کردند.
برای Bitcoin، کاربر [PublicNode](https://bitcoin.publicnode.com/) را انتخاب کرد؛
URL فقط در محیط پردازش آزمون override شد. فایل محلی `.env` بیت‌کوین تغییر
نکرد و همچنان باید برای اجرای بعدی با RPC قابل دسترس تنظیم شود.

| شبکه | ارتفاع بلاک | تراکنش ذخیره‌شده | نتیجهٔ تطبیق مستقل با RPC |
|---|---:|---:|---|
| Bitcoin | 974929 | 5838 | شناسه، مبلغ ساتوشی، ورودی/خروجی و کارمزد دقیق |
| Bitcoin | 974930 | 1964 | همان کنترل‌ها؛ مجموع دو بلاک: ۱۵۵۹۴ ورودی و ۱۷۲۵۲ خروجی |
| Ethereum | 26363251 | 423 | تمام تراکنش‌ها و ۲۶۱۲ log؛ ۱۱۱۸ انتقال توکن/NFT و کارمزد دقیق |
| BSC | 132210230 | 97 | تمام تراکنش‌ها و ۵۷۴ log؛ ۲۸۲ انتقال توکن/NFT و کارمزد دقیق |
| TRON | 87794400 | 469 | هش protobuf و index/type تمام ۴۶۹ contract؛ مبلغ/آدرس ۱۷۸ انتقال بومی دقیق |

در مجموع **۵ بلاک و ۸۷۹۱ تراکنش** پذیرفته شدند. هش‌های ثبت‌شده:

```text
Bitcoin 974929: 000000000000000000005440bda20f7c1146a37b706061866349fdcba3a1c2a1
Bitcoin 974930: 00000000000000000000642ee55c6672864ce72cb64e30c7e4361845c7d453e4
Ethereum 26363251: 0xea8d699f7610cd0a9657c158a5d60b72a6b87042738089dfba75651601796abd
BSC 132210230: 0x8c40e77f8ff698f9b9af91115aee7cb59ec08edada4f9c9dd2161278b93d309f
TRON 87794400: 00000000053ba2e0dd3f88a06d9fc7fbfed22723bf6924bb1cee6a3ee7dce6fc
```

برای BTC تمام prevoutها، outpointها، scriptها، مبلغ و کارمزد با پاسخ مستقل
verbosity=3 مقایسه شدند؛ پوشش prevout هر دو بلاک ۱ بود. probe واقعی نیز
mainnet/genesis، txindex و نمونهٔ lookup تاریخی را تأیید کرد؛ این نمونه
اثبات دسترسی به تمام تاریخچه نیست. برای EVM، وضعیت و مبلغ هر تراکنش،
receipt logها، مقدار/token ID و موقعیت انتقال‌ها تطبیق شدند. از ۱۲ تراکنش
ناموفق Ethereum و ۳ تراکنش ناموفق BSC هیچ یال انتقال وجه ساخته نشد.
۲۶۷ log با مبلغ صفر Ethereum به‌عنوان شاهد ماندند و یال وجه صفر نساختند.
در TRON تعداد ۴۶۹ resource، ۹۳ log و ۵۹ internal ذخیره شد؛ بررسی مستقل
کارمزد/resource/receipt به **۶ نمونه** محدود بود، نه تمام receiptها.

پس از بستن پردازش، اجرای دوبارهٔ همان بازه در هر چهار شبکه موفق شد:
تعداد factهای canonical ثابت، tip/hash صحیح و خطاهای حل‌نشده صفر بودند.
API واقعی هر چهار شبکه نیز `/ready=200`، درخواست بدون auth برابر `401`،
تحقیق دارای auth برابر `200` و گراف غیرخالی داشت. پاسخ BTC شامل ۵۸۳ یال
بود و با سقف موجود ۱۲۰ ثانیه پاسخ داد؛ latency/throughput پایدار اندازه‌گیری
نشده است. snapshotها بازهٔ محدود و `history_complete=false` را گزارش کردند؛
مشاهدهٔ چند بلاک جانشین به finality اثبات‌شده تبدیل نشد.

### ایرادهای یافته‌شده و اصلاح‌شده در این آزمون

- [RPC بیت‌کوین](../dockerizd_bitcoin/src/bitcoin/rpc.rs): پشتیبانی از endpoint
  بدون کلید؛ هر دو credential خالی یعنی حذف Basic Auth. جفت ناقص رد می‌شود؛
  احراز هویت Core خصوصی حفظ و حضور/عدم حضور header در تست HTTP کنترل شد.
- کارمزد کل EVM اکنون هزینهٔ execution به‌علاوهٔ blob است؛ این اشکال در هر
  دو بلاک واقعی Ethereum/BSC دیده شد. محاسبه با overflow check و رد شاهد
  ناقص اصلاح و همان بلاک‌ها از صفر دوباره دریافت و تطبیق شدند.
  هزینهٔ blob مستقل از موفقیت execution است؛ [EIP-4844](https://eips.ethereum.org/EIPS/eip-4844).
  کد: [Ethereum](../dockerizd_ethereum/src/ethereum/extractor.rs)،
  [BSC](../dockerizd_bsc/src/ingestion/model.rs).
- [تطبیق factها](../shared/warehouse/src/core.rs): بلاک بزرگ BTC از سقف
  پاسخ ۳۲ MiB عبور می‌کرد. خواندن با ترتیب کلید و صفحه‌های حداکثر ۲۵۶ ردیفی
  جایگزین پاسخ یک‌تکه شد؛ تمام ستون‌ها و ردیف اضافه همچنان کنترل می‌شوند.
  retry همان دیتابیس نیمه‌نوشته موفق شد؛ سقف ایمنی افزایش پیدا نکرد.
- [تعریف قرارداد](../shared/warehouse/src/contract.rs): ساخت مکرر تعریف ثابت
  در مسیر تحقیق BTC باعث timeout شده بود. فقط ساخت تعریف embedded در هر
  پردازش cache می‌شود؛ کنترل زندهٔ schema/ledger/checkpoint و داده برقرار است.
  پس از اصلاح، API BTC و تست واقعی مشترک هر چهار پروفایل دوباره موفق شدند.

تست‌های کتابخانهٔ نهایی: Bitcoin **۴۲**، Ethereum **۳۲**، BSC **۴۴** و TRON
**۸۱** تست عادی موفق؛ shared warehouse **۲۶** تست عادی موفق. ignoredها به
معنای اجرای آن‌ها در این فرمان عادی نیستند؛ سناریوهای قبلی بالاتر جدا ثبت‌اند.
تست واقعی `identical_core_queries_on_four_databases_preserve_evidence_and_fence_partial_facts`
پس از آخرین اصلاح مشترک موفق شد. Rustfmt و بررسی whitespace موفق شدند؛
`export_sql --check --migrations` و `--check --contract-json` ثابت کردند
قرارداد v6، SQL و checksumها تغییر نکرده‌اند.

### تکرار بازه‌ها و حدود پذیرش

ابتدا env همان نقش، endpoint و credentialهای ClickHouse آزمایشی، کلید سرویس
و writer lock مشترک آن نقش را تنظیم کنید. هر شبکه باید سرور/دیتابیس مستقل
داشته باشد؛ برای آزمون BSC نام `bsc_aml` استفاده شد. schema CLI همان نقش
را پیش از ingestor اجرا کنید. فرمان‌های زیر از ریشهٔ مخزن با env صادرشده
به پردازش اجرا می‌شوند؛ `.env` ریشه جای env هر شبکه نیست.

```bash
# Bitcoin: BITCOIN_RPC_URL=https://bitcoin-rpc.publicnode.com
# BITCOIN_RPC_USER and BITCOIN_RPC_PASSWORD must BOTH be empty for this provider.
cargo run --locked --manifest-path dockerizd_bitcoin/Cargo.toml --bin bitcoin_init_schema
BITCOIN_START_BLOCK=974929 BITCOIN_END_BLOCK=974930 \
  cargo run --locked --manifest-path dockerizd_bitcoin/Cargo.toml --bin bitcoin_aml -- once

cargo run --locked --manifest-path dockerizd_ethereum/Cargo.toml --bin ethereum_init_schema
cargo run --locked --manifest-path dockerizd_ethereum/Cargo.toml --bin ethereum_ingestor -- \
  range --from-block 26363251 --to-block 26363251

cargo run --locked --manifest-path dockerizd_bsc/Cargo.toml --bin bsc_schema
cargo run --locked --manifest-path dockerizd_bsc/Cargo.toml --bin bsc_ingest -- \
  --mode range --start 132210230 --end 132210230

cargo run --locked --manifest-path dockerizd_tron/app/Cargo.toml --bin tron_schema
cargo run --locked --manifest-path dockerizd_tron/app/Cargo.toml --bin tron_benchmark_ingestion -- \
  87794400 87794400
```

اجرای TRON بالا از ingestor اصلی برای یک بازهٔ محدود استفاده می‌کند؛ نام
benchmark به معنای پذیرش ظرفیت تولید نیست. تکرار این فرمان‌ها پس از گذشت
زمان به دسترسی تاریخی provider وابسته است و خود فرمان ingest جای oracle
مستقل مقایسهٔ مقادیر را نمی‌گیرد.

محدودیت‌های مشاهده‌شده و کار باقی‌مانده:

- trace عمومی Ethereum/BSC در دسترس نبود؛ پوشش receipt در بلاک‌های آزمون ۱
  و trace صفر ثبت شد. از حالت auto/development نمونه استفاده شد؛ نیازهای
  سخت‌گیرانهٔ production با حذف کنترل قابلیت دور زده نشدند.
- بعضی درخواست‌های state تاریخی EVM با `403` و الزام token شخصی رد شدند.
  holdings ابتدا پاسخ داشت اما در آخرین آزمون هر دو شبکه `unavailable` بود؛
  **موجودی لحظه‌ای معتبر به‌صورت مستقل تأیید نشد** و unavailable صفر نیست.
- TronGrid خطای `429` و محدودیت ۳ درخواست در ثانیه داد؛ retry داخلی بازیابی
  کرد. دریافت یک بلاک با یک worker حدود ۱۲٫۸ دقیقه طول کشید؛ این RPC عمومی
  برای ظرفیت عملیاتی هدف هنوز پذیرفته نشده است.
- follow/catch-up طولانی، تمام قابلیت‌های archive/trace/prevout، reorg واقعی،
  VMهای کاربر و مسیر Main/Neo4j در این اجرا آزموده نشدند؛ صحت آن‌ها با چند
  بلاک نتیجه‌گیری نمی‌شود. parity کامل API نیز تسک بعدی است.

پردازش‌ها، چهار ClickHouse موقت و شبکهٔ اختصاصی این آزمون پس از پایان پاک
شدند؛ Docker و cacheهای build باقی ماندند. دادهٔ آزمون برای استقرار نگه‌داری
نمی‌شود؛ پنج VM با دیتابیس‌های شبکهٔ خالی آماده خواهند شد.

## API-1: مرز ورودی و خطای چهار شبکه — ۲۰۲۶-۱۱-۰۷

سه مسیر اصلی `investigation`، `paths` و `holdings` از مرز HTTP مشترک
[chain_api](../shared/chain_api/src/lib.rs) استفاده می‌کنند. query ناشناخته،
تکراری یا غیرقابل تبدیل به نوع مورد انتظار، `400/invalid_query` می‌دهد؛
آدرس یا حد نامعتبر `400/invalid_request` و path غیر UTF-8، `400/invalid_path`.
حد درخواست در این مسیرها پیش از دسترسی به شواهد کنترل می‌شود و بی‌صدا
clamp نمی‌شود. احراز هویت فعال قبل از parse درخواست اجرا می‌شود؛ پاسخ‌های
محافظت‌شده، اعم از موفق یا خطا، `Cache-Control: no-store` دارند.

خطای API فیلد متنی `error` قبلی را نگه می‌دارد و شکل زیر دارد:

```json
{
  "api_version": "1.0",
  "error": "invalid, duplicate or unsupported query parameter",
  "code": "invalid_query",
  "status": 400
}
```

کلید نامعتبر `401/unauthorized` است. نبود شواهد/دیتابیس در خطای تحقیق،
`503/evidence_unavailable` می‌دهد و جزئیات داخلی در log سرویس می‌ماند.
payloadهای health/readiness و وضعیت typed موجودی، شکل معنایی خود را دارند؛
مثلاً holdings ناموجود EVM همچنان `unavailable` است و صفر نمی‌شود.
در Compose، احراز هویت سرویس لازم است؛ امکان توسعهٔ محلی بدون auth که
قبلاً در Ethereum/TRON وجود داشت در این گام تغییر نکرد.

نام مشترک عمق مسیر `max_hops` است. TRON نام قدیمی `max_depth` را هم
می‌پذیرد؛ ارسال هر دو رد می‌شود. BSC، `limit` را برای `max_paths` می‌پذیرد
و ارسال هم‌زمان آن دو رد می‌شود. جهت اشتباه TRON دیگر به outgoing تبدیل
نمی‌شود؛ incoming/inbound صریح به جست‌وجوی ورودی نگاشت می‌شود. Ethereum
هم outgoing را برای outbound می‌پذیرد. قابلیت معکوس برای BTC/Ethereum
صرفاً برای برابری ظاهری اضافه نشده است.

| مسیر/حد | Bitcoin | Ethereum | BSC | TRON |
|---|---|---|---|---|
| investigation `limit` | ۱ تا ۱۰۰ | ۱ تا ۵۰۰۰ | ۱ تا ۵۰۰۰ | ۱ تا ۲۰۰۰ |
| paths `max_hops` | ۱ تا ۱۰ | ۱ تا ۱۰ | ۱ تا ۱۰ | ۱ تا ۱۰؛ alias قدیمی نیز معتبر |
| paths تعداد مسیر | یک مسیر محدود؛ `max_nodes` بین ۲ تا ۲۰۰ | `limit` بین ۱ تا ۲۵ | `limit`/`max_paths` بین ۱ تا ۱۰۰ | `limit` بین ۱ تا ۵۰ |
| holdings query | `limit` بین ۱ تا ۱۰۰ | گزینهٔ query ندارد | گزینهٔ query ندارد | `limit` بین ۱ تا ۲۵۰ |

گزینه‌های فعلی دیگر هر شبکه، از جمله محدودهٔ BTC/BSC و گزینه‌های TRON،
حفظ و اعتبارسنجی می‌شوند. یکسان‌کردن اجرای فیلترها و صفحه‌بندی با snapshot
ثابت، معیار API-2 است؛ شکل کامل response/capability در API-3 پذیرفته می‌شود.

نتیجهٔ آزمون‌ها:

- چهار ماتریس HTTP با **۸۶ حالت** روی routerهای واقعی موفق شدند: query
  خراب/ناشناخته/تکراری، حد صفر/بزرگ، range معکوس، alias متعارض، آدرس/path
  نامعتبر، کلید مفقود/اشتباه، تقدم auth، JSON و no-store. این تست‌ها به
  RPC عمومی وصل نمی‌شوند و دسترسی نامعتبر به شواهد را به خطای SQL تبدیل نمی‌کنند.
- تست مثبت TRON، مقدار درخواست canonical/legacy و جهت incoming/outgoing/both
  را حفظ‌شده بررسی کرد. **۲۰۴ تست عادی کتابخانه** موفق‌اند: BTC چهل‌وسه،
  Ethereum سی‌وسه، BSC چهل‌وپنج و TRON هشتادوسه. ignoredها در این شمارش نیستند.
- چهار رگرسیون موجود روی **چهار ClickHouse مستقل و تازه** با RPC نمونه
  دوباره موفق شدند؛ داده‌های بلاک، مبالغ، تحقیق، auth و consumerها حفظ شدند:

| شبکه | رگرسیون پذیرفته‌شده |
|---|---|
| Bitcoin | `clickhouse_pipeline::pipeline_visibility_restart_utxos_paths_and_reorg` |
| Ethereum | `shared_pipeline::rpc_to_common_storage_workers_and_investigation` |
| BSC | `ingestion::service::tests::shared_live_api_metadata_nft_and_revocation` |
| TRON | `analytics_pipeline::live_governance_metadata_analytics_and_authenticated_api_use_only_shared_tables` |

فرمان چهار تست HTTP، همراه با تست‌های عادی دیگر:

```bash
cargo test --locked --manifest-path dockerizd_bitcoin/Cargo.toml --lib
cargo test --locked --manifest-path dockerizd_ethereum/Cargo.toml --lib
cargo test --locked --manifest-path dockerizd_bsc/Cargo.toml --lib
cargo test --locked --manifest-path dockerizd_tron/app/Cargo.toml --lib
# To run only the HTTP matrices, append: api_boundary_contract
```

فرمان و credentialهای fixture زنده در بخش‌های همان شبکه بالاتر ثبت شده‌اند؛
این چهار رگرسیون از همان تست‌ها استفاده کردند. اجرای این مرحله، تکرار
mainnet یا پذیرش مسیر Main/Neo4j نبود؛ آزمون محدود mainnet بخش قبلی جداست.

[OpenAPI](../contracts/chain-api-v1.openapi.yaml) با validator استاندارد 3.1
و کنترل کلید تکراری YAML پذیرفته شد. هشت توضیح flow-map قدیمی که comma
بدون quote داشتند نیز اصلاح شدند. Compose چهار شبکه و مسیر source کتابخانهٔ
جدید در buildها کنترل شدند؛ Dockerfileهای عادی و offline context مشترک را
کپی می‌کنند. lockfileهای چهار adapter فقط همین package محلی را اضافه کردند.
ساخت release image و پذیرش VM همچنان تسک عملیاتی جداست.

قرارداد SQL v6 و جداسازی دیتابیس‌ها تغییر نکردند. فقط کانتینرها و شبکهٔ
موقت همین آزمون پاک شدند؛ Docker و cacheهای build باقی ماندند.

## API-2: فیلتر و صفحه‌بندی شواهد انتقال چهار شبکه

مسیر زندهٔ `GET /api/{network}/wallet/{address}/transfers` در هر چهار adapter
از [خواندن صفحهٔ مشترک](../shared/warehouse/src/relationships/page.rs) استفاده
می‌کند. Gateway مسیر را طبق prefix فعلی به همان VM می‌فرستد. قرارداد SQL
v6 ثابت مانده و این قابلیت در همان سرویس API هر VM اجرا می‌شود؛ کتابخانهٔ HTTP
فقط خطای typed این reader را به همان قرارداد API تبدیل می‌کند.

فیلترهای مشترک `from_block`/`to_block`، `from_unix_ms`/`to_unix_ms`،
`asset_id` و `direction=incoming|outgoing|both` هستند. حد صفحه ۱ تا ۱۰۰ است.
مرزها inclusive و زمان UTC میلی‌ثانیه است. دارایی شبکهٔ دیگر و نوع نامعتبر
پیش از IO رد می‌شود؛ collection مربوط به NFT همهٔ token IDهای آن را انتخاب
می‌کند. مبلغ، مقدار ساتوشی و token ID بدون تبدیل به float حفظ می‌شوند.

ترتیب نزولی `(block_number, transaction_index, event_index, event_sub_index,
relationship_id)` و شرط کلید آخرین ردیف جای offset را گرفته‌اند. یک ردیف
بیشتر برای `has_more` خوانده می‌شود؛ ادامهٔ صفحه به آدرس/شبکه/database،
فیلترها، limit و snapshot مقید است. کلیدهای محدود ابتدا انتخاب و سپس همان
ردیف‌ها با witness دقیق UTXO تطبیق می‌شوند. cursor خراب یا ناسازگار خطای
۴۰۰ می‌دهد؛ تغییر منبع قبل/حین خواندن یا بین صفحات، **۴۰۹ و
`snapshot_changed`** می‌دهد و هیچ صفحهٔ ترکیبی برنمی‌گردد.

این سیاست عمداً با ورود بلاک جدید، replay/reorg، refresh و تغییر source
frame نیز شروع مجدد می‌خواهد. این نسخه snapshot تاریخی برای ادامهٔ cursor
پس از تغییر منبع نگه نمی‌دارد. `has_more=false` فقط انتهای فهرست فیلترشدهٔ
ذخیره‌شده است؛ نبود تاریخچهٔ خارج از snapshot را اثبات نمی‌کند.
scope این endpoint به holdings/risk/تحقیق کامل و جست‌وجوی چند hop منتقل
نمی‌شود. [نمونهٔ درخواست و ادامهٔ صفحه](USAGE_FA.md).

آزمون [transfer_pages](../shared/warehouse/tests/transfer_pages.rs) روی
ClickHouse واقعی `23.8.16.16` برای چهار پروفایل و databaseهای تازه موفق شد:

- برابر بودن اتصال همهٔ صفحات با نتیجهٔ کامل، بدون تکرار/جاافتادگی و با
  تکرارپذیری درخواست همان cursor؛ tie در event/sub-index و هویت UTXO.
- فیلتر تک‌بلاک، زمان دقیق در مرز، محدودهٔ خالی، دارایی بومی، جهت و self-transfer.
- collection و token مشخص NFT، شناسه و مبلغ برابر بیشینهٔ UInt256؛
  بیت‌کوین فقط native و همان ورودی/خروجی، با شاهد outpoint و مبلغ دقیق.
- تغییر واقعی publication بین انتخاب کلید و دریافت شاهد، رد cursor قبلی،
  و شروع مجدد موفق با همان ردیف‌ها در source frame تازه.
- تست‌های عادی مشترک، cursor خراب/بیش‌ازحد، query معکوس، دارایی ناسازگار
  و cursor متعلق به آدرس/فیلتر/database دیگر را پیش از هر IO رد کردند.

```bash
cargo test --locked --manifest-path shared/warehouse/Cargo.toml
WAREHOUSE_TEST_URL=http://127.0.0.1:18140 \
  cargo test --locked --manifest-path shared/warehouse/Cargo.toml \
  --test transfer_pages -- --ignored --nocapture --test-threads=1
```

fixture دوم فقط روی ClickHouse موقتِ تازه با حساب `warehouse_test` و رمز
`warehouse-test-only` اجرا می‌شود و database دارای جدول را نمی‌پذیرد.
این داده‌ها synthetic هستند و پذیرش دوبارهٔ RPC عمومی محسوب نمی‌شوند.

پذیرش HTTP چهار adapter با همان تست‌های زندهٔ مرحلهٔ قبل دوباره انجام شد:
دو صفحه با شناسه‌های متفاوت و snapshot برابر، فیلتر بومی و حفظ مبلغ رشته‌ای
در هر چهار API موفق شدند. BSC پس از reorg و TRON پس از replay به cursor
قبلی پاسخ `409/snapshot_changed` دادند. سناریوی BTC همچنان UTXO/reorg و
سناریوی Ethereum همچنان metadata، withdrawal، trace refresh و تحقیق را
کنترل می‌کنند؛ داده‌ها از ingestor اصلی و RPC نمونه وارد شدند.

نتیجهٔ نهایی: **۲۰۴ تست عادی کتابخانهٔ چهار adapter**، شامل چهار ماتریس
با مجموع **۱۲۲ حالت HTTP**؛ **۲۸ تست عادی shared warehouse**؛ یک تست
صفحه‌بندی زنده روی چهار پروفایل و چهار تست دریافت/مصرف‌کنندهٔ زنده موفق‌اند.
ignoredهای دیگر در این شمارش اجراشده فرض نمی‌شوند. OpenAPI 3.1، نبود کلید
تکراری YAML، Rustfmt و ارجاع‌های محلی راهنماها نیز کنترل شدند.

این مرحله پذیرش reader و API دادهٔ محدود است. پاسخ/مصرف مرکزی در بخش API-3
زیر پذیرفته شد؛ ظرفیت پیوسته/release image/VM در بخش عملیاتی TODO باقی‌اند.
کانتینرهای موقت آزمون پس
از بررسی نهایی پاک شدند؛ Docker و cacheهای build باقی ماندند.

## API-3: پاسخ مشترک و مصرف مرکزی — ۲۰۲۶-۱۱-۰۷

[قرارداد پاسخ](../shared/chain_api/src/response.rs) به investigation و paths
چهار adapter متصل شد. `chain_response_v1` افزایشی است: گراف typed، مبلغ و
NFT ID صحیح، موقعیت tx/log/sub-index/trace، بخش‌های activity/holdings/paths/
attribution و source/capability. receiptهای ریسک/موجودی بیت‌کوین دست‌نخورده‌اند؛
UTXO به انتقال فرضی آدرس‌به‌آدرس تبدیل نمی‌شود. موجودی TRON صریحاً indexed
netflow است؛ موجودی EVM finalized RPC و بیت‌کوین indexed UTXO است. endpoint
مستقل holdings پاسخ بومی را حفظ می‌کند؛ نمایش مشترک در evidence تحقیق است.

چهار تست زندهٔ دریافت/مصرف‌کننده روی **چهار ClickHouse مستقل 23.8.16.16**
موفق شدند: Bitcoin UTXO/restart/reorg، Ethereum metadata/NFT/withdrawal/
trace/intelligence، BSC NFT/review/revoke/reorg و TRON metadata/review/
exposure/replay. تست paths ترون نبود network_id را آشکار کرد؛ اصلاح و
سناریوی کامل دوباره با موفقیت اجرا شد. برچسب‌های تأییدشدهٔ سه شبکهٔ account
نیز در پاسخ مشترک با شناسه، review و evidence_refs حفظ شدند.

برای بازتولید، همان تست‌های زندهٔ معرفی‌شده در این سند را روی fixture تازه
اجرا کنید و پیش از هر دستور `CHAIN_RESPONSE_PROOF` را به مسیر مطلق JSON
همان شبکه بدهید؛ چهار خروجی در یک پوشه باشند:

| شبکه | دستور تست؛ متغیرهای اتصال/کلید fixture مطابق بخش شبکه تنظیم شوند |
|---|---|
| Bitcoin | `cargo test --locked --manifest-path dockerizd_bitcoin/Cargo.toml --test clickhouse_pipeline -- --ignored --nocapture --test-threads=1` |
| Ethereum | `cargo test --locked --manifest-path dockerizd_ethereum/Cargo.toml --test shared_pipeline rpc_to_common_storage_workers_and_investigation -- --ignored --nocapture --test-threads=1` |
| BSC | `cargo test --locked --manifest-path dockerizd_bsc/Cargo.toml --lib shared_live_api_metadata_nft_and_revocation -- --ignored --nocapture --test-threads=1` |
| TRON | `cargo test --locked --manifest-path dockerizd_tron/app/Cargo.toml --test analytics_pipeline -- --ignored --nocapture --test-threads=1` |

نام فایل‌ها `bitcoin.json`، `ethereum.json`، `bsc.json` و `tron.json` است.
این‌ها خروجی موقت تست‌اند و commit نمی‌شوند. سپس فقط با **Neo4j موقت و خالی**:

```bash
export API3_PROOF_DIR=/absolute/path/to/temporary/proofs
export API3_NEO4J_URL=http://127.0.0.1:7474
export API3_NEO4J_PASSWORD=your-disposable-test-password
cargo test --locked --manifest-path analytical_node/Cargo.toml \
  four_chain_responses_survive_real_neo4j_create_reopen_export -- \
  --ignored --nocapture --test-threads=1
```

[آزمون مرکزی](../analytical_node/src/response_contract_tests.rs) روی **Neo4j
Community 5.26.0 واقعی** موفق شد: هشت حالت wallet/paths، create/reopen/Export،
حفظ payload و projection دقیق مبلغ/دارایی/NFT/index، NULL آدرس برای گره‌های
غیرآدرسی BTC، جداسازی owner و رد قرارداد دست‌کاری‌شده. تغییر canonical
منبع هر چهار شبکه و operational frame سه شبکهٔ account، snapshot را stale
کرد؛ ریسک جاری UNKNOWN و شواهد تاریخی بدون بازنویسی payload حفظ شدند.
تست مرکزی یک upstream HTTP موقت از پاسخ‌های ثبت‌شدهٔ APIهای واقعی می‌سازد؛
RPC و ClickHouse سمت تولیدکننده نمونه/موقت‌اند. این نتیجه پذیرش پیوستهٔ
RPC عمومی/عملیاتی → چهار API هم‌زمان → Gateway/VM کاربر نیست.

رگرسیون: **۲۰۴ تست عادی adapterها، ۲۸ تست عادی warehouse، چهار تست قرارداد
پاسخ و ۹ تست عادی مرکز**؛ ignoredهای دیگر اجراشده فرض نمی‌شوند. OpenAPI
3.1 و نبود کلید تکراری، انطباق هشت پاسخ واقعی با schema، syntax اسکریپت‌های
پنل، escape ورودی، مبلغ ناموجود و قالب‌بندی دقیق UInt256/عدد منفی کنترل شدند.
schema مشترک **v6، ۴۲ جدول، ۵۶ view، ۷۳۹ ستون** و SQL/checksumها ثابت‌اند.
تست از archive رسمی Neo4j با SHA256 تأییدشده داخل کانتینر موقت استفاده کرد؛
دانلود image در registry ناپایدار بود و نسخهٔ Compose تغییر نکرد. محیط
موقت پس از آزمون پاک شد؛ daemon و cache Docker باقی ماندند.


## ING-1: دریافت پیوسته و بازیابی RPC — ۲۰۲۶-۱۱-۰۸

حلقهٔ اصلی چهار ingestor روی چهار ClickHouse مستقل `23.8.16.16` با RPC
HTTP نمونه آزموده شد: شروع از بازهٔ تنظیم‌شده، catch-up، poll در head،
رسیدن بلاک تازه، اتمام retry کوتاه RPC و ادامهٔ همان پردازش، restart از
checkpoint و حفظ شمارش/مبالغ بدون تکرار. دادهٔ تست از ingestor اصلی وارد
شد؛ fact دستی برای موفق نشان‌دادن follow ساخته نشد. schema و SQL v6 ثابت‌اند.

- Bitcoin: قطعی هنگام بررسی هش checkpoint، حفظ revision قبلی و ادامه پس
  از بازیابی؛ دریافت فقط بلاک تازه پس از restart و رد شبکهٔ غیر mainnet.
  خطای RPC دائمی با قطعی موقت و تغییر شاخهٔ نیازمند reconcile تفکیک می‌شود.
- Ethereum: backoff یک تا ۶۰ ثانیه برای خطای موقت head/receipt، حفظ prefix
  منتشرشده و حد `max-blocks`؛ کنترل دوبارهٔ chain ID و توقف روی شاهد نامعتبر.
- BSC: حفظ start صریح CLI تا ایجاد checkpoint، حساب‌کردن prefix منتشرشدهٔ
  batch شکست‌خورده، کنترل هویت endpoint همان header و failover معتبر.
  تست منفی نشان داد بررسی هویت صرفاً هنگام startup کافی نبود؛ اصلاح شد.
  بودجهٔ failure و dead-letter در follow/auto و restart دور زده نمی‌شوند؛
  repair صریح آزموده شد.
- TRON: خواندن checkpoint پیش از هر pass، بدون برگشتن به ابتدای prefix
  منتشرشده؛ پایان retry receipt و ادامهٔ همان پردازش. HTTP 401 و تعارض solid
  توقف امن دارند؛ 429/5xx و envelope محدودیت نرخ TronGrid از خطای دائمی جدا
  می‌شوند و hint تأخیر سقف دارد. body خطای احراز هویت در پیام چاپ نمی‌شود.

فرمان‌های چهار آزمون اصلی؛ URL/حساب fixture مانند بخش‌های قبل، فقط روی
ClickHouse موقت خالی. برای Bitcoin و TRON، مسیر lock موقت مستقل تنظیم کنید:

```bash
cargo test --locked --manifest-path dockerizd_bitcoin/Cargo.toml --test ingestion_resilience \
  follow_recovers_checkpoint_rpc_outage_and_reads_new_blocks_once -- --ignored --nocapture --test-threads=1
cargo test --locked --manifest-path dockerizd_ethereum/Cargo.toml --test shared_pipeline \
  follow_recovers_rpc_failures_catches_new_head_and_resumes_checkpoint -- --ignored --nocapture --test-threads=1
cargo test --locked --manifest-path dockerizd_bsc/Cargo.toml --lib \
  follow_recovers_preserves_cli_start_counts_prefix_and_respects_dead_letters -- --ignored --nocapture --test-threads=1
cargo test --locked --manifest-path dockerizd_tron/app/Cargo.toml --test committed_visibility \
  follow_resumes_published_prefix_after_rpc_outage_and_restart -- --ignored --nocapture --test-threads=1
```

نتیجهٔ نهایی: **۲۰۸ تست عادی کتابخانهٔ adapterها** (BTC: ۴۳، Ethereum: ۳۴،
BSC: ۴۶، TRON: ۸۵) و **۱۱ تست عادی RPC/CLI بیت‌کوین** موفق شدند.
**۱۷ سناریوی یکپارچهٔ یکتا** با ClickHouse واقعی پذیرفته شدند؛ اجرای
دوبارهٔ همان تست و subprocess تست lock در تعداد دوباره شمرده نشده است:

| شبکه | تعداد | مجموعهٔ اجراشده |
|---|---|---|
| Bitcoin | ۴ | follow جدید، قطعی RPC قبلی، ادامهٔ fallback پس از prefix و dead-letter/repair انتخابی در `ingestion_resilience` |
| Ethereum | ۳ | کل `shared_pipeline`: follow جدید، ingest/API/worker و انتشار ناقص/تعارض finalized |
| BSC | ۶ | تمام ignoredهای کتابخانه: شش مرز crash، dead-letter/repair، range/replay، follow جدید، API/metadata/NFT/revoke و reorg |
| TRON | ۴ | کل `committed_visibility`: follow جدید، genesis/TRC10، شاهد نامعتبر/retry و مبلغ/index/NFT/resource/replay |

Rustfmt، نبود خطای whitespace و لینک‌های محلی راهنماها نیز کنترل شدند.
کانتینرهای ClickHouse و builder و network همین آزمون پاک شدند؛ Docker daemon
و cacheها باقی ماندند. هیچ schema، جدول، سرویس یا VM تازه‌ای اضافه نشد.

این پذیرش، داده و اختلال کنترل‌شدهٔ RPC نمونه را پوشش می‌دهد. با تست طولانی
RPC عمومی/عملیاتی، ظرفیت VM یا اتصال پیوسته به Gateway/Neo4j یکی نیست.
بودجهٔ failure بیت‌کوین/BSC برقرار است و قطعی طولانی می‌تواند نیازمند repair
شود. parity کامل capability/history/gap/repair هر چهار شبکه در ING-2 باز است؛
پذیرش قبلی RPC عمومی و API-3 به همین دلیل جدا نگه داشته شده‌اند.

## ING-2: عملیات محدود تاریخچه و repair چهار شبکه — ۲۰۲۶-۱۱-۰۸

فرمان‌های `probe`، `failures`، `scan-gaps`، `replay` و range `repair` از
[launcher موجود](../scripts/vm.sh) به binary همان شبکه متصل‌اند. قرارداد
[مشترک عملیات](../shared/warehouse/src/maintenance.rs) جدول یا سرویس تازه
نمی‌سازد؛ بازپردازش از decoder، resolver و writer اصلی همان adapter استفاده
می‌کند. قفل writer، کنترل source/hash، حفظ tip در refresh تاریخی، بودجهٔ failure
و بازکردن صریح dead-letter برقرارند. ETH/TRON نیز budget پیش‌فرض ۱۰ دارند.

probe به DB وصل نمی‌شود؛ نمونهٔ start تنظیم‌شده و head نهایی را واقعاً از RPC
می‌خواند. EVM receipt/trace را با extractor اصلی می‌سنجد؛ TRON در هر بلاک
حداکثر ۸ شاهد receipt/protobuf و مسیر ویژهٔ genesis را می‌سنجد؛ Bitcoin همان
probe قبلی pruning/txindex/prevout را استفاده می‌کند. حد زمان ۱۲۰ ثانیه است.
نمونهٔ خالی/قابلیت ناقص `ready` ساختگی نمی‌دهد؛ نبود دسترسی یا شبکهٔ اشتباه
ناموفق است. `full_history_verified=false` باقی می‌ماند.

آزمون مشترک روی چهار پروفایل، صف دارای سه failure در یک ارتفاع را صفحه‌بندی
می‌کند؛ رکوردی جا نمی‌افتد. سپس یک fact تراکنش حذف می‌شود و refresh از مسیر
انتشار مشترک، شمارش را با tip/hash قبلی و revision جدید برمی‌گرداند. بازکردن
dead-letter بدون تأیید صریح، پرش از frontier و ترمیم بدون نشانگر بلاک تاریخی
رد می‌شوند. شکست callback یا موفقیت ظاهری آن بدون ترمیم facts، dead-letter را
برای follow باز نمی‌کند. این آزمون مشترک fact مصنوعی مشخص دارد؛ آزمون‌های
adapter زیر، source را از HTTP RPC نمونه و ingestor واقعی وارد می‌کنند.

| مجموعه | پوشش |
|---|---|
| [Bitcoin pipeline](../dockerizd_bitcoin/tests/clickhouse_pipeline.rs) | binary واقعی maintenance، حذف tx، scan/replay، حفظ tip و ادامهٔ رگرسیون restart/UTXO/reorg |
| [Bitcoin resilience](../dockerizd_bitcoin/tests/ingestion_resilience.rs) | رگرسیون repair انتخابی و حفظ attempts پس از واگذاری lock موجود |
| [Ethereum](../dockerizd_ethereum/tests/shared_pipeline.rs) | probe، gap تاریخی و NFT، failure تا dead-letter، repair frontier، رد شبکهٔ اشتباه؛ رگرسیون follow/انتشار ناقص/API/metadata |
| [BSC](../dockerizd_bsc/tests/support/shared_pipeline.rs) | probe، حذف tx و repair تاریخی، هویت اشتباه؛ شش رگرسیون کامل crash/dead-letter/range/follow/API/reorg |
| [TRON](../dockerizd_tron/app/tests/committed_visibility.rs) | probe و شاهد نامعتبر، gate دائمی، repair frontier و تاریخی، رد genesis اشتباه؛ پنج رگرسیون follow/genesis/receipt/NFT/resource |
| [مشترک](../shared/warehouse/tests/transfer_pages.rs) | عملیات محدود، صفحه‌بندی failure، failure ترمیم و missing marker در چهار پروفایل |

فرمان‌های آزمون اصلی؛ فقط روی DB موقت، با URL و حساب fixture همین فایل.
آزمون جدید اتریوم را **جدا** با budget دو اجرا کنید؛ رگرسیون‌های قبلی budget
معمول ۱۰ و `AML_SERVICE_KEY=ethereum-fixture-service-key-0123456789abcdef`
با `AML_SERVICE_AUTH_REQUIRED=true` می‌خواهند. این کلید صرفاً fixture است.

```bash
cargo test --locked --manifest-path shared/warehouse/Cargo.toml --test transfer_pages \
  maintenance_scans_failures_and_verified_refresh_on_four_profiles -- --ignored --nocapture --test-threads=1
cargo test --locked --manifest-path dockerizd_bitcoin/Cargo.toml --test clickhouse_pipeline -- --ignored --nocapture --test-threads=1
ETHEREUM_FAILURE_MAX_ATTEMPTS=10 cargo test --locked --manifest-path dockerizd_ethereum/Cargo.toml \
  --test shared_pipeline -- --ignored --nocapture --test-threads=1 --skip maintenance_probe
ETHEREUM_FAILURE_MAX_ATTEMPTS=2 cargo test --locked --manifest-path dockerizd_ethereum/Cargo.toml \
  --test shared_pipeline maintenance_probe -- --ignored --nocapture --test-threads=1
cargo test --locked --manifest-path dockerizd_bsc/Cargo.toml --lib -- --ignored --nocapture --test-threads=1
cargo test --locked --manifest-path dockerizd_tron/app/Cargo.toml --test committed_visibility -- --ignored --nocapture --test-threads=1
bash scripts/tests/linux-cli.sh
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
```

هویت genesis ترون با [TIP-474 رسمی](https://github.com/tronprotocol/tips/blob/master/tip-474.md)
و یک درخواست خواندنی واقعی TronGrid تطبیق داده شد. پاسخ واقعی، مقدار proto3
`number=0` را حذف می‌کند؛ این پیش‌فرض فقط در genesis با hash درست پذیرفته می‌شود.

این پذیرش، RPC کنترل‌شده و ClickHouse واقعی موقت `23.8.16.16` را پوشش می‌دهد.
scan فقط شمارش/پیوستگی/coverage بازهٔ منتشرشده را بررسی می‌کند، نه تمام مبلغ‌ها؛
replay از RPC با اعتبارسنجی کامل writer انجام می‌شود. گم‌شدن نشانگر تاریخی به
restore یا بازسازی DB خالی نیاز دارد و cursor خودکار عقب نمی‌رود. پذیرش archive
کامل، ظرفیت و پایداری RPC واقعی و مسیر پیوستهٔ چهار API تا Main/Neo4j/VM همچنان
باز است. SQL/checksum قرارداد **v6، ۴۲ جدول، ۵۶ view و ۷۳۹ ستون** تغییر نکرده است.

نتیجهٔ نهایی: **۱۸ آزمون یکپارچهٔ یکتا** موفق شدند: Bitcoin دو، Ethereum چهار،
BSC شش، TRON پنج و یک آزمون مشترک با اجرای چهار پروفایل. تکرار اجرای همان
تست و چهار پروفایل داخل یک تابع، دوباره شمرده نشده‌اند. **۲۰۹ تست عادی
کتابخانهٔ adapterها** (۴۳/۳۴/۴۶/۸۶)، **۲۹ تست عادی warehouse**، **۱۶ تست
RPC/CLI بیت‌کوین** و **۴۲ تست Python ابزارها** موفق‌اند. ساخت چهار binary و
۲۴ کنترل help/ورودی نامعتبر بدون env/DB/RPC، مجموعهٔ shell launcher با Docker
نمونه، Rustfmt، نبود خطای whitespace، UTF-8 و لینک‌های راهنما نیز موفق شدند.
آزمون shell mock است؛ سناریوهای دادهٔ جدول بالا ClickHouse واقعی دارند.
کانتینرها، volumeهای موقت و network همین مرحله پس از آزمون پاک شدند؛ daemon
و cacheهای Docker باقی ماندند. هیچ سرویس، جدول یا VM تازه‌ای اضافه نشده است.

## E2E-1: پذیرش محدود مسیر زنده تا پنل — ۲۰۲۶-۱۱-۰۸

این اجرا از پاسخ ذخیره‌شده، upstream نمونه یا seed fact استفاده نکرد. چهار
دیتابیس مستقل و خالی ClickHouse `23.8.16.16` با binary اصلی ingest شدند؛
چهار API واقعی هم‌زمان در اختیار Analytical Node واقعی قرار گرفتند و Gateway
واقعی به پنل‌های اصلی شبکه‌ها سرویس داد. Neo4j Community `5.26.0` واقعی برای
create، reopen، Export و projection استفاده شد.

| شبکه | بلوک‌های محدود واقعی | تراکنش‌های ذخیره‌شده | مبلغ انتقال تطبیق‌داده‌شده |
|---|---:|---:|---:|
| Bitcoin | ۲ | ۳٬۲۰۹ | ۱٬۳۵۹٬۱۹۰ satoshi |
| Ethereum | ۱ | ۴۲۷ | ۳۱۹٬۹۹۸٬۵۷۵٬۷۲۴٬۲۶۶٬۰۰۰ wei |
| BSC | ۱ | ۱۱۱ | ۱۳۱٬۹۱۴٬۳۲۶٬۲۵۲٬۴۷۲ wei |
| TRON | ۱ | ۲ | ۹٬۷۲۵٬۱۷۴ sun |

برای هر شبکه دو مسیر `wallet` و `paths` با RPC مستقل تطبیق داده شد و API
مستقیمِ فاقد کلید سرویس رد شد. سپس همان graph از Gateway ساخته و در Neo4j
بازگشایی/Export شد؛ مالک دیگر به تحقیق دسترسی نگرفت. تست [live-e2e.py](../gateway/tests/live-e2e.py)
مبلغ رشته‌ای، asset/index، شواهد، projection Neo4j و انزوای نشست را کنترل
می‌کند. در BSC و Ethereum، `holdings` یک مشاهدهٔ مستقل از head جاری RPC است و
ممکن است بین دو درخواست مقدار/بلوک متفاوتی داشته باشد؛ از مقایسهٔ دو درخواست
کنار گذاشته می‌شود، ولی payload کامل ایجادشده پس از reopen دقیقاً یکسان است.
قطع پاسخ حجیم RPC عمومی نیز فقط برای خواندن reference حداکثر دو بار retry
می‌شود.

تست [live-browser.cjs](../gateway/tests/live-browser.cjs) با Microsoft Edge
واقعی و بدون route interception، صفحهٔ هر چهار شبکه را باز کرد، تحقیق را
گرفت، Export را زد و همان snapshot را باز کرد؛ چهار اسکرین‌شات محلی تولید شد.
نتیجهٔ نهایی ۸ مسیر سرویس و ۴ مسیر مرورگر موفق بود.

این پذیرش به‌عمد محدود است: history/archive کامل، trace عمومی EVM، follow
طولانی، ظرفیت/بار، TLS و اجرای واقعی روی پنج VM کاربر را ثابت نمی‌کند. محیط
Docker و داده‌های آن صرفاً fixture پذیرش بودند و باید پس از بررسی پاک شوند؛
این موارد معیار باز E2E-2 هستند.

## INTEL-1 — تبدیل خوراک OFAC، ۲۰۲۶-۱۱-۰۹

فایل `SDN.XML` از سرویس رسمی OFAC دریافت شد. تاریخ انتشار ثبت‌شده در فایل
۲۰۲۶-۱۱-۰۶ بود و ۱۹٬۵۶۳ ورودی با شمارش سربرگ تطبیق داشتند. تبدیل، ۵۳۲ کاندیدای
Bitcoin، ۱۲۴ Ethereum، ۱ BSC و ۲۷۲ TRON ساخت. دو آدرس با قالب نامتناسب در
فایل جدا ثبت شدند. SHA-256 محلی در manifest ثبت شد و هش منتشرشدهٔ همین فایل
در این آزمون مستقلاً تطبیق داده نشد.
[سرویس رسمی OFAC](https://ofac.treasury.gov/sanctions-list-service).

[آزمون تبدیل](../scripts/tests/test_prepare_ofac_sdn.py) خروجی pending، UID،
هش معتبر و نامعتبر، XML ناقص، آدرس خارج از شبکه، چند UID برای یک آدرس و تقسیم
فایل‌های Bitcoin به بسته‌های حداکثر ۱۰۰تایی را کنترل کرد. از ۴۸ تست ابزارهای
Python، ۴۳ تست موفق و ۵ تست طبق تنظیمات skip شدند.

CLIهای BSC و Ethereum با `cargo build --offline --locked` ساخته شدند.
ابزارهای چهار شبکه در چهار database آزمایشی v6 روی ClickHouse موقت
`23.8.16.16` هرکدام یک نمونه از فایل رسمی وارد کردند. خواندن از ClickHouse
برای هر شبکه یک منبع با مجوز ثبت‌شده و یک claim دارای هش در وضعیت pending
برگرداند. شمار claim فعال و review در هر چهار دیتابیس صفر بود.

## INTEL-2 — مقایسه و بازبینی، ۲۰۲۶-۱۱-۰۹

تست تبدیل‌کننده، دو XML ساختگی را با هش و تاریخ انتشار مقایسه کرد. ادعای
جدید به فایل pending رفت. تغییر نام، حذف و آدرس مبهم در صف بازبینی ثبت شدند.
نسخهٔ قدیمی‌تر و مقایسهٔ فاقد هش رد شدند و مقایسهٔ فایل یکسان صف خالی داد.
از ۵۰ تست Python، ۴۵ تست موفق و ۵ تست طبق تنظیمات skip شدند.

در ClickHouse موقت `23.8.16.16` چهار دیتابیس v6 با دادهٔ ساختگی راه افتادند.
برای هر شبکه ابتدا یک ادعا pending ثبت شد. سپس اپراتور آزمایشی آن را با
CLI تأیید کرد و بعد از ثبت حذف در نسخهٔ تازه، جداگانه آن را لغو کرد. در
Ethereum درخواست با revision قدیمی رد شد و درخواست با revision جاری یک
رکورد review افزود و مقدار ادعا را به `revoked` تغییر داد.

هر ۱۲ حالت از viewهای ClickHouse و پاسخ زندهٔ API همان شبکه بررسی شدند.
پاسخ‌های API به سیاست مشترک Rust داده شدند. سیگنال
`APPROVED_RISK_LABEL` در چهار حالت approved حاضر بود و در هشت حالت
pending/revoked دیده نشد.

رگرسیون `rpc_to_common_storage_workers_and_investigation` اتریوم نیز
با RPC نمونه و ClickHouse واقعی موفق شد. پس از لغو، خروجی‌های مشتق‌شدهٔ
clustering، exposure و ریسک از viewهای جاری کنار رفتند.

## AN-1 — شواهد خوشه‌بندی و مواجهه در چهار شبکه

شش تست قرارداد مشترک موفق شدند. چهار پروفایل، تفکیک سرنخ از ارتباط بازبینی‌شده،
جهت و تفکیک مواجههٔ مستقیم/چندمرحله‌ای و حفظ ارجاع بومی را پوشش دادند.
شاهد تراکنش نامعتبر به‌عنوان شواهد معتبر عرضه نشد و دست‌کاری projection
رد شد. هشت پاسخ ثبت‌شدهٔ Wallet و Paths چهار شبکه نیز با قرارداد تازه
پذیرفته شدند و گراف و شواهد بومی آن‌ها حفظ شد. اعتبارسنجی OpenAPI و
نمایش خلاصه در پنل با پاسخ قدیمی و تازه موفق بود.

در آزمون BSC با RPC ساختگی و ClickHouse واقعی، سه فرستنده به یک مقصد
سرنخ pending ساختند. API تحقیق و endpoint اختصاصی سرنخ‌ها همان شاهد را
برگرداندند و فیلتر زمانی آن را از بازه حذف کرد. سرنخ در خروجی انتساب هویت
وارد نشد. این پذیرش، کیفیت تشخیص روی مجموعهٔ دادهٔ برچسب‌دار را جدا می‌گذارد.

تست واحد محافظ CoinJoin و رگرسیون موجود Bitcoin با دادهٔ UTXO ساختگی و
ClickHouse موقت موفق شدند. مورد همکاریِ محتمل سرنخ نداد و تأیید یک سرنخ
مستقل، عضویت را به مالکیت قطعی یا امتیاز ریسک تبدیل نکرد.

## AN-2 — اعتبارسنجی سیاست ریسک مرکزی

```bash
cargo test --locked --manifest-path analytical_node/Cargo.toml
cargo test --locked --manifest-path shared/risk_policy/Cargo.toml
```

آزمون برچسب‌دار ۲۵ نمونهٔ ساختگی را در چهار شبکه با آستانهٔ بررسی ۲۵ اجرا
می‌کند. خروجی مورد انتظار ۵ مثبت درست، ۸ منفی درست، ۱ مثبت کاذب، ۱ منفی
کاذب و ۱۰ `UNKNOWN` است (۸ نمونهٔ مثبت و ۲ نمونهٔ منفی). دو نمونهٔ خطا
عمداً ساخته شده‌اند تا شمارش خطا بررسی شود. هر دلیلِ امتیازدار با شاهد و
سهم عددی آن کنترل می‌شود. این اعداد خروجی fixture هستند و برای پذیرش عملیاتی
باید آزمون روی دادهٔ مستقلِ برچسب‌دار و مجاز تکرار شود.

رگرسیون تازگی، هش غایب یا متفاوت سیاست، نسخهٔ قدیمی، تغییر شواهد یا امتیاز
و قطع دسترسی به منبع را پوشش می‌دهد. تحقیق بدون امتیاز قابل‌استفاده می‌تواند
منبع `current` داشته باشد. تست Neo4j بخش API-3 نیز حفظ هش سیاست و رفتار
ساخت، بازگشایی و Export را روی Neo4j Community `5.26.0` موقت تأیید کرد.
هر هشت حالت Wallet و Paths در چهار شبکه موفق بودند.
fixture ترون با ClickHouse موقت `23.8.16.16` پاسخ‌های API را قبل و بعد از
لغو برچسب و روی snapshot یکسانِ بلاک‌ها تولید کرد. آزمون مرکزی، امتیاز
`UNKNOWN` و حفظ امتیاز تاریخی پس از لغو را در بازگشایی و Export تأیید کرد.

## KYT-1 — ثبت و بازبینی Watchlist

تست نود تحلیلی روی Neo4j Community `5.26.0` موقت، مسیرهای ثبت آدرس و تراکنش، جداسازی
کاربران، تکرار امن درخواست، توقف پایش و بررسی دوباره را با پاسخ‌های
fixture هر چهار شبکه پوشش می‌دهد. تغییر تنظیم هم‌زمان با بررسی قبلی
و تغییر وضعیت برچسب بدون بلاک جدید نیز در همین آزمون بررسی می‌شوند.

```bash
export API3_PROOF_DIR=/absolute/path/to/temporary/proofs
export KYT_NEO4J_URL=http://127.0.0.1:7474
export KYT_NEO4J_PASSWORD=your-disposable-test-password
cargo test --locked --manifest-path analytical_node/Cargo.toml \
  watchlist_rechecks_four_chains_and_fences_inflight_jobs -- --ignored --nocapture
```

آزمون زندهٔ `analytics_pipeline` ترون با ClickHouse `23.8.16.16` موقت، endpoint
داخلی تراکنش را برای هش موجود و غایب و درخواست بدون کلید تأیید کرد.
کامپایل قفل‌شدهٔ هر چهار آداپتور، رگرسیون تحقیق و اعتبارسنجی Nginx و OpenAPI موفق بودند.
روش تولید fixtureها و اجرای آزمون ترون در بخش‌های API-3 و TRON آمده است.

## KYT-2 — هشدار و تحویل پایدار

تست پذیرش با Neo4j Community `5.26.0` موقت و گیرندهٔ webhook محلی، یکتایی هشدار در
بررسی‌های متوالی، حفظ آن هنگام عدم قطعیت، شروع دوره پس از `no_match`،
مجوز واگذاری و انتقال وضعیت رسیدگی را کنترل می‌کند. پاسخ ۴۲۹ همراه
`Retry-After`، تکرار با همان payload و کلید، ثبت تحویل، خطای نهایی،
انقضای صف و بازیابی تلاش رهاشده را نیز تأیید کرد. رگرسیون Watchlist
با پاسخ‌های آزمایشی چهار شبکه و اعتبارسنجی تنظیم Compose موفق بودند.

```bash
export KYT_NEO4J_URL=http://127.0.0.1:7474
export KYT_NEO4J_PASSWORD=your-disposable-test-password
cargo test --locked --manifest-path analytical_node/Cargo.toml \
  alerts_survive_rechecks_and_webhook_retries_on_real_neo4j -- --ignored --nocapture
```

## CASE-1 — پرونده و بازبینی تحقیق

آزمون پذیرش روی Neo4j Community `5.26.0` موقت، ساخت پرونده، تکرار
هم‌زمان درخواست، محدودیت دسترسی، اتصال snapshot و هشدار و رسید آزمایشی
تراکنش چهار شبکه را تأیید کرد. درخواست اصلاح، تأیید نهایی و دانلود گزارش
نیز موفق بودند. snapshot دست‌کاری‌شده به بازبین تحویل داده نشد.
رگرسیون‌های KYT و تحقیق چهار شبکه و اعتبارسنجی تنظیم Nginx موفق بودند.

```bash
export CASE_NEO4J_URL=http://127.0.0.1:7474
export CASE_NEO4J_PASSWORD=your-disposable-test-password
cargo test --locked --manifest-path analytical_node/Cargo.toml \
  case_preserves_evidence_and_review_timeline_on_real_neo4j -- --ignored --nocapture
```

## SEM-1 — شواهد نسخه‌دار پروتکل، NFT و الگوهای UTXO

تست قرارداد مشترک، تفاوت ساختار ABI، هویت registry و مبلغ تأییدشده،
رد شاهد دست‌کاری‌شده، حفظ مقدار و شناسهٔ دقیق NFT و باقی‌ماندن CoinJoin
در دستهٔ سرنخ را پوشش می‌دهد. تست‌های بومی اتریوم و BSC، لاگ ناقص
میکسر و تفاوت گیرنده و relayer را بررسی می‌کنند. تست TRON تطبیق مبلغ
با انتقال canonical و جداکردن NFT از محاسبهٔ سواپ را پوشش می‌دهد.
چیدمان ABIها بر اساس قراردادهای منتشرشده بررسی شد.
[Uniswap V2](https://github.com/Uniswap/v2-core/blob/master/contracts/interfaces/IUniswapV2Pair.sol)،
[Uniswap V3](https://github.com/Uniswap/v3-core/blob/main/contracts/interfaces/pool/IUniswapV3PoolEvents.sol)،
[Balancer](https://github.com/balancer/balancer-v2-monorepo/blob/master/pkg/interfaces/contracts/vault/IVault.sol)،
[Curve](https://github.com/curvefi/curve-contract/blob/master/contracts/pools/compound/StableSwapCompound.vy)،
[Tornado](https://github.com/tornadocash/tornado-core/blob/master/contracts/Tornado.sol)

تست‌های بومی هر چهار شبکه، قرارداد مشترک و سیاست ریسک موفق بودند.
در ClickHouse موقت `23.8.16.16`، سناریوی BSC شناسهٔ کامل UInt256 و
مقدار NFT را از API برگرداند. سناریوی Bitcoin سیاست جدید را از دادهٔ
تازه تا API، در کنار رگرسیون restart و reorg، تأیید کرد. هر هشت حالت
Wallet و Paths در Neo4j Community `5.26.0` موقت موفق شدند و بخش
معنایی پس از بازخوانی و تغییر منبع به‌عنوان شاهد همان snapshot حفظ شد.
OpenAPI و رندر بخش جدید پنل با دادهٔ آزمایشی نیز کنترل شدند.

## BRIDGE-1 — اتصال شواهد cBridge

آزمون واحد اختلاف شناسه، جهت، آدرس، توکن، مبلغ و زمان را رد می‌کند. آزمون
Neo4j با آداپتورهای آزمایشی اتریوم و BSC، شروع تحقیق از سمت Relay،
محدودیت مالک و محدودهٔ تحقیق، تغییر snapshot، یکتایی اتصال، بازخوانی شاهد
و حذف با انقضای تحقیق را بررسی می‌کند.

```bash
export BRIDGE_NEO4J_URL=http://127.0.0.1:7474
export BRIDGE_NEO4J_PASSWORD=your-disposable-test-password
cargo test --locked --manifest-path analytical_node/Cargo.toml
cargo test --locked --manifest-path analytical_node/Cargo.toml \
  cross_chain_bridge_http -- --ignored --nocapture
```

همین آزمون با رسیدهای واقعی هم قابل اجراست. `BRIDGE_RPC_FIXTURE` مسیر فایل
JSON شامل دو لاگ به ترتیب اتریوم و BSC است. هر لاگ فیلدهای `tx_hash`،
`block_hash`، `block_number`، `timestamp_ms`، `log_index`، `address`،
`topics` و `data` را دارد. نمونهٔ USDT با لاگ ۸۵ در اتریوم و لاگ ۱۴۹
در BSC و فاصلهٔ ۲۰۷ ثانیه از RPC عمومی دریافت شد.
[تراکنش اتریوم](https://etherscan.io/tx/0xf4f77e0edcf8a9d6acef9b6ff7bb44e293362cd26d9da7bce16f9d61a3da2a00) و
[تراکنش BSC](https://bscscan.com/tx/0x32b7b248ede5622c21f0358716121e77e779a9aaf4111922b38dbe8844e9b5fb)

آزمون BSC از RPC آزمایشی، دریافت و ثبت رسید در ClickHouse واقعی تا
endpoint محافظت‌شده را اجرا می‌کند. بازه، جهت و خارج‌شدن رسید از شاخهٔ
canonical پس از reorg کنترل می‌شوند. متغیرهای `BSC_TEST_CLICKHOUSE_*`
باید به ClickHouse آزمایشی اشاره کنند.

```bash
cargo test --locked --manifest-path dockerizd_bsc/Cargo.toml --lib \
  bridge_reads_use_canonical_receipt_logs -- --ignored --nocapture
```

## EVAL-1 — ارزیابی آفلاین سیاست AML

رگرسیون بومی در Docker با ۴۳ تست Bitcoin، ۳۵ تست Ethereum، ۴۸ تست BSC
و ۸۶ تست TRON موفق شد. در سیاست مشترک و سرویس مرکزی نیز هرکدام ۱۵ تست
موفق شدند. آزمون‌های نیازمند DB در شمارش بومی این مرحله نیستند.

برای هر نمونه، ورودی ثبت‌شدهٔ تحقیق Wallet را در `evidence` بگذارید و
مرجع شاهد و برچسب مستقل را جدا ثبت کنید. `expected_review` تصمیم مرجع
برای ارجاع به تحلیلگر است. `expected_signals` فقط سیگنال‌هایی را نام
می‌برد که برای همان نمونه داوری شده‌اند. هر نمونه دست‌کم یکی از این دو
را می‌خواهد. نبود آدرس در یک خوراک، مبنای برچسب منفی AML نیست.

```json
{
  "contract": "aml_risk_evaluation_v1",
  "id": "review-batch-01",
  "kind": "independent",
  "cases": [{
    "id": "case-01",
    "evidence_ref": "archive:wallet-response-01",
    "label_ref": "review:case-01",
    "source_status": "current",
    "expected_review": true,
    "expected_signals": {"APPROVED_RISK_LABEL": true},
    "evidence": {
      "network_id": "eip155:1",
      "address": "0x1111111111111111111111111111111111111111",
      "source_snapshot": {
        "contract": "canonical_v1",
        "network_id": "eip155:1",
        "snapshot_id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "history_complete": false
      }
    }
  }]
}
```

در مثال، `evidence` عمداً کوتاه شده است. اجرای همین مثال برای برچسب
مثبت، `UNKNOWN` می‌دهد. در ارزیابی واقعی پاسخ ثبت‌شده را کامل بگذارید و
وضعیت شاهد در زمان ثبت را با `current`، `stale` یا `unverified` مشخص
کنید. برای دادهٔ ساختگی از `kind: "regression"` استفاده کنید. ورودی
مستقل نیازمند شناسهٔ snapshot و آدرس معتبر است و تکرار همان
آدرس و snapshot در یک شبکه رد می‌شود. برچسب‌ها را مستقل از امتیاز مدل
تهیه کنید و برای سنجش کیفیت عملیاتی، پرونده‌ها و آدرس‌های مرتبط را بین
مجموعهٔ تنظیم و مجموعهٔ ارزیابی تقسیم نکنید.

```bash
AML_RISK_EVALUATION_INPUT=/absolute/path/wallet-corpus.json \
  cargo test --locked --manifest-path analytical_node/Cargo.toml \
  evaluate_archived_risk_corpus -- --ignored --nocapture
```

گزارش JSON هش فایل و سیاست، آستانهٔ فعال ارجاع، شبکه‌های غایب، تعداد مثبت
و منفی درست و کاذب و تعداد `UNKNOWN` مثبت و منفی را برمی‌گرداند. پوشش،
precision و recall به تفکیک شبکه، پوشش تاریخچه و سیگنال گزارش می‌شوند.
Precision و recall فقط روی نمونه‌های دارای نتیجه محاسبه می‌شوند و
کنارشان پوشش اعلام می‌شود. مقدار `null` یعنی مخرج کافی وجود ندارد.
ارزیاب سیاست را روی شواهد دوباره اجرا می‌کند و امتیاز قبلی و محتوای کیف
پول را در خروجی گزارش نمی‌نویسد.

در یک نمونهٔ محدود، رسید موفق Deposit در BSC و هویت قرارداد از RPC و
مستندات پروتکل بررسی شدند. ورودی سیاست از همان رسید ساخته شد و ارزیاب
یک مثبت درست برای `MIXER_INTERACTION` ثبت کرد. این مجموعه فقط یک رخداد
و یک سیگنال برچسب‌دار داشت.
[رسید BSC](https://bscscan.com/tx/0xb003b6e98715e27e1b06f719e98c006cadefcdd68073a82ccb0da6815de2b8ab#eventlog)
و [قراردادهای منتشرشدهٔ پروتکل](https://github.com/tornadocash/docs/blob/en/general/tornado-cash-smart-contracts.md)

برای OFAC، آزمون جداگانهٔ زیر فایل رسمی آرشیوشده با هش
`bbf4658b7803ed6aa25712670bd6dfea56b6f9f93001d99e5d8ea221b237a597`
را می‌خواهد. آزمون پنج انتساب مرجع در چهار شبکه را همراه با UID، هش
منبع و وضعیت pending کنترل می‌کند. آدرس BNB Beacon و انتساب صرفاً
اتریومی در خروجی BSC پذیرفته نمی‌شوند. این آزمون کیفیت نگاشت همان
snapshot را می‌سنجد. [فهرست رسمی SDN](https://sanctionslistservice.ofac.treas.gov/api/PublicationPreview/exports/SDN.XML)
و [نمونهٔ انتشار OFAC](https://ofac.treasury.gov/recent-actions/20260807)

```bash
AML_OFAC_REFERENCE_XML=/absolute/path/archived-sdn.xml \
  python -m unittest discover -s scripts/tests -p 'test_prepare_ofac_sdn.py' -v
```

## NFT-1 — رسید بازار Seaport در تحقیق اتریوم

تست مشترک ABI، آرایهٔ ناقص، هم‌پوشانی offset، تعداد خارج از سقف، تطبیق
دارایی و مقدار دقیق ERC721/ERC1155 و دست‌کاری شواهد را کنترل می‌کند.
در آزمون ClickHouse، لاگ معتبر و لاگ مشابه از قرارداد دیگری از RPC
آزمایشی وارد می‌شوند. پاسخ کیف پول تنها رخداد قرارداد رسمی را
برمی‌گرداند و مبلغ دست‌کاری‌شده در اعتبارسنجی قرارداد API رد می‌شود.

```bash
cargo test --locked --manifest-path shared/chain_api/Cargo.toml
ETHEREUM_TEST_CLICKHOUSE_URL=http://127.0.0.1:18146 \
  AML_SERVICE_KEY=ethereum-fixture-service-key-0123456789abcdef \
  cargo test --locked --manifest-path dockerizd_ethereum/Cargo.toml --test shared_pipeline \
  rpc_to_common_storage_workers_and_investigation -- --ignored --nocapture
```

نمونهٔ واقعی از RPC عمومی در بلاک ۲۶۳۸۲۱۱۴ بررسی شد. انتقال ERC721 با
توکن ۱۱۸۳ در لاگ ۷۰۵ به رخداد لاگ ۷۰۲ مرتبط است. لاگ ۷۰۳ همان تراکنش
با جهت انتقال مشاهده‌شده سازگار نیست. برای تکرار تست، رسید موفق RPC
را در فیلد `receipt` و هش و شمارهٔ canonical بلاک را در
`block_hash` و `block_number` فایل JSON بگذارید.
[تراکنش اتریوم](https://etherscan.io/tx/0xe126be72bf421ce5bdd5090c4001bf7414a59c478deecfffdbd04d55cbbc31bc#eventlog)

```bash
NFT_TEST_PUBLIC_RECEIPT=/absolute/path/seaport-receipt.json \
  cargo test --locked --manifest-path shared/chain_api/Cargo.toml \
  public_seaport_receipt_matches -- --ignored --nocapture
```

## BRIDGE-2 — Allbridge Core بین BSC و ترون

تست تطبیق هر دو جهت، تغییر مبلغ vUSD، گیرنده، nonce، توکن مقصد،
شناسهٔ شبکه، پیام‌رسان، هش یا زمان را رد می‌کند. در نمونهٔ عمومی،
فراخوانی `receiveTokens` و لاگ شمارهٔ ۲ از رسید تأییدشدهٔ ترون خوانده
شدند. تست Rust هش پیام همان رخداد را بازسازی می‌کند. در BSC نیز
قرارداد اصلی وضعیت `sentMessages=1` را برای هش پایهٔ همین پیام
برگرداند. RPC عمومی BSC برای لاگ آرشیوی مبدأ پاسخ قابل استفاده نداد.
بازپخش انتهابه‌انتها در Docker با دو رسید آزمایشی انجام شد.
[رسید عمومی ترون](https://tronscan.org/#/transaction/fbb6411845747bd59e7aaf7c5c1f30b769eb7a554e09e99bd80b397ef4e7ec6c)

تست‌های ClickHouse از RPCهای آزمایشی استفاده می‌کنند. BSC جدا بودن
لاگ‌های cBridge و Allbridge و خواندن ارسال بر اساس nonce را بررسی می‌کند.
در ترون، لاگ هر دو رسید موفق و ناموفق در انبار ثبت می‌شود و مسیر
Allbridge فقط رسید موفق همان نسخهٔ canonical را برمی‌گرداند.

```bash
cargo test --locked --manifest-path analytical_node/Cargo.toml bridge::allbridge
WAREHOUSE_TEST_URL=http://127.0.0.1:18141 \
  cargo test --locked --manifest-path dockerizd_tron/app/Cargo.toml --test committed_visibility \
  allbridge_reads_only_successful_canonical_tron_receipts -- --ignored --nocapture
cargo test --locked --manifest-path dockerizd_bsc/Cargo.toml --lib \
  bridge_reads_use_canonical_receipt_logs -- --ignored --nocapture
```

آزمون Neo4j بخش BRIDGE-1 اکنون هر دو پروتکل را با آداپتورهای آزمایشی
می‌سنجد. برای بازپخش دو لاگ واقعی Allbridge، متغیر
`ALLBRIDGE_RPC_FIXTURE` را به فایل JSON با قالب
`[network_id, send_log, receive_log]` بدهید. ساختار هر لاگ همان ساختار
ذکرشده در BRIDGE-1 است. برای ترون، هش بلاک و تراکنش بدون پیشوند `0x`
و آدرس فرستندهٔ لاگ به قالب Base58Check ثبت می‌شود.

## BRIDGE-3 — سپردهٔ tBTC v2 از بیت‌کوین به اتریوم

نمونهٔ `shared/chain_api/tests/fixtures/tbtc-mainnet.json` از RPC عمومی هر
دو شبکه گرفته شد. وضعیت موفق رسید، تعلق لاگ به رسید و هش canonical
بلاک هر دو شبکه بررسی شدند. خروجی ۱ به مبلغ ۱٬۰۰۰٬۰۰۰ ساتوشی با
اسکریپت بازسازی‌شده مطابق است. زمان رویداد اتریوم ۲۷۶ ثانیه زودتر از
زمان بلاک بیت‌کوین است.
[تراکنش بیت‌کوین](https://mempool.space/tx/2dda71d1da8e8fe04d1a48748ca65e51205adba96a3cca79027ae12159b5ec07)
[رسید اتریوم](https://etherscan.io/tx/0x00534e80abdddbded790c661eea9803ee00d2ee75630d5818e89aee7d650f724#eventlog)

آزمون Rust تغییر مبلغ، شمارهٔ خروجی، ترتیب بایت هش، اسکریپت و قالب ABI
را رد می‌کند. تست‌های آداپتورها دادهٔ RPC آزمایشی را در ClickHouse موقت
می‌نویسند. تست Neo4j از آداپتورهای آزمایشی استفاده می‌کند و تغییر
snapshot، ثبت شمارهٔ خروجی و ثبت یک‌بارهٔ تطبیق را بررسی می‌کند.

```bash
cargo test --locked --manifest-path analytical_node/Cargo.toml mainnet_tbtc_reveal
cargo test --locked --manifest-path analytical_node/Cargo.toml tbtc_http_persists \
  -- --ignored --nocapture
cargo test --locked --manifest-path dockerizd_bitcoin/Cargo.toml --test clickhouse_pipeline \
  tbtc_funding_reads_only_published_bitcoin_outputs -- --ignored --nocapture
cargo test --locked --manifest-path dockerizd_ethereum/Cargo.toml --test shared_pipeline \
  rpc_to_common_storage_workers_and_investigation -- --ignored --nocapture
```

برای تست Neo4j، `BRIDGE_NEO4J_URL` و `BRIDGE_NEO4J_PASSWORD` را روی
نمونهٔ موقت تنظیم کنید. تست‌های ClickHouse به ترتیب به
`BITCOIN_TEST_CLICKHOUSE_URL` و `ETHEREUM_TEST_CLICKHOUSE_URL` نیاز دارند.
حساب‌های fixture بیت‌کوین و اتریوم را طبق تست‌های قبلی بسازید و
`AML_SERVICE_KEY=ethereum-fixture-service-key-0123456789abcdef`
را برای تست اتریوم تنظیم کنید.

## BRIDGE-4 — جمع‌آوری سپردهٔ tBTC v2

نمونهٔ `shared/chain_api/tests/fixtures/tbtc-sweep-mainnet.json` از
PublicNode هر دو شبکه ثبت شد. خروجی ۰ سپرده به مبلغ ۴۰۳٬۶۹۷٬۲۴۹ ساتوشی
در ورودی ۱ تراکنش sweep خرج شد. رسید موفق اتریوم در لاگ ۷۵۲ هش همان
تراکنش و شناسهٔ کیف پول ثبت‌شده در رسید سپرده را دارد. هش canonical
بلاک هر چهار شاهد بررسی شد.
[سپردهٔ بیت‌کوین](https://mempool.space/tx/b95b2e8c6074bc08878ac2c0c2c4e4f9592632885dfc0bc31f58eee4b944f56c) ·
[ثبت سپرده](https://etherscan.io/tx/0x0bd2d11ae21beba451894bd0c03772d40aced49517388ec0c3771696958d23d3#eventlog) ·
[تراکنش sweep](https://mempool.space/tx/bce49f858e30d9fa15d0c0262bca5417b7de1f6e283278fba40654f9b9547a61) ·
[رسید اثبات](https://etherscan.io/tx/0x7148dba97c3f82cb2ef87868c5800171764379ece01b526649c9280de96343e4#eventlog)

تست Rust تغییر خروجی قبلی، هش تراکنش، شناسهٔ کیف پول، قرارداد و زمان
را رد می‌کند. همان آزمون‌های ClickHouse بخش BRIDGE-3 اکنون حذف خرج پس
از reorg و رد لاگ رسید ناموفق را هم بررسی می‌کنند. تست Neo4j ثبت
جداگانهٔ reveal و sweep، تکرار امن درخواست و رد شاهد هنگام تغییر
snapshot یا نبود رسید اثبات را می‌سنجد.

```bash
cargo test --locked --manifest-path analytical_node/Cargo.toml mainnet_tbtc_sweep
cargo test --locked --manifest-path analytical_node/Cargo.toml tbtc_http_persists \
  -- --ignored --nocapture
```

## BRIDGE-5 — صدور خوش‌بینانهٔ tBTC v2

نمونهٔ `shared/chain_api/tests/fixtures/tbtc-mint-mainnet.json` از RPC
عمومی هر دو شبکه گرفته شد. بلاک‌های canonical و موفقیت دو رسید اتریوم
بررسی شدند. خروجی ۰ بیت‌کوین به مبلغ ۱٬۰۵۰٬۰۰۰ ساتوشی با سپردهٔ ثبت‌شده
مطابق است. رسید صدور، در لاگ‌های ۱۱۸ تا ۱۲۰، صدور **۰٫۰۱۰۵ tBTC** به
سپرده‌گذار و کلید همان سپرده را ثبت کرده است.
[خروجی بیت‌کوین](https://mempool.space/tx/ae078f14ee9a6028982d6bf7ba19b39df3c584fa376073df99454f3ceef160b6) ·
[ثبت سپرده](https://etherscan.io/tx/0x1e9d7613caea16245d4c811cb0f2209b5d73f2905cd9b7b01df8da23c427c435#eventlog) ·
[رسید صدور](https://etherscan.io/tx/0xcf9488b799b2aad1dde137400679f61c93334b8eb18861becc6ba4877dac66b4#eventlog)

تست Rust کلید اشتباه، لاگ ناقص، قرارداد یا مبدأ اشتباه انتقال، اختلاف
مبلغ و جابه‌جایی لاگ بین رسیدها را رد می‌کند. تست ClickHouse دادهٔ
آزمایشی را از مسیر RPC آداپتور وارد می‌کند و فقط لاگ‌های همان رسید
موفق را برمی‌گرداند. تست Neo4j با آداپتورهای آزمایشی، رد شاهد ناقص
و snapshot قدیمی، ثبت مبلغ دقیق و تکرار امن درخواست را بررسی می‌کند.

```bash
cargo test --locked --manifest-path analytical_node/Cargo.toml mainnet_tbtc_mint
cargo test --locked --manifest-path analytical_node/Cargo.toml tbtc_mint_http_persists \
  -- --ignored --nocapture
cargo test --locked --manifest-path dockerizd_ethereum/Cargo.toml --test shared_pipeline \
  tbtc_mint_reads_one_successful_canonical_receipt -- --ignored --nocapture
```

متغیرهای آزمون Neo4j و ClickHouse اتریوم همان متغیرهای بخش BRIDGE-3
هستند. آزمون آداپتور اتریوم به پایگاه موقت `ethereum_tbtc_mint_fixture`
نیاز دارد.
