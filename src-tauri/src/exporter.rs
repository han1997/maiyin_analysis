use crate::analysis::within_analysis_time_window;
use crate::error::AppError;
use crate::model::{format_datetime, AnalysisSettings, PersonAnalysis, Record};
use rust_xlsxwriter::{Format, FormatAlign, FormatBorder, Workbook};
use serde::Serialize;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::Path;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationResult {
    pub message: String,
    pub path: Option<String>,
}

fn export_error<E: std::fmt::Display>(error: E) -> AppError {
    AppError::Export(error.to_string())
}

/// Chinese display labels for each alert kind, matching the reference repo's
/// `ALERT_KIND_LABELS`. Multi-alert people join the deduped labels with `\n`.
const ALERT_KIND_LABELS: &[(&str, &str)] = &[
    ("overlap", "入住时间重叠"),
    ("same_day_many", "同日多次入住"),
    ("window_frequency", "时间窗口高频入住"),
    ("week_frequency", "7 天高频入住"),
    ("month_frequency", "30 天高频入住"),
    ("year_frequency", "365 天高频入住"),
];

fn alert_kind_label(kind: &str) -> String {
    ALERT_KIND_LABELS
        .iter()
        .find(|(key, _)| *key == kind)
        .map(|(_, label)| (*label).to_string())
        .unwrap_or_else(|| kind.to_string())
}

/// Join the deduped (order-preserving) projection of `alerts` with `sep`.
fn dedup_join<F>(alerts: &[crate::model::AlertSummary], map: F, sep: &str) -> String
where
    F: Fn(&crate::model::AlertSummary) -> String,
{
    let mut out: Vec<String> = Vec::new();
    for alert in alerts {
        let value = map(alert);
        if !out.contains(&value) {
            out.push(value);
        }
    }
    out.join(sep)
}

/// Formula-injection guard for CSV cells. Returns `Cow::Borrowed` for safe
/// values (no allocation) and prefixes `'` only for dangerous leading chars.
fn safe(value: &str) -> Cow<'_, str> {
    if value
        .chars()
        .next()
        .is_some_and(|c| matches!(c, '=' | '+' | '-' | '@' | '\t' | '\r'))
    {
        Cow::Owned(format!("'{value}"))
    } else {
        Cow::Borrowed(value)
    }
}

fn csv_writer(path: &Path) -> Result<csv::Writer<std::fs::File>, AppError> {
    let mut file = fs::File::create(path).map_err(export_error)?;
    file.write_all(&[0xef, 0xbb, 0xbf]).map_err(export_error)?;
    Ok(csv::Writer::from_writer(file))
}

pub fn export_summary_csv(
    path: &Path,
    analyses: &[PersonAnalysis],
    on_progress: Option<&(dyn Fn(usize, usize) + Send + Sync)>,
) -> Result<(), AppError> {
    let mut writer = csv_writer(path)?;
    writer
        .write_record([
            "姓名",
            "身份证号",
            "手机号",
            "户籍省",
            "户籍市",
            "户籍县区",
            "年龄",
            "性别",
            "记录总数",
            "时间窗口内入住次数",
            "7天最大次数",
            "30天最大次数",
            "365天最大次数",
            "重合天数",
            "非重合超3天数",
            "风险分",
            "风险等级",
            "预警摘要",
        ])
        .map_err(export_error)?;

    let total = analyses.len();
    if let Some(f) = on_progress {
        f(0, total);
    }
    for (index, item) in analyses.iter().enumerate() {
        let person = &item.summary;
        writer
            .write_record([
                safe(&person.name).as_bytes(),
                safe(&person.id_no).as_bytes(),
                safe(&person.phone).as_bytes(),
                safe(&person.household_province).as_bytes(),
                safe(&person.household_city).as_bytes(),
                safe(&person.household_county).as_bytes(),
                person
                    .age
                    .map(|age| age.to_string())
                    .unwrap_or_default()
                    .as_bytes(),
                safe(&person.gender).as_bytes(),
                person.total_records.to_string().as_bytes(),
                person.frequency_window_count.to_string().as_bytes(),
                person.max_week_count.to_string().as_bytes(),
                person.max_month_count.to_string().as_bytes(),
                person.max_year_count.to_string().as_bytes(),
                person.overlap_days.to_string().as_bytes(),
                person.sequential_days.to_string().as_bytes(),
                person.score.to_string().as_bytes(),
                safe(&person.level).as_bytes(),
                safe(&person.alert_titles.join("；")).as_bytes(),
            ])
            .map_err(export_error)?;
        if let Some(f) = on_progress {
            f(index + 1, total);
        }
    }
    writer.flush().map_err(export_error)
}

