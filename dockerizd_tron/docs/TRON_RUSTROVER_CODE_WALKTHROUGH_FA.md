<!-- @style: ./style.css -->
# سناریوی ارائه کد پروژه TRON AML در RustRover

این فایل یک مستند عمومی نیست. ترتیب آن دقیقاً برای زمانی نوشته شده که RustRover را باز کرده‌ام و می‌خواهم پروژه را طوری توضیح بدهم که انگار خودم آن را طراحی و پیاده‌سازی کرده‌ام.

هدف ارائه این است که سه مسیر را روی کد نشان بدهم:

1. اطلاعات TRON از کجا وارد می‌شوند و چگونه در ClickHouse ذخیره می‌شوند؟
2. وقتی Wallet را جست‌وجو می‌کنیم، اطلاعات دقیقاً از کجا خوانده و چگونه در UI نمایش داده می‌شوند؟
3. Neo4j چه زمانی پر می‌شود و node و edge دقیقاً در کدام خطوط نوشته می‌شوند؟

## پاسخ خیلی کوتاه معماری

من معماری را با این جمله شروع می‌کنم:

> ClickHouse پایگاه داده اصلی و منبع حقیقت پروژه است. سرویس Rust بلاک و Receipt را از TRON می‌گیرد، factهای لازم را استخراج می‌کند و در ClickHouse می‌نویسد. هنگام جست‌وجوی Wallet، API دوباره از ClickHouse می‌خواند و JSON گراف را به UI می‌دهد. UI گراف را با SVG رسم می‌کند. Neo4j فقط وقتی endpoint مخصوص import اجرا شود، با همان nodeها و edgeهای خوانده‌شده از ClickHouse پر می‌شود.

نکته بسیار مهم:

- جست‌وجوی معمولی Wallet در Neo4j چیزی ذخیره نمی‌کند.
- UI برای رسم گراف از Neo4j نمی‌خواند.
- UI گراف را از پاسخ JSON سرویس Rust رسم می‌کند.
- Neo4j یک projection انتخابی و قابل بازسازی از ClickHouse است.

---

# بخش اول: معرفی ساختار پروژه در RustRover

ابتدا پنل Project را باز می‌کنم و پوشه‌های اصلی را معرفی می‌کنم.

```text
app/
├── Cargo.toml
├── README.md
├── sql/
│   └── init_database_tron.sql
├── web/
│   └── index.html
└── src/
    ├── main.rs
    ├── lib.rs
    ├── config.rs
    ├── bin/
    ├── db/
    ├── handlers/
    ├── helper/
    ├── models/
    ├── progress/
    ├── services/
    ├── tasks/
    └── utils/
```

توضیحی که می‌دهم:

- `main.rs` نقطه شروع ingest است.
- `src/bin` برنامه‌های اجرایی مستقل مثل API، replay و metadata worker را دارد.
- `tasks` loopهای بلندمدت را مدیریت می‌کند.
- `helper` ارتباط سطح پایین با TRON RPC را انجام می‌دهد.
- `services` منطق اصلی کسب‌وکار و AML است.
- `models` قرارداد داده Rust با ClickHouse و JSON API است.
- `handlers` لایه HTTP و Axum است.
- `db` کنترل schema و checkpoint را انجام می‌دهد.
- `sql` ساختار کامل ClickHouse را تعریف می‌کند.
- `web/index.html` کل رابط تحقیق Wallet و رسم SVG graph را دارد.

---

# بخش دوم: مسیر ورود داده از TRON تا ClickHouse

## مرحله 1: شروع ingest از `src/main.rs`

فایل `src/main.rs` را باز می‌کنم و خطوط 3 تا 8 را نشان می‌دهم:

```rust
use arz_axum_for_services::config::AppConfig;
use arz_axum_for_services::tasks::fetch_loop::run_tron_loop;

#[tokio::main]
async fn main() -> Result<()> {
    run_tron_loop(AppConfig::from_env()).await
}
```

توضیح من:

> این binary مخصوص ingest است. ابتدا تنظیمات را با `AppConfig::from_env()` می‌سازم و بعد `run_tron_loop` را اجرا می‌کنم. چون کار شبکه و دیتابیس asynchronous است، runtime پروژه Tokio است.

بعد با Ctrl+Click روی `AppConfig` وارد `src/config.rs` می‌شوم.

## مرحله 2: تنظیمات در `src/config.rs`

در خطوط 4 تا 8 سه حالت sync تعریف شده است:

- `Backfill`: یک بازه تاریخی را می‌گیرد و تمام می‌شود.
- `Live`: برای دنبال کردن وضعیت فعلی شبکه است.
- `Auto`: از آخرین checkpoint ادامه می‌دهد؛ حالت پیش‌فرض پروژه است.

در خطوط 22 تا 44 ساختار `AppConfig` قرار دارد. گروه‌های اصلی آن عبارت‌اند از:

- اتصال ClickHouse؛ URL، user، password و database.
- اتصال TRON RPC و API key.
- نقطه شروع و تعداد تراکنش.
- concurrency و timeout.
- اندازه batch و زمان flush.
- اتصال Neo4j.

در خطوط 46 تا 85 تابع `from_env` مقدارها را از environment یا فایل تنظیمات می‌خواند. خط 50 نشان می‌دهد حالت پیش‌فرض `SyncMode::Auto` است.

برای ارائه مقدار secret یا API key را نمی‌خوانم؛ فقط توضیح می‌دهم که config یک محل متمرکز برای ساخت clientهاست.

## مرحله 3: loop اصلی در `src/tasks/fetch_loop.rs`

با Ctrl+Click روی `run_tron_loop` وارد این فایل می‌شوم.

### خطوط 19 تا 25: آماده‌سازی

```rust
let admin_client = Client::default()
    .with_url(&config.clickhouse_url)
    .with_user(&config.clickhouse_user)
    .with_password(&config.clickhouse_pass);
validate_tron_schema(&admin_client).await?;

let loader = Arc::new(LoaderTron::new(&config).await?);
```

توضیح من:

> اول schema را validate می‌کنم تا اگر SQL و مدل Rust با هم ناسازگار باشند، ingest با داده ناقص شروع نشود. بعد `LoaderTron` را می‌سازم که تمام clientها و batcherها را یکجا نگه می‌دارد.

### خطوط 26 تا 33: تعیین بلاک شروع

