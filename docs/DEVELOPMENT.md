# 开发指南

本文覆盖：从零把项目跑起来、日常命令清单、Windows 下的构建坑、代码风格约定。

配套文档：[ARCHITECTURE.md](ARCHITECTURE.md)（结构与设计取舍）、
[API.md](API.md)、[DATA.md](DATA.md)、[SECURITY.md](SECURITY.md)、
[TROUBLESHOOTING.md](TROUBLESHOOTING.md)、[RELEASE.md](RELEASE.md)。

---

## 1. 环境要求

| 依赖 | 版本要求 | 用途 | 必需 |
| --- | --- | --- | --- |
| Rust | **1.85 或更高**（edition 2024） | 后端服务与托盘面板 | 是 |
| Visual Studio 生成工具 | 2022，含「使用 C++ 的桌面开发」 | MSVC 链接器与 Windows SDK | 是 |
| Node.js | **20 或更高**（含 npm） | 前端构建（Vite / TypeScript） | 是 |
| Python | 3.8 或更高 | 仅用于示例数据与口令哈希脚本（只用标准库） | 否 |
| Git | 任意版本 | 版本管理 | 否 |

版本下限写在两个 crate 的 `Cargo.toml` 里：

```toml
edition = "2024"
rust-version = "1.85"
```

仓库**不固定**工具链版本：只声明 MSRV 是 1.85，实际用哪个 stable
由本机 rustup 决定，CI 则由 `dtolnay/rust-toolchain@stable` 装上
（带 `rustfmt` 与 `clippy` 两个组件）。

> 为什么不放 `rust-toolchain.toml`？因为 `channel = "stable"` 解析成的是
> **当前 rustup 安装的 host triple** 对应的工具链，而不是 `default_toolchain`。
> 混合安装的环境（本机 host 是 `x86_64-pc-windows-gnu`、默认工具链是
> `stable-x86_64-pc-windows-msvc`）里，这会让每个 `cargo` 命令都去下载
> 另一个工具链，甚至报
> `failed to install component: 'clippy-preview-...', detected conflict`。
> 需要固定时用本机的 `rustup override set` 即可，不影响仓库。
>
> 确认本机用的是 MSVC 工具链：
>
> ```powershell
> rustup toolchain list
> $env:RUSTUP_TOOLCHAIN = "stable-x86_64-pc-windows-msvc"
> cargo fmt --all -- --check
> ```

