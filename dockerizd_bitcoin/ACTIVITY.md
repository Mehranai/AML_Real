# fingerprint و activity بیت‌کوین

AML-05 آمار آدرس را از دادهٔ canonical همان ClickHouse بیت‌کوین محاسبه می‌کند.
محاسبه هنگام درخواست است؛ جدول، migration، دیتابیس مرکزی یا worker جدید ندارد.
schema مشترک نسخهٔ ۵ و جداسازی VMهای شبکه حفظ شده‌اند.

## آمار چه معنایی دارد؟

| خروجی | معنی |
|---|---|
| `transaction_count` | تعداد occurrenceهای تراکنش مرتبط با آدرس |
| `received_output_count` / `spent_input_count` | تعداد outputهای آدرس و inputهای خرج‌کنندهٔ outputهای آن |
| `transfer_count` | مجموع همین legهای مشاهده‌شده؛ تعداد پرداخت‌های تخصیص‌یافته نیست |
| `received_sats` / `spent_sats` | جمع صحیح مقدار output و inputهای مربوط به آدرس |
| `net_flow_sats` | received منهای spent در محدودهٔ انتخابی؛ موجودی کیف پول نیست |
| `receiving_transactions` / `spending_transactions` | تراکنش‌های دارای هر سمت؛ یک تراکنش می‌تواند در هر دو شمارش باشد |
| `both_sides_transactions` | آدرس در ورودی و خروجی همان تراکنش دیده شده؛ change یا self-transfer قطعی نیست |
| `fees_of_involved_transactions_sats` | کارمزد کل تراکنش‌های مرتبط، هر occurrence یک بار |
| `spending_transaction_fees_sats` | کارمزد کل تراکنش‌های دارای input از آدرس، هر occurrence یک بار |
| `wallet_attributed_fee_sats` | `null`؛ سهم واقعی کارمزدِ پرداخت‌شده توسط این آدرس تعیین نشده است |
| `first_seen_unix_ms` / `last_seen_unix_ms` | کمترین/بیشترین زمان هدر در محدوده؛ در نبود تراکنش `null` |
| `first_seen_block` / `last_seen_block` | کمترین/بیشترین ارتفاع در همان محدوده؛ مستقل از ترتیب timestamp هدر |
| `maximum_transaction_inputs` / `maximum_transaction_outputs` | بیشترین تعداد ورودی تأمین‌کننده/خروجی تراکنش مرتبط؛ مالکیت مشترک استنباط نمی‌شود |
| `multi_input_transactions` / `multi_output_transactions` | شمارش شکل تراکنش، بدون detector یا امتیاز رفتاری تازه |
| `unique_counterparties` / `upstream_subjects` / `downstream_subjects` | تعداد subjectهای حاضر در context جهت‌دار تراکنش‌ها، نه اشخاص یا مالکان یکتا |

coinbase به‌عنوان دریافت قابل مشاهده شمرده می‌شود؛ marker ورودی coinbase یک
input تأمین‌کننده محسوب نمی‌شود و کارمزد آن به آدرس نسبت داده نمی‌شود. تمام جمع
مبالغ با UInt256 و net flow با تفریق صحیح انجام می‌شود؛ JSON مبلغ را رشته می‌فرستد.
نمایش BTC دقیقاً هشت اعشار دارد. جمع گردش تاریخی می‌تواند از supply یا UInt64
بزرگ‌تر شود؛ برای چنین جمعی مقدار به UInt64 محدود نمی‌شود.

## زمان، طرف مقابل و محدودیت نمایش

`activity` جمع روزانهٔ UTC است: تعداد تراکنش و leg، received/spent، net flow و
کارمزد کل تراکنش‌های همان روز. فقط روزهای فعال برمی‌گردند؛ روز خالی به‌طور مصنوعی
پر نمی‌شود. `recent_transactions` تراکنش‌های اخیر را بر اساس ارتفاع و موقعیت در
بلاک مرتب می‌کند؛ زمان هدر الزاماً صعودی نیست.

طرف‌های مقابل از دو نوع مشاهده ساخته می‌شوند:

- input خارجی در تراکنشی که output برای آدرس دارد؛ `upstream_transactions`.
- output خارجی در تراکنشی که input از آدرس دارد؛ `downstream_transactions`.

در هر subject، تکرار legهای همان تراکنش شمارش تراکنش را چند برابر نمی‌کند. آدرس
مورد بررسی از فهرست peer حذف می‌شود؛ script بدون آدرس همچنان نوع `script` دارد.
مقدار تخصیص‌یافته به طرف مقابل `null` است. حضور دو subject در یک تراکنش، پرداخت
قطعی بین آن‌ها، انتقال تمام مبلغ یا مالکیت مشترک را اثبات نمی‌کند.

جمع آمار و تعداد subjectها روی تمام تراکنش‌های موجود در محدودهٔ انتخابی محاسبه
می‌شوند. `limit` فقط نمایش تراکنش‌های اخیر و graph را محدود می‌کند؛ activity و
counterparty نیز سقف نمایش جدا دارند. `activity_coverage` رسیدن به هر سقف را
جدا گزارش می‌کند؛ محدودیت فهرست، summary را به «چهل تراکنش آخر» تبدیل نمی‌کند.

## فیلتر و API

