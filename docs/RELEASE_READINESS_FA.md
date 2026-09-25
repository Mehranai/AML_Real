# چک‌لیست تحویل و چرخه واقعی داده
تاریخ بررسی: 2026-09-25

این فایل نتیجه بازبینی کد و آزمون‌های مشخص است، نه گواهی آماده‌بودن کامل برای production یا هم‌ارزی با Chainalysis. فضای زیاد سرور، نبود trace، برچسب معتبر یا تطبیق دو طرف bridge را جبران نمی‌کند.

## ۱. آیا اول سه ستون و بعد تمام ستون‌های دیگر پر می‌شوند؟
خیر. ستون، جدول و مرحله پردازش سه مفهوم متفاوت‌اند. در پروژه قرارداد «پایان ingest سپس پرشدن همه جدول‌ها» وجود ندارد.

مسیر واقعی:

```text
RPC/node -> block + receipt [+ EVM trace]
         -> decode transfers / supported protocol events
         -> raw ClickHouse facts
         -> block completion journal + checkpoint
         -> canonical views
             -> background metadata / analytics
             -> wallet investigation on request
                 -> main Analytical Node / evidence risk
                 -> central Neo4j temporary snapshot
                 -> browser graph
                 -> explicit Export makes snapshot persistent
```

- ingest تاریخچه را جلو می‌برد و سپس در همان حلقه بلاک‌های جدید را دنبال می‌کند؛ لازم نیست برنامه دیگری را بعد از پایان اجرا کنید.
- تحلیل‌های پس‌زمینه می‌توانند هم‌زمان با ingest، داده ذخیره‌شده تا checkpoint را پردازش کنند.
- جستجوی ولت روی داده موجود اجرا می‌شود. تاریخی که هنوز دریافت نشده، در نمودار وجود ندارد.
- Neo4j مرکزی نسخه تحقیق را نگه می‌دارد؛ منبع اصلی تراکنش‌ها ClickHouse هر شبکه است.
- نمایش ارتباط چند آدرس به معنای اثبات عبور همان واحدهای پول از تمام واسطه‌ها نیست، خصوصاً در صرافی‌های تجمیعی، swap و bridge.

### producerها
| شبکه / زمان | چه کاری واقعاً اجرا می‌شود؟ |
| --- | --- |
| TRON / ingest | پنج batcher برای transactions، address_relationships، transaction_features، semantic_aml_events و token_metadata_discoveries؛ علاوه بر journal، failure و checkpoint |
| TRON / پس‌زمینه | token metadata worker و tron_analytics_worker؛ کشف ادعاهای PENDING و exposure به seedهای فعال |
| Ethereum / ingest | بلاک، تراکنش، receipt/log، انتقال ETH و توکن، NFT و رویدادهای decoderهای پشتیبانی‌شده؛ internal transfer فقط با trace قابل دسترس |
| Ethereum / پس‌زمینه | metadata و worker clustering/exposure؛ نبود seed با NO_SEEDS مشخص می‌شود |
| BSC / ingest | follow، receipt/log، انتقال و رویدادهای پشتیبانی‌شده، journal و epoch/canonical state؛ internal transfer وابسته به trace |
| BSC / پس‌زمینه و درخواست | metadata در worker؛ exposure هنگام investigation؛ cluster-candidates یک endpoint/ابزار جداست، نه worker سراسری |
| هر سه / داده هویتی | import منابع و برچسب‌های مستند، بررسی تحلیلگر و انتشار انتساب تأییدشده |
| هر سه / جستجوی ولت | گراف، مسیر، fingerprint، فعالیت، holdings و شواهد طبق پوشش و سقف query |
| VM اصلی / درخواست | ارزیابی غیر ML بر اساس شواهد برگشتی و ذخیره snapshot موقت در Neo4j |

