use crate::error::AppError;
use crate::model::{calculate_age, ImportStats, Record};
use calamine::{open_workbook_auto, Reader};
use chrono::{Duration, Local, NaiveDate, NaiveDateTime};
use encoding_rs::GBK;
use regex::Regex;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use walkdir::WalkDir;

const SUPPORTED_EXTENSIONS: [&str; 3] = ["xls", "xlsx", "csv"];
const FIELDS: [&str; 21] = [
    "name",
    "id_no",
    "phone",
    "household_province",
    "household_city",
    "household_county",
    "household_region",
    "household_address",
    "birth_date",
    "age",
    "gender",
    "hotel_name",
    "province",
    "city",
    "county",
    "region",
    "address",
    "room_no",
    "check_in",
    "register_time",
    "check_out",
];

pub struct ImportedData {
    pub records: Vec<Record>,
    pub stats: ImportStats,
    pub file_count: usize,
    pub title: String,
}

pub fn expand_folders(paths: &[String]) -> Vec<String> {
    let mut files = Vec::new();
    for path in paths {
        for entry in WalkDir::new(path).into_iter().filter_map(Result::ok) {
            if entry.file_type().is_file() && is_supported(entry.path()) {
                files.push(entry.path().to_string_lossy().into_owned());
            }
        }
    }
    files.sort_by_key(|value| value.to_lowercase());
    files
}

pub fn import_paths(paths: &[String]) -> Result<ImportedData, AppError> {
    let files: Vec<PathBuf> = paths
        .iter()
        .map(PathBuf::from)
        .filter(|path| is_supported(path))
        .collect();
    if files.is_empty() {
        return Err(AppError::Validation(
            "请选择 .xls、.xlsx 或 .csv 文件".into(),
        ));
    }

    let mut stats = ImportStats::default();
    let mut records = Vec::new();
    let mut seen = HashSet::new();
    let mut uid = 1_u64;
    let mut reasons = Vec::new();

    for path in &files {
        let rows = read_table(path)?;
        let (header_index, mut indexes, score) = detect_header_row(&rows);
        let data_start = if indexes.get("id_no").is_some_and(|items| !items.is_empty())
            && indexes
                .get("check_in")
                .is_some_and(|items| !items.is_empty())
        {
            header_index + 1
        } else if let Some(start) = detect_template_data_start(&rows) {
            indexes = template_indexes();
            start
        } else if let Some((start, inferred)) = infer_core_fields(&rows, indexes) {
            indexes = inferred;
            start
        } else {
            reasons.push(format!(
                "{} 未识别到证件号码或入住时间列（表头得分 {}）",
                file_name(path),
                score
            ));
            continue;
        };

        for (row_index, row) in rows.iter().enumerate().skip(data_start) {
            if row.iter().all(|value| value.trim().is_empty()) {
                continue;
            }
            let id_no = compact_identity(&pick(row, indexes.get("id_no")));
            let check_in_text = pick(row, indexes.get("check_in"));
            if id_no.is_empty() || check_in_text.trim().is_empty() {
                stats.missing_id_count += 1;
                continue;
            }

            let check_in = parse_datetime(&check_in_text);
            let check_out_text = pick(row, indexes.get("check_out"));
            let check_out = parse_datetime(&check_out_text);
            let register_time_text = pick(row, indexes.get("register_time"));
            let register_time = parse_datetime(&register_time_text);
            let mut issues = Vec::new();
            if check_in.is_none() {
                issues.push("入住时间无法解析".into());
            }
            if !check_out_text.is_empty() && check_out.is_none() {
                issues.push("退房时间无法解析".into());
            }
            if let (Some(start), Some(end)) = (check_in, check_out) {
                if end <= start {
                    issues.push("退房时间早于或等于入住时间".into());
                }
                if end - start < Duration::minutes(10) {
                    stats.short_stay_count += 1;
                    continue;
                }
            }

            let area = lookup_identity_area(&id_no);
            let source_household = pick(row, indexes.get("household_region"));
            let household_region = if area.region().is_empty() {
                source_household
            } else {
                area.region()
            };
            let birth = identity_birth_date(&id_no)
                .or_else(|| parse_date(&pick(row, indexes.get("birth_date"))));
            let age = parse_age(&pick(row, indexes.get("age")))
                .or_else(|| calculate_age(birth, Local::now().date_naive()));
            let gender = normalize_gender(&pick(row, indexes.get("gender")), &id_no);

            let record = Record {
                uid,
                source_file: file_name(path),
                source_row: row_index + 1,
                name: pick(row, indexes.get("name")),
                id_no: id_no.clone(),
                phone: pick(row, indexes.get("phone")),
                hotel_name: pick(row, indexes.get("hotel_name")),
                province: pick(row, indexes.get("province")),
                city: pick(row, indexes.get("city")),
                county: pick(row, indexes.get("county")),
                region: pick(row, indexes.get("region")),
                address: pick(row, indexes.get("address")),
                room_no: pick(row, indexes.get("room_no")),
                check_in_text,
                register_time_text,
                check_out_text,
                check_in,
                register_time,
                check_out,
                person_key: format!("id:{id_no}"),
                household_province: nonempty(
                    pick(row, indexes.get("household_province")),
                    &area.province,
                ),
                household_city: nonempty(pick(row, indexes.get("household_city")), &area.city),
                household_county: nonempty(
                    pick(row, indexes.get("household_county")),
                    &area.county,
                ),
                household_region,
                household_address: pick(row, indexes.get("household_address")),
                age,
                gender,
                issues,
            };
            let key = deduplication_key(&record);
            if !seen.insert(key) {
                stats.duplicate_count += 1;
                continue;
            }
            records.push(record);
            uid += 1;
        }
    }

    if records.is_empty() {
        let detail = if reasons.is_empty() {
            "记录为空、缺少必填字段，或全部入住不足 10 分钟".into()
        } else {
            reasons.join("；")
        };
        return Err(AppError::Empty(detail));
    }
    stats.imported = records.len();
    let title = if files.len() == 1 {
        file_name(&files[0])
    } else {
        format!("{} 个导入文件", files.len())
    };
    Ok(ImportedData {
        records,
        stats,
        file_count: files.len(),
        title,
    })
}