```text
GET /api/bitcoin/wallet/{address}/activity
GET /api/bitcoin/wallet/{address}/investigation
```

هر دو endpoint پارامترهای زیر را می‌پذیرند؛ service key و `no-store` مسیرهای
دیگر برقرارند. endpoint activity فقط تحلیل را می‌خواند؛ investigation علاوه بر
آن، مسیر موجود exposure/risk و snapshot مرکزی در عبور از Gateway را انجام می‌دهد.

| پارامتر | پیش‌فرض / محدودیت |
|---|---|
| `from_block` / `to_block` | اختیاری، شامل دو مرز |
| `from_unix_ms` / `to_unix_ms` | زمان UTC هدر به میلی‌ثانیه، شامل دو مرز |
| `direction` | `all`، `received`، `spent` یا `both` |
| `limit` | ۴۰؛ از ۱ تا ۱۰۰، برای recent transactions و graph |
| `activity_limit` | ۹۰؛ از ۱ تا ۳۶۶ روز فعال |
| `counterparty_limit` | ۲۰؛ از ۱ تا ۱۰۰ subject |

فیلتر received، تراکنش both را نیز شامل می‌شود؛ spent هم همین‌طور. دو فیلتر
ارتفاع و زمان با AND ترکیب می‌شوند. مرز معکوس، جهت نامعتبر، پارامتر ناشناخته و
سقف نامعتبر رد می‌شوند. نمونه:

```text
/api/bitcoin/wallet/ADDRESS/investigation?from_block=800000&to_block=800100&direction=spent
```

**این فیلترها مخصوص analytics هستند.** graph همچنان محلهٔ canonical اخیر خود،
holdings کل پنجرهٔ UTXO دریافت‌شده و exposure مسیر محدود خودش را دارند. ریسک از
برچسب معتبر و همان exposure استفاده می‌کند؛ محدودکردن تاریخ فعالیت، خودبه‌خود
مسیر exposure را محدود نمی‌کند. `query_scope` و توضیح پنل این تفاوت را نشان می‌دهند.
هماهنگ‌کردن فیلتر تمام بخش‌ها همچنان بخشی از API-02 است.

برای مثال، در fixture آدرسی ۶۰ ساتوشی دریافت می‌کند و بعداً در تراکنش دیگری ۶۰
ساتوشی input و ۹ ساتوشی output دارد. آمار کامل received=69، spent=60 و net=9 است.
اگر فقط تراکنش دوم انتخاب شود، net=-51 می‌شود؛ holdings کامل snapshot همچنان
۹ ساتوشی است. این رفتار عمدی است و آدرس با net منفی الزاماً موجودی منفی ندارد.

## snapshot، تاریخچه و خطا

کلید occurrence شامل شبکه، ارتفاع/hash بلاک، commit، revision، txid و موقعیت
تراکنش است. دو occurrence تاریخی با txid تکراری یکی نمی‌شوند. قبل و بعد از queryها
فنس زنجیره بررسی می‌شود. rollback، شاخهٔ orphan را از آمار canonical کنار می‌گذارد؛
درج فیزیکی بدون انتشار، آمار جاری ایجاد نمی‌کند.

`query_scope.analytics` محدودهٔ دریافت‌شده و فیلتر درخواست را اعلام می‌کند؛
`requested_block_bounds_ingested` می‌گوید مرزهای ارتفاعی صریح داخل پنجرهٔ دریافت
هستند یا نه. `history_from_genesis=false` یعنی تاریخچهٔ دریافت جزئی است. نبود
تراکنش در این محدوده، اثبات نبود فعالیت در کل شبکه نیست. قبل از نخستین بلاک،
وضعیت waiting و summary_complete=false برمی‌گردد.

سقف اجرای analytics، ۱۰۰ هزار تراکنش منطبق است. عبور از آن HTTP 422 می‌دهد و
نیاز به محدودکردن ارتفاع یا زمان دارد. هر query سقف ۱۵ ثانیه، دو thread و ۲۵۶ MiB
حافظه دارد؛ شکست منابع یا تغییر source خروجی ظاهراً کامل تولید نمی‌کند. این
سقف‌ها جای benchmark روی نود/دیتای production و بهینه‌سازی مسیرهای DB-09 را نمی‌گیرند.

فیلترها داخل fingerprint هم ثبت می‌شوند تا ورودی ارزیابی ریسک قابل بازسازی باشد.
پاسخ کامل شامل activity، asset flows، counterparties و query_scope در snapshot
تحقیق Neo4j حفظ می‌شود. Export همان snapshot را دائمی می‌کند و رسید ذخیره‌سازی
می‌دهد؛ reopen داده و فیلتر ذخیره‌شده را برمی‌گرداند. دادهٔ زنده با آمار تاریخی
ترکیب نمی‌شود.

SQL فعال prefix در `sql/queries/activity.sql` و reader در `src/activity.rs` است؛
query از viewهای canonical قرارداد مشترک مصرف می‌کند. جدول feature یا semantic
با رکورد ساختگی پر نمی‌شود؛ worker/detectorهای آن‌ها هنوز مراحل جدا هستند.

[اعتبارسنجی](ACTIVITY_VALIDATION.md)، [ساختار بک‌اند](../docs/BACKEND_ARCHITECTURE_FA.md)،
[کارهای باقی‌مانده](../TODO_BITCOIN.md).