```rust
let last_synced = get_last_synced_block(&loader.clickhouse).await?;
let start_block = resolve_start_block_tron(..., last_synced).await?;
```

توضیح من:

> checkpoint از ClickHouse خوانده می‌شود. در حالت Auto اگر آخرین بلاک موفق N باشد، شروع بعدی N+1 است. به همین دلیل restart برنامه ingest را از صفر تکرار نمی‌کند.

### خطوط 50 تا 72: loop و retry

در خط 51 `fetch_tron` اجرا می‌شود. اگر موفق شود، خطوط 54 و 55 آخرین checkpoint را می‌خوانند و `next_block = last_synced + 1` می‌شود. اگر خطا رخ دهد، delay از یک ثانیه شروع و تا 60 ثانیه افزایش پیدا می‌کند.

اگر `FinalizedHashConflict` رخ دهد، برنامه عمداً متوقف می‌شود، چون تناقض در شواهد بلاک final نباید نادیده گرفته شود.

## مرحله 4: container سرویس‌ها در `src/services/loader.rs`

ساختار `LoaderTron` در خطوط 16 تا 26 را نشان می‌دهم:

```rust
pub struct LoaderTron {
    pub clickhouse: Arc<Client>,
    pub tron_client: Arc<TronClient>,
    pub rpc_limiter: Arc<Semaphore>,
    pub transaction_batcher: Arc<TransactionBatcher>,
    pub relationship_batcher: Arc<RelationshipBatcher>,
    pub semantic_event_batcher: Arc<SemanticEventBatcher>,
    pub token_metadata_discovery_batcher: Arc<TokenMetadataDiscoveryBatcher>,
    pub transaction_feature_batcher: Arc<TransactionFeatureBatcher>,
}
```

توضیح من:

> این ساختار dependency container بخش ingest است. به‌جای اینکه برای هر transaction دوباره ClickHouse client یا RPC client بسازم، آن‌ها را یک بار می‌سازم و با `Arc` بین taskهای asynchronous به اشتراک می‌گذارم.

در خطوط 30 تا 46 ClickHouse client، `TronClient` و semaphore ساخته می‌شوند. Semaphore تعداد requestهای هم‌زمان RPC را محدود می‌کند.

در خطوط 55 تا 79 batcherهای جدول‌های اصلی ساخته می‌شوند.

در خطوط 84 تا 89 تابع `flush_batches` تمام batcherها را flush می‌کند.

## مرحله 5: ارتباط با TRON در `src/helper/tron.rs`

این فایل wrapper مربوط به TRON RPC است. در ارائه می‌گویم:

> `fetcher.rs` نباید جزئیات HTTP، header API key و endpointهای TRON را بداند. این مسئولیت در `TronClient` جدا شده است. این client latest solid block، block by number و transaction receipt را می‌گیرد.

مزیت این جداسازی این است که در آینده می‌توان endpoint بیرونی را با Full Node محلی عوض کرد، بدون اینکه منطق AML در `fetcher.rs` تغییر کند.

## مرحله 6: دریافت بلاک در `src/services/tron/fetcher.rs`

این فایل مهم‌ترین فایل ingest است.

### تابع `fetch_tron_with_options` در خطوط 329 به بعد

در خطوط 335 و 336 latest solid block و end block مشخص می‌شوند. فقط بلاک final/solid ingest می‌شود.

در خطوط 351 تا 362 loop بلاک ساخته می‌شود و `get_block(current_block)` از TRON client فراخوانی می‌شود.

سپس کد موارد زیر را validate می‌کند:

- `blockID`
- `parentHash`
- block timestamp
- اینکه بلاک قبلاً با همان hash ثبت شده یا نه
- اینکه replay اجباری است یا ingest معمولی

### پردازش موازی transactionها

در خطوط 529 تا 569 transactionهای بلاک با `buffer_unordered` پردازش می‌شوند. میزان concurrency از `tx_worker_concurrency` می‌آید.

من توضیح می‌دهم:

> ترتیب تکمیل transactionها مهم نیست، ولی قبل از ثبت موفقیت بلاک باید همه آن‌ها موفق شده باشند. به همین دلیل خطاها جمع می‌شوند و اگر حتی یک transaction شکست بخورد، بلاک موفق ثبت نمی‌شود.

## مرحله 7: قلب ingest یعنی `process_tx`

تابع `process_tx` از خط 48 شروع می‌شود.

### 7.1 استخراج شناسه و قرارداد اصلی

- خطوط 49 تا 52: `txID` استخراج می‌شود.
- خطوط 54 و 55: نوع contract، initiator، target و contract address استخراج می‌شوند.
- خط 56: انتقال‌های native و TRC10 از transaction اصلی استخراج می‌شوند.

### 7.2 دریافت Receipt

```rust
let receipt = {
    let _permit = loader.rpc_limiter.acquire().await?;
    loader.tron_client.get_tx_receipt(&txid).await?
};
```

این قسمت در خطوط 58 تا 62 است.

توضیح من:

> خود transaction فقط درخواست اجرا را نشان می‌دهد. Receipt نتیجه واقعی را می‌دهد: موفقیت، fee، energy، bandwidth، event log و internal transfer. برای AML باید اثر واقعی اجرا را ذخیره کنیم، نه فقط intent تراکنش را.

در خطوط 83 تا 88 اگر status موفق باشد TRC20 log و internal transfer اضافه می‌شوند. اگر transaction شکست خورده باشد transferهای مشتق‌شده پاک می‌شوند تا جریان مالی غیرواقعی نسازیم.

### 7.3 ذخیره transaction

در خطوط 100 تا 116 یک `TransactionRow` ساخته و به `transaction_batcher` داده می‌شود.

فایل `src/models/tron/modules.rs:12-24` قرارداد ستون‌های آن را نشان می‌دهد:

- `tx_hash`: شناسه یکتای تراکنش.
- `block_number`: شماره بلاک.
- `timestamp`: زمان شبکه.
- `initiator_address`: امضاکننده یا شروع‌کننده.
- `target_address`: مقصد اصلی contract.
- `contract_address`: قرارداد هوشمند درگیر.
- `contract_type`: نوع contract در TRON.
- `fee`: هزینه تراکنش.
- `energy_usage_total`: مصرف Energy.
- `net_usage`: مصرف Bandwidth.
- `status`: موفق یا ناموفق بودن اجرا.

### 7.4 کشف token metadata

