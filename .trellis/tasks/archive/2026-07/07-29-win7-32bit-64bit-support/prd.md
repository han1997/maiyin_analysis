# 支持 Win7 32 位与 64 位运行

## Goal

让软件能在 Windows 7（32 位与 64 位）上正常运行，避免出现 32 位 Win7 无法运行的情况。

## What I already know

### 主阻塞：Tauri 2 + WebView2 与 Win7 不兼容

* **Tauri 2.11.5**（`src-tauri/Cargo.toml:27`）→ wry 0.55 / webview2-com 0.38.2，强依赖 Microsoft Edge WebView2。
* **WebView2 已于 2023-10 停止支持 Win7/8/8.1**：最后一个兼容 Win7 的 WebView2 Runtime 版本为 `110.0.1587.140`（EOL，不再接收安全更新）。
* `tauri.conf.json:38-41` 当前用 `downloadBootstrapper`——在 Win7 上会拉到不兼容的新版安装器，安装失败或装上后无法运行。
* **WebView2 是 Tauri 2 的硬依赖**，这是 Win7 支持的根本障碍。Tauri 2 官方支持基线为 Windows 10 1809+。

### 次阻塞：32 位构建工具链未配置

* 无 `rust-toolchain.toml`、无 `.cargo/config.toml`、无 CI（`.github/workflows` 不存在）。
* 现有工具链仅 `x86_64-pc-windows-msvc`（64 位）。
* npm 侧 `@tauri-apps/cli` 仅安装 `win32-x64-msvc` 原生二进制（无 ia32）。
* 产 32 位需 `rustup target add i686-pc-windows-msvc` + MSVC 32 位 C++ 工具链（供 `libsqlite3-sys` bundled 编译）+ `tauri build --target i686-pc-windows-msvc`。

### 依赖层评估（Win7/32 位无独立障碍）

* 除 `libsqlite3-sys`（bundled，需 32 位 C 编译器）外，全部纯 Rust：calamine/rxls/rust_xlsxwriter/csv/chrono/uuid/rayon/encoding_rs/lz4_flex/regex/walkdir。
* 前端 `structuredClone`（Chromium 98+）在 WebView2 110.x 上可用，非阻塞。
* SQLite 官方支持 Win7 32 位，运行时无问题。

### 当前打包配置

* `tauri.conf.json` bundle：targets="all"（nsis+msi），`webviewInstallMode=downloadBootstrapper`。
* 无 `minimumSystemVersion`、无 `webviewFixedRuntime`、无自定义 NSIS/WiX。
* 构建命令：`npm run tauri build`（无 `--target` 标志）。

## Assumptions (temporary)

* 用户可能不知道 Tauri 2 + WebView2 的 Win7 硬限制——需先告知。
* "避免 32 位 Win7 无法运行"可能指：(a) 真的在 Win7 上跑，或 (b) 至少产 32 位包让 64 位 Win7 以外的 32 位 Windows 能跑（但 WebView2 仍是障碍）。

## Open Questions

* None — 已确认双架构包 + Win10+ 方案。

## Requirements

* 产出 32 位（`i686-pc-windows-msvc`）与 64 位（`x86_64-pc-windows-msvc`）双架构安装包。
* 64 位构建不受影响（现有配置保持）。
* 32 位构建通过 `rustup target add i686-pc-windows-msvc` + `tauri build --target i686-pc-windows-msvc` 产出。
* 确保 `libsqlite3-sys`（bundled）在 32 位交叉编译时能找到 MSVC 32 位 C 编译器。
* 明确文档化最低系统要求为 Windows 10 1809+（WebView2 硬依赖）。
* 64 位质量门全绿（回归不受影响）。

## Acceptance Criteria

* [ ] 32 位构建可产出（`tauri build --target i686-pc-windows-msvc` 产出 NSIS/MSI 安装包）。
* [ ] 64 位构建不受影响（现有流程不变）。
* [ ] `cargo fmt --check`、`cargo test`、`cargo clippy -- -D warnings` 全绿（64 位回归）。
* [ ] 构建说明文档化双架构命令。

## Definition of Done

* 32 位构建命令可重复执行。
* 64 位回归不受影响。
* 质量门全绿。

## Out of Scope (explicit)

* Win7/8/8.1 支持（WebView2 EOL，不可行）。
* 冻结 EOL WebView2 110.x 固定运行时。
* 换框架（Electron/Wails/原生）。
* CI 自动化构建（仓库无 CI，手动构建即可）。
* 自动检测目标架构选择安装包。

## Technical Approach

### 1. Rust 工具链配置

* 添加 `rust-toolchain.toml`（稳定渠道，明确 `x86_64-pc-windows-msvc` 默认目标，不强制 32 位——32 位通过命令行 `--target` 指定）。
* 文档化：`rustup target add i686-pc-windows-msvc` 一次性安装 32 位目标。

### 2. `tauri.conf.json` — 明确最低系统要求

* `bundle.windows` 段加 `minimumSystemVersion: "10.0.17763"`（Win10 1809，WebView2 基线）——防止在更低版本上安装后崩溃。
* `webviewInstallMode` 保持 `downloadBootstrapper`（Win10+ 上 WebView2 可正常安装）。

### 3. 构建脚本

* `package.json` 加可选脚本（方便用户）：
  * `"tauri:build": "tauri build"`（64 位，默认）
  * `"tauri:build:32": "tauri build --target i686-pc-windows-msvc"`（32 位）
* 不改 `"tauri": "tauri"` 代理。

### 4. MSVC 32 位 C 工具链

* `libsqlite3-sys`（bundled）需 32 位 C 编译器。文档化前提：Visual Studio Build Tools 需含 "C++ build tools" + "MSVC v143 - x86/x64 build tools"（默认含 32 位）。
* 这是构建前提，不改代码。

### 5. README 文档化

* 更新 `README.md` 构建说明：双架构命令 + 最低系统要求 + 32 位前提。

## Decision (ADR-lite)

**Context**: 用户要求支持 Win7 32/64 位。探查发现 Tauri 2 强依赖 WebView2，而 Microsoft 已 2023-10 停止 Win7 上的 WebView2 支持（最后兼容版 110.x EOL）。Win7 支持在 Tauri 2 下不可行。

**Decision**: 放弃 Win7 支持，改为产 32 位 + 64 位双架构包，最低系统要求 Windows 10 1809+。32 位通过 `rustup target add i686-pc-windows-msvc` + `tauri build --target i686-pc-windows-msvc` 交叉编译产出。`tauri.conf.json` 加 `minimumSystemVersion` 防止低版本安装。

**Consequences**: Win7/8/8.1 用户无法使用（接受）。32 位 Win10+ 用户获得支持。64 位构建不受影响。依赖层全部纯 Rust（除 libsqlite3-sys 需 32 位 C 编译器），无 32 位代码障碍。

## Technical Notes

* 主文件：`src-tauri/Cargo.toml`（依赖）、`src-tauri/tauri.conf.json`（bundle/webview）。
* 质量命令：`cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`cargo test --manifest-path src-tauri/Cargo.toml`、`cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`。
* 参考研究档案：`.trellis/tasks/archive/2026-07/07-15-tauri-refactor-maiyin-analysis/research/tauri-migration.md`（Tauri 2 迁移记录）。