### چرا بعضی جدول‌ها می‌توانند خالی باشند؟
- بدون رویداد bridge در تاریخچه پشتیبانی‌شده، انتظار ردیف bridge نداریم.
- بدون واردکردن و تأیید برچسب، نام معتبر صرافی از هیچ جا ساخته نمی‌شود.
- بدون seed پرریسک معتبر، نبود exposure به معنای نبود پول مشکوک نیست.
- نبود metadata نباید با decimals=0 یا موجودی صفر واقعی اشتباه شود.
- کاندیدای رفتاری، صرافی تأییدشده نیست؛ تشابه رفتاری مالکیت مشترک را ثابت نمی‌کند.
- بخش ML فعال نشده است. جدول‌ها/ابزارهای غیرفعال ML نباید برای ظاهراً کامل‌شدن دیتابیس داده ساختگی بگیرند.
- import، review، replay و benchmark ابزارهای عملیاتی هستند؛ اجرای خودکار همه فایل‌های src/bin در startup نادرست و گاهی مخرب است.

## ۲. اصلاحات انجام‌شده در این ادامه
- [x] canonical viewهای انتقال و تراکنش TRON فقط بلاک COMPLETE را مصرف می‌کنند.
- [x] semantic_aml_events_canonical اضافه شد و activity از آن می‌خواند.
- [x] محاسبه wallet_asset_balances فقط deltaهای بلاک کامل را می‌بیند؛ deltaهای خام حذف نمی‌شوند و پس از commit بدون reinsert قابل مشاهده‌اند.
- [x] ارتقای همین ordinary viewها در startup برای volume قدیمی انجام می‌شود؛ raw table، materialized view و داده‌های کاربر حذف نمی‌شوند.
- [x] تست واقعی بلاک COMPLETE/FAILED/PROCESSING/بدون journal، مسیر گراف، activity و balances اضافه و اجرا شد.
- [x] در Ethereum، investigation و risk هر دو فقط آخرین run را بررسی می‌کنند. اگر آخرین run ناموفق، RUNNING یا NO_SEEDS باشد، موفقیت قدیمی به عنوان نتیجه جاری مصرف نمی‌شود.
- [x] مسیرهای Ethereum که seed آن‌ها دیگر فعال/پرریسک نیست، از شواهد جاری حذف می‌شوند.
- [x] خطای واقعی worker تحلیل Ethereum دیگر با یک log و خواب تا چرخه روز بعد پنهان نمی‌ماند؛ پس از تأخیر کوتاه خروج خطادار دارد تا restart/monitoring آن را ببیند.
- [x] ساخت پیش‌فرض BSC آنلاین و از Cargo.lock شد؛ دیگر vendor-linux.tar.gz خارج از Git الزام ساخت عادی نیست. روش قبلی در Dockerfile.offline باقی است.
- [x] preflight VM دوباره مجموع RAM ماشین‌های باقی‌مانده و فضای ذخیره را کنترل می‌کند؛ مقدار ثابت دو گیگابایت حذف شد.
- [x] check-runtime وضعیت همه سرویس‌های لازم را بررسی می‌کند؛ استقرار کامل VM از آن استفاده می‌کند. check ساده همچنان برای API-only کاربرد دارد.
- [x] README با Neo4j مرکزی، پورت‌های سه شبکه و وضعیت واقعی BSC اصلاح شد.

در اجرای کامل Docker، restart از image جدید لازم است؛ صرف resume یا وجود فایل جدید روی میزبان، کد داخل image قدیمی را عوض نمی‌کند.