در خطوط 118 تا 138 آدرس تمام TRC20های مشاهده‌شده جمع می‌شود و `TokenMetadataDiscoveryRow` ساخته می‌شود.

این مرحله metadata را همان لحظه از شبکه نمی‌گیرد. فقط می‌گوید «این token دیده شده و باید worker آن را بررسی کند». Worker مستقل بعداً symbol، name و decimals را کامل می‌کند.

### 7.5 classification

در خطوط 141 تا 159 تابع classify با contract address، method data و transferها اجرا می‌شود.

این تابع **Wallet را به پول‌شویی یا سالم طبقه‌بندی نمی‌کند**. کاری که انجام می‌دهد، تشخیص context فنی قرارداد و تراکنش است؛ برای مثال می‌گوید این اجرای قرارداد احتمالاً مربوط به DEX، Bridge، Lending یا Token است.

ورودی آن در tron_classifier/types.rs فقط شامل این موارد است:

- contract_address: آدرس قراردادی که فراخوانی شده است.
- method_data: داده فراخوانی قرارداد که 8 کاراکتر اول آن method selector است.
- transfers: انتقال‌های واقعی استخراج‌شده از transaction و Receipt.

تابع classify در tron_classifier/classifier.rs سه مرحله را به‌ترتیب اجرا می‌کند:

1. **Known protocol:** آدرس contract در registry.rs جست‌وجو می‌شود. مثلاً اگر آدرس SunSwap باشد، category برابر Dex و protocol برابر SunSwap می‌شود.
2. **Method signature:** اگر contract شناخته‌شده نباشد، 8 کاراکتر اول method data در method_decoder.rs بررسی می‌شود. مثلاً selector مربوط به swapExactTokensForTokens به GenericDex / Dex نگاشت می‌شود.
3. **Flow analysis:** اگر دو روش قبل نتیجه ندهند، flow_analyzer.rs جریان دارایی را بررسی می‌کند. اگر برای یک actor حداقل یک دارایی خارج و یک دارایی دیگر وارد شده باشد، یک DEX/swap احتمالی با protocol برابر FlowBasedDex تشخیص داده می‌شود.

اگر هیچ evidence پیدا نشود، خروجی Unknown است.

خروجی ClassificationResult شامل این فیلدهاست:

- protocol: نام پروتکل، مانند SunSwap، GenericDex یا Unknown.
- category: خانواده قرارداد، مانند Dex، Bridge، Lending یا Token.
- confidence: قدرت heuristic این تشخیص بین صفر و یک.
- detection_source: دلیل تشخیص؛ known_protocol، method_signature، flow_analysis یا none.
- method_id: selector شناسایی‌شده، اگر تشخیص از method آمده باشد.

این خروجی چند کاربرد دارد:

- در fetcher.rs:149-159 مشخص می‌کند تراکنش یک contract call معنادار است یا نه.
- در fetcher.rs:172-173 category نوع Bridge به bridge detector به‌عنوان hint داده می‌شود.
- در semantic_event_builder نام protocol، منبع تشخیص و confidence داخل evidence رویداد ذخیره می‌شوند.
- در transaction_type.rs با eventهای swap/bridge/mint/liquidity ترکیب می‌شود تا transaction_type و transaction_subtype نهایی ساخته شوند.
- نتیجه در transaction_features ذخیره می‌شود و بعداً Fingerprint، Activity، Graph، فیلترهای UI و featureهای ML از آن استفاده می‌کنند.

مثال: اگر Wallet قرارداد ناشناخته‌ای را صدا بزند، ولی method selector آن 38ed1739 باشد، classifier آن را GenericDex / Dex تشخیص می‌دهد. سپس transaction_type.rs با توجه به transferها و swap event می‌تواند نوع نهایی تراکنش را swap / token_swap قرار دهد.

تفاوت مهم این دو مرحله:

- classify می‌گوید «این قرارداد یا context احتمالاً متعلق به چه خانواده‌ای است؟»
- classify_transaction_semantics می‌گوید «عمل واقعی این transaction چه بوده است؟»

در وضعیت فعلی این classifier یک سیستم deterministic و rule-based است، نه AI. Registry فعلی فقط چند پروتکل و method مشخص دارد و confidence آن heuristic است، نه احتمال آماری کالیبره‌شده. بنابراین برای enrichment و ساخت feature مفید است، اما به‌تنهایی نباید به‌عنوان تشخیص پول‌شویی استفاده شود.

### 7.6 ساخت semantic AML event

در خطوط 161 تا 190 detectorهای زیر اجرا می‌شوند:

- swap
- bridge
- mint/burn
- liquidity add/remove

بعد `build_semantic_event_rows` آن‌ها را به row قابل ذخیره در `semantic_aml_events` تبدیل می‌کند.

تأکید می‌کنم که semantic event به معنی اثبات جرم نیست؛ فقط یک fact رفتاری قابل جست‌وجو است.

### 7.7 ساخت fund-flow relationship

در خطوط 193 تا 198:

```rust
let relationships =
    build_relationships(&txid, block_number, timestamp, &canonical_transfers);

for row in relationships {
    loader.relationship_batcher.push(row).await?;
}
```

`relationship_builder.rs` transferها را به `AddressRelationshipRow` تبدیل می‌کند.

ساختار آن در `models/tron/relationship.rs` است:

- `relationship_id`: شناسه یکتای edge.
- `from_address`: فرستنده واقعی دارایی.
- `to_address`: گیرنده واقعی دارایی.
- `token_address`: TRX، TRC10 یا قرارداد TRC20.
- `tx_hash`: تراکنش منبع.
- `block_number`: بلاک منبع.
- `timestamp`: زمان انتقال.
- `amount`: مقدار خام و بدون از دست دادن precision.
- `transfer_type`: native، trc10، trc20 یا internal.

این جدول پایه اصلی fund-flow graph است.

### 7.8 ساخت transaction feature

در خطوط 200 تا 230 نوع و subtype نهایی ساخته می‌شود. فیلدهای `is_swap`، `is_bridge`، `is_mint`، `is_burn`، liquidity و contract call در `transaction_features` ذخیره می‌شوند.

این featureها بعداً توسط Fingerprint، Activity و مدل ML مصرف می‌شوند.

## مرحله 8: Batcher و INSERT واقعی ClickHouse

فایل `src/services/tron/batcher/impls.rs` را باز می‌کنم.

این فایل mapping صریح Rust type به ClickHouse table است:

