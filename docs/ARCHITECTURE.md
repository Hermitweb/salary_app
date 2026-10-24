# 架构说明

本文描述「员工工资查询系统」的整体结构、请求链路、托盘控制面板的内部机制，
以及几个关键设计取舍背后的原因。

配套文档：[API.md](API.md)（接口清单）、[DATA.md](DATA.md)（数据格式）、
[DEVELOPMENT.md](DEVELOPMENT.md)（开发指南）、[SECURITY.md](SECURITY.md)（安全说明）。

---

## 1. 全局视图

系统由两个 Rust crate 和一套前端组成，全部打包在同一个 Cargo workspace 里：

| 组成 | 位置 | crate / 包名 | 产物 |
| --- | --- | --- | --- |
| 后端服务 | `src-tauri/` | `salary-app-backend`（lib 名 `salary_app_backend`） | `salary-app-backend.exe` |
| 前端源码 | `src-tauri/src/`（`.tsx` / `.ts` / `.css`） | 无（npm 包 `salary-app-frontend`） | `src-tauri/dist/` |
| 托盘控制面板 | `tray/` | `salary-tray`（lib 名 `salary_tray`） | `salary-tray.exe` |

> 注意目录结构上的一处特殊之处：`src-tauri/src/` 里同时放着**前端源码**
> （`App.tsx`、`pages/`、`services/api.ts`、`styles.css`）和**后端源码**
> （`lib.rs`、`handlers.rs` 等 `.rs` 文件）。Vite 以 `src-tauri/src/` 为入口，
> Cargo 以 `src-tauri/src/lib.rs` 为库入口，两者在同一个目录里但互不干扰。

整体分层如下：

```
┌───────────────────────────────────────────────────────────────┐
│  员工浏览器 / 手机                                             │
│  React + TypeScript + Vite，构建产物托管在 src-tauri/dist/    │
└────────────────────────┬──────────────────────────────────────┘
                         │  HTTP :5001   /   HTTPS :5002
                         │  Authorization: Bearer <token>
┌────────────────────────▼──────────────────────────────────────┐
│  salary-app-backend（src-tauri/）                             │
│                                                               │
│  ┌───────────────┐  ┌────────────────┐  ┌──────────────────┐  │
│  │ lib.rs        │  │ server.rs      │  │ handlers.rs      │  │
│  │ 路由注册      │  │ HTTP/HTTPS 监听│  │ 业务编排         │  │
│  │ 静态资源 + SPA│  │ CORS / TLS     │  │ （登录、查工资） │  │
│  └───────┬───────┘  └────────────────┘  └────────┬─────────┘  │
│          │                                       │            │
│  ┌───────▼───────────────────────────────────────▼─────────┐  │
│  │ auth.rs      令牌签发与校验、登录限流                    │  │
│  │ password.rs  PBKDF2 口令哈希                            │  │
│  │ error.rs     错误类型 → HTTP 状态码映射                 │  │
│  ├─────────────────────────────────────────────────────────┤  │
│  │ cache.rs     Moka 缓存，键=路径，值=内容+文件 mtime       │  │
│  ├─────────────────────────────────────────────────────────┤  │
│  │ excel_parser.rs  表结构识别、列定位、字段显示配置解析     │  │
│  ├─────────────────────────────────────────────────────────┤  │
│  │ models.rs  paths.rs  port.rs  audit.rs                  │  │
│  └─────────────────────────────────────────────────────────┘  │
└────────────────────────┬──────────────────────────────────────┘
                         │  读文件
        ┌────────────────▼─────────────────┐
        │  data/（SALARY_DATA_DIR 可改）    │
        │  员工列表.xlsx / salary/*.xlsx   │
        │  fields.txt / secret.key /       │
        │  certs/ / audit.log / port.txt   │
        └──────────────────────────────────┘

┌───────────────────────────────────────────────────────────────┐
│  salary-tray（tray/，Windows 原生 GUI，无 WebView）           │
│  托盘图标 + 控制面板窗口（自绘画布）                          │
│  直接调用 salary-app-backend 的库接口，在本进程内启停服务     │
└───────────────────────────────────────────────────────────────┘
```

---