> 本项目的构建依赖 MSVC 链接器（见 [5.1](#51-linkexe-被-git-bash-抢走)），
> 所以要用 MSVC 工具链而不是 GNU。

安装 Rust：

```powershell
winget install Rustlang.Rustup
rustup update stable
rustup component add clippy rustfmt
```

安装 Node.js 20+：

```powershell
winget install OpenJS.NodeJS.LTS
```

确认工具链：

```powershell
cargo --version   # 应当是 1.85 或更高
rustc --version
node --version    # v20 或更高
npm --version
python --version
```

---

## 2. 依赖安装

### 2.1 Rust 依赖

`Cargo.lock` 已纳入版本管理，直接用锁定版本安装：

```powershell
cargo fetch --locked
```

### 2.2 前端依赖

前端在 `src-tauri/` 目录（不是仓库根目录）：

```powershell
cd src-tauri
npm ci          # 首次安装 / 依赖有更新时
npm install     # 需要新增依赖时
```

`npm ci` 要求 `package-lock.json` 与 `package.json` 一致，比 `npm install` 更严格，
CI 里用的就是它。

### 2.3 Python 依赖

`scripts/generate_sample_data.py` 与 `scripts/hash_passwords.py` **只依赖标准库**，
用 `zipfile` + `xml` 直接读写 xlsx，不需要 `pip install openpyxl`。

---

## 3. 从零跑起来

以下步骤在**仓库根目录**执行。

### 3.1 第 1 步：生成示例数据

```powershell
python scripts/generate_sample_data.py --data-dir data --months 3
```

会生成：

- `data/员工列表.xlsx`：5 名员工（含 1 名已离职），口令是**明文**；
- `data/salary/<年> 年 <月> 月员工工资.xlsx`：最近 3 个月的工资表。

同时生成一份集成测试用的固定明文样本（CI 也会生成它）：

```powershell
python scripts/generate_sample_data.py --data-dir data --months 3 `
  --employee-fixture src-tauri/tests/fixtures/员工列表-测试用.xlsx
```

### 3.2 第 2 步：把口令换成哈希

示例数据里的口令是明文，与 Python 原版一致。正式使用前应当升级成 PBKDF2 哈希：

```powershell
# 先看看会改哪些口令，不写文件
python scripts/hash_passwords.py --dry-run

# 原地替换，原文件备份为 data/员工列表.xlsx.bak
python scripts/hash_passwords.py --in-place

# 确认新表可用后，删掉明文备份
Remove-Item data/员工列表.xlsx.bak
```

脚本只处理表头里带「密码」二字的列。表里没有「密码」列时脚本会直接报错退出，
不会去动手机号 / 紧急联系人号码那几列。已经是哈希值的单元格原样保留，
所以重复执行是安全的。

### 3.3 第 3 步：构建前端

```powershell
cd src-tauri
npm ci
npm run build      # 产物在 src-tauri/dist/
cd ..
```

前端没构建时后端会返回一张「前端尚未构建」的提示页（`lib::FRONTEND_MISSING_HTML`），
服务本身仍然可用。

### 3.4 第 4 步：启动后端

```powershell
# 开发模式
cargo run --bin salary-app-backend

# 或者用脚本（会自动检查数据目录、构建前端、处理 MSVC 链接器路径）
scripts\start.ps1
scripts\start.ps1 -NoBrowser     # 不自动打开浏览器
```

启动后：

| 地址 | 说明 |
| --- | --- |
| <http://localhost:5001> | HTTP，局域网内其他电脑也能用 |
| <https://localhost:5002> | HTTPS，自签名证书，首次访问需在浏览器里确认 |
| <http://localhost:5001/healthz> | 健康检查，返回纯文本 `OK` |

示例账号（**网页端不区分管理员，所有账号权限相同**）：

| 工号 | 密码 | 备注 |
| --- | --- | --- |
| 001 | `salary2026` | 张三 / 工程部 |
| 003 | `admin2026` | 王五 / 人力资源部（历史遗留的名字，权限与其他人相同） |

第 2 步做完之后，`data/员工列表.xlsx` 里存的是 `$pbkdf2-sha256$...` 这样的哈希串，
但登录时照常填上面的明文口令即可。

### 3.5 第 5 步：启动托盘控制面板

```powershell
# 直接跑
cargo run -p salary-tray

# 或者用脚本（会先编译再启动）
scripts\start-tray.ps1
```

启动后系统托盘出现一个图标，右键菜单有四项：显示面板 / 启动服务 / 停止服务 / 退出。

> **托盘不会自动启动服务。** 服务由面板里的「启动服务」按钮按需拉起
> （跑在托盘进程内的独立线程上）。想直接跑服务就用第 3.4 步的方式。
> 托盘面板也能识别并结束用 `start.bat` 独立启动的服务。

托盘面板有三个页面：

- **服务控制**：运行状态、局域网访问地址、端口设置、账号密码设置、启停、打开浏览器、打开数据目录、
  放行防火墙端口、开机自启（注册表 `Run` 项）、深浅主题切换；
- **字段显示**：列出所有工资表里出现过的字段，切换「显示 / 跟随 / 隐藏」三态，
  保存到 `data/fields.txt`。字段列表按当前工资表自动生成，工资表有增删改时列表会自动更新；
- **使用手册**：面板内自带的使用说明，可滚动查看。

### 3.6 前端热更新开发

改前端时用 Vite 开发服务器（3000 端口），`/api` 与 `/healthz` 会自动代理到后端，
浏览器始终同源访问，不需要额外配跨域：

```powershell
# 终端 1：跑后端
cargo run --bin salary-app-backend

# 终端 2：跑前端
cd src-tauri
npm run dev       # http://localhost:3000
```

后端地址可以用环境变量覆盖：

```powershell
$env:SALARY_BACKEND = "http://localhost:5001"
```

---

## 4. 日常命令清单

### 4.1 Rust

```powershell
# 构建（整个 workspace）
cargo build

# 只构建后端
cargo build --bin salary-app-backend

# 只构建托盘
cargo build -p salary-tray

# release 构建
cargo build --release --bin salary-app-backend
cargo build --release -p salary-tray

# 运行
cargo run --bin salary-app-backend
cargo run -p salary-tray

# 测试
cargo test                        # 当前 crate
cargo test --workspace            # 整个 workspace
cargo test --workspace --locked   # CI 用的形式

# Lint：必须零告警
cargo clippy --workspace --all-targets -- -D warnings

# 格式化
cargo fmt --all
cargo fmt --all -- --check        # 只检查不修改（CI 用的形式）

# 依赖
cargo fetch --locked
cargo tree -p salary-tray         # 查依赖树
```

### 4.2 前端（在 `src-tauri/` 目录执行）

```powershell
cd src-tauri

npm ci                 # 安装依赖
npm run typecheck      # tsc --noEmit，类型检查
npm run build          # tsc --noEmit && vite build，产物到 src-tauri/dist/
npm run dev            # Vite 开发服务器（3000 端口）
npm run preview        # 预览构建产物（4173 端口）
```

### 4.3 仓库脚本

```powershell
# 构建（会自动修正 MSVC 链接器路径、切换控制台编码）
scripts\build.ps1              # 后端 debug
scripts\build.ps1 -Release     # 后端 release
scripts\build.ps1 -Test        # 跑测试
scripts\build.ps1 -Frontend    # 只构建前端
scripts\build.ps1 -Tray        # 只构建托盘
scripts\build.ps1 -All         # 前端 + 后端 + 托盘

# bash 版本（Git Bash / Linux / macOS）
scripts/build.sh
scripts/build.sh --release
scripts/build.sh --test
scripts/build.sh --frontend
scripts/build.sh --tray
scripts/build.sh --all

# 一键启动
scripts\start.ps1              # 准备数据 -> 构建前端 -> 构建后端 -> 启动服务
scripts\start.ps1 -Release
scripts\start.ps1 -NoBrowser
scripts\start-tray.ps1         # 编译并启动托盘控制面板

# 一键自检（格式 + lint + 测试 + 前端构建，末尾打印汇总表）
scripts\verify.ps1
scripts\verify.ps1 -SkipFrontend      # 跳过前端相关步骤
scripts\verify.ps1 -SkipTests         # 跳过测试
scripts\verify.ps1 -Bench             # 额外跑 cargo bench --workspace

# 数据
python scripts/generate_sample_data.py --data-dir data --months 3
python scripts/hash_passwords.py --dry-run
python scripts/hash_passwords.py --in-place

# 调试：截取托盘面板窗口（输出到 scripts/tray-panel.png）
powershell -ExecutionPolicy Bypass -File scripts\print-window.ps1

# 调试：模拟点击托盘面板（参数是画布客户区坐标）
powershell -ExecutionPolicy Bypass -File scripts\tray-click.ps1 100 268

# 调试：向正在编辑的输入框里敲字（第二个参数是先退格的次数）
powershell -ExecutionPolicy Bypass -File scripts\tray-type.ps1 "salary2026" 0

# 自检：模拟左键点托盘图标，看面板会不会被唤起
powershell -ExecutionPolicy Bypass -File scripts\tray-click-test.ps1
```

> `tray-click.ps1` / `tray-type.ps1` 用 `SendMessage` 而不是 `PostMessage`：
> 跨进程投递的 `WM_CHAR` 不一定会被消息循环取到，键盘消息用同步发送才稳。
>
> `tray-click-test.ps1` 给托盘图标的隐藏窗口（类名 `tray_icon_app`）发一条
> `WM_USER_TRAYICON` + `WM_LBUTTONUP`，等价于用户左键点图标：
> 投递路径不通就说明托盘线程没在泵窗口消息。

### 4.4 CI 做的检查

`.github/workflows/ci.yml` 分成两个 job。**托盘面板只能在 Windows 上构建**
（它依赖 `native-windows-gui` / `winapi` / `tray-icon`），所以 Linux job 只跑后端。

**job `backend`（ubuntu-latest）——后端与前端：**

```
python scripts/generate_sample_data.py
cargo fmt --all -- --check
cargo clippy -p salary-app-backend --all-targets -- -D warnings
cargo test -p salary-app-backend
cd src-tauri && npm ci
cd src-tauri && npm run typecheck
cd src-tauri && npm run build
```

**job `windows`（windows-latest）——整仓：**

```
python scripts/generate_sample_data.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench --workspace --no-run      # 只编译基准，不跑（跑起来要好几分钟）
cd src-tauri && npm ci
cd src-tauri && npm run build
```

workflow 顶层还配了并发取消（同分支的新提交会取消上一次未跑完的任务）
与 `workflow_dispatch` 手动触发。

本地想一次跑完这些检查，直接用：

```powershell
scripts\verify.ps1
```

它跑的正是上面那几步（格式、整仓 lint、整仓测试、前端类型检查、前端构建），
每步计时并在最后打印汇总表；任一步失败即退出码 1。

> **CI 的两个 job 都会先跑 `python scripts/generate_sample_data.py` 生成示例数据**，
> 所以依赖 `data/salary/` 的集成测试在 CI 上会**真正跑到**而不是静默跳过
> （跳过时打印「示例数据缺失，跳过」）。本地跑测试前同样要先生成示例数据，
> 见 [第 7.1 节](#71-集成测试全部静默跳过)。

---

## 5. Windows 下的 MSVC 链接器与路径坑

### 5.1 `link.exe` 被 Git Bash 抢走

**症状：**

```
error: linking with `link.exe` failed:
  /usr/bin/link: extra operand '...rcgu.o'
```

**原因：** Git Bash / MSYS 终端里，MSYS 自带的 `/usr/bin/link.exe`（GNU coreutils 的
`link`，不是链接器）排在 PATH 前面，抢在 MSVC 的 `link.exe` 之前被调用。

**解决办法（按推荐顺序）：**

1. **用仓库脚本构建。** `scripts/build.ps1` 与 `scripts/build.sh` 会自动探测
   Visual Studio 安装目录（`vswhere.exe` 或常见路径 `C:\Program Files\Microsoft Visual Studio\2022\...`），
   把 `VC\Tools\MSVC\<版本>\bin\HostX64\x64` 插到 PATH 最前面：

   ```powershell
   scripts\build.ps1 -All
   ```

2. **在 Visual Studio 开发者命令行（Developer Command Prompt）里构建。**
   那个环境里 `link.exe` 本来就是 MSVC 的，无需任何额外配置。

3. **手动把 MSVC 目录加到 PATH 最前面**（先自动找到 `link.exe` 在哪，再改 PATH）：

   ```powershell
   $link = Get-ChildItem "C:\Program Files\Microsoft Visual Studio\2022\*\VC\Tools\MSVC\*\bin\HostX64\x64\link.exe" |
       Sort-Object FullName -Descending | Select-Object -First 1
   $env:PATH = "$($link.DirectoryName);$env:PATH"
   link.exe /?
   ```

   最后一行应当打印 MSVC 链接器的版本信息；如果报「不是内部或外部命令」，
   说明没找到装 Visual Studio。

`.cargo/config.toml` 顶部的注释就是这件事的记录：

```toml
# 说明：在 Git Bash / MSYS 终端里，`/usr/bin/link.exe`（GNU coreutils 的 link）
# 会抢在 MSVC 的 `link.exe` 前面，导致链接失败：
#   error: linking with `link.exe` failed:
#   /usr/bin/link: extra operand '...rcgu.o'
# 解决办法是让 MSVC 的链接器目录排在 PATH 最前面。
```

> `.cargo/config.toml` 本身只配了 `[net] retry = 3`（依赖下载慢时多给 cargo 几次重试），
> 链接器路径不在配置里改，而是由脚本在运行时改 PATH。

### 5.2 中文注释导致的脚本解析错误

`scripts/*.ps1` **必须保存为「带 BOM 的 UTF-8」**。Windows PowerShell 5.1
默认按系统 ANSI 代码页（简体中文下是 936 / GBK）读取无 BOM 的脚本，
中文注释会让解析器错位，报「意外的标记 }」这类看不懂的错。

用 VS Code 保存时选 "Save with encoding: UTF-8 with BOM"。

> 同理，根目录的 `start.bat` / `start-tray.bat` 应当**只含 ASCII**：
> `cmd.exe` 按 OEM 代码页读 `.bat`，非 ASCII 字节可能吞掉换行。
> 中文提示放在 `scripts\start.ps1` 里（UTF-8 with BOM）。

### 5.3 控制台中文乱码

Windows 控制台默认代码页是 936（GBK），而 Rust 进程输出的是 UTF-8 字节，
不切换的话日志里的中文会变成乱码。三个办法：

```powershell
# 办法 1：切到 UTF-8 代码页（当前窗口有效）
chcp 65001

# 办法 2：跑仓库脚本（build.ps1 / start.ps1 / start-tray.ps1 内部已处理）
scripts\build.ps1

# 办法 3：PowerShell 里设置输出编码
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
```

### 5.4 运行目录影响数据目录

数据目录的解析顺序（`src-tauri/src/paths.rs`）是：

1. `SALARY_DATA_DIR` 环境变量；
2. `cargo run` 注入的 `CARGO_MANIFEST_DIR` → 该目录的上一级 + `data`；
3. 编译期 `CARGO_MANIFEST_DIR`（目录还在就说明在这台机器上开发）；
4. 可执行文件上两级的 `data/`（workspace 构建），否则可执行文件同级的 `data/`（便携模式）。

第 2 条意味着：**`cargo run` 时数据目录由「crate 所在目录」决定，与你在哪个目录敲命令无关。**
在 `src-tauri/` 下跑 `cargo run -p salary-tray` 和在仓库根跑指向的是同一个 `data/`。
但直接双击 `target\debug\salary-app-backend.exe`（不经过 cargo）时，
环境变量不存在，走第 4 条。

要指定别的数据目录：

```powershell
$env:SALARY_DATA_DIR = "D:\salary-data"
```

### 5.5 端口是成对的

HTTP 端口 `P` 与 HTTPS 端口 `P + 1` 是绑定的，所以端口上限是 **65534**。
`SALARY_PORT=65535` 会被判定为非法并回退到 `data/port.txt`。
详见 `src-tauri/src/port.rs`。

---

## 6. 代码风格约定

### 6.1 注释写「为什么」，不写「做了什么」

这是仓库里最一致的一条约定。已有的注释基本都在解释**理由、约束和历史坑**，
而不是复述代码。

```rust
// 不好：复述代码
// 遍历行
for row in rows { ... }

// 好：解释为什么
// 工号在员工表与工资表里可能一个是数字、一个是文本（例如「1」与「001」），
// 所以这里必须与登录校验共用 excel_parser::ids_match：
// 口径一旦分叉，员工就会遇到「能登录却查不到工资记录」。
```

其它例子：

- `paths.rs`：「数据目录与前端静态资源目录在整个进程内必须保持一致，
  因此集中在这里解析，避免各模块各写一份导致互相错位。」
- `port.rs`：「端口上限这种细节很容易漏掉一处（曾经就漏过：`SALARY_PORT=65535`
  会让 HTTPS 端口和 HTTP 端口撞车，服务整个起不来）。」
- `canvas.rs`：「整块重绘会让窗口在鼠标移动时看起来在闪，所以能定位到具体区域的改动都走这里。」

**写注释的判据：** 如果一句话只是在描述代码在做什么，删掉它；
如果它解释了「为什么这样写」「以前踩过什么坑」「不这么写会出什么问题」，留下它。

### 6.2 注释与文档用中文

Rust 源码、脚本注释、文档全部用中文。`docs/` 下的文档也全部是中文。
标识符、类型名、函数名仍用英文。

文档字符串用 `///`（三斜杠）写给 API 使用者看，模块顶部用 `//!` 写模块说明。

### 6.3 路径一律走 `src-tauri/src/paths.rs`

**不要在业务代码里直接拼路径。** 所有运行时路径都必须通过 `paths` 模块拿：

```rust
// 正确
let file = paths::employee_file();
let dir = paths::salary_dir();
let key = paths::secret_key_file();
let log = paths::audit_log_file();
let config = paths::field_config_file();
let port = paths::port_file();
let certs = paths::cert_dir();

// 错误：硬编码相对路径
let file = PathBuf::from("data/员工列表.xlsx");
```

理由：`paths` 统一处理了 `SALARY_DATA_DIR` 覆盖、发布模式与开发模式的差异、
以及 `OnceLock` 缓存（在进程内只解析一次，避免环境变量中途变化导致路径漂移）。
散落各处的相对路径会让托盘面板与后端服务指向不同的数据目录。

托盘 crate 里也一样：托盘通过 `salary_app_backend::paths` 拿路径，
不自己拼。

### 6.4 新增 / 删除路由必须同步 `docs/API.md`

`docs/API.md` 是接口清单的唯一权威来源。改动 `src-tauri/src/lib.rs` 的
`configure_routes` 时，必须同步更新 `docs/API.md`：

- 新增接口 → 补上路径、方法、认证要求、请求体、响应体、状态码；
- 删除接口 → 从 `docs/API.md` 里删掉，并考虑是否需要迁移说明；
- 改字段或状态码 → 同步改。

判断标准：**只看 `docs/API.md`，就应该能写出正确的客户端代码。**

### 6.5 其它约定

| 约定 | 说明 |
| --- | --- |
| 错误处理 | 后端统一用 `error::AppError`（`thiserror`），`Display` 文案即返回给前端的 `error` 字段；`ResponseError::status_code` 负责映射 HTTP 状态码 |
| 认证 | 需要登录的处理器，参数里加 `user: AuthUser`；不要自己解析 `Authorization` 头 |
| 端口规则 | 端口解析与校验一律走 `port` 模块，不要自己写 `parse` |
| 幂等比较 | 比较口令、签名等敏感值时用常量时间比较（`constant_time_eq`），不要用 `==` |
| 缓存 | 解析 Excel 的结果走 `cache::DataCache`，不要在处理器里直接调 calamine |
| 托盘绘制 | 画布只负责「画」和「报动作」，业务逻辑放 `panel.rs`；耗时操作走 `Action` 通道，不要在窗口过程里做 |
| 主题 | 新增颜色一律加进 `theme::Palette`，不要在绘制代码里写死 RGB |
| 格式化 | 提交前跑 `cargo fmt --all`；CI 会用 `--check` 校验 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` 必须零告警 |
| 依赖版本 | `Cargo.lock` 纳入版本管理，提交时一并更新；CI 用 `--locked` 构建 |

### 6.6 测试约定

- 单元测试与被测代码放在同一个 `.rs` 文件的 `#[cfg(test)] mod tests` 里。
- 集成测试放在 `src-tauri/tests/` 下，每个文件是一个独立的测试二进制。
- 依赖 `data/` 目录的集成测试**在数据缺失时跳过而不是失败**：

  ```rust
  if !sample_data_ready() {
      eprintln!("示例数据缺失，跳过");
      return;
  }
  ```

- 测试代码里**不存放任何口令**。集成测试从员工表里读取登录凭据：

  ```rust
  let Some((employee_id, password)) = staff_credentials() else { ... };
  ```

- 会改写数据目录的测试单独成文件（`field_config_test.rs`），
  避免与其它集成测试互相干扰。

---

## 7. 常见坑

### 7.1 集成测试全部静默跳过

`cargo test` 跑完显示 9 个用例「跳过」，往往是因为示例数据路径不对。
集成测试用的数据目录是：

```rust
let source = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data");
```

`CARGO_MANIFEST_DIR` 指向 `src-tauri/`，仓库根目录在它**上面一级**。
多退一级会指向仓库外面，工资表拷不过来，用例就会静默跳过。

修复：

```powershell
python scripts/generate_sample_data.py --data-dir data --months 3 `
  --employee-fixture src-tauri/tests/fixtures/员工列表-测试用.xlsx
cargo test --workspace
```

### 7.2 页面提示「前端尚未构建」

后端已启动但没找到前端产物。在 `src-tauri/` 目录执行：

```powershell
cd src-tauri
npm ci
npm run build
```

### 7.3 改了 `data/fields.txt` 但界面没变

- 确认改的是**同一个** `data/fields.txt`（看后端启动日志里的「数据目录」那一行）。
- 确认键名与表头**去首尾空白后**完全一致。表头与配置键都走
  `excel_parser::normalize_field_name` 去空白，带空格不会让配置失效，
  但「绩效」和「绩效工资」是两个不同的字段。
- 确认值写对了：`show` / `显示` / `true` / `1` / `yes` / `y` / `是` 是显示；
  `hide` / `隐藏` / `false` / `0` / `no` / `n` / `否` 是隐藏；
  其它一律按「跟随」处理（值为空或 0 时隐藏）。
- 托盘面板保存时会**重写整个文件**（保留注释、空行与无法识别的行），
  所以在面板里改完之后，手工加的注释要重新加。

### 7.4 托盘面板改了配置，后端没读到

后端**每次查询都重读** `data/fields.txt`，所以正常情况下保存即生效。
若不生效，检查：

- 托盘与后端是否指向同一个数据目录（看两边的「数据目录」输出）；
- 是否用 `SALARY_DATA_DIR` 给其中一个指了别的目录。

### 7.5 端口被占用

```powershell
# 看谁占着 5001
netstat -ano -p tcp | Select-String ":5001"

# 看进程是谁
tasklist /FI "PID eq <上一步的 PID>" /NH /FO CSV
```

常见的占用者是上一次没退干净的服务进程。托盘面板的「停止服务」只会结束
**进程名里含 `salary`** 的进程（防止误杀无关程序），别的程序需要自己处理。

### 7.6 改了后端代码但行为没变

- 缓存：Excel 换了会自动失效（按 mtime），但**代码**不会——改了解析逻辑要重启服务。
- 前端：`assets/` 下的文件带内容哈希，浏览器会长期缓存；
  `index.html` 是 `no-cache`，每次回源校验。强制刷新（Ctrl+F5）能解决大部分情况。
- 托盘面板：改的是 GDI 绘制，必须重新编译并重启托盘程序。

### 7.7 `cargo build` 报链接错误

见 [5.1](#51-linkexe-被-git-bash-抢走)。

### 7.8 托盘程序双击没反应

托盘程序是 `windows` 子系统（`#![windows_subsystem = "windows"]`），
不弹控制台窗口，出错时用消息框提示。如果连消息框都没有，多半是程序根本
没启动成功——用 `cargo run -p salary-tray` 从终端跑，能看到标准错误输出。

### 7.9 端口改了以后连不上

端口是成对的：HTTP `P`、HTTPS `P + 1`。改端口后：

- 防火墙要放行**两个**端口（托盘面板的「放行防火墙端口」按钮会一起放行）；
- 面板的「本机访问地址」和「局域网访问地址」会自动跟着变；
- 已经在浏览器里存的令牌不受影响（令牌与端口无关）。

---

## 8. 修改代码后的自检清单

一条命令跑完全部检查（格式、整仓 lint、整仓测试、前端类型检查、前端构建）：

```powershell
scripts\verify.ps1
```

或者手动逐步来：

```powershell
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd src-tauri; npm run typecheck; npm run build; cd ..
```

改动涉及接口时，额外确认 `docs/API.md` 已经同步。
改动涉及数据格式时，确认 `docs/DATA.md` 已经同步。