```text
TransactionRow              -> transactions
TokenMetadataDiscoveryRow   -> token_metadata_discoveries
AddressRelationshipRow      -> address_relationships
SemanticAmlEventRow         -> semantic_aml_events
TransactionFeatureRow       -> transaction_features
```

بعد `batcher/generic.rs:46-89` را نشان می‌دهم.

- `push` row را داخل buffer می‌گذارد.
- اگر اندازه buffer به max برسد، `flush_pending` اجرا می‌شود.
- خط 77 با `T::TABLE` INSERT را برای جدول درست می‌سازد.
- خطوط 79 تا 82 rowها را می‌نویسند.
- خط 84 با `insert.end()` batch را commit می‌کند.
- خطوط 86 و 87 فقط پس از موفقیت، buffer را drain می‌کنند.

این بخش پاسخ دقیق سؤال «کدام خط در ClickHouse ذخیره می‌کند؟» است:

```rust
let mut insert = self.clickhouse.insert::<T>(T::TABLE).await?;
insert.write(&value).await?;
insert.end().await?;
```

## مرحله 9: ترتیب امن پایان بلاک

در `fetcher.rs:618-653` ترتیب مهم زیر وجود دارد:

1. `loader.flush_batches()`
2. `record_ingested_block(...)`
3. `save_sync_state(..., current_block)`

یعنی ابتدا factها ذخیره می‌شوند، بعد بلاک موفق ثبت می‌شود و در آخر checkpoint جلو می‌رود. اگر flush شکست بخورد، checkpoint نباید جلو برود.

---

# بخش سوم: ساختار SQL و ارتباط آن با کد

فایل `sql/init_database_tron.sql` تنها schema اجرایی ClickHouse است.

برای توضیح آن می‌گویم جدول‌ها چهار سطح دارند:

## 1. Ledger facts

- `transactions`: نتیجه اجرای هر transaction.
- `address_relationships`: انتقال واقعی asset بین دو آدرس.
- `semantic_aml_events`: swap، bridge، mint/burn و liquidity.
- `transaction_features`: classification و flagهای رفتاری.

این چهار جدول مستقیماً توسط ingest و batcherها پر می‌شوند.

## 2. Reliability و metadata

- `ingested_blocks`: hash، parent، finality و وضعیت ingest بلاک.
- `sync_state`: آخرین checkpoint.
- `ingestion_failures`: خطای stage، retry و resolution.
- `ingestion_benchmarks`: throughput، حجم و latency.
- `token_metadata_discoveries`: tokenهای دیده‌شده.
- `token_metadata_jobs`: صف metadata worker.
- `token_metadata`: symbol، name، decimals و وضعیت verification.

## 3. Intelligence و graph context

- `intelligence_sources`: منبع label و trust tier.
- `intelligence_reviews`: تصمیم analyst.
- `entity_labels`: برچسب تأییدشده.
- `exchange_addresses`: آدرس و نقش صرافی.
- `address_entity`: اتصال آدرس به entity.
- `address_cluster_claims`: evidence مربوط به clustering.
- `address_cluster_memberships`: عضویت آدرس در cluster.
- `cluster_versions`: نسخه و وضعیت cluster.
- `exposure_seeds`: seedهای پرریسک تأییدشده.
- `exposure_runs`: اجرای propagation.
- `address_exposure`: نتیجه exposure هر آدرس.

## 4. Analytical Node و ML lifecycle

- `analysis_subjects`: آخرین وضعیت تحلیلی هر subject.
- `wallet_analysis_snapshots`: snapshot نسخه‌بندی‌شده تحقیق.
- `wallet_analysis_evidence`: evidenceهای snapshot.
- `wallet_ml_feature_snapshots`: feature vector مدل.
- `wallet_ml_model_registry`: مدل‌های ثبت‌شده.
- `wallet_ml_model_deployments`: مدل فعال.
- `wallet_ml_predictions`: خروجی inference.

## Viewهای مهم

- `transactions_canonical`: نسخه معتبر transaction.
- `address_relationships_canonical`: relationship معتبر به‌همراه semantic fields.
- `exchange_flows_canonical`: relationshipهای دارای context صرافی.
- `mv_wallet_asset_delta_transfer_from_v3`: delta منفی sender.
- `mv_wallet_asset_delta_transfer_to_v3`: delta مثبت receiver.
- `wallet_asset_balances`: جمع deltaها و join با token metadata.

نکته ارائه:

> جدول پایه تاریخچه و evidence را نگه می‌دارد. View canonical داده مناسب query را ارائه می‌دهد. به همین دلیل graph query مستقیم از `address_relationships_canonical` می‌خواند، نه از row خام و نه از Neo4j.

---

# بخش چهارم: شروع سرویس وب و API

## مرحله 1: binary وب در `src/bin/tron_graph_api.rs`

این binary جدا از ingest اجرا می‌شود.

خطوط 9 تا 14 config و schema را validate می‌کنند. خطوط 16 تا 18 listener را روی آدرس API می‌سازند. خط 22 مهم است:

```rust
axum::serve(listener, build_router()).await?;
```

یعنی Axum router شروع به پاسخ‌گویی می‌کند.

## مرحله 2: ارائه HTML در `handlers/dashboard.rs`

```rust
pub async fn dashboard() -> Html<&'static str> {
    Html(include_str!("../../web/index.html"))
}
```

فایل UI در زمان compile داخل binary قرار می‌گیرد و route `/` آن را برمی‌گرداند.

## مرحله 3: routeها در `src/router.rs`

مهم‌ترین endpointها:

- `GET /api/tron/wallet/{address}/investigation`
- `GET /api/tron/wallet/{address}/graph`
- `GET /api/tron/wallet/{address}/holdings`
- `GET /api/tron/wallet/{address}/fingerprint`
- `GET /api/tron/wallet/{source}/paths/{target}`
- `POST /api/tron/wallet/{address}/neo4j/import`
- `POST /api/tron/wallet/{source}/paths/{target}/neo4j/import`
- `GET /api/analysis/tron/wallet/{address}`

برای جست‌وجوی اصلی، خطوط 77 تا 80 را نشان می‌دهم:

```rust
.route(
    "/api/tron/wallet/{address}/investigation",
    get(tron_wallet_investigation::tron_wallet_investigation),
)
```

---

# بخش پنجم: مسیر کامل Wallet Search از UI تا ClickHouse

