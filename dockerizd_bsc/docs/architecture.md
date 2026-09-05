# BSC AML Architecture

## 1. اصل معماری

BSC یک شبکه EVM است. بنابراین لایه دریافت block، transaction، receipt، log، trace و آدرس باید از
الگوی Ethereum استفاده کند. خروجی AML، مدل داده، investigation و graph باید با قابلیت‌های TRON هم‌سطح باشد.

```text
BSC node / temporary RPC
        |
        v
RPC capability probe
        |
        v
Finalized block fetcher -> receipt/log/trace extractor -> canonical normalizer
        |                                                    |
        |                                                    v
        |                                         semantic event producers
        v                                                    |
ClickHouse: source of truth <--------------------------------+
        |
        +-> wallet investigation / holdings / fingerprint / path search
        |                                      |
        |                                      v
        +-------------------------------> Neo4j projection
                                               |
                                               v
                                   graph and evidence UI
```

ClickHouse مرجع شواهد است. Neo4j دیتابیس اصلی نیست و باید از canonical relationships قابل بازسازی باشد.
در آینده node شخصی BSC جای RPC موقت را می‌گیرد، بدون آن‌که schema یا domain contract تغییر کند.

## 2. مرزبندی سرویس‌ها

ساختار هدف فقط هنگام پیاده‌سازی هر بخش ایجاد می‌شود:

```text
dockerizd_bsc/
  Cargo.toml
  Cargo.lock
  Dockerfile
  docker-compose.yml
  .env.example
  src/
    bin/                 schema, probe, range/auto/follow, replay, repair and benchmark entrypoints
    config.rs            validated BSC-only configuration
    domain/              network-qualified address and asset identifiers
    ingestion/           RPC fetch, normalization, transfer extraction and ClickHouse commit
      model.rs           strict RPC evidence normalization and UInt256 handling
      rpc.rs             concurrent bounded receipt/callTracer retrieval, round-robin and failover
      transfers.rs       native/ERC-20/ERC-721/ERC-1155/internal edge decoder
      store.rs           revision-gated writer, cursor, tombstone, gap/failure and benchmark storage
      service.rs         range/auto/follow coordinator, ordered commit, reorg/replay/repair
    runtime.rs           shared schema and all-endpoint startup validation
    rpc.rs               startup capability and chain-id probe
    db/                  immutable migration runner and schema validation
  sql/
    migrations/          immutable, checksummed migrations
  tests/
    fixtures/            sanitized RPC golden fixtures
  docs/
```

پوشه‌های graph، investigation، intelligence و web فقط در فاز مربوط به خودشان و همراه implementation و
test واقعی ایجاد می‌شوند.

Binaryها فقط وقتی اضافه می‌شوند که implementation، config، health check و test داشته باشند.

## 3. قرارداد هویت و asset

- Network: `eip155:56`
- Address key: `eip155:56:<lowercase_address>`
- Native BNB: `eip155:56/native:bnb`
- Fungible token: `eip155:56/erc20:<lowercase_contract>`
- NFT: `eip155:56/erc721:<lowercase_contract>/<token_id>`
- Multi-token: `eip155:56/erc1155:<lowercase_contract>/<token_id>`

استانداردهای قراردادی BSC عموماً event signatureهای EVM را به کار می‌برند. در لایه داخلی از identifierهای
EVM موجود استفاده می‌کنیم تا query چندشبکه‌ای یکسان بماند؛ UI می‌تواند آن‌ها را با نام BEP-20 و موارد مشابه نشان دهد.

همه آدرس‌ها قبل از storage normalize می‌شوند. آدرس یکسان روی Ethereum و BSC دو subject متفاوت است.

## 4. finality، reorg و idempotency

منبع production باید tag `finalized` را پشتیبانی کند. BSC RPC رسمی این tag و endpointهای finalized را تعریف می‌کند.
حالت confirmation-depth فقط برای development مجاز است و data coverage باید آن را صریحاً non-finalized نشان دهد.

هر block با `block_number`, `block_hash`, `parent_hash` و وضعیت completeness ثبت می‌شود. facts شامل block hash هستند.
canonical view فقط facts متعلق به canonical block hash را برمی‌گرداند. در reorg، row جدید append می‌شود و facts قدیمی
از view حذف منطقی می‌شوند؛ mutation حجیم ClickHouse در مسیر ingest استفاده نمی‌شود.

از migration `20260904_0003` هر fact بلاکی علاوه بر hash دارای `block_state_revision` است. writer ابتدا
transaction/log/relationship و token discovery را با revision جدید flush می‌کند و marker `ingested_blocks` را آخر
می‌نویسد. view فقط factهایی را می‌پذیرد که hash و revision هر دو با marker کامل برابر باشند؛ بنابراین crash وسط write
یا replay همان hash داده ناقص را visible نمی‌کند.

Cursor تنها بعد از تکمیل transaction، receipt، log، trace موردنیاز و flush موفق همه batchها جلو می‌رود.
Restart باید idempotent باشد و block ناقص را replay کند. trace در production `required` است؛ نبود آن failure قابل مشاهده است،
نه موفقیت ناقص. برای rollout موقت می‌توان coverage ناقص را ذخیره کرد، اما UI باید محدودیت را نمایش دهد.

Historical fetch در chunkهای bounded موازی است، ولی parent-hash validation، block marker و cursor به ترتیب ارتفاع
انجام می‌شوند. در startup، hash آخرین checkpoint با marker محلی و RPC مقایسه می‌شود. اختلاف، جست‌وجوی common
ancestor را تا `BSC_REORG_MAX_DEPTH` فعال می‌کند؛ revisionهای بعدی با marker `canonical=0/status=orphaned`
نامرئی و cursor به ancestor بازگردانده می‌شود. حذف فیزیکی fact در مسیر repair وجود ندارد.