fn read_table(path: &Path) -> Result<Vec<Vec<String>>, AppError> {
    match extension(path).as_str() {
        "csv" => read_csv(path),
        "xls" | "xlsx" => read_workbook(path),
        _ => Err(AppError::Validation(format!(
            "不支持的文件格式：{}",
            file_name(path)
        ))),
    }
}

fn read_workbook(path: &Path) -> Result<Vec<Vec<String>>, AppError> {
    let mut workbook = open_workbook_auto(path)
        .map_err(|error| AppError::Read(format!("{}：{error}", file_name(path))))?;
    let mut best_rows = Vec::new();
    let mut best_score = 0;
    for sheet_name in workbook.sheet_names().to_owned() {
        let range = workbook.worksheet_range(&sheet_name).map_err(|error| {
            AppError::Parse(format!("{} / {}：{error}", file_name(path), sheet_name))
        })?;
        let rows = range
            .rows()
            .map(|row| row.iter().map(ToString::to_string).collect::<Vec<_>>())
            .filter(|row| row.iter().any(|value| !value.trim().is_empty()))
            .collect::<Vec<_>>();
        if rows.is_empty() {
            continue;
        }
        if detect_template_data_start(&rows).is_some() {
            return Ok(rows);
        }
        let (_, indexes, score) = detect_header_row(&rows);
        if indexes.get("id_no").is_some_and(|items| !items.is_empty())
            && indexes
                .get("check_in")
                .is_some_and(|items| !items.is_empty())
        {
            return Ok(rows);
        }
        if infer_core_fields(&rows, indexes).is_some() {
            return Ok(rows);
        }
        if score > best_score {
            best_score = score;
            best_rows = rows;
        }
    }
    (!best_rows.is_empty())
        .then_some(best_rows)
        .ok_or_else(|| AppError::Empty(format!("{} 中没有可读取的数据工作表", file_name(path))))
}

fn read_csv(path: &Path) -> Result<Vec<Vec<String>>, AppError> {
    let bytes = fs::read(path)?;
    let content = if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        String::from_utf8_lossy(&bytes[3..]).into_owned()
    } else if bytes.starts_with(&[0xff, 0xfe]) {
        decode_utf16(&bytes[2..], true)
    } else if bytes.starts_with(&[0xfe, 0xff]) {
        decode_utf16(&bytes[2..], false)
    } else if let Ok(text) = String::from_utf8(bytes.clone()) {
        text
    } else {
        let (text, _, _) = GBK.decode(&bytes);
        text.into_owned()
    };
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(content.as_bytes());
    reader
        .records()
        .map(|row| {
            row.map(|record| record.iter().map(str::to_string).collect())
                .map_err(|error| AppError::Parse(format!("{}：{error}", file_name(path))))
        })
        .collect()
}