این مهم‌ترین قسمت ارائه است.

## مرحله 1: کاربر Search را در UI می‌زند

فایل `web/index.html` را باز می‌کنم و تابع `loadInvestigation` در خط 1167 را نشان می‌دهم.

خطوط 1181 تا 1187 پارامترها را می‌سازند:

- graph depth
- edge limit
- fingerprint window
- top counterparties
- max events
- holdings limit

خط 1189 درخواست اصلی را می‌فرستد:

```javascript
fetch(`/api/tron/wallet/${encodeURIComponent(address)}/investigation?${query.toString()}`)
```

## مرحله 2: Router درخواست را به Handler می‌دهد

در `router.rs:78-80` endpoint به `tron_wallet_investigation` وصل است.

## مرحله 3: Handler آدرس و client را آماده می‌کند

فایل `handlers/tron_wallet_investigation.rs:23-43` را باز می‌کنم.

در این handler:

1. `AppConfig::from_env()` تنظیمات را می‌خواند.
2. `normalize_wallet_address` آدرس TRON را validate و canonical می‌کند.
3. `clickhouse_client(&config)` client متصل به `tron_db` می‌سازد.
4. خطوط 39 و 40 `build_wallet_investigation` را اجرا می‌کنند.
5. نتیجه با `Json(...)` به UI برمی‌گردد.

`handlers/tron_common.rs:42-64` کد مشترک validation و ساخت ClickHouse/Neo4j client را نگه می‌دارد.

## مرحله 4: Service پاسخ unified را می‌سازد

فایل `services/tron/wallet_investigation.rs:73-119` را باز می‌کنم.

در خطوط 78 تا 97 از `tokio::try_join!` استفاده شده است:

```text
build_wallet_flow_graph
build_wallet_holdings
build_wallet_fingerprint
build_wallet_activity
load_wallet_intelligence
load_wallet_exposure_summary
```

توضیح من:

> این queryها مستقل هستند، بنابراین آن‌ها را موازی اجرا می‌کنم تا endpoint مجموع زمان همه queryها را منتظر نماند.

در خط 98 AI فعلی با `build_disabled_wallet_ai_risk` غیرفعال نگه داشته شده است.

## مرحله 5: Graph از ClickHouse خوانده می‌شود

با Ctrl+Click روی `build_wallet_flow_graph` به `services/tron/neo4j/flow_graph.rs:144` می‌روم.

اسم پوشه `neo4j` نباید گمراه‌کننده باشد. این service ابتدا ClickHouse را query می‌کند و فقط در صورت وجود Neo4j client، projection را نیز می‌نویسد.

در خطوط 155 و 156 depth و limit محدود می‌شوند. در خطوط 158 تا 160:

```rust
let edges =
    load_relationship_neighborhood(clickhouse.clone(), address, depth, per_address_limit)
        .await?;
```

## مرحله 6: BFS همسایه‌ها را گسترش می‌دهد

تابع `load_relationship_neighborhood` در خطوط 505 تا 551 است.

- `frontier`: آدرس‌هایی که در عمق فعلی باید query شوند.
- `visited`: جلوگیری از پردازش دوباره node.
- `edge_ids`: جلوگیری از edge تکراری.
- `next_frontier`: آدرس‌های عمق بعد.

در هر دور، `load_relationships_for_addresses` برای frontier فعلی اجرا می‌شود. دو سر edge برای مرحله بعد اضافه می‌شوند تا depth کامل شود.

## مرحله 7: query واقعی ClickHouse

تابع `load_relationships_for_addresses` در `flow_graph.rs:554-643` است.

خط 595 جدول اصلی را مشخص می‌کند:

```sql
FROM address_relationships_canonical AS ar
```

خط 608 اطلاعات transaction را اضافه می‌کند:

```sql
FROM transactions_canonical
```

خط 621 context صرافی را اضافه می‌کند:

```sql
FROM exchange_flows_canonical
```

مهم‌ترین شرط در خط 634 است:

```sql
WHERE ar.from_address IN ? OR ar.to_address IN ?
```

یعنی هر relationship که Wallet در سمت فرستنده یا گیرنده باشد خوانده می‌شود.

خطوط 639 تا 641 addressها و limit را bind می‌کنند. خط 642 داده را واقعاً از ClickHouse می‌گیرد:

```rust
.fetch_all::<RelationshipReadRow>()
```

## مرحله 8: تبدیل rowهای دیتابیس به JSON graph

در `flow_graph.rs:1000` تابع `relationship_row_to_edge` هر row ClickHouse را به `FlowEdge` تبدیل می‌کند.

ساختارهای پاسخ در `neo4j/types.rs` هستند:

- `FlowNode`: آدرس و metadata مربوط به entity، exchange و cluster.
- `FlowEdge`: انتقال به‌همراه transaction، classification و exchange evidence.
- `WalletFlowGraph`: آدرس root، nodeها، edgeها، مبادی ورودی و تعاملات صرافی.

در `flow_graph.rs:892` تابع `load_node_metadata` اطلاعات nodeها را از جداول exchange/entity/cluster می‌خواند.

## مرحله 9: بخش‌های دیگر Investigation از چه جدول‌هایی می‌خوانند؟

### Holdings

`wallet_holdings.rs:58-89` از View زیر می‌خواند:

```sql
FROM wallet_asset_balances
```

این View deltaهای ورودی و خروجی را جمع و token metadata را join می‌کند.

### Fingerprint

`wallet_fingerprint.rs` از موارد زیر استفاده می‌کند:

- `address_relationships_canonical`
- `transaction_features`
- `exchange_addresses FINAL`
- `address_entity FINAL`
- `transactions_canonical`

خروجی آن volume، counterparties، direction، exchange interaction، semantic flags و شاخص‌های رفتار Wallet است.

### Activity

`wallet_activity.rs` از `address_relationships_canonical`، `transaction_features` و `semantic_aml_events FINAL` timeline می‌سازد.

### Intelligence

`entity_intelligence.rs` از entity، label، exchange، cluster، review و exposure seed اطلاعات هویتی می‌سازد.

### Exposure

`wallet_exposure.rs` از `address_exposure FINAL` و `exposure_seeds FINAL` خلاصه ریسک مسیر را می‌خواند.

## مرحله 10: JSON به UI برمی‌گردد

در `web/index.html:1198-1204` قسمت‌های پاسخ داخل state قرار می‌گیرند:

```javascript
state.graph = state.investigation.graph;
state.holdings = state.investigation.holdings;
state.fingerprint = state.investigation.fingerprint;
state.activity = state.investigation.activity;
state.intelligence = state.investigation.intelligence;
state.dataQuality = state.investigation.data_quality;
```

خط 1212 تابع `renderAll()` را اجرا می‌کند.

---

# بخش ششم: نمودار دقیقاً کجا و چگونه رسم می‌شود؟

این بخش کاملاً در `web/index.html` است، نه در Neo4j.

## `renderAll` در خطوط 1343 تا 1356

این تابع تمام panelهای UI را به‌روزرسانی می‌کند:

- metrics
- fingerprint
- activity
- intelligence
- holdings
- data quality
- graph
- exchange table
- node details
- Neo4j information
- path result

## `renderGraph` در خطوط 1682 تا 1769

ترتیب رسم:

1. خط 1683 `state.graph` را می‌گیرد.
2. خطوط 1697 و 1698 edgeها و nodeهای فیلترشده را می‌سازند.
3. خط 1700 با `layoutNodes` برای nodeها مختصات محاسبه می‌کند.
4. خطوط 1713 تا 1735 برای هر edge یک SVG path منحنی و arrow می‌سازند.
5. خطوط 1737 تا 1758 برای هر node یک SVG circle و label می‌سازند.
6. خط 1760 با این دستور نمودار را واقعاً روی صفحه قرار می‌دهد:

```javascript
svg.innerHTML = marker + edgeMarkup + nodeMarkup;
```

7. خطوط 1762 تا 1768 click handler هر node را اضافه می‌کنند.

## layout در خطوط 1771 تا 1842

`layoutNodes` از root Wallet یک BFS روی nodeهای JSON انجام می‌دهد و آن‌ها را بر اساس level روی حلقه‌های اطراف Wallet قرار می‌دهد.

پس جمله دقیق ارائه این است:

> Rust داده گراف را به‌شکل JSON می‌سازد، ولی رسم بصری node و edge در browser و در تابع `renderGraph` انجام می‌شود.

---

# بخش هفتم: Neo4j دقیقاً چه زمانی و کجا پر می‌شود؟

## ابتدا تفاوت GET و POST را نشان می‌دهم

در `handlers/tron_graph.rs:19-38` handler جست‌وجوی معمولی قرار دارد:

```rust
build_wallet_flow_graph(
    clickhouse,
    None,
    &address,
    ...
)
```

خط 29 مقدار `None` است. یعنی این درخواست فقط ClickHouse را می‌خواند.

در unified investigation نیز `wallet_investigation.rs:81` مقدار `None` فرستاده می‌شود. پس Search معمولی Neo4j write ندارد.

## کاربر دکمه projection را می‌زند

در `web/index.html:1265` تابع `projectVisibleGraph` شروع می‌شود.

- خط 1288 endpoint import مسیر بین دو Wallet را انتخاب می‌کند.
- خط 1294 endpoint import گراف Wallet را انتخاب می‌کند.
- خطوط 1297 تا 1300 درخواست `POST` را می‌فرستند.

## Handler import

در `handlers/tron_graph.rs:40-59`:

- خط 46 ClickHouse client را می‌سازد.
- خط 47 Neo4j client را می‌سازد.
- خطوط 49 تا 55 builder را با `Some(&neo4j)` صدا می‌زنند.

```rust
let graph = build_wallet_flow_graph(
    clickhouse,
    Some(&neo4j),
    &address,
    ...
)
```

یعنی همان queryهای ClickHouse اجرا می‌شوند، ولی این بار نتیجه در Neo4j نیز project می‌شود.

## نوشتن Wallet node

در `flow_graph.rs:182-196` شرط زیر اجرا می‌شود:

```rust
if let Some(neo4j) = neo4j {
    upsert_wallet_with_metadata(...).await?;
}
```

تابع اصلی node در `neo4j/nodes.rs:5` است.

در خطوط 18 تا 31 Cypher زیر ساخته می‌شود:

```cypher
MERGE (w:Wallet { chain: 'tron', address: $address })
SET w:TronAddress,
    w.label = $label,
    w.node_type = $node_type,
    w.entity_name = $entity_name,
    w.exchange_name = $exchange_name,
    w.cluster_id = $cluster_id
```

نوشتن واقعی node در خطوط 47 تا 50 انجام می‌شود:

```rust
neo4j.graph.run(q).await
```

## نوشتن transfer edge

در `flow_graph.rs:202-205`:

```rust
if let Some(neo4j) = neo4j {
    for edge in &edges {
        merge_transfer_edge(neo4j, edge).await?;
    }
}
```

فراخوانی دقیق edge در خط 204 است.

بعد وارد `neo4j/edges.rs` می‌شوم.

خط 30 رابطه را تعریف می‌کند:

```cypher
MERGE (a)-[t:{relationship_type} { id: $edge_id }]->(b)
```

خطوط 31 تا 57 properties مربوط به transaction، token، amount، block، classification و exchange را روی رابطه قرار می‌دهند.

نوشتن واقعی edge در خطوط 102 تا 105 است:

```rust
neo4j
    .graph
    .run(q)
    .await
```

این دقیق‌ترین پاسخ به سؤال «کدام خط در Neo4j ذخیره می‌کند؟» است.

## نتیجه معماری Neo4j

> ClickHouse rowها را نگه می‌دارد. `flow_graph.rs` آن‌ها را به `FlowNode` و `FlowEdge` تبدیل می‌کند. اگر handler مقدار `Some(&neo4j)` بدهد، `nodes.rs` و `edges.rs` آن‌ها را با Cypher MERGE در Neo4j می‌نویسند. اگر مقدار `None` باشد، فقط JSON برمی‌گردد.

---

# بخش هشتم: جست‌وجوی مسیر بین دو Wallet تا 10 hop

## UI

تابع `loadWalletPath` در `web/index.html:1221` source، target، depth، limit و direction را می‌خواند.

خط 1241 endpoint زیر را فراخوانی می‌کند:

```text
GET /api/tron/wallet/{source}/paths/{target}
```

## Router و Handler

route در `router.rs:89-95` است.

handler خواندن در `handlers/tron_wallet_paths.rs:21-44` قرار دارد و `None` را به Neo4j می‌دهد.

handler import در خطوط 46 تا 70 است و `Some(&neo4j)` را می‌دهد.

## Service

