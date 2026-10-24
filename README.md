# 员工工资查询系统

面向单位内部的工资自助查询服务。员工用自己的工号和密码登录后，只能看到自己每个月的工资条。

- **后端**：Rust + Actix-web，一个进程同时提供 REST API 与前端页面
- **前端**：React + TypeScript + Vite，界面与 Python 原版（`templates/login.html`、`templates/salary.html`）保持一致，构建产物由后端一并托管
- **数据源**：Excel（员工列表 + 每月工资表），解析后按文件修改时间缓存

---

## 快速开始

### 1. 准备数据

把真实数据放进 `data/` 目录（格式见 [docs/DATA.md](docs/DATA.md)）：

```
data/
├── 员工列表.xlsx          # 必需：工号、密码、姓名、部门
└── salary/
    ├── 2026 年 10 月员工工资.xlsx
    └── 2026 年 09 月员工工资.xlsx
```

没有现成数据时，可以生成一份示例数据先跑起来：

```bash
python scripts/generate_sample_data.py --data-dir data
```

### 2. 启动

有两种方式，按场景选：

**方式一：直接跑服务**（适合部署在服务器上）

Windows 上双击 `start.bat`，或在任意终端里：

```bash
# PowerShell（Windows）
scripts/start.ps1

# bash（Git Bash / Linux / macOS）
scripts/build.sh --all && cargo run --bin salary-app-backend
```

**方式二：托盘控制面板**（适合本机运维）

双击 `start-tray.bat`，或：

```bash
cargo run -p salary-tray
```

启动后系统托盘出现一个图标，右键菜单可以：

| 菜单项 | 作用 |
| --- | --- |
| 显示面板 | 打开控制面板窗口 |
| 启动服务 / 停止服务 | 启停 HTTP + HTTPS 服务 |
| 退出 | 退出程序（会一并停掉服务） |

控制面板有三个页面：

- **服务控制**：运行状态、局域网 HTTP/HTTPS 访问地址、端口设置（写入 `data/port.txt`）、
  账号密码设置（选账号、选工资表字段、读取口令，保存成网页登录凭据）、
  启停服务、打开网页面板、打开数据目录、开机自启（注册表 `Run` 项）、放行防火墙端口；
  服务意外退出时面板会自动把它拉起来（保活）。
- **字段显示**：列出所有工资表里出现过的字段，切换「显示 / 跟随 / 隐藏」三态并保存到 `data/fields.txt`。
  字段列表按当前工资表自动生成：工资表有增删改时列表会跟着更新；
- **使用手册**：面板内自带的使用说明（怎么启动、员工怎么查工资、字段三态、常见问题）。

面板是深浅两套主题（右上角切换），标题栏跟着主题走；
侧边栏 logo、标题栏图标、任务栏图标、托盘图标用的是**同一张**原版应用图标
（`tray/resources/app_icon.ico`）。

> 服务跑在托盘进程内（独立线程），因此面板启停服务不需要拉起子进程；
> 如果服务是用 `start.bat` 独立启动的，面板也能识别并结束它。

首次启动会自动安装前端依赖、构建前端、编译后端。完成后浏览器会打开：

| 地址 | 说明 |
| --- | --- |
| <http://localhost:5001> | HTTP（局域网内其它电脑也可用这个地址） |
| <https://localhost:5002> | HTTPS（自签名证书，首次访问需在浏览器里确认） |

示例数据里的账号（网页端不区分管理员，所有账号权限相同）：

| 工号 | 密码 |
| --- | --- |
| 001 | salary2026 |
| 003 | admin2026 |

> 示例员工表里存的是**哈希口令**（打开 `data/员工列表.xlsx` 看到的是
> `$pbkdf2-sha256$...` 这样的串），但登录时照常填上面的明文口令即可。
> 正式使用前请务必替换 `data/员工列表.xlsx` 中的示例账号。

---

## 常用命令

