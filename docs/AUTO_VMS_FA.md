# ساخت خودکار چهار VM روی یک کامپیوتر

این مسیر با مسیر «چهار VM آماده» فرق دارد. از این به بعد خود پروژه با ابزار Multipass چهار VM واقعی Ubuntu 24.04 می‌سازد، Docker را داخل آن‌ها نصب می‌کند، فایل‌های اجرای هر نقش را منتقل می‌کند و سرویس‌ها را روشن می‌کند. ایجاد دستی VM یا تایپ IP و متغیرهای ترمینال لازم نیست.

## یک بار روی کامپیوتر میزبان

- Windows 11 Pro با Hyper-V فعال یا Linux سازگار با Multipass و مجازی‌سازی سخت‌افزاری لازم است.
- Python نسخه 3.10 یا بالاتر و [Multipass رسمی](https://canonical.com/multipass/install) را نصب کنید. این پیش‌نیاز میزبان است، نه ساخت دستی VM.
- روی ویندوز Docker Desktop باید در حالت Linux containers باشد. روی Linux از Docker Engine و Compose v2 استفاده کنید.
- اگر بسته کامل imageها را با گزینه `--bundle` بدهید، Docker روی میزبان لازم نیست؛ در VMها خودکار نصب می‌شود.
- نصب hypervisor یا reboot ویندوز توسط اسکریپت انجام نمی‌شود؛ این تغییرها به دسترسی مدیر سیستم نیاز دارند.
- برای ساخت اولیه، اینترنت جهت دانلود Ubuntu و بسته‌های Docker لازم است؛ image bundle این نصب را کاملاً آفلاین نمی‌کند.

## اجرای ویندوز

PowerShell را باز کنید. تمام فرمان‌ها روی خود کامپیوتر میزبان اجرا می‌شوند، نه داخل VM:

```powershell
cd D:\Sarbazi\AML_Whole
python scripts/provision_vms.py doctor
python scripts/provision_vms.py up
```

`doctor` فقط بررسی می‌کند و هیچ VM نمی‌سازد. `up` همان بررسی را تکرار می‌کند و در صورت مناسب بودن شرایط، ساخت و اجرای چهار VM را انجام می‌دهد.
imageهای فعلی برنامه باید در Docker موجود باشند؛ روی سیستمی که هنوز image ندارد یا بعد از تغییر کد، از این دستور استفاده کنید:

```powershell
python scripts/provision_vms.py up --build
```

ساخت اولیه Rust ممکن است طولانی باشد و اینترنت و فضای آزاد بیشتری بخواهد. اجرای عادی بدون `--build` همان imageهای موجود را استفاده می‌کند، نه الزاماً آخرین تغییرات سورس.

## اجرای Linux

پس از نصب Python 3 و Multipass و آماده کردن Docker:

```bash
cd /path/to/AML_Whole
python3 scripts/provision_vms.py doctor
python3 scripts/provision_vms.py up
```

این همان ابزار است؛ هیچ فایل PowerShell داخل VMها اجرا نمی‌شود.

## چه چیزی خودکار اتفاق می‌افتد؟

1. بررسی فایل‌های env، رمزهای لازم، ابزارها، فضای آزاد و RAM.
2. آماده‌سازی و checksum گرفتن از هفت image؛ اگر بسته تحویل داده باشید، صحت همان بسته بررسی می‌شود.
3. ساخت `aml-auto-main`، `aml-auto-tron`، `aml-auto-ethereum` و `aml-auto-bsc`.
4. نصب Docker و Compose از بسته‌های Ubuntu با cloud-init و انتظار تا پایان نصب.
5. دریافت IP واقعی هر VM از شبکه Multipass؛ آدرس‌های نمونه 10.20.0.x در این مسیر استفاده نمی‌شوند.
6. ساخت نسخه مخصوص VM از envهای موجود، بدون تغییر فایل‌های اصلی پروژه؛ رمزها و RPC از همان فایل‌های اصلی می‌آیند.
7. انتقال فقط imageها و فایل‌های اجرای همان نقش؛ سورس کامل، target، git، بکاپ خصوصی و اطلاعات دیتابیس کپی نمی‌شوند.
8. اجرای APIها و ClickHouse در VMهای شبکه، سپس اجرای وب، Analytical Node و تنها Neo4j در VM اصلی.
9. بررسی سلامت سرویس‌ها و اتصال از VM اصلی به هر سه API؛ در پایان URL صفحه اصلی چاپ می‌شود، مثلاً `http://192.168.64.10:8080`.

آدرس چاپ‌شده را روی مرورگر همین کامپیوتر باز کنید؛ برای این روش از `localhost:8080` یا IPهای نمونه قبلی استفاده نکنید.
دسترسی از کامپیوترهای دیگر شبکه ممکن است به تنظیم routing/firewall میزبان نیاز داشته باشد؛ شبکه پیش‌فرض Multipass، پل شبکه شرکت نیست.

## منابع و فضای ذخیره‌سازی

تنظیمات در `deployment/vms.json` است: VM اصلی 4 GiB RAM و 30 GiB دیسک، هر شبکه 3 GiB RAM و 50 GiB دیسک.
جمع RAM اختصاص‌یافته 13 GiB و سقف دیسک‌ها 180 GiB است؛ حداقل 14 GiB RAM آزاد هنگام ساخت همه VMها بررسی می‌شود. برای میزبان 24 تا 32 GiB RAM مناسب‌تر است.

ابزار هنگام ساخت اولیه به شکل محافظه‌کارانه 190 GiB فضای آزاد روی درایو ذخیره VMها و حداقل 5 GiB روی درایو پروژه درخواست می‌کند؛ برای build این حد دوم 20 GiB است. ظرفیت دیسک مجازی سقف قابل رشد است، نه دانلود فوری 180 GiB.
این ظرفیت‌ها برای راه‌اندازی و تست‌اند، نه نگهداری تاریخچه کامل سه شبکه.

محل پیش‌فرض بررسی دیسک: درایو سیستم ویندوز یا `/var/snap/multipass/common` در Linux.
اگر محل نگهداری Multipass را قبلاً تغییر داده‌اید، مسیر واقعی را بدهید:

```powershell
python scripts/provision_vms.py up --storage-path E:\MultipassData
```

این گزینه فقط فضای مسیر را بررسی می‌کند؛ محل ذخیره Multipass را جابه‌جا نمی‌کند.
[راهنمای رسمی تغییر محل ذخیره](https://canonical.com/multipass/docs/latest/how-to-guides/customise-multipass/configure-where-multipass-stores-external-data/).
بعد از ساخت VMها، تغییر سایز یا نام در فایل تنظیمات خودکار اعمال نمی‌شود؛ ابزار برای جلوگیری از تداخل توقف می‌کند. تغییر ظرفیت VMهای موجود باید جداگانه مدیریت شود.

## انتقال به سیستم کارفرما

پروژه را همراه چهار env آماده و بسته هفت-image منتقل کنید. بسته قبلیِ پنج-image که فاقد Analytical Node و BSC است کافی نیست.
روی ماشین سازنده از ابزار موجود `bash scripts/export-images.sh --include-bsc` استفاده کنید.
همچنین پس از اجرای موفق سازنده جدید، پوشه `.local-vms/images` دارای manifest و imageهای موردنیاز است و می‌تواند بسته image تحویل باشد؛ فقط همین زیرپوشه را منتقل کنید، نه کل state.

روی مقصد، پس از نصب Python و Multipass:

```powershell
python scripts/provision_vms.py up --bundle D:\AML-Images
```

نیازی به import دستی imageها، ساخت VMها یا نصب Rust نیست. سیستم مقصد VMهای تازه می‌سازد؛ داده ClickHouse و Neo4j سیستم قبلی منتقل نمی‌شود.
برای گرفتن پروژه از Git مطمئن شوید چهار env واقعاً در نسخه تحویلی موجودند؛ نام فایل‌ها در README آمده است.

## شروع دریافت بلاک‌ها

اجرای عادی فقط API، دیتابیس و صفحه وب را روشن می‌کند؛ با ClickHouse خالی، نمودار واقعی کیف پول هنوز داده ندارد.
شروع ingestion باید صریح باشد:

```powershell
python scripts/provision_vms.py up --with-ingestion
```

اگر از بسته استفاده کرده‌اید، `--bundle` را هم در این فرمان بیاورید.
دریافت از checkpoint موجود ادامه پیدا می‌کند؛ در دیتابیس خالی و env فعلی از بلاک صفر است و می‌تواند زمان و فضای زیادی مصرف کند.
فعال بودن ingestion در state ثبت می‌شود؛ اجرای بعدی up بدون فلگ آن را خاموش نمی‌کند. برای توقف کل سامانه از stop استفاده کنید.
این VMها برنامه AML را اجرا می‌کنند، نه full node ترون، Reth یا BSC؛ منبع دریافت همان RPC موجود در env است.

## وضعیت، توقف و اجرای دوباره

```powershell
python scripts/provision_vms.py status
python scripts/provision_vms.py check
python scripts/provision_vms.py stop
python scripts/provision_vms.py up
```

`stop` فقط VMهای متعلق به همین استقرار را خاموش می‌کند؛ دیسک و دیتابیس حذف نمی‌شوند.
`up` تکراری همان VMها را استفاده می‌کند و تنظیم IPها را تازه می‌کند؛ VM تکراری نمی‌سازد.
پس از reboot میزبان، اگر DHCP آدرس‌ها را تغییر داده باشد، up را اجرا کنید.
برای دیدن لاگ ترون:

```powershell
multipass exec aml-auto-tron -- sudo bash /home/ubuntu/AML_Whole/scripts/vm.sh tron logs
```

فایل `.local-vms/state.json` شناسه مالکیت VMها را نگه می‌دارد. این پوشه خصوصی و خارج از Git است؛ آن را هنگام کار با همین VMها حذف نکنید.
اگر کار قطع شد، دوباره up بزنید. بعد از قطع اجباری خود process، ممکن است `operation.lock` بماند؛ فقط پس از اطمینان از نبود process فعال آن فایل قفل را حذف کنید.
هیچ دستور حذف VM، purge یا پاک‌سازی دیتابیس در سازنده وجود ندارد. تغییر رمز دیتابیس بعد از ساخت، بدون فرآیند تغییر رمز داخل دیتابیس پذیرفته نمی‌شود.

اگر خود نصب اولیه Ubuntu/cloud-init خطا بدهد، up آن خطا را پنهان نمی‌کند. با `multipass exec NAME -- sudo cloud-init status --long` وضعیت و با `multipass exec NAME -- sudo tail -n 100 /var/log/cloud-init-output.log` علت را ببینید؛ نصب ناموفق باید قبل از ادامه رفع شود.

## محدودیت و امنیت

این مسیر نصب آزمایشی قابل تکرار است، نه تایید نهایی production. نسخه فعلی auth صفحه وب را خاموش می‌خواهد و از رمزهای فعلی env استفاده می‌کند؛ در اینترنت عمومی منتشر نکنید.
پورت دیتابیس‌ها localhost است. برای پورت API هر شبکه یک قانون اختصاصی DOCKER-USER نصب می‌شود تا فقط VM اصلی به آن دسترسی داشته باشد؛ قواعد دیگر firewall پاک نمی‌شوند.
برای production باید احراز هویت تحلیل‌گر، TLS/VPN، رمزهای خصوصی، backup/restore، مانیتورینگ و ظرفیت واقعی تاریخچه شبکه جداگانه نهایی شوند.
`check` آمادگی APIها و اتصال VMها را تایید می‌کند، نه کامل بودن داده‌های بلاک یا صحت تحلیل یک کیف پول خاص.

تست خودکار سازنده بدون ساخت VM:
```powershell
python -m unittest discover -s scripts/tests -p test_provision_vms.py -v
```

مستندات پایه: [Multipass launch](https://canonical.com/multipass/docs/latest/reference/command-line-interface/launch/)،
[انتقال فایل](https://canonical.com/multipass/docs/latest/reference/command-line-interface/transfer/)،
[بسته Docker Compose در Ubuntu 24.04](https://packages.ubuntu.com/noble/docker-compose-v2).