Failureهای retryable پس از `BSC_FAILURE_MAX_ATTEMPTS` به `dead` می‌روند. gap repair آن‌ها را خودکار اجرا نمی‌کند؛
operator باید `--include-dead` بدهد تا transition صریح requeue و سپس resolved ثبت شود. endpointهای fallback همگی
در startup chain/capability validation می‌شوند و load اولیه round-robin با failover درخواست retryable انجام می‌شود.

برای backfill تاریخی internal transferها، منبع RPC باید trace تاریخی همان range را ارائه دهد؛ معمولاً این کار به node
archive-capable یا provider معادل نیاز دارد. پس از catch-up و اعتبارسنجی کامل، می‌توان node را با retention محدودتر نگه داشت،
چون transfer edge و trace-derived evidence در ClickHouse باقی می‌مانند. raw trace JSON ذخیره نمی‌شود. اگر بعداً decoder
وابسته به trace تغییر اساسی کند، بازپردازش تاریخ قدیمی بدون trace source تاریخی ممکن نیست و این محدودیت باید در runbook ثبت شود.

## 5. داده‌ای که ذخیره می‌کنیم

### Canonical evidence

- block identity، timestamp، parent و completeness؛
- transaction envelope، sender/receiver، value، input، status و fee/gas evidence؛
- EVM logs با address، topics و data برای semantic replay؛
- native، internal، ERC-20، ERC-721 و ERC-1155 transfer edges؛
- transaction features و semantic AML events؛
- token metadata؛ holdings در Phase 7 از canonical relationshipها مشتق می‌شود و جدول balance تکراری ساخته نمی‌شود؛
- sync state، failures و ingestion benchmarks.

Receipt جداگانه ذخیره نمی‌شود اگر تمام فیلدهای مورد استفاده آن در transaction و logs پوشش داده شده باشند.
raw block JSON، responseهای تکراری RPC، state trie، bytecode همه contractها و mempool history در ClickHouse ذخیره نمی‌شوند.

### Intelligence and analysis

- source و review history برچسب‌ها؛
- entity/address mapping و cluster evidence؛
- exposure run و explainable paths؛
- wallet analysis snapshots فقط پس از تثبیت قرارداد investigation.

در این فاز جدول ML، model registry، prediction یا risk probability ساخته نمی‌شود.

## 6. semantic evidence خاص BSC

Decoder فقط به نام method تکیه نمی‌کند. evidence ترکیبی از protocol registry تأییدشده، contract address، event topics،
token movements، call traces و جهت وجوه است. دسته‌ها شامل swap، bridge، liquidity add/remove، mint/burn، lending،
staking، mixer/privacy و system transaction هستند.

رجیستری append-only است. `protocol_contract_registry_current` آخرین تصمیم هر آدرس و view فعال فقط رکورد
`approved + enabled` را ارائه می‌کند. classifier برای هر بلاک یک snapshot از رجیستری cache می‌کند، آن را در بازه
`BSC_PROTOCOL_REGISTRY_REFRESH_BLOCKS` تازه می‌کند و event/feature را پیش از complete marker با همان revision می‌نویسد.
بنابراین قطع process یا replay نمی‌تواند fact نیمه‌کاره یا event تکراری در view canonical ایجاد کند.

system contract و validator operation باید از رفتار عادی wallet تفکیک شوند تا fingerprint را آلوده نکنند، ولی evidence
لازم برای audit حذف نمی‌شود. registryهای protocol و entity versioned و reviewable هستند؛ نام protocol داخل decoder hardcode نمی‌شود.

Bridge event باید حداقل این فیلدها را تولید کند:

```text
event_id, protocol, network_id, remote_network_id, tx_hash, subject_address,
remote_receiver, bridge_message_id, correlation_key, asset_in, asset_out,
amount_in, amount_out, confidence, evidence_refs
```

این contract به correlator چندشبکه‌ای آینده اجازه می‌دهد مسیر را بعد از bridge ادامه دهد. BSC به تنهایی نباید ادعا کند
که leg مقصد را پیدا کرده است؛ correlation نیازمند داده ingestشده شبکه مقصد است.

تمام confidenceهای این لایه confidence کیفیت evidence هستند. هیچ‌کدام risk score یا احتمال پول‌شویی نیستند و
تعامل با آدرس scam/mixer تأییدشده نیز صرفاً یک fact تحقیقی است، نه نتیجه حقوقی درباره wallet.

## 7. graph و investigation

درخواست wallet ابتدا ClickHouse را query می‌کند، subgraph محدود را می‌سازد و سپس همان evidence را با network-qualified
node IDs به Neo4j project می‌کند. source-to-target search سقف 10 hop دارد و باید direction، path limit، per-address limit،
زمان اجرا، expanded node count و `truncated` را برگرداند.

خروجی wallet شامل graph، holdings، fingerprint، trend، counterparties، swaps، bridges، entity evidence، clusters،
exposure paths و data quality است. risk probability فعلاً وجود ندارد. نبود داده، نبود trace و truncate شدن نتیجه سه حالت جدا هستند.

## 8. مرز production

- API، ClickHouse و Neo4j در production عمومی نمی‌شوند؛ فقط gateway احرازهویت‌شده در دسترس است.
- secretها فقط از environment/secret manager خوانده و هیچ‌گاه log نمی‌شوند.
- containerها non-root، read-only، بدون capability و با health/readiness واقعی اجرا می‌شوند.
- access log نباید wallet address کامل یا RPC credential را ثبت کند.
- metrics شامل finalized lag، gap count، retry، failed block، receipt/trace coverage، batch size، query latency و truncation است.
- backup/restore ClickHouse و rebuild Neo4j قبل از release آزمایش می‌شود.