## 2. 两个 crate 的职责边界与依赖方向

依赖是**单向**的：

```
        ┌──────────────────┐
        │   salary-tray    │──────依赖（path）──────┐
        └──────────────────┘                        │
                                                   ▼
                                        ┌────────────────────────┐
                                        │  salary-app-backend    │
                                        └────────────────────────┘
```

`tray/Cargo.toml` 里写着：

```toml
salary-app-backend = { path = "../src-tauri" }
```

`salary-app-backend` **不依赖** `salary-tray`。这条边界是刻意的：

| crate | 负责 | 不负责 |
| --- | --- | --- |
| `salary-app-backend` | HTTP/HTTPS 服务、认证、Excel 解析、缓存、审计日志、路径与端口规则 | 任何界面 |
| `salary-tray` | 托盘图标、控制面板窗口、服务启停与状态展示、字段配置编辑 | 业务规则、鉴权、Excel 解析 |

由此带来的直接好处：

1. **后端可以脱离 Windows 独立运行**。`salary-app-backend` 不含任何 Win32 依赖，
   编译到 Linux / macOS 也能跑（`cargo build --bin salary-app-backend`）。
2. **托盘复用的全部是纯逻辑**。托盘直接复用后端的 `paths`（路径解析）、
   `port`（端口规则）、`models::FieldState`（三态枚举）、`excel_parser`
   （字段名规范化与配置解析），没有第二份实现，因此不会出现「后端认一种字段名、
   托盘认另一种」的分叉。
3. **托盘不需要自己实现一遍业务**。托盘面板的「字段显示」页直接读写
   `data/fields.txt`，用的就是后端 `ExcelParser::load_field_config` 解析的那份格式。

托盘对后端的具体依赖点：

| 托盘模块 | 用到的后端接口 | 用途 |
| --- | --- | --- |
| `service.rs` | `server::spawn_server` / `ServerRuntime` | 在本进程内启停 HTTP + HTTPS 服务 |
| `service.rs` | `port::read_from_file` / `port::write_to_file` | 与后端同一套端口解析与校验规则 |
| `fields.rs` | `excel_parser::normalize_field_name` / `ExcelParser::load_field_config` | 字段名口径与配置解析 |
| `fields.rs` | `paths::salary_dir` / `paths::field_config_file` | 数据目录定位 |
| `panel.rs` | `models::FieldState` | 三态（显示 / 跟随 / 隐藏） |
| `panel.rs` | `paths::salary_dir` | 统计可查询月份数 |
| `credentials.rs` | `excel_parser::load_employees` / `excel_parser::ids_match` | 账号列表与工资表取值，口径与登录一致 |
| `credentials.rs` | `handlers::latest_salary_file` | 「最新是哪份工资表」与后端同一口径 |
| `http.rs` | `POST /api/auth/login` | 用凭据换一个会话令牌（口令校验只在后端做） |

---

## 3. 请求链路

### 3.1 路由与中间件

路由全部注册在 `src-tauri/src/lib.rs:156` 的 `configure_routes`：

| 方法 | 路径 | 处理器 | 需要认证 |
| --- | --- | --- | --- |
| GET | `/healthz` | `handlers::healthcheck` | 否 |
| POST | `/api/auth/login` | `handlers::login` | 否 |
| POST | `/api/logout` | `handlers::logout` | 是 |
| GET | `/api/available_months` | `handlers::available_months` | 是 |
| GET | `/api/salary/{year}/{month}` | `handlers::salary_query` | 是 |
| GET | `/api/salary?year=&month=` | `handlers::salary_query_params` | 是 |
| POST | `/api/audit` | `handlers::audit_record` | 是 |
| GET | `/favicon.ico` | `lib::favicon` | 否 |
| 其它 | 全部 | `lib::serve_frontend`（SPA 兜底） | 否 |

全局中间件只有两层（`server.rs:271`）：

```
Logger（CORS）  →  App（挂载 cache / auth_context）  →  路由
```

**认证不是 `wrap` 出来的中间件，而是提取器。** `auth::AuthUser` 实现了
`actix_web::FromRequest`，需要登录的处理器把 `user: AuthUser` 写成参数即可：

