# 麦隐研判

原 `maiyin_analysis` Python/Tkinter 工具的 Tauri 2 重构版本。应用面向旅馆业入住数据核查，敏感数据在本机解析、分析、保存和导出，不启动本地 HTTP 服务，也不接入远程接口。

## 当前实现

- React 19 + TypeScript + Vite 产品界面。
- Tauri 2 桌面壳与原生文件选择。
- Rust 后端模型、导入适配器、风险分析、历史持久化与导出模块。
- `.xls`、`.xlsx`、`.csv` 文件和文件夹递归导入入口。
- 表头识别、模板固定列位、核心字段推断、身份证户籍地映射、去重、短入住过滤。
- 同日重合、同日多次非重合、30 天高频、365 天高频预警和评分。
- 历史加载、多选合并、存放目录迁移、人员详情和证据读取。
- 人员汇总 CSV、规范化明细 CSV、风险 Excel、导入模板导出。
- 浏览器演示适配器。没有 Rust 时仍可预览完整界面和交互状态，但不会解析真实文件。

文件夹导入会递归扫描全部子目录，扩展名大小写不敏感，并跳过其他格式。空目录、路径不存在或目录遍历失败会返回明确错误，不会静默停留在导入状态。

## 架构

```text
Excel / CSV
    ↓
Rust importer → normalized records → Rust analysis → versioned local sessions
                                          ↓
React AppApi contract ← Tauri commands ← summaries / detail-on-demand
    ↓
table, filters, detail inspector, export feedback
```

生产环境只保留一套 Rust 业务规则。TypeScript 负责界面、查询状态和展示格式，不复制风险评分逻辑。Tauri 命令采用粗粒度调用，人员证据按需读取，避免在 WebView 和 Rust 之间反复传输全部原始记录。

## 浏览器预览

当前电脑只安装了 Node.js，可以直接运行：

```powershell
npm install
npm run dev
```

如果 `registry.npmjs.org` 在当前网络超时，可只对本次安装使用镜像，不会修改全局配置：

```powershell
npm install --registry=https://registry.npmmirror.com
```

打开 `http://127.0.0.1:1420`。页面顶部会明确显示“浏览器演示模式”。

## Tauri 桌面运行

Windows 需要：

1. Microsoft C++ Build Tools，包含“使用 C++ 的桌面开发”。
2. Microsoft Edge WebView2 Runtime。
3. Rust stable 工具链。

安装 Rust：

```powershell
winget install --id Rustlang.Rustup
```

重新打开终端后验证：

```powershell
rustc --version
cargo --version
```

运行桌面开发版：

```powershell
npm run tauri dev
```

构建安装包：

```powershell
npm run tauri build
```

## 质量检查

```powershell
npm run lint
npm run test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

当前环境已确认使用 `rustc 1.96.0` 和 `cargo 1.96.0`。前端 lint、3 项测试和生产构建通过；Rust `cargo check`、2 项测试及 Clippy 零警告检查通过；`tauri build --no-bundle` 已生成原生 EXE：

```text
src-tauri\target\release\maiyin-analysis.exe
```

MSI 和 NSIS 打包需要分别从 GitHub 下载 WiX/NSIS 工具。当前网络下载发生全局超时，因此本轮确认了 EXE 构建，但没有生成安装器。

## 老式 `.xls` 兼容闸门

原 Python 版本特意使用 `xlrd` 读取 `.xls`，因为部分旅馆业导出文件通过 Calamine 读取时可能出现中文文本损坏。因此当前 Rust 实现不能仅凭“可以打开 `.xls`”就宣称完全等价。

正式迁移需要加入脱敏的真实 `.xls` 样本，并对照原程序验证：

- 工作表选择一致。
- 中文字段文本一致。
- 身份证号和入住时间列推断一致。
- 日期、空值和数值单元格一致。

如果 Calamine 未通过这些样本，只为 `.xls` 增加窄范围兼容读取器，其他分析、历史和导出仍保留在 Rust 中。

## 本地数据目录

Tauri 默认使用当前用户的应用数据目录，并在其中创建：

```text
MaiyinAnalysisData\sessions
MaiyinAnalysisData\index.json
MaiyinAnalysisData\exports
```

界面允许更改存放目录。迁移时复制已有会话，不删除原目录中的原始 Excel 或 CSV。
