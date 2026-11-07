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