pub fn export_raw_csv(
    path: &Path,
    records: &[Record],
    settings: &AnalysisSettings,
    on_progress: Option<&(dyn Fn(usize, usize) + Send + Sync)>,
) -> Result<(), AppError> {
    let mut writer = csv_writer(path)?;
    writer
        .write_record([
            "源文件",
            "源表行号",
            "姓名",
            "身份证号",
            "手机号",
            "户籍省",
            "户籍市",
            "户籍县区",
            "户籍地区划",
            "户籍地详址",
            "年龄",
            "性别",
            "酒店名称",
            "省",
            "市",
            "县区",
            "地域省市县",
            "地址",
            "房间号",
            "入住时间",
            "登记时间",
            "退房时间",
            "数据问题",
        ])
        .map_err(export_error)?;

    // Pre-count the in-window records so the progress bar has an accurate total
    // before the (potentially large) write loop starts.
    let in_window: Vec<&Record> = records
        .iter()
        .filter(|record| within_analysis_time_window(record, settings))
        .collect();
    let total = in_window.len();
    if let Some(f) = on_progress {
        f(0, total);
    }
    for (index, record) in in_window.iter().enumerate() {
        writer
            .write_record([
                safe(&record.source_file).as_bytes(),
                record.source_row.to_string().as_bytes(),
                safe(&record.name).as_bytes(),
                safe(&record.id_no).as_bytes(),
                safe(&record.phone).as_bytes(),
                safe(&record.household_province).as_bytes(),
                safe(&record.household_city).as_bytes(),
                safe(&record.household_county).as_bytes(),
                safe(&record.household_region).as_bytes(),
                safe(&record.household_address).as_bytes(),
                record
                    .age
                    .map(|age| age.to_string())
                    .unwrap_or_default()
                    .as_bytes(),
                safe(&record.gender).as_bytes(),
                safe(&record.hotel_name).as_bytes(),
                safe(&record.province).as_bytes(),
                safe(&record.city).as_bytes(),
                safe(&record.county).as_bytes(),
                safe(&record.region).as_bytes(),
                safe(&record.address).as_bytes(),
                safe(&record.room_no).as_bytes(),
                format_datetime(record.check_in).as_bytes(),
                format_datetime(record.register_time).as_bytes(),
                format_datetime(record.check_out).as_bytes(),
                safe(&record.issues.join("；")).as_bytes(),
            ])
            .map_err(export_error)?;
        if let Some(f) = on_progress {
            f(index + 1, total);
        }
    }
    writer.flush().map_err(export_error)
}

fn border_format() -> Format {
    Format::new()
        .set_border(FormatBorder::Thin)
        .set_border_color("#D4DCE5")
}

