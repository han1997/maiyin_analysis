# 风险导出去除空的省市地域列

## Goal

风险导出（`risk_xlsx`）的省、市、地域省市县三列在实际数据中始终为空，直接从导出中去除这三列。

## What I already know

* `risk_xlsx` 当前 26 列（`exporter.rs:279-306`），其中证据明细块（col 14-25）含：
  * col 19 = 省（`record.province`）
  * col 20 = 市（`record.city`）
  * col 22 = 地域省市县（`record.region`）
* 这三列在实际数据中始终为空——用户确认。
* 去除后列数从 26 降到 23。
* 需同步调整：headers 数组、detail 数组、col 偏移、autofilter 范围、测试断言。
* 县区（col 21 = `record.county`）保留——用户只说去掉省/市/地域省市县。

## Requirements

* 从 `risk_xlsx` 去除「省」「市」「地域省市县」三列。
* 列数 26 → 23。
* `autofilter` 范围同步调整。
* 测试断言同步更新。
* 不影响 `summary_csv` / `raw_csv` / `template_xlsx`。
* 质量门全绿。

## Out of Scope

* 改其它导出格式。
* 改 raw_csv 的省/市/地域省市县列。

## Technical Notes

* 文件：`src-tauri/src/exporter.rs`（headers :279-306、detail :398-410、col 偏移 :422-430、autofilter）。
* 质量命令：`cargo fmt --check`、`cargo test`、`cargo clippy --all-targets -- -D warnings`、`npm run lint`、`npm run test`、`npm run build`。