```bash
# 构建
scripts/build.sh              # 后端 debug
scripts/build.sh --release    # 后端 release
scripts/build.sh --frontend   # 只构建前端
scripts/build.sh --tray       # 只构建托盘控制面板
scripts/build.sh --test       # 运行测试
scripts/build.sh --all        # 前端 + 后端

# 直接用 cargo
cargo build --release --bin salary-app-backend
cargo build --release -p salary-tray
cargo run -p salary-tray        # 启动托盘控制面板
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all

# 员工表口令升级（把明文换成哈希）
python scripts/hash_passwords.py --dry-run
python scripts/hash_passwords.py

# 前端（在 src-tauri 目录）
cd src-tauri
npm install
npm run dev        # Vite 开发服务器（3000 端口，/api 自动代理到后端）
npm run build      # 构建到 src-tauri/dist
npm run typecheck  # TypeScript 类型检查
```

---

## 架构

```
浏览器
  │  http://localhost:5001
  ▼
┌──────────────────────────────────────────┐
│ Actix-web（同一个进程，两个监听端口）      │
│                                          │
│  /api/*        REST 接口                 │
│  /其它路径      前端 SPA（React 构建产物） │
│    /            工资条页                  │
└──────────────┬───────────────────────────┘
               │
      ┌────────┴────────┐
      ▼                 ▼
  DataCache         审计日志
  （Moka，按 mtime 失效）  data/audit.log
      │
      ▼
  data/员工列表.xlsx
  data/salary/*.xlsx

托盘控制面板（tray/，Windows 原生窗口）
  ├── 托盘图标：显示面板 / 启停服务 / 退出
  ├── 服务标签页：状态、地址、端口、开机自启、防火墙
  └── 字段标签页：工资条字段的三态显示配置（data/fields.txt）
```

服务可以独立运行（`salary-app-backend`），也可以由托盘面板在**同一进程内**拉起
（独立线程 + 独立 actix 运行时），两种方式共用同一套后端代码。

### 目录结构