pub fn export_risk_xlsx(
    path: &Path,
    analyses: &[PersonAnalysis],
    records: &[Record],
    on_progress: Option<&(dyn Fn(usize, usize) + Send + Sync)>,
) -> Result<(), AppError> {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.set_name("风险合并明细").map_err(export_error)?;

    // Header format: bold white text on dark navy, centered, wrapped, bordered.
    let header_format = border_format()
        .set_bold()
        .set_font_color("#FFFFFF")
        .set_background_color("#17324D")
        .set_align(FormatAlign::Center)
        .set_align(FormatAlign::VerticalCenter)
        .set_text_wrap();

    // Body format: vertical-centered, wrapped, bordered (left-aligned default).
    let body_format = border_format()
        .set_align(FormatAlign::VerticalCenter)
        .set_text_wrap();

    // Center format: centered both axes, wrapped, bordered.
    let center_format = border_format()
        .set_align(FormatAlign::Center)
        .set_align(FormatAlign::VerticalCenter)
        .set_text_wrap();

    // Per-level formats applied to the 风险等级 column.
    let level_format = |background: &str, font: &str| {
        border_format()
            .set_align(FormatAlign::Center)
            .set_align(FormatAlign::VerticalCenter)
            .set_text_wrap()
            .set_background_color(background)
            .set_font_color(font)
    };
    let high_format = level_format("#FDE8E7", "#8C2929");
    let mid_format = level_format("#FFF2D8", "#7B551B");
    let watch_format = level_format("#FFF8D8", "#635A24");
    let normal_format = level_format("#E8F4EC", "#315A40");

    let headers = [
        "姓名",
        "身份证号",
        "手机号",
        "户籍省",
        "户籍市",
        "户籍县区",
        "年龄",
        "性别",
        "风险等级",
        "风险分",
        "预警类型",
        "预警级别",
        "风险标题",
        "风险说明",
        "源文件",
        "源表行号",
        "酒店名称",
        "酒店地址",
        "房间号",
        "县区",
        "入住时间",
        "退房时间",
        "登记时间",
    ];
    for (column, value) in headers.iter().enumerate() {
        worksheet
            .write_string_with_format(0, column as u16, *value, &header_format)
            .map_err(export_error)?;
    }
    worksheet.set_row_height(0, 28.0).map_err(export_error)?;
    for column in 0..23u16 {
        worksheet
            .set_column_width(column, 15.0)
            .map_err(export_error)?;
    }

    let risk_people: Vec<&PersonAnalysis> = analyses
        .iter()
        .filter(|item| !item.alerts.is_empty())
        .collect();
    let record_map: HashMap<u64, &Record> =
        records.iter().map(|record| (record.uid, record)).collect();

    let total = risk_people.len();
    if let Some(f) = on_progress {
        f(0, total);
    }

    let mut row: u32 = 1;
    for (index, item) in risk_people.iter().enumerate() {
        let person = &item.summary;

        // Unique union of evidence ids across all alerts, sorted by (check_in, uid).
        let mut evidence_ids: Vec<u64> = item
            .alerts
            .iter()
            .flat_map(|alert| alert.evidence_ids.iter().copied())
            .collect();
        let mut seen = HashSet::new();
        evidence_ids.retain(|uid| seen.insert(*uid));
        evidence_ids.sort_by_key(|uid| {
            let check_in = record_map
                .get(uid)
                .and_then(|record| record.check_in)
                .unwrap_or(chrono::NaiveDateTime::MIN);
            (check_in, *uid)
        });

        let start_row = row;
        let evidence_records: Vec<&Record> = evidence_ids
            .iter()
            .filter_map(|uid| record_map.get(uid).copied())
            .collect();
        // Each person occupies at least one evidence row (empty detail when no
        // matching record is found), matching the reference `[None]` fallback.
        let row_count = evidence_records.len().max(1);
        let end_row = start_row + row_count as u32 - 1;

        let risk_types = dedup_join(&item.alerts, |alert| alert_kind_label(&alert.kind), "\n");
        let severities = dedup_join(&item.alerts, |alert| alert.severity.clone(), "、");
        let titles = dedup_join(&item.alerts, |alert| alert.title.clone(), "\n");
        let details = dedup_join(&item.alerts, |alert| alert.detail.clone(), "\n");
        // Age (col 6) and score (col 9) are numeric. `merge_range` only accepts
        // strings, so the loop below registers the merge / writes with blank
        // placeholders here; the numeric values are then written to the first
        // cell of each (merged) range after the loop. Matches the reference
        // repo, which writes score/age as numbers so Excel doesn't flag them
        // as text-stored numbers.
        let person_values = [
            person.name.clone(),
            person.id_no.clone(),
            person.phone.clone(),
            person.household_province.clone(),
            person.household_city.clone(),
            person.household_county.clone(),
            String::new(),
            person.gender.clone(),
            person.level.clone(),
            String::new(),
            risk_types,
            severities,
            titles,
            details,
        ];

        let level_fmt: &Format = match person.level.as_str() {
            "高风险" => &high_format,
            "中风险" => &mid_format,
            "关注" => &watch_format,
            _ => &normal_format,
        };

        // Write the evidence detail rows at columns 14-22 (one row per record).
        for (offset, current_row) in (start_row..=end_row).enumerate() {
            if let Some(record) = evidence_records.get(offset) {
                let detail = [
                    record.source_file.clone(),
                    record.hotel_name.clone(),
                    record.address.clone(),
                    record.room_no.clone(),
                    record.county.clone(),
                    format_datetime(record.check_in),
                    format_datetime(record.check_out),
                    format_datetime(record.register_time),
                ];
                worksheet
                    .write_string_with_format(current_row, 14, &detail[0], &body_format)
                    .map_err(export_error)?;
                worksheet
                    .write_number_with_format(
                        current_row,
                        15,
                        record.source_row as f64,
                        &body_format,
                    )
                    .map_err(export_error)?;
                for (col_offset, value) in detail[1..].iter().enumerate() {
                    worksheet
                        .write_string_with_format(
                            current_row,
                            (16 + col_offset) as u16,
                            value,
                            &body_format,
                        )
                        .map_err(export_error)?;
                }
            } else {
                // `[None]` evidence row: blank detail columns keep the borders intact.
                for col in 14u16..=22 {
                    worksheet
                        .write_string_with_format(current_row, col, "", &body_format)
                        .map_err(export_error)?;
                }
            }
        }

        // Write/merge the 14-column person block. Columns 1,2,6,7,9,11 are centered;
        // column 8 (风险等级) takes the per-level format; the rest use body format.
        let needs_merge = end_row > start_row;
        for (col, value) in person_values.iter().enumerate() {
            let fmt: &Format = match col {
                8 => level_fmt,
                1 | 2 | 6 | 7 | 9 | 11 => &center_format,
                _ => &body_format,
            };
            if needs_merge {
                worksheet
                    .merge_range(start_row, col as u16, end_row, col as u16, value, fmt)
                    .map_err(export_error)?;
            } else {
                worksheet
                    .write_string_with_format(start_row, col as u16, value, fmt)
                    .map_err(export_error)?;
            }
        }

        // Overwrite the numeric person columns with real numbers. The loop
        // above registered merges / wrote blanks for cols 6 (age) and 9 (score);
        // we now place the numeric value in the first cell of each range. For
        // merged ranges this preserves the merge while making the value a true
        // number; a missing age stays blank.
        worksheet
            .write_number_with_format(start_row, 9, person.score as f64, &center_format)
            .map_err(export_error)?;
        if let Some(age) = person.age {
            worksheet
                .write_number_with_format(start_row, 6, age as f64, &center_format)
                .map_err(export_error)?;
        }

        row = end_row + 1;
        if let Some(f) = on_progress {
            f(index + 1, total);
        }
    }

    worksheet.set_freeze_panes(1, 0).map_err(export_error)?;
    // Autofilter spans the header (row 0) through the last written row.
    let last_row = row.saturating_sub(1);
    if last_row >= 1 {
        worksheet
            .autofilter(0, 0, last_row, 22)
            .map_err(export_error)?;
    }

    workbook.save(path).map_err(export_error)
}

