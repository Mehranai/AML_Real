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

گام API-1 نیز پذیرفته شد: ورودی و خطای investigation/paths/holdings چهار
شبکه قرارداد مشترک دارند؛ پارامتر نامعتبر صریح رد می‌شود. API-2 نیز مسیر
`transfers` را با فیلترهای مشترک و صفحه‌بندی مقید به snapshot در هر چهار VM
متصل کرد. API-3 نیز پاسخ تحقیق/مسیر را با `chain_response_v1` و مدل موجودی
صریح یکدست کرد؛ حفظ مبلغ، NFT و شاهد UTXO در ذخیره/بازگشایی/Export با
Neo4j واقعی پذیرفته شد. پنل شواهد و تازگی منبع را نشان می‌دهد؛ تغییر منبع
ریسک جاری را UNKNOWN می‌کند. پذیرش پیوستهٔ RPC تا مرکز روی VMها باقی است.

E2E-1 نیز در ۲۰۲۶-۱۱-۰۸ با دادهٔ محدود RPC عمومی واقعی تکمیل شد: چهار
ClickHouse مستقل، چهار API واقعی، Gateway، Analytical Node، Neo4j و مرورگر
واقعی برای Wallet و Paths هر چهار شبکه اجرا شدند. مقدار انتقال با RPC و
شواهد ذخیره‌شده پس از reopen/Export تطبیق داده شد. این تأیید ظرفیت، archive
کامل یا پنج VM کاربر نیست و در E2E-2 باقی می‌ماند.

ING-1 نیز دریافت پیوسته و بازیابی اتصال را روی چهار ClickHouse مستقل با
RPC نمونه پذیرفت: حفظ checkpoint و prefix، دریافت بلاک تازه پس از قطعی،
کنترل دوبارهٔ هویت منبع و رعایت dead-letter؛ ۱۷ سناریوی یکپارچه موفق‌اند.
ING-2 فرمان‌های probe/failures/scan-gaps/replay/repair را در launcher هر چهار
VM متصل کرد؛ بازپردازش محدود از همان ingestor و قفل writer استفاده می‌کند.
probe دسترسی نمونه به start/head را می‌سنجد؛ پوشش کامل archive و پذیرش طولانی
RPC/VM همچنان باز است. [روش عملیات و محدودیت‌ها](docs/USAGE_FA.md).

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