`build_wallet_path_graph` در `flow_graph.rs:256` شروع می‌شود.

- خط 270 depth را به 1 تا 10 محدود می‌کند.
- خط 271 حداکثر path را به 50 محدود می‌کند.
- خط 272 limit هر آدرس را به 2000 محدود می‌کند.
- خطوط 275 تا 284 `find_wallet_paths` را اجرا می‌کنند.

`find_wallet_paths` در خط 384 یک queue از stateهای مسیر می‌سازد. هر state شامل current node، nodeهای مسیر و edgeهای مسیر است. جست‌وجو تا max depth ادامه پیدا می‌کند و برای جلوگیری از انفجار graph، تعداد nodeهای expandشده محدود است.

در نسخه import، خطوط 326 تا 350 nodeها و edgeهای مسیر پیدا‌شده را در Neo4j می‌نویسند.

---

# بخش نهم: نقش تمام پوشه‌ها و فایل‌ها

این بخش برای سؤال‌های پایان ارائه است. لازم نیست همه را در ارائه اصلی باز کنم، ولی باید بدانم هر فایل کجای معماری است.

## فایل‌های ریشه `src`

- `src/main.rs`: entrypoint ingest دائمی TRON.
- `src/lib.rs`: export کردن moduleهای crate برای binaryهای مختلف.
- `src/config.rs`: تمام configuration و SyncMode.
- `src/router.rs`: routeهای Axum.

## پوشه `src/bin`

- `tron_graph_api.rs`: اجرای dashboard و API روی port سرویس.
- `tron_replay_blocks.rs`: replay کنترل‌شده یک range از بلاک‌ها.
- `tron_benchmark_ingestion.rs`: اندازه‌گیری throughput، حجم و investigation latency.
- `tron_token_metadata_worker.rs`: کامل کردن metadata توکن‌های کشف‌شده.
- `tron_ingest_entity_labels.rs`: وارد کردن labelها و آدرس‌های شناخته‌شده.
- `tron_register_intelligence_source.rs`: ثبت منبع intelligence و trust tier.
- `tron_review_intelligence.rs`: ثبت تصمیم analyst روی claim یا label.
- `tron_discover_address_clusters.rs`: اجرای clustering و ذخیره claim/membership/version.
- `tron_propagate_exposure.rs`: اجرای propagation از seedهای پرریسک.
- `tron_export_wallet_graph.rs`: export کردن graph یک Wallet برای بررسی یا انتقال.

## پوشه `src/db`

- `mod.rs`: export ماژول‌های دیتابیس.
- `tron_schema.rs`: validate کردن database، table، column و type مورد انتظار.
- `sync_state.rs`: خواندن و ذخیره آخرین بلاک syncشده.

## پوشه `src/handlers`

- `mod.rs`: export handlerها.
- `dashboard.rs`: برگرداندن `web/index.html` برای route اصلی.
- `health.rs`: health و readiness پایه سرویس.
- `status.rs`: status عمومی برنامه.
- `tron_common.rs`: error type، normalize address و ساخت clientهای مشترک.
- `tron_graph.rs`: GET graph و POST Neo4j import.
- `tron_wallet_paths.rs`: path search و path import تا 10 hop.
- `tron_wallet_investigation.rs`: endpoint unified تحقیق Wallet.
- `tron_wallet_holdings.rs`: endpoint مستقل Holdings.
- `tron_wallet_fingerprint.rs`: endpoint مستقل Fingerprint.
- `tron_wallet_ai_risk.rs`: endpoint AI؛ فعلاً خروجی disabled در flow اصلی.
- `tron_wallet_analysis.rs`: endpoint Analytical Node snapshot.
- `tron_ingestion_health.rs`: health تخصصی ingest و lag/errorها.

## پوشه `src/helper`

- `mod.rs`: export helperها.
- `tron.rs`: HTTP/RPC client شبکه TRON و تبدیل responseها.

## پوشه `src/models`

- `mod.rs`: export مدل‌ها.
- `token_metadata.rs`: مدل metadata token.

### پوشه `src/models/tron`

- `mod.rs`: export مدل‌های TRON.
- `modules.rs`: rowهای transaction، semantic event، block، failure، benchmark، metadata discovery و feature.
- `relationship.rs`: row اصلی fund-flow edge.
- `intelligence.rs`: مدل source، label، review، entity و cluster intelligence.
- `exposure.rs`: مدل seed، run و address exposure.
- `exchange.rs`: مدل آدرس و جریان صرافی.

## پوشه `src/progress`

- `mod.rs`: export progress utilities.
- `core.rs`: نمایش یا مدیریت progress عملیات طولانی.

## فایل‌های سطح اول `src/services`

- `mod.rs`: export serviceها.
- `loader.rs`: dependency container و batcherهای ingest.
- `sync_logic.rs`: تصمیم‌گیری start block برای sync modeهای مختلف.

## پوشه `src/services/tron`

- `mod.rs`: export کل قابلیت‌های TRON.
- `fetcher.rs`: دریافت بلاک، process transaction، flush و checkpoint.
- `transfer_extractor.rs`: استخراج native، TRC10، TRC20 و internal transfer.
- `relationship_builder.rs`: تبدیل transfer به relationship پایدار.
- `semantic_event_builder.rs`: تبدیل AML event به row ClickHouse.
- `transaction_type.rs`: تعیین type و subtype نهایی تراکنش.
- `observation_window.rs`: محاسبه مرز زمانی تحلیل Wallet.
- `performance.rs`: metricهای throughput و storage/latency.
- `risk_math.rs`: utilityهای عددی ریسک؛ AI در flow فعلی غیرفعال است.
- `ingestion_state.rs`: ثبت بلاک موفق/ناموفق، hash conflict و repair state.
- `ingestion_health.rs`: محاسبه lag، failure count و سلامت ingest.
- `tron_metadata_worker.rs`: claim کردن job، فراخوانی metadata و upsert نتیجه.
- `address_clustering.rs`: ساخت claim و membership خوشه آدرس‌ها.
- `entity_intelligence.rs`: source/label/review/exchange/entity/cluster context.
- `wallet_holdings.rs`: query موجودی از `wallet_asset_balances`.
- `wallet_fingerprint.rs`: محاسبه fingerprint رفتاری Wallet.
- `wallet_activity.rs`: timeline و eventهای فعالیت.
- `wallet_exposure.rs`: query و خلاصه exposure.
- `wallet_investigation.rs`: ترکیب تمام خروجی‌های تحقیق.
- `wallet_ai_risk.rs`: feature snapshot، model registry/deployment و inference infrastructure.
- `analytical_node.rs`: ساخت و بازیابی snapshot تحلیلی و evidence.