```
.
├── Cargo.toml                 # workspace 定义
├── Cargo.lock                 # 依赖锁定（已纳入版本管理）
├── start.bat                  # Windows 一键启动服务
├── start-tray.bat             # Windows 一键启动托盘控制面板
├── scripts/
│   ├── build.sh               # bash 构建脚本（自动修正 MSVC 链接器路径）
│   ├── build.ps1              # PowerShell 构建脚本
│   ├── start.ps1              # PowerShell 一键启动服务
│   ├── start-tray.ps1         # PowerShell 启动托盘控制面板
│   ├── generate_sample_data.py # 示例数据生成器（仅用标准库）
│   └── hash_passwords.py      # 员工表口令批量哈希（仅用标准库）
├── data/                      # 运行时数据（Excel、证书、密钥、日志）
│   ├── README.md
│   ├── 员工列表.xlsx
│   └── salary/*.xlsx
├── docs/                      # 文档（索引见 docs/README.md）
│   ├── README.md              # 文档索引
│   ├── API.md                 # REST 接口文档
│   ├── DATA.md                # 数据格式说明
│   ├── ARCHITECTURE.md        # 架构与调用链
│   ├── DEVELOPMENT.md         # 开发环境与代码约定
│   ├── TESTING.md             # 测试分层与用例清单
│   ├── PERFORMANCE.md         # 性能基线
│   ├── SECURITY.md            # 安全说明与部署建议
│   ├── RELEASE.md             # 发布、打包与升级
│   └── TROUBLESHOOTING.md     # 排障手册
├── src-tauri/                 # 后端 + 前端
│   ├── Cargo.toml
│   ├── index.html             # 前端入口 HTML
│   ├── package.json           # 前端依赖
│   ├── vite.config.ts
│   ├── src/                   # 前端源码（React + TypeScript）
│   │   ├── main.tsx
│   │   ├── App.tsx
│   │   ├── styles.css         # 界面样式（对齐 Python 原版模板）
│   │   ├── pages/             # 登录页 / 工资条页
│   │   ├── components/        # ProtectedRoute
│   │   └── services/api.ts
│   ├── src/                   # 后端源码（Rust）
│   │   ├── lib.rs             # 库入口：路由与静态资源托管
│   │   ├── main.rs            # 进程入口：HTTP/HTTPS 监听
│   │   ├── server.rs          # 服务构建与启停
│   │   ├── auth.rs            # 会话令牌、登录限流、认证提取器
│   │   ├── password.rs        # 口令哈希（PBKDF2）与明文兼容
│   │   ├── port.rs            # 端口解析与校验（后端与托盘共用）
│   │   ├── handlers.rs        # REST 处理器
│   │   ├── excel_parser.rs    # Excel 解析
│   │   ├── cache.rs           # 缓存
│   │   ├── models.rs          # 领域模型
│   │   ├── error.rs           # 错误类型与 HTTP 状态码映射
│   │   ├── audit.rs           # 审计日志与轮转
│   │   └── paths.rs           # 路径解析
│   └── tests/
│       ├── integration_test.rs  # HTTP 集成测试
│       ├── field_config_test.rs # 字段显示配置测试
│       ├── e2e_test.rs          # 端到端测试（真进程 + 真 TCP）
│       └── fixtures/          # 测试用 Excel 样本
└── tray/                      # 托盘控制面板（Windows 原生窗口）
    ├── Cargo.toml
    ├── build.rs               # 内嵌 comctl32 v6 清单
    ├── salary-tray.manifest
    ├── salary-tray.rc
    └── src/
        ├── main.rs            # 入口：托盘图标 + 面板
        ├── tray_icon.rs       # 托盘图标与菜单
        ├── panel.rs           # 控制面板窗口
        ├── canvas.rs          # 自绘画布：卡片、按钮、列表行、局部重绘
        ├── paint.rs           # GDI 绘制原语（圆角、发丝线、文字、直线）
        ├── theme.rs           # 配色、字体与尺寸（深浅两套主题）
        ├── brand.rs           # 品牌标记：logo / 标题栏 / 任务栏 / 托盘同一图形
        ├── service.rs         # 服务启停、端口、状态探测
        └── fields.rs          # 动态字段发现与 fields.txt 读写
```

---

## 认证

