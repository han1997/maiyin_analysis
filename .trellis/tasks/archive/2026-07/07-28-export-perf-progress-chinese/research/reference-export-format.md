# Reference Repo Export Format

Source: `C:\Users\hanhu\Code\maiyin_analysis\desktop_app\io_service.py` (Python + `xlsxwriter`).

## summary_csv — 18 columns

Headers (io_service.py:158-177):
```
姓名, 身份证号, 手机号, 户籍省, 户籍市, 户籍县区, 年龄, 性别, 记录总数, 时间窗口内入住次数, 7天最大次数, 30天最大次数, 365天最大次数, 重合天数, 非重合超3天数, 风险分, 风险等级, 预警摘要
```

Field mapping (io_service.py:179-201):
- `item.name`, `item.id_no`, `item.phone` → 姓名/身份证号/手机号
- `item.household_province/city/county` → 户籍省/市/县区 (split, NOT single `household_region`)
- `item.age` (None→""), `item.gender` → 年龄/性别
- `item.total_records` → 记录总数
- `item.frequency_window_count` → 时间窗口内入住次数 (**NEW column — PersonSummary must add this field**)
- `item.max_week_count/max_month_count/max_year_count` → 7/30/365天最大次数
- `item.overlap_days` → 重合天数
- `item.sequential_days` → 非重合超3天数
- `item.score` → 风险分 (write as number)
- `item.level` → 风险等级
- `"；".join(alert.title for alert in item.alerts)` → 预警摘要

Encoding: `utf-8-sig` (BOM). `_safe_csv` formula-injection guard for `= + - @ \t \r`.

## raw_csv — 23 columns

Headers (io_service.py:208-232):
```
源文件, 源表行号, 姓名, 身份证号, 手机号, 户籍省, 户籍市, 户籍县区, 户籍地区划, 户籍地详址, 年龄, 性别, 酒店名称, 省, 市, 县区, 地域省市县, 地址, 房间号, 入住时间, 登记时间, 退房时间, 数据问题
```

Field mapping (io_service.py:237-262):
- `record.source_file/source_row` → 源文件/源表行号
- `record.name/id_no/phone` → 姓名/身份证号/手机号
- `record.household_province/city/county/region/address` → 户籍省/市/县区/区划/详址
- `record.age` (None→""), `record.gender` → 年龄/性别
- `record.hotel_name` → 酒店名称
- `record.province/city/county/region/address` → 省/市/县区/地域省市县/地址
- `record.room_no` → 房间号
- `format_datetime(record.check_in/register_time/check_out)` → 入住/登记/退房时间 (`%Y-%m-%d %H:%M`)
- `"；".join(record.issues)` → 数据问题

Filter: `within_analysis_scope(record, settings) AND within_analysis_time_window(record, settings)`.
Note: newUI's `Record` already has all these split fields (`household_province/city/county/region/address`, `province/city/county/region`, `age`, `gender`, `check_in/register_time/check_out` as `NaiveDateTime`). No new Record fields needed — just use them instead of the single `household_region` and `*_text` strings.

## risk_xlsx — 26 columns, sheet name "风险合并明细"

### ALERT_KIND_LABELS (io_service.py:266-273)
```python
{
    "overlap": "入住时间重叠",
    "same_day_many": "同日多次入住",
    "window_frequency": "时间窗口高频入住",
    "week_frequency": "7 天高频入住",
    "month_frequency": "30 天高频入住",
    "year_frequency": "365 天高频入住",
}
```

### Headers (io_service.py:292-319)
```
姓名, 身份证号, 手机号, 户籍省, 户籍市, 户籍县区, 年龄, 性别, 风险等级, 风险分, 预警类型, 预警级别, 风险标题, 风险说明, 源文件, 源表行号, 酒店名称, 酒店地址, 房间号, 省, 市, 县区, 地域省市县, 入住时间, 退房时间, 登记时间
```
- Columns 0-13 = person block (merged vertically across evidence rows)
- Columns 14-25 = evidence detail (one row per evidence record)