## ۳. شرط‌های باز برای تحویل واقعی
### مسدودکننده ادعای پوشش کامل
- [ ] RPC دارای trace برای Ethereum و BSC: در probe این بررسی، eth_chainId پاسخ داد ولی debug_traceBlockByNumber روی بلاک آزمایشی با -32601 پاسخ داده شد. حالت auto می‌تواند بدون internal transfer ادامه دهد. این موفقیت کامل ingest نیست. پیش از ادعای پوشش کامل، provider/node مناسب انتخاب، بازه‌های فاقد trace replay و پوشش دوباره بررسی شود. این probe به‌تنهایی قابلیت همه بازه‌های تاریخی provider را اندازه نمی‌گیرد.
- [ ] تاریخچه پیوسته واقعی: بالا بودن checkpoint، پیوستگی از genesis یا نبود حفره در گذشته را ثابت نمی‌کند. شماره آخر و پوشش receipt/trace و replay هر بازه باید بررسی شوند.
- [ ] داده هویتی عملیاتی: منبع معتبر، مجوز استفاده، آدرس‌های مستند صرافی/bridge/mixer، seedهای تأییدشده و مسئول review لازم است. وجود کد import به معنای وجود این داده‌ها نیست؛ موجودی واقعی برچسب‌های سرور مقصد هنوز تأیید نشده است.
- [ ] ردیابی میان‌زنجیره‌ای bridge: رویداد bridge یا correlation احتمالی، اثبات تطبیق تراکنش مقصد نیست. پیوند تأییدشده دو شبکه با message ID/شواهد پروتکل و reconciliation هنوز یک قابلیت جدا و ناتمام است.
- [ ] پوشش decoderها: فقط پروتکل‌ها/نسخه‌های پشتیبانی‌شده قابل تشخیص‌اند. وضعیت unknown باید صریح بماند؛ همه swapها و bridgeها خودکار شناخته نمی‌شوند.

### شرط‌های عملیاتی سرور
- [ ] ساخت سرد imageها و نصب چهار VM روی Linux مقصد، با بررسی معماری x86-64 و قابلیت virtualization واقعی میزبان؛ در این بررسی محلی انجام نشده است.
- [ ] فایل‌های تنظیمات: .env ریشه tracked است، اما .env شبکه‌ها در checkout فعلی به دلیل gitignoreهای داخلی همراه Git نیستند. چهار فایل پیکربندی را از مسیر خصوصی به همان مسیرهای سرور منتقل کنید؛ clone به‌تنهایی تنظیمات محلی را منتقل نمی‌کند. image bundle نیز شامل .env یا دیتابیس نیست.
- [ ] دسترسی امن UI: provisioner فعلی برای محیط خصوصی/آزمایش با Basic Auth خاموش طراحی شده است. آن را بدون لایه دسترسی احراز هویت‌شده و TLS روی اینترنت عمومی نگذارید. فعال‌کردن auth در مسیر استقرار خودکار هنوز باید تکمیل و تست شود.
- [ ] رمزها/کلیدهایی که قبلاً در مخزن Public قرار گرفته‌اند باید محرمانه فرض نشوند؛ حذف از فایل، دسترسی قبلی به آن‌ها را پس نمی‌گیرد. rotation باید برنامه‌ریزی شود، نه با تغییر ناگهانی رمز دیتابیس موجود.
- [ ] آزمون پیشرفت واقعی ingest در هر سه شبکه و restart بدون تکرار canonical؛ healthy بودن PID کافی نیست.
- [ ] آزمون انتها‌به‌انتها با ولت‌ها و تراکنش‌های شناخته‌شده: ClickHouse -> API شبکه -> VM اصلی -> Neo4j موقت -> Export -> بازیابی بعد از restart.
- [ ] آزمون توقف یک شبکه بدون توقف سایر شبکه‌ها و بررسی دیتابیس همان VM.
- [ ] backup و restore آزمایشی ClickHouse، Neo4j و state volumeها؛ برنامه rollback نسخه و schema.
- [ ] پایش lag، آخرین تحلیل موفق، خطای RPC، failure/replay backlog، تازگی metadata/labels و هشدار کمبود منابع؛ check-runtime جای این پایش را نمی‌گیرد.
- [ ] آزمون بار و اجرای پایدار روی حجم هدف. هزینه FINAL و joinهای journal در TRON و queryهای coverage/exposure روی تاریخچه بزرگ هنوز benchmark نشده است.
- [ ] نمایش freshness و پوشش تاریخچه در UI باید کامل‌تر شود؛ به‌ویژه گزارش TRON هنوز اثبات مستقل تاریخچه پیوسته از genesis ارائه نمی‌کند.
- [ ] بازتحلیل پس از تغییر برچسب‌های قدیمی و تست مرزهای صرافی/bridge در داده واقعی؛ اسکن بازه اخیر جای backfill کامل را نمی‌گیرد.