- 登录成功后签发一个无状态会话令牌：`base64url(载荷).hex(HMAC-SHA256(载荷))`，默认 30 分钟有效（与 Python 原版一致）。
- 载荷里的员工身份由服务端签发，**接口不接受工号参数**，因此无法查询他人工资。
- 令牌过期后，前端收到 401 会自动清除本地令牌并跳回登录页，登录页显示「登录状态已过期，请重新登录」。
- 登录失败按 IP 与工号两个维度计数：15 分钟内失败 5 次即锁定 15 分钟（HTTP 429）。
- 工号在员工表与工资表里的匹配口径一致：文本 `001` 与数字 `1` 视为同一人，不会出现「能登录却查不到工资」。
- 「密码」列可以存明文，也可以存 PBKDF2 哈希，两种格式可以混用，登录时自动分派。
  批量升级见 [口令哈希](#口令哈希升级)。
- 网页端不区分管理员：能登录的员工权限完全相同，只能看到自己的工资条。

### 口令哈希升级

员工表原本直接保存明文口令（与 Python 原版一致），初始口令往往就是 HR 登记的
紧急联系人号码。表一旦泄露就等于泄露了每个人的口令。

用仓库里的脚本可以批量换成哈希口令（只依赖 Python 标准库）：

```bash
# 先看看会改哪些口令，不写文件
python scripts/hash_passwords.py --dry-run

# 生成新表：data/员工列表-哈希版.xlsx，确认后再替换
python scripts/hash_passwords.py
```

脚本只处理表头里带「密码」二字的列，表里没有「密码」列时会直接报错退出，
不会去动手机号 / 紧急联系人号码那几列。重复执行是安全的——已经是哈希值的单元格原样保留。

哈希格式是 PHC 标准字符串 `$pbkdf2-sha256$<迭代次数>$<盐>$<派生密钥>`，
算法实现在 `src-tauri/src/password.rs`，与脚本的算法有跨实现的测试向量保证一致。

## 缓存

员工表和工资表解析结果缓存在内存里，缓存键是文件路径，条目记录写入时的文件修改时间。Excel 被替换后，下一次请求会自动重新解析，不需要重启服务。

## 审计日志

登录、退出、查询、导出等关键动作都会写进 `data/audit.log`（JSON Lines，每行一条）。

- 记录内容：时间、客户端 IP、动作、工号、姓名、详情；
- 客户端 IP 取自 TCP 对端地址。本系统是局域网直连部署、没有反向代理，`X-Forwarded-For` 之类的头可以被任意伪造，拿它们做审计依据会让日志失去意义；
- 日志超过 5 MB 自动轮转：当前日志改名为 `data/audit.log.1`，只保留最近一份备份；
- 审计日志只写文件、不提供网页接口，运维直接看 `data/audit.log` 即可。

## 静态资源

前端构建产物由后端一并托管，并带上缓存相关响应头：

- `assets/` 下的文件（Vite 产物带内容哈希）返回 `Cache-Control: public, max-age=31536000, immutable`，浏览器可以长期缓存；
- `index.html` 返回 `Cache-Control: no-cache`，每次回源校验，避免前端更新后员工还看到旧版本；
- 两者都带 `ETag`，浏览器带 `If-None-Match` 回来时服务端直接回 `304 Not Modified`。

## 环境变量

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `SALARY_PORT` | `5001` | HTTP 端口，HTTPS 使用端口 +1。优先级高于 `data/port.txt`。取值必须在 1–65534 之间（要预留 HTTPS 端口），越界时忽略并回退到 `data/port.txt` |
| `SALARY_DATA_DIR` | `data/` | 数据目录 |
| `SALARY_WEB_DIR` | 自动探测 | 前端构建产物目录 |
| `SALARY_NO_BROWSER` | — | 设为 `1` 时不自动打开浏览器 |
| `SALARY_ALLOW_ORIGIN` | — | 跨域策略。未设置时只允许同源访问。填 `https://example.com` 只放行这一个来源；填 `*` 或任意非空值放行全部来源 |
| `RUST_LOG` | `salary_app=debug,actix_web=info` | 日志级别 |

端口解析顺序与 Python 原版一致：`SALARY_PORT` 环境变量 → `data/port.txt` → 默认 `5001`。
端口号统一由后端的 `port` 模块校验，托盘面板与后端进程用的是同一套规则。
启动时会打印本机访问地址、局域网访问地址和监听地址，方便员工用手机连。

---

## 常见问题

**构建报 `linking with link.exe failed` / `/usr/bin/link: extra operand`**

在 Git Bash 里，MSYS 自带的 `/usr/bin/link.exe` 抢占了 MSVC 的链接器。用 `scripts/build.sh` 或 `scripts/build.ps1` 会自动把 MSVC 的链接器目录放到 PATH 最前面；在 Visual Studio 开发者命令行里则无需处理。

**页面提示「前端尚未构建」**

后端已启动但没有找到前端产物。在 `src-tauri` 目录执行 `npm install && npm run build`。

**登录提示「认证失败: 工号或密码错误」**

检查 `data/员工列表.xlsx` 是否存在、是否包含工号列与密码列（列名可以是「工号 / 员工号 / 账号」与「密码 / 手机号 / 联系电话 / 紧急联系人号码」——与 Python 原版一致，初始密码就是 HR 登记的紧急联系人号码）。

**工资表月份识别不出来**

文件名需要包含年月，支持 `2026 年 10 月…`、`2026-10…`、`202610…` 三种写法。

---

## 许可

内部项目，未指定公开许可证。员工数据属于敏感信息，**不要提交到版本库**（`.gitignore` 已排除 `data/` 下的 Excel 文件）。
