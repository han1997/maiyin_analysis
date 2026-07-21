use crate::analysis::within_analysis_time_window;
use crate::error::AppError;
use crate::model::StoredSession;
use rust_xlsxwriter::{Format, FormatAlign, Workbook};
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::path::Path;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationResult {
    pub message: String,
    pub path: Option<String>,
}

pub fn export_to(
    kind: &str,
    session: &StoredSession,
    path: &Path,
) -> Result<OperationResult, AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| AppError::Export(error.to_string()))?;
    }
    match kind {
        "summary_csv" => export_summary_csv(path, session)?,
        "risk_xlsx" => export_risk_xlsx(path, session)?,
        "raw_csv" => export_raw_csv(path, session)?,
        "template_xlsx" => export_template(path)?,
        _ => return Err(AppError::Validation("未知导出类型".into())),
    }
    Ok(OperationResult {
        message: format!("已导出到 {}", path.display()),
        path: Some(path.to_string_lossy().into_owned()),
    })
}

fn csv_writer(path: &Path) -> Result<csv::Writer<std::fs::File>, AppError> {
    let mut file = fs::File::create(path).map_err(|error| AppError::Export(error.to_string()))?;
    file.write_all(&[0xef, 0xbb, 0xbf])
        .map_err(|error| AppError::Export(error.to_string()))?;
    Ok(csv::Writer::from_writer(file))
}

fn export_summary_csv(path: &Path, session: &StoredSession) -> Result<(), AppError> {
    let mut writer = csv_writer(path)?;
    writer
        .write_record([
            "姓名",
            "身份证号",
            "手机号",
            "户籍地",
            "年龄",
            "性别",
            "记录总数",
            "7天最大次数",
            "30天最大次数",
            "365天最大次数",
            "重合天数",
            "非重合超3天数",
            "风险分",
            "风险等级",
            "预警摘要",
        ])
        .map_err(|error| AppError::Export(error.to_string()))?;
    for item in &session.analyses {
        let person = &item.summary;
        writer
            .write_record([
                safe(&person.name),
                safe(&person.id_no),
                safe(&person.phone),
                safe(&person.household_region),
                person.age.map(|age| age.to_string()).unwrap_or_default(),
                safe(&person.gender),
                person.total_records.to_string(),
                person.max_week_count.to_string(),
                person.max_month_count.to_string(),
                person.max_year_count.to_string(),
                person.overlap_days.to_string(),
                person.sequential_days.to_string(),
                person.score.to_string(),
                person.level.clone(),
                safe(&person.alert_titles.join("；")),
            ])
            .map_err(|error| AppError::Export(error.to_string()))?;
    }
    writer
        .flush()
        .map_err(|error| AppError::Export(error.to_string()))
}

fn export_raw_csv(path: &Path, session: &StoredSession) -> Result<(), AppError> {
    let mut writer = csv_writer(path)?;
    writer
        .write_record([
            "源文件",
            "源表行号",
            "姓名",
            "身份证号",
            "手机号",
            "户籍地",
            "旅馆名称",
            "省",
            "市",
            "县区",
            "地址",
            "房间号",
            "入住时间",
            "登记时间",
            "退房时间",
            "数据问题",
        ])
        .map_err(|error| AppError::Export(error.to_string()))?;
    for record in session
        .records
        .iter()
        .filter(|record| within_analysis_time_window(record, &session.settings))
    {
        writer
            .write_record([
                safe(&record.source_file),
                record.source_row.to_string(),
                safe(&record.name),
                safe(&record.id_no),
                safe(&record.phone),
                safe(&record.household_region),
                safe(&record.hotel_name),
                safe(&record.province),
                safe(&record.city),
                safe(&record.county),
                safe(&record.address),
                safe(&record.room_no),
                safe(&record.check_in_text),
                safe(&record.register_time_text),
                safe(&record.check_out_text),
                safe(&record.issues.join("；")),
            ])
            .map_err(|error| AppError::Export(error.to_string()))?;
    }
    writer
        .flush()
        .map_err(|error| AppError::Export(error.to_string()))
}

fn export_risk_xlsx(path: &Path, session: &StoredSession) -> Result<(), AppError> {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet
        .set_name("风险人员")
        .map_err(|error| AppError::Export(error.to_string()))?;
    let header = Format::new()
        .set_bold()
        .set_align(FormatAlign::Center)
        .set_background_color("#E8EEF3");
    let headers = [
        "姓名",
        "身份证号",
        "手机号",
        "户籍地",
        "年龄",
        "性别",
        "风险等级",
        "风险分",
        "预警类型",
        "级别",
        "标题",
        "说明",
        "证据数量",
    ];
    for (column, value) in headers.iter().enumerate() {
        worksheet
            .write_string_with_format(0, column as u16, *value, &header)
            .map_err(|error| AppError::Export(error.to_string()))?;
    }
    let mut row = 1;
    for item in session
        .analyses
        .iter()
        .filter(|item| !item.alerts.is_empty())
    {
        for alert in &item.alerts {
            let values = [
                item.summary.name.clone(),
                item.summary.id_no.clone(),
                item.summary.phone.clone(),
                item.summary.household_region.clone(),
                item.summary
                    .age
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                item.summary.gender.clone(),
                item.summary.level.clone(),
                item.summary.score.to_string(),
                alert.kind.clone(),
                alert.severity.clone(),
                alert.title.clone(),
                alert.detail.clone(),
                alert.evidence_count.to_string(),
            ];
            for (column, value) in values.iter().enumerate() {
                worksheet
                    .write_string(row, column as u16, value)
                    .map_err(|error| AppError::Export(error.to_string()))?;
            }
            row += 1;
        }
    }
    worksheet.autofit();
    workbook
        .save(path)
        .map_err(|error| AppError::Export(error.to_string()))
}

fn export_template(path: &Path) -> Result<(), AppError> {
    let bytes = include_bytes!("../resources/旅馆业数据导入模板.xlsx");
    fs::write(path, bytes).map_err(|error| AppError::Export(error.to_string()))
}

fn safe(value: &str) -> String {
    if value
        .chars()
        .next()
        .is_some_and(|value| matches!(value, '=' | '+' | '-' | '@' | '\t' | '\r'))
    {
        format!("'{value}")
    } else {
        value.to_string()
    }
}