```rust
pub async fn available_months(_user: AuthUser) -> HttpResponse { ... }
```

提取器在处理器函数体执行之前运行，因此**处理器内部拿到的 `user` 一定已经通过认证**。
没有认证提取器的路由（登录、健康检查）自然就是公开的。

### 3.2 登录：`POST /api/auth/login`

```
浏览器
  │  { "username": "001", "password": "••••" }
  ▼
handlers::login (handlers.rs:153)
  │
  ├─ 1. client_ip(req)  ← TCP 对端地址，不是 X-Forwarded-For
  ├─ 2. 限流预检查：key = "ip:<ip>" 与 "user:<工号>"，任一被锁 → 429
  ├─ 3. paths::employee_file() 不存在 → 404
  ├─ 4. cache.get_employees(&employee_file)          ── 命中 mtime 则直接返回
  │       └─ 未命中 → excel_parser::ExcelParser::load_employees
  │             └─ read_grid() → 判定标准模式 / 原始模式 → 定位各列
  ├─ 5. ExcelParser::authenticate(&employees, username, password)
  │       ├─ excel_parser::ids_match(工号, 输入)   ← 文本 001 与数字 1 视为同一人
  │       ├─ password::verify(存储值, 输入)
  │       │     ├─ 值以 "$pbkdf2-sha256$" 开头 → 按 PHC 串解析并重新派生比较
  │       │     └─ 否则 → 按明文常量时间比较（历史兼容）
  │       └─ 在职状态检查：命中"离职"等取值 → 401「该账号已停用」
  ├─ 6. 成功：清除两个维度的失败计数 → auth.issue_token(&employee)
  │       └─ 载荷 { employee_id, name, dept, exp }（exp = now + 30 分钟）
  │       └─ 令牌 = base64url(json) + "." + hex(HMAC-SHA256(json))
  ├─ 7. audit::record(ip, "LOGIN", 工号, 姓名, "登录成功")
  └─ 8. 返回 200 { ok, token, employee }
```

失败路径：口令错 → `auth::RateLimiter::record_failure` 记一次（两个维度各记一次），
达到阈值（15 分钟内 5 次）即锁定 15 分钟并返回 429；否则返回 401
「认证失败: 工号或密码错误」——**口令错和工号不存在返回同一句提示**，
不暴露工号是否存在。

### 3.3 查工资：`GET /api/salary/{year}/{month}`

```
浏览器
  │  Authorization: Bearer <token>
  ▼
auth::AuthUser（FromRequest 提取器）
  │  ├─ 取 Authorization 头（"Bearer " 前缀可选）
  │  ├─ auth::verify_session_token(token, secret_key)
  │  │     ├─ 拆载荷与签名 → HMAC-SHA256 重算 → 常量时间比较
  │  │     ├─ 反序列化 SessionToken
  │  │     └─ exp 已过期 → 401
  │  └─ 失败一律 401
  ▼
handlers::salary_query (handlers.rs:243)
  │
  ├─ 1. 月份不在 1–12 → 400
  ├─ 2. find_salary_file(year, month)
  │       └─ 扫 data/salary/ 下 *.xlsx / *.xls
  │       └─ handlers::extract_year_month(文件名) 解析年月
  │       └─ 找不到 → 404
  ├─ 3. cache.get_salary(&file)                    ── 命中 mtime 则直接返回
  │       └─ 未命中 → cache::load_salary_from_disk（calamine 读首个工作表）
  │             └─ 首行做表头，excel_parser::normalize_field_name 去首尾空白
  │             └─ 其余行转成 HashMap<列名, 文本>
  ├─ 4. excel_parser::find_employee_id_column(headers)  ← 员工表与工资表共用关键字
  │       └─ 找不到 → 500
  ├─ 5. handlers::find_employee_row(rows, 工号列, 令牌里的工号)
  │       └─ excel_parser::ids_match 逐行匹配
  │       └─ 找不到 → 404「<工号> 在 <年> 年 <月> 月没有工资记录」
  ├─ 6. ExcelParser::load_field_config(paths::field_config_file())
  │       └─ 每次请求都重新读 data/fields.txt
  │       └─ 解析失败时 unwrap_or_default() → 退化成「全部跟随」
  ├─ 7. 按 field_config.should_show(表头, 值) 过滤出可见列
  │       └─ 跟随：值为空或 0 时隐藏
  │       └─ 显示：强制显示
  │       └─ 隐藏：强制隐藏
  │       └─ 工号列始终保留在 data 里（即便为空）
  ├─ 8. audit::record(ip, "VIEW", 工号, 姓名, "2026-10")
  └─ 9. 返回 200 { ok, year, month, headers, values, data }
```