pub fn export_template(path: &Path) -> Result<(), AppError> {
    let bytes = include_bytes!("../resources/旅馆业数据导入模板.xlsx");
    fs::write(path, bytes).map_err(export_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AlertSummary, HotelRegion, PersonSummary};
    use calamine::{open_workbook, Data, Reader, Xlsx};
    use chrono::NaiveDateTime;
    use std::path::PathBuf;

    fn sample_record(uid: u64, hotel: &str, room: &str, check_in: &str, check_out: &str) -> Record {
        let parse = |value: &str| NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M").ok();
        Record {
            uid,
            source_file: "test.xlsx".into(),
            source_row: uid as usize + 1,
            name: "张三".into(),
            id_no: "341024198809128135".into(),
            phone: "13905591234".into(),
            hotel_name: hotel.into(),
            province: "安徽省".into(),
            city: "黄山市".into(),
            county: "祁门县".into(),
            region: "安徽省黄山市祁门县".into(),
            address: "测试路 1 号".into(),
            room_no: room.into(),
            check_in_text: check_in.into(),
            register_time_text: String::new(),
            check_out_text: check_out.into(),
            check_in: parse(check_in),
            register_time: None,
            check_out: parse(check_out),
            person_key: "id:341024198809128135".into(),
            household_province: "安徽省".into(),
            household_city: "黄山市".into(),
            household_county: "祁门县".into(),
            household_region: "安徽省 黄山市 祁门县".into(),
            household_address: "户籍地址".into(),
            age: Some(37),
            gender: "男".into(),
            issues: vec![],
        }
    }

    fn sample_analyses() -> Vec<PersonAnalysis> {
        let alert = AlertSummary {
            kind: "overlap".into(),
            severity: "高".into(),
            score: 27,
            title: "2026-05-01 入住时间重叠".into(),
            detail: "2 对记录存在入住到退房时间交叉".into(),
            evidence_count: 2,
            evidence_ids: vec![1, 2],
        };
        let summary = PersonSummary {
            person_key: "id:341024198809128135".into(),
            name: "张三".into(),
            id_no: "341024198809128135".into(),
            phone: "13905591234".into(),
            household_region: "安徽省 黄山市 祁门县".into(),
            household_province: "安徽省".into(),
            household_city: "黄山市".into(),
            household_county: "祁门县".into(),
            age: Some(37),
            gender: "男".into(),
            total_records: 2,
            frequency_window_count: 2,
            max_week_count: 2,
            max_month_count: 2,
            max_year_count: 2,
            overlap_days: 1,
            sequential_days: 0,
            score: 27,
            level: "中风险".into(),
            alert_count: 1,
            alert_titles: vec!["2026-05-01 入住时间重叠".into()],
            hotel_names: vec!["甲旅馆".into(), "乙旅馆".into()],
            hotel_regions: vec![HotelRegion {
                province: "安徽省".into(),
                city: "黄山市".into(),
                county: "祁门县".into(),
                region: "安徽省黄山市祁门县".into(),
            }],
        };
        vec![PersonAnalysis {
            summary,
            alerts: vec![alert],
        }]
    }

    fn sample_records() -> Vec<Record> {
        vec![
            sample_record(1, "甲旅馆", "301", "2026-05-01 09:30", "2026-05-01 13:00"),
            sample_record(2, "乙旅馆", "302", "2026-05-01 10:00", "2026-05-01 12:00"),
        ]
    }

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("maiyin-export-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn data_to_string(data: &Data) -> String {
        match data {
            Data::String(value) => value.clone(),
            Data::Int(value) => value.to_string(),
            Data::Float(value) => value.to_string(),
            _ => String::new(),
        }
    }

    #[test]
    fn summary_csv_has_eighteen_columns_bom_and_split_household_fields() {
        let path = temp_path("人员汇总.csv");
        export_summary_csv(&path, &sample_analyses(), None).unwrap();

        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(
            &bytes[0..3],
            &[0xef, 0xbb, 0xbf],
            "UTF-8 BOM must be present"
        );

        let content = String::from_utf8(bytes).unwrap();
        let mut lines = content.lines();
        let header = lines.next().unwrap();
        assert!(header.contains("户籍省"));
        assert!(header.contains("户籍市"));
        assert!(header.contains("户籍县区"));
        assert!(header.contains("时间窗口内入住次数"));
        assert!(header.contains("7天最大次数"));
        assert!(header.contains("365天最大次数"));
        assert_eq!(header.split(',').count(), 18);

        let row = lines.next().unwrap();
        assert!(row.contains("安徽省"));
        assert!(row.contains("黄山市"));
        assert!(row.contains("祁门县"));
        // frequency_window_count (2) appears as a field; total_records is also 2.
        assert!(row.contains(",2,2,"));
    }

    #[test]
    fn summary_csv_prefixes_formula_injection_leading_chars() {
        let mut analyses = sample_analyses();
        analyses[0].summary.name = "=evil".into();

        let path = temp_path("injection.csv");
        export_summary_csv(&path, &analyses, None).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        // The first data field must be prefixed with a single quote.
        assert!(content.contains("'=evil"));
        assert!(!content.contains(",=evil"));
    }

    #[test]
    fn raw_csv_has_twenty_three_columns_and_formatted_datetimes() {
        let path = temp_path("原始明细.csv");
        let records = sample_records();
        export_raw_csv(&path, &records, &AnalysisSettings::default(), None).unwrap();

        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[0..3], &[0xef, 0xbb, 0xbf]);
        let content = String::from_utf8(bytes).unwrap();
        let mut lines = content.lines();
        let header = lines.next().unwrap();
        assert!(header.contains("户籍地区划"));
        assert!(header.contains("户籍地详址"));
        assert!(header.contains("地域省市县"));
        assert!(header.contains("年龄"));
        assert!(header.contains("性别"));
        assert_eq!(header.split(',').count(), 23);

        let row = lines.next().unwrap();
        // NaiveDateTime formatted with %Y-%m-%d %H:%M.
        assert!(row.contains("2026-05-01 09:30"));
        assert!(row.contains("2026-05-01 13:00"));
    }

    #[test]
    fn risk_xlsx_sheet_name_headers_and_merged_person_block() {
        let path = temp_path("风险合并.xlsx");
        let records = sample_records();
        let analyses = sample_analyses();
        export_risk_xlsx(&path, &analyses, &records, None).unwrap();

        let mut workbook: Xlsx<_> = open_workbook(&path).unwrap();
        assert_eq!(workbook.sheet_names(), vec!["风险合并明细".to_string()]);

        let range = workbook.worksheet_range("风险合并明细").unwrap();
        // Row 0: 23-column header.
        let header: Vec<String> = (0..23)
            .map(|col| range.get((0, col)).map(data_to_string).unwrap_or_default())
            .collect();
        assert_eq!(header[0], "姓名");
        assert_eq!(header[8], "风险等级");
        assert_eq!(header[10], "预警类型");
        assert_eq!(header[14], "源文件");
        assert_eq!(header[19], "县区");
        assert_eq!(header[22], "登记时间");

        // Person block (cols 0-13) is merged across the two evidence rows:
        // values live on row 1, row 2 is blank in the merged region.
        assert_eq!(
            range.get((1, 0)).map(data_to_string).unwrap_or_default(),
            "张三"
        );
        assert_eq!(
            range.get((1, 8)).map(data_to_string).unwrap_or_default(),
            "中风险"
        );
        assert_eq!(
            range.get((1, 10)).map(data_to_string).unwrap_or_default(),
            "入住时间重叠"
        );
        assert!(
            matches!(range.get((2, 0)), Some(Data::Empty)),
            "merged person cell must be blank"
        );

        // Numeric person columns are written as real numbers (not text), so
        // Excel treats score/age as numeric and the file matches the reference
        // repo output without "number stored as text" warnings.
        assert!(
            matches!(range.get((1, 9)), Some(Data::Float(_)) | Some(Data::Int(_))),
            "风险分 (col 9) must be numeric, got {:?}",
            range.get((1, 9))
        );
        assert!(
            matches!(range.get((1, 6)), Some(Data::Float(_)) | Some(Data::Int(_))),
            "年龄 (col 6) must be numeric, got {:?}",
            range.get((1, 6))
        );

        // Evidence detail rows: cols 14-22 carry one record per row.
        assert_eq!(
            range.get((1, 16)).map(data_to_string).unwrap_or_default(),
            "甲旅馆"
        );
        assert_eq!(
            range.get((2, 16)).map(data_to_string).unwrap_or_default(),
            "乙旅馆"
        );
        // source_row is numeric.
        assert!(matches!(
            range.get((1, 15)),
            Some(Data::Float(_)) | Some(Data::Int(_))
        ));
    }

    #[test]
    fn risk_xlsx_supports_progress_callback() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let calls = Arc::new(AtomicUsize::new(0));
        let calls_clone = calls.clone();
        let callback = move |current: usize, total: usize| {
            calls_clone.fetch_add(1, Ordering::Relaxed);
            assert_eq!(total, 1);
            assert!(current <= total);
        };
        let path = temp_path("progress.xlsx");
        let records = sample_records();
        let analyses = sample_analyses();
        export_risk_xlsx(&path, &analyses, &records, Some(&callback)).unwrap();
        // start (0/total) + per-person (1/total) = at least 2 calls.
        assert!(calls.load(Ordering::Relaxed) >= 2);
    }
}
