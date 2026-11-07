# AML چهار شبکه

بک‌اند Rust برای تحقیق و پایش پول‌شویی در **TRON، Ethereum، BSC و Bitcoin**.
چهار VM شبکه، هرکدام با ClickHouse مستقل و قرارداد دادهٔ مشترک؛ یک VM اصلی
برای پنل، سرویس مرکزی و Neo4j. دیتابیس استقرار از صفر شروع می‌شود.

**وضعیت: هنوز آمادهٔ تحویل کامل و هم‌سطح Chainalysis نیست.** writer بلاک
و checkpoint هر چهار شبکه از قرارداد مشترک استفاده می‌کنند. رکوردهای عملیاتی
metadata/intelligence/exposure ترون نیز به همان جدول‌ها متصل شده‌اند؛
فایل‌ها و mountهای DDL قبلی حذف شده‌اند و نصب از صفر روی چهار ClickHouse
مستقل، شکل یکسان را تأیید کرده است. پذیرش مسیر داده در سطح کد با ۱۳ سناریوی
ClickHouse واقعی و RPC نمونه موفق شد. آزمون محدود RPC عمومی واقعی نیز روی
۵ بلاک و ۸۷۹۱ تراکنش، دریافت تا API شبکه و اجرای مجدد را تأیید کرد؛ trace و
state تاریخی عمومی محدودند. پذیرش کامل RPC و VM همچنان باقی است.

| مرجع | محتوا |
|---|---|
| [TODO_LIST](TODO_LIST.md) | تنها برنامهٔ ادامهٔ چهار شبکه، ترتیب و معیار پذیرش |
| [وضعیت و پاک‌سازی](docs/PROJECT_STATUS_FA.md) | آنچه واقعاً داریم، شکاف‌ها و دلیل حذف/حفظ فایل‌ها |
| [ساخت دستی VM](docs/VM_SETUP_FA.md) | منابع پایلوت، شبکه، Ubuntu/Docker، env و ترتیب اجرا |
| [استفاده](docs/USAGE_FA.md) | روشن/خاموش‌کردن، تحقیق، snapshot، Export و عملیات |
| [معماری](docs/BACKEND_ARCHITECTURE_FA.md) | نقش سرویس‌ها و جریان داده |
| [ClickHouse مشترک](docs/warehouse/README_FA.md) | قرارداد فعلی، جدول‌ها و تفاوت‌های شبکه‌ها |
| [آزمون‌ها](docs/TESTING.md) | بررسی کد و آزمون روی ClickHouse موقت |

## اجرا

پس از تنظیم env طبق راهنما، از ریشهٔ مخزن روی هر VM فقط role خودش را اجرا کنید:

```bash
bash scripts/vm.sh tron up --build
bash scripts/vm.sh ethereum up --build
bash scripts/vm.sh bsc up --build
bash scripts/vm.sh bitcoin up --build
# فرمان‌های بالا روی چهار VM جدا اجرا می‌شوند؛ سپس روی Main:
bash scripts/vm.sh main up --build
```

این فرمان‌ها VM نمی‌سازند. برای آزمایش اولیه `--api-only` بدهید؛ برای دریافت
بلاک و workerها آن را حذف کنید. RPC واقعی و رمزها لازم‌اند. جزئیات و وضعیت
فعلی همسان‌سازی را پیش از اجرا از [راهنما](docs/VM_SETUP_FA.md) بخوانید.

روی Docker Desktop ویندوز، helper محلی موجود است:

```powershell
python scripts/run_local.py up --build --with-ingestion
```

این helper محیط محلی را تنظیم می‌کند و جای استقرار پنج VM نیست. از volume
آزمایشی نسخه‌های حذف‌شده برای استقرار تازه استفاده نکنید. `.env`، کلیدها،
داده، image bundle و cacheها در Git قرار نمی‌گیرند؛ نمونه‌ها commit می‌شوند.

سندهای Word و بسته‌های تحویلی قبلی در `docs/deliverables` نگه‌داری شده‌اند؛
مرجع جاری وضعیت، مستندات بالا هستند.