请求链路上的三个关键点：

1. **身份完全来自令牌**。接口不接受工号参数，处理器拿到的工号只可能来自
   服务端签名的令牌载荷，因此无法越权查询他人工资。
2. **工号匹配口径只有一处**。登录校验（`excel_parser::ids_match`）与工资行查找
   （`handlers::find_employee_row`）调用的是同一个函数。若两处分叉，就会出现
   「能登录却查不到工资记录」。
3. **工号列定位也只有一处**。员工表与工资表共用 `excel_parser::ID_KEYWORDS`
   （工号 / 员工号 / 员工编号 / 账号），工资表叫「员工号」、员工表叫「工号」也能对上。

### 3.4 缓存

`cache::DataCache` 用 Moka 缓存两类解析结果：

| 缓存 | 容量 | 键 | 值 |
| --- | --- | --- | --- |
| 员工表 | 500 | 文件路径字符串 | `Arc<EmployeesData>` + 写入时的 mtime |
| 工资表 | 36 | 文件路径字符串 | `(Vec<String>, Vec<SalaryRow>)` + 写入时的 mtime |

命中时比较 mtime（纳秒）：一致直接返回，不一致就 `invalidate` 后重新解析。
**Excel 被替换后下一次请求自动重新解析，不需要重启服务。**

---

## 4. 托盘控制面板的架构

托盘是纯 Win32 + NWG 程序，**没有 WebView**。整个面板是一张自绘画布：
卡片、按钮、状态 pill、字段列表行全部由 GDI 画出来，鼠标交互靠命中测试完成。

### 4.1 线程与窗口

```
salary-tray 进程（#![windows_subsystem = "windows"]，不弹控制台）
│
├─ 主线程：NWG 事件循环
│   ├─ tray_icon::spawn(...) 在独立线程常驻（托盘图标必须在创建它的线程里用）
│   ├─ panel::build(service, tray命令通道, 主题通道)
│   │   ├─ NWG::Window（父窗口，1040×600，客户区）
│   │   │   └─ Canvas::create() → 子窗口 HWND（WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN）
│   │   │         └─ 窗口过程 canvas_proc：绘制 + 鼠标 + 键盘
│   │   ├─ AnimationTimer（120ms 一跳）——驱动动作消费与状态刷新
│   │   └─ 窗口标题栏：brand::apply_window_icon + theme::apply_title_bar
│   └─ native_windows_gui::dispatch_thread_events()  ← 事件循环在这里跑
│
├─ 线程「salary-server」：server::spawn_server(port)
│   └─ 独立 actix 运行时，同时监听 HTTP :5001 与 HTTPS :5002
│      （托盘启停服务不需要拉起子进程）
│
└─ 线程「salary-port-probe」：每 2 秒探测一次端口是否在监听
   └─ 只做 TCP connect，不发 HTTP 请求；结果写进 AtomicBool 缓存
```

### 4.2 数据流：从鼠标点击到界面刷新

