# 参与开发

## 环境准备

| 依赖 | 版本 | 用途 |
| --- | --- | --- |
| Rust | 1.85+ | 后端。Windows 上还需要 Visual Studio Build Tools 的「使用 C++ 的桌面开发」工作负载 |
| Node.js | 18+ | 前端构建 |
| Python | 3.8+ | 仅用于生成示例数据（标准库即可，无需 pip 安装） |

```bash
# 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 安装 Node.js
winget install OpenJS.NodeJS.LTS
```

Windows 上如果通过 Git Bash 构建，**务必使用 `scripts/build.sh` 或 `scripts/build.ps1`**：MSYS 自带的 `/usr/bin/link.exe` 会抢占 MSVC 的链接器，导致链接失败。脚本会自动修正 PATH。

---

## 日常开发

```bash
# 后端（改完直接重启即可，缓存按文件 mtime 自动失效）
cargo run --bin salary-app-backend

# 前端热更新（在 src-tauri 目录）
cd src-tauri && npm run dev
```

前端开发服务器监听 3000 端口，并把 `/api` 与 `/healthz` 代理到 `http://localhost:5001`，因此开发时前后端同源，不需要额外配置跨域。

改动 `src-tauri/src/` 下的前端文件后需要重新执行 `npm run build`，后端才会托管新产物；也可以用 `SALARY_WEB_DIR` 指向一个自定义目录来试验其它构建产物。

> **修改 `scripts/*.ps1` 时必须保留 UTF-8 BOM。**
> Windows PowerShell 5.1 按系统 ANSI 代码页（简体中文环境为 936）读取无 BOM 的脚本，
> 中文字节的 GBK 解码会连带吃掉换行符，导致解析器错位并报
> 「表达式或语句中包含意外的标记 `}`」——这种错误看着像括号不配对，实际是编码问题。
> VS Code 里选 "Save with encoding: UTF-8 with BOM" 即可。
>
> 脚本里也刻意避免了 PowerShell 7 才有的语法（如 `$IsWindows`、`?.`），
> 以保证在 Windows PowerShell 5.1 上能直接运行。

---

## 提交前检查

```bash
cargo fmt --all -- --check      # 格式
cargo clippy --all-targets -- -D warnings   # lint
cargo test                     # 单元测试 + 集成测试
cd src-tauri && npm run typecheck && npm run build
```

CI 会跑同样的检查，任何一项失败都不会合并。

---

## 代码约定

- **错误处理**：后端统一使用 `error::AppError`，`Display` 的文案就是返回给前端的 `error` 字段。写新错误类型时同时更新 `ResponseError::status_code`。
- **认证**：需要登录的接口在处理器参数里加 `AuthUser`。**不要**用请求参数传工号——员工身份必须来自会话令牌。
- **新增路由**：在 `src-tauri/src/lib.rs` 的 `configure_routes` 里注册，并在 `docs/API.md` 里补充说明。
- **新增 Excel 字段**：解析逻辑集中在 `excel_parser.rs`，列名识别统一走 `find_column_index`（**先精确匹配、后模糊匹配**），不要在别处再写一份列名判断。
- **工号匹配**：登录校验和工资查询共用 `excel_parser::ids_match`，不要改成严格字符串相等——两张表里工号可能一个是数字、一个是文本。
- **口令**：口令的哈希与明文兼容逻辑集中在 `password.rs`，不要在别处直接比对口令字符串。
- **端口**：端口的解析、范围校验与读写集中在 `port.rs`。后端进程与托盘面板都要用它，不要各写一份——曾经就因为漏了范围校验，`SALARY_PORT=65535` 会让 HTTPS 端口和 HTTP 端口撞车，整个服务起不来。
- **路径**：所有路径都从 `paths.rs` 取，不要在别处拼 `data/` 目录。
- **前端**：TypeScript 严格模式；接口调用统一走 `services/api.ts`，不要在组件里直接用 axios。
- 注释写「为什么」，不写「做了什么」。

---

## 目录速览

| 路径 | 内容 |
| --- | --- |
| `src-tauri/src/lib.rs` | 路由注册、静态资源托管 |
| `src-tauri/src/main.rs` | 进程入口：证书、HTTP/HTTPS 监听 |
| `src-tauri/src/server.rs` | HTTP/HTTPS 服务的构建与启停 |
| `src-tauri/src/auth.rs` | 会话令牌、登录限流、提取器 |
| `src-tauri/src/password.rs` | 口令哈希（PBKDF2）与明文兼容 |
| `src-tauri/src/port.rs` | 端口解析与校验（后端与托盘共用） |
| `src-tauri/src/handlers.rs` | REST 处理器 |
| `src-tauri/src/excel_parser.rs` | Excel 解析 |
| `src-tauri/src/cache.rs` | 按 mtime 失效的缓存 |
| `src-tauri/src/paths.rs` | 数据目录与前端产物目录解析 |
| `src-tauri/src/pages/` | 前端页面（登录页 / 工资条页） |
| `src-tauri/src/services/api.ts` | 前端接口层 |
| `src-tauri/tests/` | 集成测试 |
| `scripts/hash_passwords.py` | 员工表口令批量哈希（纯标准库） |
| `tray/src/panel.rs` | 托盘控制面板窗口 |
| `tray/src/service.rs` | 服务启停、端口与状态探测 |
| `tray/src/fields.rs` | 动态字段发现与 `fields.txt` 读写 |

> 托盘面板是 Windows 原生窗口（`native-windows-gui`）。
> 它需要内嵌 comctl32 v6 清单才能正常启动，
> 相关文件是 `tray/build.rs`、`tray/salary-tray.rc` 与 `tray/salary-tray.manifest`——
> 改动清单时记得保持三者一致。

---

## 数据安全

- 不要把真实员工数据提交到版本库。
- 不要把令牌签名密钥（`data/secret.key`）提交到版本库。
- 新增日志时不要打印密码或完整令牌。