fn decode_utf16(bytes: &[u8], little_endian: bool) -> String {
    let units = bytes.chunks_exact(2).map(|pair| {
        if little_endian {
            u16::from_le_bytes([pair[0], pair[1]])
        } else {
            u16::from_be_bytes([pair[0], pair[1]])
        }
    });
    String::from_utf16_lossy(&units.collect::<Vec<_>>())
}

type FieldIndexes = HashMap<&'static str, Vec<usize>>;

fn detect_header_row(rows: &[Vec<String>]) -> (usize, FieldIndexes, usize) {
    let mut best = (0, empty_indexes(), 0);
    for (index, row) in rows.iter().take(200).enumerate() {
        let indexes = compile_field_indexes(row);
        let recognized = indexes.values().filter(|items| !items.is_empty()).count();
        let score = recognized
            + usize::from(!indexes["id_no"].is_empty()) * 8
            + usize::from(!indexes["check_in"].is_empty()) * 5
            + usize::from(!indexes["name"].is_empty()) * 2
            + usize::from(!indexes["hotel_name"].is_empty()) * 2;
        if score > best.2 {
            best = (index, indexes, score);
        }
    }
    best
}

fn compile_field_indexes(headers: &[String]) -> FieldIndexes {
    let normalized: Vec<String> = headers
        .iter()
        .map(|value| normalize_header(value))
        .collect();
    let mut result = empty_indexes();
    for field in FIELDS {
        let aliases = aliases(field)
            .iter()
            .map(|value| normalize_header(value))
            .collect::<Vec<_>>();
        let exact = normalized
            .iter()
            .enumerate()
            .filter_map(|(index, header)| aliases.contains(header).then_some(index))
            .collect::<Vec<_>>();
        if !exact.is_empty() {
            result.insert(field, exact);
            continue;
        }
        let fuzzy = normalized
            .iter()
            .enumerate()
            .filter_map(|(index, header)| {
                aliases
                    .iter()
                    .any(|alias| alias.chars().count() >= 3 && header.contains(alias))
                    .then_some(index)
            })
            .collect();
        result.insert(field, fuzzy);
    }
    result
}

fn detect_template_data_start(rows: &[Vec<String>]) -> Option<usize> {
    rows.iter().take(500).position(|row| {
        row.len() > 18
            && is_identity_number(&row[4])
            && parse_datetime(&row[7]).is_some()
            && (!row[0].trim().is_empty() || !row[10].trim().is_empty())
    })
}

fn infer_core_fields(
    rows: &[Vec<String>],
    mut indexes: FieldIndexes,
) -> Option<(usize, FieldIndexes)> {
    let max_columns = rows.iter().take(500).map(Vec::len).max().unwrap_or(0);
    let mut id_scores = vec![0; max_columns];
    let mut date_scores = vec![0; max_columns];
    for row in rows.iter().take(500) {
        for (index, value) in row.iter().enumerate() {
            if is_identity_number(value) {
                id_scores[index] += 1;
            }
            if parse_datetime(value).is_some_and(|value| value.year() >= 2000) {
                date_scores[index] += 1;
            }
        }
    }
    if indexes["id_no"].is_empty() {
        if let Some((index, _)) = id_scores
            .iter()
            .enumerate()
            .max_by_key(|(_, score)| *score)
            .filter(|(_, score)| **score > 0)
        {
            indexes.insert("id_no", vec![index]);
        }
    }
    if indexes["check_in"].is_empty() {
        if let Some((index, _)) = date_scores
            .iter()
            .enumerate()
            .max_by_key(|(_, score)| *score)
            .filter(|(_, score)| **score > 0)
        {
            indexes.insert("check_in", vec![index]);
        }
    }
    let (Some(id_column), Some(date_column)) = (
        indexes["id_no"].first().copied(),
        indexes["check_in"].first().copied(),
    ) else {
        return None;
    };
    let start = rows.iter().position(|row| {
        row.get(id_column)
            .is_some_and(|value| is_identity_number(value))
            && row
                .get(date_column)
                .and_then(|value| parse_datetime(value))
                .is_some()
    })?;
    Some((start, indexes))
}