```
 用户移动鼠标
     │
     ▼
 canvas_proc: WM_MOUSEMOVE
     │  layout.hit_test(x, y)          ← 从后往前找，后加入的区域在上层
     │  hover 变了才继续
     ▼
 invalidate_region(hwnd, 新旧区域求并集)   ← 只重绘受影响的一小块
     │
     ▼
 WM_PAINT
     │  1. GetUpdateRect(hwnd)   ← 必须在 BeginPaint 之前取，
     │                            │   BeginPaint 之后区域就被标记为已处理
     │  2. BeginPaint
     │  3. IntersectClipRect(backBuffer, 更新区域)
     │  4. render(backBuffer, model, fonts)   ← 整块重画到后备缓冲
     │  5. BitBlt(屏幕DC ← 后备缓冲)           ← 一次贴图，中间没有半成品
     │  6. EndPaint
     ▼
 用户点击
     │
     ▼
 canvas_proc: WM_LBUTTONDOWN → 记录 pressed，SetCapture
 canvas_proc: WM_LBUTTONUP
     │  hit_test 得到 Hit → action_for(hit, model) 映射成 Action
     ▼
 Sender<Action>.send(action)          ← 不在窗口过程里做耗时操作
     │
     ▼
 面板定时器 tick（每 120ms）
     │  while let Ok(action) = action_receiver.try_recv() { handle_action(...) }
     ▼
 handle_action：改 Model（Canvas::update 会重算布局）
     │  再 Canvas::repaint(&[受影响的区域])   ← 局部重绘，不是整块
     ▼
 WM_PAINT → 后备缓冲 → BitBlt
```

**为什么要走通道而不是直接在窗口过程里执行？** 启停服务、放行防火墙端口这类操作
会阻塞几百毫秒到几秒，直接在窗口过程里做，窗口会被 Windows 判定为「未响应」。
统一丢进 `mpsc` 通道，由定时器在事件循环里消费，界面线程就不会卡住。

### 4.3 双缓冲与局部重绘

| 机制 | 实现 | 解决的问题 |
| --- | --- | --- |
| 双缓冲 | `canvas::BackBuffer`：`CreateDIBSection` 建 32 位位图 + `CreateCompatibleDC`，`biHeight` 取负表示自上而下 | 直接往屏幕 DC 画会分很多笔慢慢刷出来，肉眼看到「闪一下」 |
| 整块重绘不擦背景 | `WM_ERASEBKGND` 直接 `return 1` | 背景交给 `WM_PAINT` 统一画，系统擦一次、画布再画一次 = 两次擦除 = 闪烁 |
| 局部重绘 | `invalidate_region` 用带 RECT 的 `InvalidateRect`，只标记受影响区域 | 整块重绘会让窗口在鼠标移动时看起来在闪 |
| 剪裁 | `IntersectClipRect` 把后备缓冲 DC 剪到更新区域 | 既省了画多余部分的力气，也保证贴回屏幕时只有这一块是新的 |
| 区域合并 | `canvas::union` 把「旧 hover 区域」与「新 hover 区域」求并集后一起重绘 | 只画新区域会让旧区域残留上一个状态 |
| 按需重绘 | `Canvas::update` **不自动重绘**；状态没变时 `refresh_status` 直接 return | 定时器每 120ms 问一次状态，无条件重绘会让整个面板不停闪烁 |

### 4.4 命中测试

布局 `Layout` 同时承担两件事：算出每个控件的矩形，并登记一张命中表。

```rust
pub struct Layout {
    pub hits: Vec<(Hit, Rect)>,   // 命中区域，后加入的在上层
    pub field_rows: Vec<Rect>,    // 字段行（按可视行序）
    ...
}

pub fn hit_test(&self, x: i32, y: i32) -> Option<Hit> {
    self.hits.iter().rev()
        .find(|(_, rect)| rect.contains(x, y))
        .map(|(hit, _)| *hit)
}
```

`Hit` 枚举覆盖导航、主题切换、启停、打开浏览器 / 数据目录、防火墙、开机自启、
端口输入框、应用端口、字段行、切换状态、保存、重新扫描。`Hit` → `Action` 的
映射集中在 `canvas::action_for` 一处。

**字段行索引的两套编号**是这里最容易出错的地方：

- `Hit::FieldRow(index)` 里的 `index` 是**可视行号**（0–8，`LIST_ROWS = 9`）；
- `Model::selected` 里存的是**字段的绝对序号**（在完整字段列表中的位置）；
- `Model::scroll` 是滚动偏移（绝对序号）。

因此把绝对序号换算成可视行号必须做减法：`index.checked_sub(scroll)`。
`panel.rs` 的 `Action::SelectField` 与 `Action::CycleField` 都遵循这一换算；
一旦把两者混用，就会出现「滚动后选中项串行」——即点第 3 行却高亮了第 5 行。

### 4.5 三条通道

面板与外部有三类异步通信，都是单向通道：