حد پیش‌فرض مسیر و حجم گراف تعمدی است. «تا ۱۰ hop» به معنای تمام مسیرهای ممکن یک گراف بزرگ بدون سقف زمان، یال یا تعداد مسیر نیست. محدودیت‌ها و نتیجه ناقص باید همراه نتیجه خوانده شوند.

## ۴. استفاده روی سرور
روی Linux میزبان، از ریشه پروژه، پس از نصب Python 3.10+، Multipass، Docker و Compose و انتقال خصوصی فایل‌های .env:

```bash
python3 scripts/provision_vms.py doctor --build
python3 scripts/provision_vms.py up --build --with-ingestion
python3 scripts/provision_vms.py check
python3 scripts/provision_vms.py status
```

این دستورات VMها را با مشخصات deployment/vms.json می‌سازند؛ اندازه‌های پیش‌فرض، ظرفیت آرشیو کامل شبکه‌ها نیستند. قبل از ساخت روی سرور، دیسک و RAM هر VM را متناسب با برنامه واقعی تعیین کنید. زیادبودن دیسک میزبان، دیسک مهمان را خودکار نامحدود نمی‌کند.

Multipass فایل‌های VM را در محل خودش می‌گذارد، نه لزوماً کنار پروژه. --storage-path فقط فضای محل واقعی ذخیره Multipass را بررسی می‌کند و فایل‌ها را جابه‌جا نمی‌کند. برای محل سفارشی، مسیر واقعی را بعد از پیکربندی خود Multipass به doctor و up بدهید.

روی کامپیوتر محدود محلی برای این بازبینی VM، sync تاریخچه یا build کامل imageها اجرا نشد؛ فقط یک ClickHouse آزمایشی با سقف 768 MiB و یک CPU استفاده شد.

### بررسی سرویس و دیتابیس بدون خاموشی VM
```bash
python3 scripts/provision_vms.py ps tron
python3 scripts/provision_vms.py logs tron
python3 scripts/provision_vms.py db tron --query "SELECT * FROM sync_state FINAL WHERE chain='tron' FORMAT Vertical"
python3 scripts/provision_vms.py db ethereum --query "SELECT * FROM sync_state FINAL WHERE network_id='eip155:1' FORMAT Vertical"
python3 scripts/provision_vms.py db bsc --query "SELECT * FROM sync_state_current WHERE network_id='eip155:56' FORMAT Vertical"

python3 scripts/provision_vms.py pause ethereum
python3 scripts/provision_vms.py db ethereum --query "SHOW TABLES"
python3 scripts/provision_vms.py resume ethereum
```

فرمان زیر داخل VM همان شبکه، از پوشه پروژه، API و workerها را کنترل می‌کند:

```bash
sudo bash scripts/vm.sh tron check-runtime
```

pause برنامه‌های همان شبکه را متوقف می‌کند و ClickHouse/VM را نگه می‌دارد؛ pause-ingestion فقط workerها را متوقف می‌کند. resume با image موجود و checkpoint قبلی ادامه می‌دهد.

برای بررسی داده TRON، روی viewهای canonical query بزنید. جدول خام ممکن است داده بلاک ناموفق داشته باشد و این برای عیب‌یابی نگه‌داری می‌شود:

```sql
SELECT * FROM address_relationships_canonical
WHERE from_address = 'WALLET_ADDRESS' OR to_address = 'WALLET_ADDRESS'
LIMIT 20 FORMAT Vertical;

SELECT * FROM semantic_aml_events_canonical
WHERE subject_address = 'WALLET_ADDRESS'
LIMIT 20 FORMAT Vertical;

SELECT table, sum(rows) AS physical_rows,
       formatReadableSize(sum(bytes_on_disk)) AS size
FROM system.parts
WHERE active AND database = currentDatabase()
GROUP BY table ORDER BY sum(bytes_on_disk) DESC;
```

physical_rows شمارش فیزیکی است و قبل از merge الزاماً تعداد تراکنش یکتا نیست. برای بررسی پیشرفت، checkpoint را با فاصله زمانی دوباره بخوانید؛ COUNT کل تاریخچه را دائماً اجرا نکنید.