fn template_indexes() -> FieldIndexes {
    let mut indexes = empty_indexes();
    for (field, values) in [
        ("name", vec![0]),
        ("id_no", vec![4]),
        ("phone", vec![18]),
        ("household_region", vec![5, 16]),
        ("household_address", vec![6]),
        ("birth_date", vec![2]),
        ("gender", vec![1]),
        ("hotel_name", vec![10]),
        ("county", vec![15, 22]),
        ("address", vec![17]),
        ("room_no", vec![9]),
        ("check_in", vec![7]),
        ("register_time", vec![13, 14]),
        ("check_out", vec![8]),
    ] {
        indexes.insert(field, values);
    }
    indexes
}

fn empty_indexes() -> FieldIndexes {
    FIELDS
        .into_iter()
        .map(|field| (field, Vec::new()))
        .collect()
}

fn aliases(field: &str) -> &'static [&'static str] {
    match field {
        "name" => &["姓名", "入住人姓名", "旅客姓名", "人员姓名"],
        "id_no" => &[
            "证件号码",
            "身份证号",
            "身份证号码",
            "身份证件号码",
            "证件号",
            "公民身份号码",
            "旅客证件号码",
            "入住人身份证号",
        ],
        "phone" => &["联系电话", "手机号", "手机号码", "联系方式", "入住人手机号"],
        "hotel_name" => &[
            "旅馆名称",
            "酒店名称",
            "旅店名称",
            "场所名称",
            "住房场所名称",
        ],
        "province" => &["省", "省份"],
        "city" => &["市", "城市"],
        "county" => &["县", "区县", "县区"],
        "region" => &["地域省市县", "行政区划", "所属地区", "省市县", "辖区"],
        "address" => &["地址", "酒店地址", "旅馆地址", "详细地址", "场所地址"],
        "room_no" => &["房间号", "房号", "客房号", "入住房号", "房号手牌号"],
        "check_in" => &[
            "入住时间",
            "入住日期时间",
            "住宿时间",
            "开房时间",
            "到店时间",
        ],
        "register_time" => &[
            "登记时间",
            "录入时间",
            "报送时间",
            "上传时间",
            "传送时间",
            "入库时间",
        ],
        "check_out" => &["退房时间", "退租时间", "离店时间", "退宿时间"],
        "household_province" => &["户籍省"],
        "household_city" => &["户籍市"],
        "household_county" => &["户籍县区"],
        "household_region" => &["户籍地区划", "籍贯", "户籍地"],
        "household_address" => &["户籍地详址", "户籍地址"],
        "birth_date" => &["出生日期", "出生年月"],
        "age" => &["年龄"],
        "gender" => &["性别"],
        _ => &[],
    }
}