| 通道 | 类型 | 方向 | 用途 |
| --- | --- | --- | --- |
| 动作通道 | `mpsc::Sender<Action>` | 画布 → 面板 | 点击产生的业务动作 |
| 托盘命令通道 | `mpsc::Sender<TrayCommand>` | 托盘图标 → 面板 | 显示面板 / 启停服务 / 退出 |
| 主题通道 | `mpsc::Sender<Theme>` | 面板 → 托盘图标 | 主题切换后托盘图标换色 |

三条通道都在面板的定时器 tick 里用 `try_recv` 排空。托盘图标线程则在
`theme_receiver.recv()` 上阻塞等待，收到新主题就重建图标——托盘图标只能在
创建它的线程里更新，所以这个线程必须常驻。

### 4.6 主题与品牌

- `theme::Palette` 收敛了全部颜色，主题切换时只换这一个结构体，布局与绘制代码不动。
- 视觉规范来自仓库根目录的《设计风格规范.md》：深色 Linear 风 / 浅色 Meta 风两套主题。
- 标题栏是系统画的，只能通过 DWM 指定颜色：`theme::apply_title_bar` 设置
  `DWMWA_CAPTION_COLOR`（底色）、`DWMWA_TEXT_COLOR`（文字色）、
  `DWMWA_BORDER_COLOR`（边框色），以及 `DWMWA_USE_IMMERSIVE_DARK_MODE`。
  Windows 11 能精确指定前三个，Windows 10 最多只能开沉浸式深色模式。
- `brand` 模块让侧边栏 logo、标题栏图标、任务栏图标、托盘图标共用同一个图形
  （`assets/icon-source.jpg` 生成的应用图标）：图标像素由系统解码
  （`GetIconInfo` + `GetDIBits`）取成 RGBA，标题栏 / 任务栏直接 `LoadImage`，
  托盘与侧边栏 logo 复用同一份像素，网页端用同一源图生成的 PNG。

### 4.7 账号密码：控制面板不查工资，只保管凭据

「服务控制」页左上角的账号密码卡把控制面板和工资查询彻底分开——面板不提供查工资的界面，
它只保管一组能登录网页的凭据：

```
员工列表.xlsx ──► 账号下拉（工号 · 姓名 · 部门，已停用的不列）
工资表最新一份 ──► 来源字段下拉（表头）──► 「读取」──► 填进密码框
                                                    │
                                     保存到 credentials.json（DPAPI 加密）
                                                    │
                        「打开网页并登录」：托盘 POST /api/auth/login 换会话令牌
                                                    │
                              浏览器打开 /?token=… ──► 前端换员工信息后抹掉地址栏
```

四个设计要点：

1. **口径不重复实现**。账号来自 `ExcelParser::load_employees`，工号匹配用
   `excel_parser::ids_match`，「最新是哪份工资表」用 `handlers::latest_salary_file`——
   全部复用后端同一套逻辑，面板看到的和网页端查到的必然一致。
2. **口令校验只在后端做**。托盘不自己比对密码，而是把凭据交给 `POST /api/auth/login`，
   由后端完成 PBKDF2 校验并签发令牌。托盘这边只拿到令牌。
3. **密码不落明文**。落盘用 Windows DPAPI（`CryptProtectData`）加密，
   只有当前 Windows 用户能解开；浏览器地址里带的也是令牌而不是密码。
4. **不引入 HTTP 客户端依赖**。托盘只需要向本机服务发一个 JSON POST，
   `tray/src/http.rs` 用 `std::net::TcpStream` 手写几十行即可，
   少一条供应链依赖，离线构建也不会卡住。

---

## 5. 关键设计取舍

### 5.1 为什么用 calamine 解析 Excel

| 备选 | 问题 |
| --- | --- |
| COM 自动化（Excel 装在服务器上） | 要求目标机器装 Office；服务端跑非交互会话时 COM 极不可靠；进程崩溃会带崩 Excel |
| 第三方库（openpyxl / rust_xlsxreader 之类） | `openpyxl` 要额外装 Python 依赖；`rust_xlsxreader` 只支持 xlsx，不支持老式的 `.xls` |
| **calamine** | 纯 Rust、无外部依赖、无宏执行、支持 xlsx / xls / xlsb / ods，`open_workbook_auto` 按扩展名自动选解析器 |