### ساخت BSC
ساخت عادی آنلاین، از طریق --build در launcher، نیازمند دسترسی سرور ساخت به registry imageها و crateهاست و Cargo.lock را رعایت می‌کند.

ساخت جایگزین با وابستگی vendored، از پوشه dockerizd_bsc:

```bash
bash scripts/refresh-linux-vendor.sh
docker build -f Dockerfile.offline -t bsc-aml-service:local .
```

این روش همچنان imageهای پایه را لازم دارد؛ برای سرور بدون اینترنت، image bundle آماده و بررسی‌شده منتقل کنید. archive وابستگی تولیدی را داخل Git قرار ندهید.

## ۵. تفسیر خروجی ریسک
ML فعال نشده است. امتیاز فعلی یک سیاست نسخه‌دار بر اساس شواهد، انتساب‌های تأییدشده، exposure جهت‌دار و الگوهای پشتیبانی‌شده است؛ احتمال آماری ارتکاب پول‌شویی نیست. NO_SIGNALS یا نبود مسیر، گواهی سالم‌بودن ولت نیست.

برای ارائه، عبارت دقیق این است: «سامانه شواهد قابل مشاهده در تاریخچه ذخیره‌شده را ردیابی و برای بررسی تحلیلگر اولویت‌بندی می‌کند؛ نتیجه با پوشش داده، محدودیت query و منابع هویتی همراه است.»

## ۶. نتیجه آزمون‌ها

| آزمون اجراشده | نتیجه |
| --- | --- |
| کتابخانه TRON | ۸۲ موفق |
| کتابخانه Ethereum | ۳۵ موفق؛ تست نیازمند ClickHouse جداگانه اجرا شد |
| کتابخانه BSC | ۴۴ موفق؛ پنج تست نیازمند ClickHouse جداگانه اجرا شدند |
| Analytical Node | ۱۶ موفق |
| اسکریپت‌های Python | ۳۷ موفق؛ خطای قبلی preflight رفع شد |
| قرارداد مسیردهی Gateway | ۵ موفق |
| قرارداد CLI لینوکس | موفق، از جمله رد worker مفقود/خاموش/ناسالم؛ Docker/VM/HTTP در این مجموعه test double هستند |
| Compose هر چهار role | config --quiet موفق؛ بدون اجرای stack اصلی |
| کامپایل ورودی‌های Ethereum | cargo check --offline --locked --bins موفق |
| TRON committed_visibility | روی ClickHouse 23.8 واقعی موفق؛ گراف، activity و balances بلاک ناقص را نمی‌دیدند و پس از COMPLETE درست به‌روز شدند |
| TRON analytics_pipeline | روی ClickHouse 23.8 واقعی موفق؛ اجرای خود worker، resume، PENDING، مرز صرافی و جلوگیری از انتشار خروجی ناقص |
| Ethereum latest exposure run | روی ClickHouse 23.8 واقعی موفق؛ وضعیت‌های RUNNING/FAILED/COMPLETE/NO_SEEDS و جداسازی network |
| پنج تست پایگاه داده BSC | روی ClickHouse 23.8 واقعی موفق؛ migration، crash، replay، reorg و dead-letter/requeue |

در مجموع هشت تست یکپارچگی بالا واقعاً با ClickHouse اجرا شدند؛ RPC و تراکنش‌ها در این تست‌ها fixture کنترل‌شده‌اند، نه اثبات دریافت کامل شبکه زنده. کانتینر تست متعلق به همین بازبینی است و داده‌های اصلی سه شبکه تغییر نکرده‌اند.

build سرد imageهای Linux، استقرار چهار VM واقعی، تست UI/Neo4j انتها‌به‌انتها روی داده زنده، soak و backup/restore هنوز اجرا نشده‌اند. نتیجه این آزمون‌ها اجازه ادعای «هیچ نقصی باقی نمانده» یا «پوشش کامل پول‌شویی» نمی‌دهد. تست fixture رفتار مشخص را اثبات می‌کند، نه پوشش جامع شبکه یا کارایی روی میلیاردها تراکنش.
