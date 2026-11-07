# BSC ClickHouse data mapping

منبع دقیق ستون، نوع، default، CHECK، index و view در
[قرارداد مشترک](../../docs/warehouse/current-contract.json) و شرح جدول‌ها در
[راهنمای مشترک](../../docs/warehouse/README_FA.md) است. این فایل copy مستقل
DDL نیست. DB شبکه `bsc_aml` و هویت آن `eip155:56` هستند.

| دادهٔ BSC | جدول مشترک |
|---|---|
| header، پوشش receipt/trace، complete و شمارش facts | `aml_ingested_blocks` |
| تراکنش، status، مبلغ/fee صحیح | `aml_transactions` |
| انتقال native/ERC20/ERC721/ERC1155/internal با موقعیت اصلی | `aml_address_relationships` |
| log اصلی، topic/data، log index | `aml_evm_logs` |
| callTracer اصلی، مسیر، موفقیت/خطا و value | `aml_evm_traces` |
| payload و receipt اصلی تراکنش | `aml_raw_transactions` |
| feature/classification و رخداد semantic با شاهد و detector version | `aml_transaction_features`، `aml_semantic_aml_events` |
| مشاهدهٔ توکن، metadata و retry job | `aml_metadata_discoveries`، `aml_asset_metadata`، `aml_metadata_jobs` |
| پروتکل، source، claim و review/revoke | `aml_protocol_registry`، `aml_intelligence_sources`، `aml_intelligence_claims`، `aml_intelligence_reviews` |
| cursor، failure/dead-letter و benchmark | `aml_sync_state`، `aml_ingestion_failures`، `aml_ingestion_benchmarks` |

مبلغ خام و token ID با float منتقل نمی‌شوند. NFT ID بخشی از هویت asset
است؛ ERC1155 batch موقعیت sub-index واقعی دارد. جزئیات gas/input/creation
در شاهد نرمال‌شدهٔ تراکنش و پاسخ اصلی RPC محفوظ‌اند و API با CTE می‌خواند.

جمع semantic چند انتقال ممکن است از UInt256 بزرگ‌تر شود. رشتهٔ دقیق آن
در شاهد خام و API بومی حفظ می‌شود؛ ستون عددی مشترک در این حالت NULL است
و علت در `evidence_json` ثبت می‌شود. مقدار اشباع‌شده یا wrapشده ساخته نمی‌شود.

لاگ/trace یا fact تخصصی نامرتبط برای شبکه‌ای دیگر تولید نمی‌شود. جداول
Bitcoin/TRON در BSC خالی‌اند. دادهٔ دریافت‌نشده با صفر مالی جایگزین نمی‌شود.
همهٔ facts بلاک به همان commit/revision تعلق دارند؛ view canonical فقط
نسخهٔ کاملِ زیر checkpoint منتشرشده را نمایش می‌دهد.

[نگاشت مستقیم RPC](../src/ingestion/mapping.rs)،
[انتشار/بازیابی](../src/ingestion/store.rs)،
[projectionهای API](../src/db/read.rs)،
[عملیات metadata/intelligence](../src/db/records.rs).