选 calamine 的三个决定性理由：

1. **零外部依赖**。部署到一台没装 Office 的 Windows 机器上，`cargo build --release`
   出来的 exe 直接能跑，不需要注册 COM 组件、不需要装 Python。
2. **只读、纯解析**。calamine 不执行宏、不写文件，解析一个被污染的 xlsx 不会
   在服务器上触发任何代码执行路径。
3. **接口够用且可控**。`worksheet_range_at(0)` 拿到首个工作表的二维数据，
   正好够做「首行是表头、其余是数据行」这种简单表结构；数字、日期、布尔、
   错误单元格的语义由 `cell_to_string` 统一成文本，符合「工资条字段一律按文本展示」
   的需求。

代价是失去公式计算能力——`.xlsx` 里的公式读到的是缓存值而不是计算结果。
对工资表来说这恰好是想要的行为（HR 导出的通常是数值）。

### 5.2 为什么字段配置每次请求都重读

`data/fields.txt` 只有几十行、几 KB，读取成本可以忽略，但它每次工资查询都被
重新读一遍（`handlers.rs:305`）。这是刻意的：

1. **改了立刻生效，不用重启服务**。运维在托盘面板里点「保存」，下一位员工
   刷新页面就看到新的字段布局。如果把配置也放进 Moka 缓存，就得再引入一套
   失效机制（监听文件、比较 mtime、暴露刷新接口）才能达到同样的效果。
2. **避免缓存与文件不一致**。配置写入端（托盘面板的 `fields::save_config`）用
   **原子写**（先写临时文件再 `rename`），因此读取端要么看到旧的完整内容，
   要么看到新的完整内容，不会读到半截。原子写让「每次重读」这件事变安全。
3. **写入方只有一个**。配置只有一个写入口（托盘面板），写入频率极低，
   不需要担心读写竞争。

代价是每次请求多一次文件读。相对于解析 Excel 和做 PBKDF2 校验，这点开销可以忽略。

### 5.3 为什么托盘不用 WebView 而自己用 GDI 画

托盘控制面板本来可以用 WebView2 嵌一个 HTML 页面，代码量会小很多。这里选择
完全自绘，理由如下：

1. **不引入 WebView2 运行时**。WebView2 是 Evergreen 运行时，多数 Windows 10/11
   机器预装，但在精简镜像、服务器核心版、域控等环境下不一定有；缺了它程序会
   直接起不来。自绘方案只需要 Win32 + GDI，系统自带。
2. **exe 体积与依赖**。自绘的托盘程序不需要打包 WebView2 运行时，
   产物只有几 MB；带运行时的分发包要多出上百 MB。
3. **完全掌控重绘行为**。设计规范要求深色主题下界面不能出现系统控件的
   亮色边框。WebView 里的 CSS 动画、焦点环、滚动条都要逐个覆盖才能统一，
   而自绘可以精确控制「哪一块脏了就重绘哪一块」，配合双缓冲做到零闪烁。
4. **没有中间层**。自绘直接走 `WM_PAINT` + `BitBlt`，
   少了一层「DOM → 合成 → 位图」的转换，中间层是界面卡顿的主要来源。
5. **品牌一致性**。侧边栏 logo、标题栏图标、任务栏图标、托盘图标、网页 favicon
   要合成同一个图形。自绘侧边栏直接贴系统解码好的图标位图
   （`brand::Logo`），标题栏 / 任务栏 / 托盘用同一份 exe 资源，
   网页端用同一源图生成的 PNG，做到「看起来是同一个图标」。

代价是所有控件都要自己实现：圆角矩形、发丝线、胶囊按钮、列表滚动、
文本输入与光标，全部在 `tray/src/paint.rs` 与 `tray/src/canvas.rs` 里手写。

### 5.4 其它取舍

