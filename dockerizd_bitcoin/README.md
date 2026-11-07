# Bitcoin AML

adapter مستقل Bitcoin mainnet در سامانهٔ چهار شبکه. ClickHouse محلی از
همان قرارداد مشترک استفاده می‌کند و Neo4j فقط روی Main است. هدف پروژه
محدود به Bitcoin نیست؛ [برنامهٔ واحد](../TODO_LIST.md) مرجع ادامه است.

قابلیت‌های موجود: دریافت بلاک و resolve prevout، checkpoint و reorg،
failure/repair، تحقیق UTXO، graph/path، activity، holdings، intelligence
دارای review، کاندیدای cluster، exposure و ریسک توضیح‌پذیر. worker تحلیل
بازه‌های محدود را تازه‌سازی می‌کند؛ پوشش تمام تاریخچه یا همهٔ آدرسها را
تضمین نمی‌کند. پذیرش RPC/VM واقعی و parity نهایی محصول باقی است.

## اجرا

از ریشهٔ مخزن، بعد از تنظیم `dockerizd_bitcoin/.env`:

```bash
bash scripts/vm.sh bitcoin up --build
bash scripts/vm.sh bitcoin check-schema
bash scripts/vm.sh bitcoin probe
```

RPC باید mainnet و unpruned و قادر به تأمین prevout موردنیاز باشد. بازهٔ
شروع را صریح انتخاب کنید؛ start=0 یعنی دریافت از genesis. کلید API باید
با Main یکی باشد. تنظیمات کامل در [.env.example](.env.example) و
[راهنمای VM](../docs/VM_SETUP_FA.md) است.

نصب تازه فقط `aml_*` می‌سازد. backfill از دیتابیس قدیمی و شرط readiness
وابسته به آن حذف شده است؛ ingest/replay بلاک و repair از نود حفظ شده‌اند.
[SQL](sql/README.md) از قرارداد مشترک تولید می‌شود.

ساختار source: `src/bitcoin` برای RPC/ingest/prevout/repair، `src/domain`
برای مدل، `src/db` برای client/schema/query، `src/storage` برای writer و
lock، و ماژول‌های analytics/intelligence/clustering/exposure/risk برای تحلیل.

راهنمای قابلیت‌ها: [intelligence](INTELLIGENCE.md)، [clustering](CLUSTERING.md)،
[exposure](EXPOSURE.md)، [risk](RISK.md)، [activity](ACTIVITY.md)،
[semantics](SEMANTICS.md)، [holdings](HOLDINGS.md)، [analytics](ANALYTICS.md).
تست‌ها: [TESTING](../docs/TESTING.md). استفاده: [USAGE](../docs/USAGE_FA.md).