fn pick(row: &[String], indexes: Option<&Vec<usize>>) -> String {
    indexes
        .into_iter()
        .flatten()
        .filter_map(|index| row.get(*index))
        .find(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

fn normalize_header(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .filter(|char| !char.is_whitespace() && !"()（）:：_-./、\u{feff}\u{200b}".contains(*char))
        .collect()
}

fn parse_datetime(value: &str) -> Option<NaiveDateTime> {
    let text = value.trim();
    if text.is_empty() {
        return None;
    }
    if let Ok(serial) = text.parse::<f64>() {
        if (20_000.0..90_000.0).contains(&serial) {
            let epoch = NaiveDate::from_ymd_opt(1899, 12, 30)?.and_hms_opt(0, 0, 0)?;
            return Some(epoch + Duration::milliseconds((serial * 86_400_000.0).round() as i64));
        }
    }
    if text.chars().all(|char| char.is_ascii_digit()) && matches!(text.len(), 8 | 12 | 14) {
        let format = match text.len() {
            8 => "%Y%m%d",
            12 => "%Y%m%d%H%M",
            _ => "%Y%m%d%H%M%S",
        };
        if text.len() == 8 {
            return NaiveDate::parse_from_str(text, format)
                .ok()?
                .and_hms_opt(0, 0, 0);
        }
        if let Ok(value) = NaiveDateTime::parse_from_str(text, format) {
            return Some(value);
        }
    }
    let cleaned = text
        .replace(['年', '月'], "/")
        .replace('日', "")
        .replace(['.', '-'], "/")
        .replace('T', " ");
    [
        "%Y/%m/%d %H:%M:%S",
        "%Y/%m/%d %H:%M",
        "%Y/%m/%d %H%M%S",
        "%Y/%m/%d %H%M",
    ]
    .iter()
    .find_map(|format| NaiveDateTime::parse_from_str(&cleaned, format).ok())
    .or_else(|| {
        NaiveDate::parse_from_str(&cleaned, "%Y/%m/%d")
            .ok()?
            .and_hms_opt(0, 0, 0)
    })
}

fn parse_date(value: &str) -> Option<NaiveDate> {
    parse_datetime(value).map(|value| value.date())
}
fn parse_age(value: &str) -> Option<u8> {
    static AGE: OnceLock<Regex> = OnceLock::new();
    let capture = AGE
        .get_or_init(|| Regex::new(r"\d{1,3}").unwrap())
        .find(value)?
        .as_str()
        .parse::<u8>()
        .ok()?;
    (capture <= 130).then_some(capture)
}
fn compact_identity(value: &str) -> String {
    value.split_whitespace().collect::<String>().to_uppercase()
}
fn is_identity_number(value: &str) -> bool {
    identity_regex().is_match(&compact_identity(value))
}
fn identity_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| Regex::new(r"^(?:\d{17}[\dX]|\d{15})$").unwrap())
}
fn identity_birth_date(id_no: &str) -> Option<NaiveDate> {
    if id_no.len() == 18 {
        NaiveDate::parse_from_str(&id_no[6..14], "%Y%m%d").ok()
    } else if id_no.len() == 15 {
        NaiveDate::parse_from_str(&format!("19{}", &id_no[6..12]), "%Y%m%d").ok()
    } else {
        None
    }
}
fn normalize_gender(value: &str, id_no: &str) -> String {
    let text = value.trim().to_lowercase();
    if text.contains('男') || matches!(text.as_str(), "m" | "male" | "1") {
        return "男".into();
    }
    if text.contains('女') || matches!(text.as_str(), "f" | "female" | "2") {
        return "女".into();
    }
    let index = if id_no.len() == 18 {
        16
    } else if id_no.len() == 15 {
        14
    } else {
        return String::new();
    };
    id_no
        .as_bytes()
        .get(index)
        .and_then(|value| char::from(*value).to_digit(10))
        .map(|value| if value % 2 == 1 { "男" } else { "女" }.into())
        .unwrap_or_default()
}

fn deduplication_key(record: &Record) -> String {
    [
        record.id_no.clone(),
        record.hotel_name.clone(),
        record.province.clone(),
        record.city.clone(),
        record.county.clone(),
        record.region.clone(),
        record.address.clone(),
        record.room_no.clone(),
        date_key(record.check_in, &record.check_in_text),
        date_key(record.check_out, &record.check_out_text),
    ]
    .join("\u{1f}")
}

fn date_key(parsed: Option<NaiveDateTime>, raw: &str) -> String {
    parsed
        .map(|value| format!("dt:{}", value.format("%Y-%m-%dT%H:%M:%S")))
        .unwrap_or_else(|| format!("raw:{}", raw.trim()))
}

fn nonempty(primary: String, fallback: &str) -> String {
    if primary.trim().is_empty() {
        fallback.to_string()
    } else {
        primary
    }
}
fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_lowercase()
}
fn is_supported(path: &Path) -> bool {
    SUPPORTED_EXTENSIONS.contains(&extension(path).as_str())
}
fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("未命名文件")
        .to_string()
}

#[derive(Default)]
struct IdentityArea {
    province: String,
    city: String,
    county: String,
}
impl IdentityArea {
    fn region(&self) -> String {
        [
            self.province.as_str(),
            self.city.as_str(),
            self.county.as_str(),
        ]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
    }
}

#[derive(Deserialize)]
struct AreaFile {
    codes: HashMap<String, AreaEntry>,
}
#[derive(Deserialize)]
struct AreaEntry {
    province: String,
    city: String,
    county: String,
}

fn lookup_identity_area(id_no: &str) -> IdentityArea {
    static DATA: OnceLock<AreaFile> = OnceLock::new();
    let data = DATA.get_or_init(|| {
        serde_json::from_str(include_str!("../resources/area_codes.json")).unwrap_or(AreaFile {
            codes: HashMap::new(),
        })
    });
    let code = id_no.get(..6).unwrap_or_default();
    data.codes
        .get(code)
        .map(|entry| IdentityArea {
            province: entry.province.clone(),
            city: entry.city.clone(),
            county: entry.county.clone(),
        })
        .unwrap_or_default()
}

use chrono::Datelike;