| 取舍 | 选择 | 原因 |
| --- | --- | --- |
| TLS 实现 | rustls，且**只开 `ring` 特性** | `aws-lc-rs` 需要 C 工具链与 NASM，在 Windows 上构建环境复杂；`ring` 纯 Rust，交叉编译友好。`main.rs` 里显式 `install_default()` 避免多 provider 冲突 |
| 端口规则 | 统一在 `port` 模块，后端与托盘共用 | 曾经漏过一处：`SALARY_PORT=65535` 会让 HTTPS 端口和 HTTP 端口撞车，服务整个起不来。因此上限锁死 65534 |
| 令牌 | 无状态 HMAC 自签，不落库 | 令牌不存服务端，重启服务不影响已登录员工；`secret.key` 一旦丢失，所有令牌立即失效 |
| 管理员角色 | 已彻底移除 | 网页端区分管理员没有实际价值（每个员工只能看自己的工资），却带来了额外的鉴权分支与一处越权风险面。移除后所有能登录的员工权限完全相同 |
| 客户端 IP | 只取 TCP 对端地址 | 本系统是局域网直连部署、没有反向代理，`X-Forwarded-For` 可以被任意伪造，拿它做审计依据会让日志失去意义 |
| 跨域 | 默认只允许同源 | 令牌放在 `Authorization` 头而不是 Cookie，不放开跨域时浏览器只能同源访问 |
| 托盘与服务的关系 | 服务跑在托盘进程内（独立线程） | 启停不需要拉起子进程，也不怕子进程变成僵尸；托盘退出时服务随之停止 |
| 停掉「别人」的服务 | 只结束进程名含 `salary` 的进程 | 端口上完全可能是别的程序，先用 `tasklist` 确认身份再 `taskkill` |

---

## 6. 目录与模块速查

```
src-tauri/src/
├── lib.rs           路由注册、静态资源托管、SPA 兜底、缓存与 ETag 策略
├── main.rs          进程入口：监听 HTTP/HTTPS、打印访问地址、打开浏览器
├── server.rs        服务构建与启停、CORS 策略、自签名证书、密钥加载
├── handlers.rs      REST 处理器：登录、月份列表、工资查询、审计上报
├── auth.rs          令牌签发与校验、登录限流、AuthUser 提取器
├── password.rs      PBKDF2-HMAC-SHA256 与明文兼容
├── cache.rs         Moka 缓存（按 mtime 失效）
├── excel_parser.rs  Excel 解析、列定位、字段显示配置
├── models.rs        领域模型、FieldState 三态、空值/零值判定
├── error.rs         AppError → HTTP 状态码映射
├── audit.rs         审计日志写入与轮转
├── paths.rs         数据目录 / 前端目录 / 各文件路径的统一解析
├── port.rs          端口解析与校验
└── src/             前端源码（React + TypeScript）

tray/src/
├── main.rs          入口：托盘图标 + 面板 + 事件循环
├── panel.rs         控制面板窗口：动作处理、定时刷新、通知条
├── canvas.rs        自绘画布：布局、命中测试、绘制、局部重绘、双缓冲
├── paint.rs         GDI 绘制原语：圆角矩形、发丝线、圆点、文字
├── theme.rs         配色、字体、尺寸、标题栏 DWM 主题
├── brand.rs         品牌标记：logo / 标题栏 / 任务栏 / 托盘同一图形
├── service.rs       服务启停、端口、状态探测、防火墙、开机自启
├── fields.rs        动态字段发现与 fields.txt 原子读写
├── credentials.rs   网页登录凭据：账号发现、字段读取、DPAPI 加密存取
├── http.rs          最小 HTTP 客户端：向本地服务换会话令牌
├── clipboard.rs     剪贴板写入（复制地址、复制凭据）
├── networks.rs      支持访问的网络配置（data/networks.txt）
└── tray_icon.rs     系统托盘图标与右键菜单
```

---

## 7. 延伸阅读

- 接口清单与请求示例：[API.md](API.md)
- 数据文件格式、字段配置语义：[DATA.md](DATA.md)
- 从零跑起来、日常命令、代码约定：[DEVELOPMENT.md](DEVELOPMENT.md)
- 口令哈希、限流、密钥保管、部署建议：[SECURITY.md](SECURITY.md)
- 出问题时怎么查：[TROUBLESHOOTING.md](TROUBLESHOOTING.md)
- 构建产物、打包、升级与回滚：[RELEASE.md](RELEASE.md)
