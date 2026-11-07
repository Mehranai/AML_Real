# TRON AML

کد Rust در `app` است. VM ترون شامل API، ingestor، metadata/analytics
workerها و ClickHouse مستقل است؛ Neo4j تحقیق روی VM اصلی قرار دارد.
نوع قرارداد، TRX/TRC10/TRC20، receipt/internal و classifierهای بومی در کد
موجودند؛ پوشش کامل قرارداد/NFT و رفتار چهار شبکه هنوز باید پذیرفته شود.

**شکاف باقی‌مانده:** writer و checkpoint ترون مشترک شده‌اند؛ core readerها
projection مشترک دارند و metadata/intelligence/exposure نیز به همان قرارداد
متصل شده‌اند. SQL بومی و mount آن حذف شده‌اند؛ job `tron-schema` پیش از API/workerها
قرارداد مشترک را نصب می‌کند. [SQL مشترک](../shared/warehouse/sql/README.md).
[TODO_LIST](../TODO_LIST.md)، [وضعیت واقعی](../docs/PROJECT_STATUS_FA.md).

از ریشهٔ مخزن، بعد از تنظیم `dockerizd_tron/app/.env`:

```bash
bash scripts/vm.sh tron up --build
bash scripts/vm.sh tron check-schema
bash scripts/vm.sh tron check-runtime
```

RPC/key، بازهٔ شروع، رمز DB و service key را از
[نمونه](app/.env.example) تنظیم کنید. `nodee/Tron` کد نود اختیاری قبلی است؛
اجرای backend آن را خودکار راه‌اندازی نمی‌کند و منابع full node باید جدا
تعیین شوند. [VM](../docs/VM_SETUP_FA.md)، [استفاده](../docs/USAGE_FA.md)،
[schema مشترک](../docs/warehouse/README_FA.md)، [تست](../docs/TESTING.md).