### Formats (io_service.py:320-350)
- **Header format**: bold, white text (`#FFFFFF`), bg `#17324D`, center, vcenter, border 1 `#D4DCE5`, row height 28.
- **Body format**: vcenter, text_wrap, border 1 `#D4DCE5`.
- **Center format**: align center, vcenter, text_wrap, border 1 `#D4DCE5`.
- **Level formats** (applied to 风险等级 column, index 8):
  - 高风险: bg `#FDE8E7`, font `#8C2929`
  - 中风险: bg `#FFF2D8`, font `#7B551B`
  - 关注: bg `#FFF8D8`, font `#635A24`
  - 正常: bg `#E8F4EC`, font `#315A40`
  - All level formats also: align center, vcenter, text_wrap, border 1 `#D4DCE5`.

### Row structure (io_service.py:354-423)
1. Filter `risk_people = [item for item in analyses if item.alerts]`.
2. Build `record_map = {record.uid: record for record in records}`.
3. For each person:
   - Collect `evidence_ids = sorted(unique union of alert.evidence_ids across all alerts)`, sorted by `(record.check_in, uid)`.
   - `evidence = [record_map[uid] for uid in evidence_ids if uid in record_map] or [None]`.
   - `risk_types = "\n".join(dict.fromkeys(ALERT_KIND_LABELS.get(alert.kind, alert.kind) for alert in item.alerts))` — Chinese labels, newline-joined, dedup preserving order.
   - `severities = "、".join(dict.fromkeys(alert.severity for alert in item.alerts))` — dedup, comma-joined.
   - `titles = "\n".join(dict.fromkeys(alert.title for alert in item.alerts))`.
   - `details = "\n".join(dict.fromkeys(alert.detail for alert in item.alerts))`.
   - `person_values` (14 cols): name, id_no, phone, household_province, household_city, household_county, age, gender, level, score, risk_types, severities, titles, details.
   - For each evidence record, `detail_values` (12 cols): source_file, source_row, hotel_name, address, room_no, province, city, county, region, check_in (formatted), check_out (formatted), register_time (formatted).
   - Write evidence rows at columns 14-25.
   - If `end_row > start_row`: `merge_range` each of the 14 person columns from start_row to end_row with the person value + appropriate format. Column 8 (风险等级) uses `level_formats[item.level]`; columns 1,2,6,7,9,11 use center_format; others use body_format.
   - Else (single evidence row): write each person value at its column with the same format logic.

### Sheet setup (io_service.py:428-432)
- Column widths set (specific values in io_service.py:428).
- `freeze_panes(1, 0)` — freeze header row.
- `autofilter(0, 0, output_row - 1, 25)` — filter on all columns.

### XLSX safety
- `strings_to_formulas: False`, `strings_to_urls: False` — prevents formula injection and URL auto-linking. newUI's `rust_xlsxwriter` equivalent: use `write_string` for all cells (already does), and `strings_to_urls` equivalent is the default behavior (no auto-hyperlink). No `safe()` needed for XLSX.

## Key newUI model gaps to close

1. **`PersonSummary` missing `frequency_window_count`** (model.rs:149-180). Add `#[serde(default)] pub frequency_window_count: usize` (default 0 for legacy sessions). Populate in `analysis.rs` `summarize_person` or wherever `PersonSummary` is built — the count of records within the analysis time window for this person.
2. `Record` has ALL fields needed for raw_csv split columns — just use `household_province/city/county/region/address` instead of `household_region`, and `province/city/county/region` instead of single region, and `age/gender` (already present), and `check_in/register_time/check_out` (NaiveDateTime) with `format("%Y-%m-%d %H:%M")` instead of `*_text`.
3. `rust_xlsxwriter` merge: `worksheet.merge_range(start_row, col, end_row, col, value, format)` — single-cell merge for vertical merge. Verify the API signature in rust_xlsxwriter 0.96.

## Progress callback pattern (reference)
io_service.py:277 — `export_risk_excel(path, analyses, records, progress_callback=None)`, invoked at io_service.py:425 every 100 people + at end. The newUI equivalent uses `Option<&dyn Fn(usize, usize) + Send + Sync>` (current, total) + 50ms throttle in `make_progress_callback`.