## پوشه `src/services/tron/batcher`

- `mod.rs`: export batcherها.
- `traits.rs`: قرارداد `BatchInsert` و نام جدول.
- `impls.rs`: mapping هر Rust row به ClickHouse table.
- `generic.rs`: buffer، flush، INSERT و retry-safe drain.
- `transactions.rs`: type alias/factory batcher تراکنش.
- `relationships.rs`: batcher relationship.
- `semantic_events.rs`: batcher semantic event.
- `token_metadata_discoveries.rs`: batcher token discovery.
- `transaction_features.rs`: batcher feature.

## پوشه `src/services/tron/tron_classifier`

- `mod.rs`: export classifier.
- `types.rs`: input/output و enumهای classification.
- `registry.rs`: اطلاعات protocol/contractهای شناخته‌شده.
- `method_decoder.rs`: decode method selector و signature.
- `protocol_detector.rs`: شناسایی protocol از contract و method.
- `flow_analyzer.rs`: تحلیل جهت و نوع asset flow.
- `confidence.rs`: ترکیب evidence و ساخت confidence.
- `classifier.rs`: orchestration نهایی classifier.

## پوشه `src/services/tron/aml`

- `mod.rs`: export detectorها.
- `types.rs`: مدل مشترک AML event و transfer ساده.
- `flow_engine.rs`: utility تحلیل flowهای ورودی/خروجی.
- `swap_detector.rs`: تشخیص exchange دو دارایی.
- `bridge_detector.rs`: تشخیص evidence مربوط به bridge.
- `mint_burn_detector.rs`: تشخیص ارتباط با zero address.
- `liquidity_detector.rs`: تشخیص liquidity add/remove.

## پوشه `src/services/tron/exposure`

- `mod.rs`: export engine exposure.
- `propagation.rs`: پیمایش graph از seedها و ساخت مسیر/فاصله.
- `scorer.rs`: amount/time/distance weighting و امتیاز exposure.

## پوشه `src/services/tron/neo4j`

- `mod.rs`: export قابلیت‌های graph.
- `client.rs`: اتصال Neo4j و ایجاد constraint/index.
- `types.rs`: DTOهای `FlowNode`، `FlowEdge`، `WalletFlowGraph` و path.
- `flow_graph.rs`: query ClickHouse، BFS، path search، metadata و projection orchestration.
- `nodes.rs`: Cypher مربوط به Wallet، Exchange، Entity و Cluster nodeها.
- `edges.rs`: Cypher مربوط به transfer و exchange interaction edgeها.

## پوشه `src/tasks`

- `mod.rs`: export taskها.
- `fetch_loop.rs`: ingest loop، Auto checkpoint و retry.
- `exposure_task.rs`: اجرای دوره‌ای یا هماهنگ‌شده exposure propagation.

## پوشه `src/utils`

- `mod.rs`: export utilityها.
- `tron_address.rs`: validate و normalize آدرس Base58/hex شبکه TRON.

## پوشه `web`

- `index.html`: HTML، CSS و JavaScript کامل dashboard، Wallet investigation، graph، holdings، fingerprint، path search و projection.

## پوشه `sql`

- `init_database_tron.sql`: منبع واحد ساخت database، table، Materialized View و Viewهای canonical.

---

# بخش دهم: جمع‌بندی آماده برای خواندن

اگر بخواهم کل پروژه را در دو دقیقه جمع‌بندی کنم، این متن را می‌خوانم:

> نقطه شروع ingest در `main.rs` است. از آنجا `run_tron_loop` اجرا می‌شود. این loop schema را validate می‌کند، checkpoint را از ClickHouse می‌خواند و با `LoaderTron` کلاینت TRON، ClickHouse و batcherها را می‌سازد. سپس `fetcher.rs` بلاک‌های solid را می‌گیرد. برای هر transaction، Receipt گرفته می‌شود تا اجرای واقعی، fee، Energy، logهای TRC20 و internal transfer مشخص شوند. `process_tx` یک transaction row، تعدادی relationship، semantic AML event و transaction feature می‌سازد. `GenericBatcher` این rowها را با `insert.write` و `insert.end` در جدول‌های ClickHouse ذخیره می‌کند. فقط بعد از flush موفق، بلاک ثبت و checkpoint جلو برده می‌شود.

> برای جست‌وجوی Wallet، JavaScript در `web/index.html:1189` endpoint unified investigation را صدا می‌زند. Router درخواست را به handler می‌دهد و handler تابع `build_wallet_investigation` را اجرا می‌کند. این service به‌صورت موازی graph، holdings، fingerprint، activity، intelligence و exposure را می‌سازد. Graph query از `address_relationships_canonical` شروع می‌شود و transaction و exchange evidence را join می‌کند. BFS همسایه‌ها را تا depth انتخاب‌شده گسترش می‌دهد. نتیجه به‌صورت `FlowNode` و `FlowEdge` در JSON برمی‌گردد. تابع `renderGraph` در browser مختصات nodeها را محاسبه می‌کند و در خط 1760 با `svg.innerHTML` نمودار را رسم می‌کند.

> Search عادی Neo4j را تغییر نمی‌دهد، چون handler مقدار `None` را به graph builder می‌دهد. وقتی دکمه import زده شود، handler مقدار `Some(&neo4j)` می‌دهد. در این حالت `flow_graph.rs` برای nodeها `upsert_wallet_with_metadata` و برای edgeها `merge_transfer_edge` را اجرا می‌کند. Cypher node در `nodes.rs` و Cypher relationship در `edges.rs` ساخته می‌شوند. write واقعی با `neo4j.graph.run(q).await` انجام می‌شود. بنابراین ClickHouse منبع حقیقت، Neo4j projection قابل بازسازی، و UI مصرف‌کننده JSON گراف است.

## چهار خطی که حتماً باید به خاطر داشته باشم

1. ClickHouse graph read: `flow_graph.rs:595` و `flow_graph.rs:634-642`.
2. UI request: `web/index.html:1189`.
3. UI graph draw: `web/index.html:1760`.
4. Neo4j edge write: `flow_graph.rs:204` سپس `edges.rs:102-105`.
