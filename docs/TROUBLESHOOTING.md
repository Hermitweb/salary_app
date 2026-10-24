# 排障手册

按「**症状 → 可能原因 → 怎么确认 → 怎么修**」组织。先用症状定位到对应小节，
再顺着「怎么确认」查证，最后照「怎么修」处理。

配套文档：[ARCHITECTURE.md](ARCHITECTURE.md)、[DEVELOPMENT.md](DEVELOPMENT.md)、
[API.md](API.md)、[DATA.md](DATA.md)、[SECURITY.md](SECURITY.md)。

---

## 目录

1. [服务起不来](#1-服务起不来)
2. [端口被占用](#2-端口被占用)
3. [登录提示「认证失败」](#3-登录提示认证失败)
4. [查不到工资记录](#4-查不到工资记录)
5. [工资条字段缺失](#5-工资条字段缺失)
6. [局域网访问不了](#6-局域网访问不了)
7. [托盘面板不出现或白屏](#7-托盘面板不出现或白屏)
8. [托盘面板点不动](#8-托盘面板点不动)
9. [字段页列出的字段和工资表对不上](#9-字段页列出的字段和工资表对不上)
10. [审计日志不写](#10-审计日志不写)
11. [中文乱码](#11-中文乱码)
12. [`cargo build` 链接失败](#12-cargo-build-链接失败)
13. [账号密码设置用不了](#13-账号密码设置用不了)
14. [其它常见问题](#14-其它常见问题)

---

## 1. 服务起不来

**症状：** 双击 `start.bat` 闪退；`cargo run --bin salary-app-backend` 打印
「服务启动失败：…」后退出；托盘面板点「启动服务」后立刻报错。

### 可能原因

| 原因 | 典型报错 |
| --- | --- |
| 端口被占用 | `HTTP 端口 5001 无法监听: Address already in use` |
| HTTPS 端口被占用 | `HTTPS 端口 5002 无法监听: …` |
| 端口号越界 | `端口号必须在 1-65534 之间（需要预留 HTTPS 端口 +1）` |
| 数据目录不可写 | `服务启动失败：…`（`create_dir_all` 失败） |
| 证书生成失败 | `无法创建密钥文件 …`（`data/certs/` 不可写） |
| 前端未构建（**不影响启动**） | 页面显示「前端尚未构建」 |

### 怎么确认

看服务启动时打印的横幅：

```
==================================================
  员工工资查询系统
==================================================
  本机访问: http://localhost:5001
  局域网访问: http://192.168.1.20:5001
  HTTPS: https://localhost:5002
  监听地址: 0.0.0.0:5001
==================================================
```

另外这两行日志也很有用：

```
数据目录: E:\workspace\salary_app_rust\data
```

看到这一行才能确认服务用的是哪个数据目录（托盘和后端必须一致）。

### 怎么修

- **端口被占用** → 见 [第 2 节](#2-端口被占用)。
- **端口越界** → 端口是成对的（HTTP `P`、HTTPS `P + 1`），上限 65534。
  改 `data/port.txt` 或设 `SALARY_PORT` 为 1–65534 之间的值。
- **数据目录不可写** → 检查 `SALARY_DATA_DIR` 指向的目录是否存在、有没有写权限。
  目录不存在时服务会自动创建，但父目录不可写就会失败。
- **证书生成失败** → 删掉 `data/certs/` 下损坏的文件再重启：
  `Remove-Item data\certs\* -Recurse -Force`（自签证书可以随便重新生成）。
- **前端未构建** → 这不是启动失败，只是页面提示。执行：

  ```powershell
  cd src-tauri
  npm ci
  npm run build
  ```

---

## 2. 端口被占用

**症状：** 启动报 `Address already in use`；或者浏览器打开的是**上一个**
还没退干净的旧服务。

### 可能原因

- 上一次的服务进程没退干净（Ctrl+C 没生效、窗口被直接关掉）。
- 托盘面板启动的服务还在跑（托盘里点「停止服务」即可）。
- 别的程序占用了 5001 或 5002。
- **只占用了 5002**（HTTPS 端口）：HTTP 起来了但 HTTPS 起不来，整个服务启动失败。

### 怎么确认

```powershell
# 看谁占着 5001 / 5002
netstat -ano -p tcp | Select-String ":5001|:5002"

# 看这个 PID 是谁
tasklist /FI "PID eq <上一步的 PID>" /NH /FO CSV
```

也可以直接用托盘面板的「停止服务」：它会先 `netstat` 找 PID，
再用 `tasklist` 确认进程名**含 `salary`**，确认是自己家的人才 `taskkill`。

### 怎么修

1. **是自己家的服务** → 托盘面板点「停止服务」，或：

   ```powershell
   taskkill /PID <PID> /T /F
   ```

2. **是别的程序** → 换一个端口。改 `data/port.txt`（托盘面板的端口输入框也是写这个文件），
   或者设环境变量：

   ```powershell
   $env:SALARY_PORT = "8080"
   ```

   注意 **HTTPS 端口是 `P + 1`**，两个端口都要空出来；防火墙也要放行两个。

3. **只是想换个端口不想改文件** → 用 `SALARY_PORT` 环境变量，它的优先级高于
   `data/port.txt`。

---

## 3. 登录提示「认证失败」

**症状：** 登录页显示「认证失败: 工号或密码错误」或「认证失败: 员工表缺少工号列，登录已禁用，请联系管理员检查员工列表.xlsx」。

### 可能原因

| 原因 | 说明 |
| --- | --- |
| 工号或口令填错 | 最常见 |
| 口令被 `hash_passwords.py` 改过，但改的表不是服务在用的那张 | 两个数据目录 |
| 员工表里没有工号列 | 表结构不对 |
| 员工表里没有口令列 | 会被回落到手机号 / 联系电话 / 紧急联系人号码 |
| 工号格式不匹配 | 存的是数字 `1`，输入 `001` 也是可以的（`ids_match` 容忍）；但存的是 `EMP001`、输入 `001` 就不行 |
| 账号已离职 | 提示是「该账号已停用」，不是「工号或密码错误」 |
| 触发了登录限流 | 提示是「账号或网络已被临时锁定」，HTTP 429 |
| 令牌密钥变了 | 只影响已登录的会话，不影响登录 |

### 怎么确认

**先确认服务用的是哪个员工表。** 启动日志里的「数据目录」那一行说了算：

```
数据目录: E:\workspace\salary_app_rust\data
```

**再确认员工表长什么样。** 打开那个目录下的 `员工列表.xlsx`：

- 第一行应当是表头，且含有 `工号`（或 `员工号` / `员工编号` / `账号`）之一；
- 需要有口令列：`密码` / `手机号` / `联系电话` / `紧急联系人` / `紧急联系人号码`；
- 注意**表头首尾不能有多余空格**：识别是「先精确匹配、再模糊匹配」，
  精确匹配用的是去空白后的表头，所以 `工号 ` 能被识别，但 `员工 工号` 不能。

**用接口直接试**，绕过前端：

```powershell
curl.exe -s -X POST http://localhost:5001/api/auth/login `
  -H "Content-Type: application/json" `
  -d '{"username":"001","password":"salary2026"}'
```

返回 `{"ok":true,"token":"..."}` 说明后端没问题，是前端或输入的问题；
返回 401 说明确实是后端拒绝。

**看审计日志**判断是否真的到过后端：

```powershell
Get-Content data\audit.log -Tail 10 -Encoding UTF8
```

有 `LOGIN_FAIL` 说明请求到了后端；什么都没有说明请求根本没发出去（网络 / 前端问题）。

### 怎么修

- **口令填错** → 注意口令是 `trim()` 过的，前后空格不影响；但全角字符和半角数字
  （`salary２０２６` vs `salary2026`）会不一样。
- **改错了表** → 用 `python scripts/hash_passwords.py --file <正确的路径> --in-place`。
- **缺工号列** → 员工表加一列 `工号`（或 `员工号` / `账号`）。
- **缺口令列** → 加一列 `密码`，然后跑一次 `hash_passwords.py --in-place`。
  没有这一列时后端会回落到手机号 / 紧急联系人号码作为初始口令——
  这意味着**员工可能不知道自己的初始口令是什么**。
- **被限流了** → 等 15 分钟；或重启服务（限流状态在内存里，重启即清空）。
  注意重启会让所有员工重新登录。
- **确认是不是标准模式** → 表头里只要出现任意一个标准字段名
  （工号 / 员工号 / 姓名 / 部门 / 手机号 / 密码 / 管理员 / 联系电话 / 紧急联系人 /
  紧急联系人号码 / 离职 / 离职否）就按标准模式解析；否则按**原始模式**解析，
  也就是「每列一名员工，前 4 行依次是工号 / 密码 / 姓名 / 部门」。

---

## 4. 查不到工资记录

**症状：** 登录成功，月份列表里也有这个月，但点进去提示
「001 在 2026 年 10 月没有工资记录」；或者月份列表里根本没有这个月。

### 可能原因

| 原因 | 说明 |
| --- | --- |
| 工资表文件名里没有可识别的年月 | 后端按文件名解析年月，不看文件内容 |
| 工资表放在了 `data/` 根目录而不是 `data/salary/` | 后端只扫 `data/salary/` |
| 工号列在工资表里叫别的名字 | 工资表必须有能识别成工号的列 |
| 工号值对不上 | 数字 `1` 与文本 `001` 是同一人，但 `EMP001` 与 `001` 不是 |
| 这个月本来就没有这名员工的行 | 离职员工在示例数据里是没有工资行的 |
| 同年同月有多个文件 | 查询时**只取第一个**（按文件名排序） |
| 月份参数越界 | 提示是 400「月份 N 超出范围」 |

### 怎么确认

**1. 看月份列表**（确认文件名能被识别）：

```powershell
curl.exe -s http://localhost:5001/api/available_months -H "Authorization: Bearer <令牌>"
```

返回的 `months` 里没有这个月 → 文件名有问题。

**2. 看文件名规则。** 后端按以下顺序匹配（`handlers::extract_year_month`）：

   | 写法 | 示例 |
   | --- | --- |
   | 年 / 月 | `2026 年 10 月员工工资.xlsx`、`2026年5月工资表.xlsx` |
   | 分隔符 | `2026-10 工资.xlsx`、`2026_10 工资.xlsx`、`2026/10 工资.xlsx` |
   | 紧凑写法 | `202605 工资.xlsx`（文件名里的数字必须正好 6 位） |

   紧凑写法有额外限制：如果文件名里一共有 7 位以上数字（像「2026年05月第2版」），
   会被判为不匹配。

**3. 看目录里到底有什么**：

   ```powershell
   Get-ChildItem data\salary
   ```

**4. 看工资表的表头。** 第一行必须有一列能识别成工号：
   `员工号` / `工号` / `员工编号` / `账号`。
   注意这是「先精确匹配、再模糊匹配」：表里有 `工号` 和 `工号状态` 两列时会选 `工号`。

**5. 直接比对这个人的工号在两张表里是不是同一个**：

   ```powershell
   python -c "import sys; sys.path.insert(0,'scripts'); from hash_passwords import read_xlsx; print(read_xlsx('data/salary/2026 年 10 月员工工资.xlsx')[0])"
   ```

   打印的是工资表第一行（表头）。对照 `data/员工列表.xlsx` 的工号列。

### 怎么修

- **文件名问题** → 改名成含年月的形式，例如 `2026 年 10 月员工工资.xlsx`。
- **放错目录** → 移到 `data/salary/` 下。放错位置的工资表**后端查不到**，
  但托盘面板的字段页**能扫到**（它同时扫 `data/` 和 `data/salary/`）——
  这正是「字段页列出的字段和工资表对不上」的成因，见 [第 9 节](#9-字段页列出的字段和工资表对不上)。
- **工号列改名** → 把工资表里的工号列改成 `员工号` / `工号` / `员工编号` / `账号` 之一。
- **工号值不一致** → 让两张表用同一种写法。注意 `1` 与 `001` 其实是等价的
  （`ids_match` 会按数值比较），真正对不上的是 `EMP001` 这种带前缀的写法。
- **多个文件同月** → 删掉多余的那一个，或者确保要用的那个文件名排在前面
  （`salary_files()` 排序后取第一个）。

---

## 5. 工资条字段缺失

**症状：** 工资条上少了某些字段（比如「绩效工资」不见了），或者某字段本该空着却显示成 `0`。

### 可能原因

字段显示由 `data/fields.txt` 控制，三种策略：

| 策略 | 含义 | 配置写法 |
| --- | --- | --- |
| 跟随（默认） | 值为空或为 0 时**隐藏**，否则显示 | `auto` / `跟随`（或不写这一行） |
| 显示 | 无论取值如何都显示 | `show` / `显示` / `true` / `1` / `yes` / `y` / `是` |
| 隐藏 | 始终隐藏 | `hide` / `隐藏` / `false` / `0` / `no` / `n` / `否` |

所以字段缺失通常是这几种情况之一：

1. 该字段的**值是空或 0**，被「跟随」策略隐藏了；
2. `fields.txt` 里把这个字段设成了 `隐藏`；
3. **配置键与表头对不上**（两边都要去首尾空白后完全一致）；
4. 托盘面板保存时把配置重写了，手工加的规则被覆盖。

「值为空或 0」的判定（`models::is_empty_or_zero`）覆盖这些写法：
空串、纯空格、`0`、`0.0`、`0.00`、`¥0`、`￥0.00`。
解析不出来的文本（比如「待定」）按**非零**处理，会显示出来。

### 怎么确认

**1. 看配置内容：**

   ```powershell
   Get-Content data\fields.txt -Encoding UTF8
   ```

**2. 看键名对不对。** 配置的键与工资表的表头都要走
`excel_parser::normalize_field_name` 去首尾空白，所以 `绩效工资` 与 ` 绩效工资 `
等价，但 `绩效工资` 与 `绩效 工资`（中间有空格）**不是**同一个字段。

**3. 看这个字段在工资表里的实际值。** 用「显示」强制显示它：

   ```
   绩效工资 = 显示
   ```

   刷新页面。如果出现了，说明原来的值是空或 0，被「跟随」隐藏了。

**4. 注意工号列是特殊的**：工号列**始终保留**在返回的 `data` 里（即便为空），
与 Python 原版一致。

### 怎么修

- 想要它始终出现 → 在 `data/fields.txt` 里写 `字段名 = 显示`。
- 想要它按值自动出现消失 → 写 `字段名 = 跟随`。
- 想要它永远不出现 → 写 `字段名 = 隐藏`。
- **改完不生效** → 确认改的是服务在用的那个 `data/fields.txt`
  （看启动日志的「数据目录」），并确认托盘面板没有把文件覆盖回去。

> 后端**每次查询都重读** `data/fields.txt`，所以改完刷新页面即可，不需要重启服务。

---

## 6. 局域网访问不了

**症状：** 本机 `http://localhost:5001` 正常，同事访问 `http://<局域网IP>:5001` 打不开。

### 可能原因

| 原因 | 说明 |
| --- | --- |
| Windows 防火墙拦截 | 最常见。默认阻止入站连接 |
| 端口不通 | 服务没启动，或监听在别的端口上 |
| IP 不对 | 面板显示的「局域网访问地址」取自出口网卡，VPN / 虚拟网卡下可能是错的 |
| 网络隔离 | 访客网络 / AP 隔离 / 不同网段 |
| 用了 HTTPS 但证书不匹配 | 自签证书的 SAN 只有 `localhost` 与 `127.0.0.1`，用 IP 访问会提示域名不匹配 |
| 服务只在本机可见 | 端口没开放给内网 |

### 怎么确认

**1. 确认服务确实监听在所有网卡上。** 启动横幅里应当有：

   ```
   监听地址: 0.0.0.0:5001
   ```

   `0.0.0.0` 表示所有网卡。如果是 `127.0.0.1`，局域网访问不了。

**2. 确认端口在防火墙上放行了：**

   ```powershell
   netsh advfirewall firewall show rule name=all | Select-String "5001|5002"
   ```

**3. 确认 IP 是对的：**

   ```powershell
   ipconfig
   ```

   面板显示的地址来自「UDP connect 不发包」的探测技巧，取的是**默认路由**的出口网卡。
   装了 VPN、虚拟网卡（VirtualBox / Hyper-V / WSL）的机器上，
   这个地址可能是 `10.x.x.x`、`172.x.x.x` 之类的虚拟网段，同事访问不到。
   这时请用 `ipconfig` 里**真正连着单位局域网的那块网卡**的 IPv4 地址。

**4. 从另一台机器测：**

   ```powershell
   Test-NetConnection -ComputerName <服务器IP> -Port 5001
   ```

   `TcpTestSucceeded : False` 说明网络层就不通（防火墙 / 路由 / AP 隔离）。

**5. 在服务器本机用局域网 IP 试：**

   ```powershell
   curl.exe -s http://<局域网IP>:5001/healthz
   ```

   本机能通、别的机器不通 → 防火墙或网络隔离。
   本机也不通 → 服务或 IP 有问题。

### 怎么修

**放行防火墙端口**（托盘面板的「放行防火墙端口」按钮，需要管理员权限）：

```powershell
netsh advfirewall firewall delete rule name="工资查询系统 5001"
netsh advfirewall firewall add rule name="工资查询系统 5001" dir=in action=allow protocol=TCP localport=5001
netsh advfirewall firewall add rule name="工资查询系统 5002" dir=in action=allow protocol=TCP localport=5002
```

> 托盘面板的按钮会**先删同名旧规则再添加**，所以反复点击不会堆积规则。
> 但它放行的是**所有来源**——长期使用建议手工加上 `remoteip=192.168.1.0/24` 限定网段，
> 见 [SECURITY.md](SECURITY.md) 的部署建议。

**其它处置**：

- IP 选错了 → 用 `ipconfig` 里真实网卡的地址；
- HTTPS 证书不匹配 → 改用 HTTP（5001），或者自行为内网域名签发证书并替换
  `data/certs/` 下的两个文件；
- 需要 HTTPS → 见 [SECURITY.md](SECURITY.md) 的「HTTPS 与自签名证书」。

---

## 7. 托盘面板不出现或白屏

**症状：** 托盘里找不到图标；或者面板窗口打开了但一片空白；或者弹了个错误消息框。

### 可能原因

| 现象 | 原因 |
| --- | --- |
| 托盘里没有图标 | 图标被 Windows 收进了「隐藏的图标」区域；或者程序启动就失败了 |
| 托盘图标在但点左键没反应 | 托盘图标是 `tray_icon` crate 在**独立线程**里创建的，那个线程崩了图标就没了 |
| 面板窗口根本没出现 | 之前点过「关闭」——**关闭按钮只是收起到托盘，服务继续运行**。用托盘菜单「显示面板」或左键单击图标唤回 |
| 白屏 | 画布子窗口没画出来：后备缓冲位图或 DC 创建失败 |
| 弹消息框 | 启动失败（`native_windows_gui::init()` 或 `panel::build()` 失败） |

### 怎么确认

**1. 先看托盘的隐藏图标区。** 点任务栏右下角的 `^` 展开。

**2. 从终端启动托盘，看标准错误输出：**

   ```powershell
   cargo run -p salary-tray
   ```

   托盘是 `windows` 子系统（不弹控制台），从终端跑才能看到 `eprintln!` 的输出。

**3. 看窗口是不是被收到了别处。** 面板创建时是**居中偏上**显示的：

   ```
   位置 = (屏幕宽 - 1040) / 2, (屏幕高 - 600) / 3
   ```

   如果接了多显示器，窗口可能开在另一块屏幕上。

**4. 用 PrintWindow 截图看窗口到底画成什么样**（不受遮挡影响）：

   ```powershell
   powershell -ExecutionPolicy Bypass -File scripts\print-window.ps1
   ```

   输出到 `scripts/tray-panel.png`。

**5. 确认托盘程序进程还在：**

   ```powershell
   tasklist /FI "IMAGENAME eq salary-tray.exe"
   ```

### 怎么修

- **图标在隐藏区** → 把它拖到可见区，或在「设置 → 个性化 → 任务栏 → 其他系统托盘图标」里打开。
- **之前关过面板** → 托盘菜单「显示面板」，或**左键单击**托盘图标。
- **白屏 / 画布创建失败** → 重启托盘程序。这类失败通常是一次性的（GDI 资源临时不足），
  反复重启无效时重启系统。
- **消息框报错** → 按提示处理。常见的是「无法创建控制面板：…」，先 `cargo run -p salary-tray`
  从终端看完整错误。
- **托盘图标线程崩了** → 重启托盘程序。托盘图标必须在创建它的线程里更新，
  所以那个线程会一直阻塞在 `theme_receiver.recv()` 上等主题变更。

---

## 8. 托盘面板点不动

**症状：** 面板显示了，但点击按钮没反应；或者鼠标移过去没有 hover 效果；或者整个窗口卡住。

### 可能原因

| 原因 | 说明 |
| --- | --- |
| 有一个模态对话框挡着 | 放行防火墙失败时会弹 `nwg::modal_info_message`，不关掉它主窗口收不到输入 |
| 正在做耗时操作 | 点「启动服务」/「停止服务」会在界面线程上同步执行，界面会**卡住几秒**，不是死了 |
| 点的是端口输入框 | 点端口输入框是**进入编辑态**，不是立刻应用；要按回车或点「应用端口」 |
| 窗口在后台 | 托盘面板是普通窗口，点了没反应先确认它在前台（托盘菜单「显示面板」会 `set_focus`） |
| 端口探测线程卡住 | 端口探测每 2 秒做一次 TCP connect（300ms 超时），正常不会卡界面 |

### 怎么确认

**1. 找有没有模态框。** 面板可能被一个错误对话框挡住了，看整个屏幕而不只是面板区域。

**2. 看它是不是真的卡住了。** 任务管理器里看 `salary-tray.exe` 的 CPU 占用：

- CPU 持续 100% → 死循环或重绘风暴；
- CPU 接近 0 → 是在等输入（模态框 / 没聚焦）。

**3. 试一下键盘。** 端口输入框获得焦点后（点一下输入框），按 `Esc` 取消编辑、
`Backspace` 删除、`Enter` 应用。键盘能用说明鼠标位置没算错。

**4. 确认画布子窗口盖住了整个客户区。** 画布是 `WS_CHILD | WS_VISIBLE`，
铺满 `1040 × 600`。如果窗口被拖到了比这个尺寸小的地方，多出来的区域就没有画布。

### 怎么修

- **模态框** → 关掉它（按回车或点「确定」）。
- **卡住几秒** → 是正常的。`server::spawn_server` 最多等 10 秒
  （`READY_TIMEOUT`）确认绑定结果。停止独立启动的服务时还要跑
  `netstat` + `tasklist` + `taskkill`，也需要一点时间。
- **端口输入框** → 点「应用端口」按钮，或直接按回车。`Esc` 放弃修改。
- **窗口没聚焦** → 托盘菜单「显示面板」。
- **重绘风暴 / 真的卡死** → 重启托盘程序。如果每次都复现，把
  `RUST_LOG` 设成 `debug` 再跑，看有没有大量重复日志。

---

## 9. 字段页列出的字段和工资表对不上

**症状：** 托盘面板「字段显示」页列出的字段，比工资表实际有的多或少；
或者某个字段明明在工资表里，列表里却没有。

### 可能原因

**托盘面板的字段发现规则与后端查询工资表的规则不一样**，这是设计上就存在的差异：

| | 托盘面板 `fields::discover_fields` | 后端 `handlers::salary_files` |
| --- | --- | --- |
| 扫描目录 | `data/salary/` **和** `data/` | 只有 `data/salary/` |
| 文件名要求 | 文件名**必须含「工资」二字** | 只要是 `.xlsx` / `.xls` |
| 取哪些列 | 每个文件的**首行**全部非空列 | 首行全部非空列（去首尾空白） |

所以：

1. 工资表叫 `2026-10.xlsx`（**没有「工资」二字**）→ 后端能查到、月份列表里也有，
   但**托盘字段页扫不到**；
2. 工资表直接放在 `data/` 根目录（不在 `data/salary/` 里）→ 后端**查不到**，
   但托盘字段页**能扫到**；
3. 面板 KPI 卡片上的「月份数」用的是 `panel::count_months`，
   同样只统计文件名含「工资」的表，**与月份列表的数量可能对不上**。

其它可能原因：

- 字段名带首尾空白。表头与配置键都去空白，所以「绩效工资 」和「绩效工资」是同一个；
  但托盘列表里显示的是去空白后的名字。
- 托盘面板用的是**另一个数据目录**，扫到的是另一批工资表。
- 刚替换了工资表，面板还没「重新扫描」。

### 怎么确认

**1. 对比两边的文件列表：**

   ```powershell
   Get-ChildItem data\salary
   Get-ChildItem data -Filter *.xls*
   ```

**2. 确认托盘与后端的数据目录一致。** 后端看启动日志的「数据目录」；
   托盘面板在服务页的「打开数据目录」按钮里也能直接打开它。

**3. 看后端的月份列表**（它只认 `data/salary/`）：

   ```powershell
   curl.exe -s http://localhost:5001/api/available_months -H "Authorization: Bearer <令牌>"
   ```

**4. 确认文件名含「工资」二字。** 这一条是最容易踩的。

### 怎么修

- **统一命名规范**：所有工资表都放在 `data/salary/` 下，文件名形如
  `2026 年 10 月员工工资.xlsx`。这样三套规则（后端查询、月份列表、面板字段发现）
  才对得上。
- **点面板的「重新扫描」按钮**（字段页右下角），它会重新扫一遍所有工资表。
  注意重新扫描会保留你正在编辑的状态优先级（内存中的 > `fields.txt` 里的）。
- **改完文件名后**，后端会立刻生效（缓存按 mtime 失效），面板需要点「重新扫描」。

---

## 10. 审计日志不写

**症状：** `data/audit.log` 不存在、不更新，或者里面没有 `VIEW` 记录。

### 可能原因

| 原因 | 说明 |
| --- | --- |
| 根本没触发审计动作 | 只有登录、退出、查工资、导出、打印才写 |
| 数据目录不可写 | 写文件失败是**静默忽略**的，不会报错 |
| 看的是另一个 `audit.log` | 服务与托盘可能指向不同的数据目录 |
| 日志已轮转 | 超过 5 MB 时当前日志改名为 `audit.log.1` |
| 前端上报的审计没到 | `POST /api/audit` 失败时前端只 `console.warn`，不影响用户 |

### 怎么确认

**1. 确认文件在哪：**

   ```powershell
   Get-ChildItem data -Filter "audit.log*"
   Get-Content data\audit.log -Tail 5 -Encoding UTF8
   ```

**2. 确认服务在写。** 审计事件同时进 tracing 日志，所以看服务终端输出：

   ```
   INFO salary_app: 审计事件 action=VIEW employee=001 ip=192.168.1.30 detail=2026-10
   ```

   终端里有、文件里没有 → **文件写入失败**（权限或磁盘满）。
   终端里也没有 → 动作压根没触发。

**3. 手动触发一次**：在网页上查一次工资条，再看文件。

**4. 确认操作类型。** 这几种动作会写日志：

   | action | 何时触发 |
   | --- | --- |
   | `LOGIN` / `LOGIN_FAIL` / `LOGIN_BLOCKED` | 登录成功 / 失败 / 被限流 |
   | `LOGOUT` | 点退出登录 |
   | `VIEW` | 查询某个月的工资条 |
   | `EXPORT_IMG` / `EXPORT_PDF` / `PRINT` | 前端上报的导出、打印操作 |

   **单纯打开页面、刷新页面不会写日志。**

**5. 检查轮转。** 超过 5 MB 时当前日志会改名成 `audit.log.1`：

   ```powershell
   Get-ChildItem data -Filter "audit.log*"
   ```

   只有 `.1` 没有当前的，说明刚刚轮转过。

### 怎么修

- **文件写不进去** → 检查 `data/` 目录的写权限与磁盘空间：

  ```powershell
  Get-PSDrive -Name E
  icacls data
  ```

  修好权限后重试；如果文件被锁住，先删掉再让服务重建。

- **想手动清空** → 停掉服务后删掉 `data/audit.log`（轮转备份 `audit.log.1` 也会被覆盖）。
  审计日志在删之前最好先归档。

- **导出动作没记录** → 导出/打印是前端通过 `POST /api/audit` 上报的，
  失败只会打一条 `console.warn`。用浏览器开发者工具的 Network 面板看这个请求的状态码。

---

## 11. 中文乱码

**症状：** 终端里的中文变成 `???` 或 `锟斤拷`；`.ps1` 脚本报「意外的标记 }」；
`.bat` 里的中文显示成乱码；网页上的中文正常但日志里乱码。

### 可能原因

| 场景 | 原因 | 修法 |
| --- | --- | --- |
| **终端输出** | Windows 控制台默认代码页是 **936（GBK）**，Rust 进程输出的是 UTF-8 字节 | `chcp 65001`，或用仓库脚本（已内置） |
| **`.ps1` 脚本解析报错** | PowerShell 5.1 按系统 ANSI 代码页读**无 BOM** 的脚本，中文注释让解析器错位 | 存成「UTF-8 with BOM」 |
| **`.bat` 里的中文** | `cmd.exe` 按 OEM 代码页读 `.bat`，非 ASCII 字节可能吞掉换行 | `.bat` 只写 ASCII，中文放 `.ps1` |
| **文件内容本身乱码** | 用错的编码保存过 | 重新保存为 UTF-8 |

### 怎么确认

**看当前代码页：**

```powershell
chcp
```

输出 `936` 就是 GBK。

**看乱码出现在哪一层：**

- 浏览器页面正常、终端乱码 → 纯终端编码问题，数据本身没问题；
- 终端也乱、或者文件里存的就是乱码 → 数据/文件编码问题。

**用十六进制看文件真实编码：**

```powershell
Format-Hex data\fields.txt | Select-Object -First 3
```

UTF-8 的中文是 `E4 B8 80` 这种三字节；GBK 是 `D2 BB` 这种两字节。

### 怎么修

**终端输出乱码：**

```powershell
# 办法 1：切到 UTF-8（当前窗口有效）
chcp 65001

# 办法 2：跑仓库脚本（build.ps1 / start.ps1 / start-tray.ps1 内部已处理）
scripts\build.ps1

# 办法 3：PowerShell 里设输出编码
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
```

**`.ps1` 脚本解析报错** → 用 VS Code 保存为 "UTF-8 with BOM"，
或用记事本「另存为 → UTF-8」（Windows 10 1903 之后的记事本默认带 BOM）。

**`.bat` 乱码** → 参照 `start.bat` 的做法：`.bat` 里只写 ASCII 注释，
中文提示放在 `scripts\start.ps1` 里。

> 顺带一提：根目录的 `start-tray.bat` 目前里头的注释是乱码（文件本身存成了
> UTF-8 但被 `cmd.exe` 按 OEM 代码页读）。它不影响运行（注释而已），
> 但如果要修，按 `start.bat` 的做法把它改成纯 ASCII 注释即可。

---

## 12. `cargo build` 链接失败

**症状：**

```
error: linking with `link.exe` failed:
  /usr/bin/link: extra operand '...rcgu.o'
```

或者：

```
error: linking with `link.exe` failed: The system cannot find the file specified
```

### 可能原因

| 原因 | 说明 |
| --- | --- |
| **Git Bash / MSYS 里 `/usr/bin/link.exe` 抢了 MSVC 的链接器** | 最常见。MSYS 自带的 `link` 是 GNU coreutils 的「创建硬链接」工具，不是链接器 |
| 没装 MSVC 生成工具 | 装 Visual Studio 2022 的「使用 C++ 的桌面开发」 |
| MSVC 路径没在 PATH 里 | 装了但当前终端没初始化开发环境 |

### 怎么确认

看 PATH 里排在前面的 `link.exe` 是哪一个：

```powershell
Get-Command link.exe -All | Select-Object Source
```

或在 Git Bash 里：

```bash
which -a link.exe
```

**正确**的结果应该指向 `C:\Program Files\Microsoft Visual Studio\2022\...\VC\Tools\MSVC\...\bin\HostX64\x64\link.exe`。
如果指向 `/usr/bin/link` 或 `C:\Program Files\Git\usr\bin\link.exe`，就是它了。

### 怎么修

**办法 1：用仓库脚本（推荐）。** 脚本会自动探测 MSVC 目录并插到 PATH 最前面：

```powershell
scripts\build.ps1 -All
```

```bash
scripts/build.sh --all
```

**办法 2：Visual Studio 开发者命令行。** 「Developer Command Prompt for VS 2022」
里的 `link.exe` 本来就是对的，不用管 PATH。

**办法 3：手动改 PATH（当前窗口有效）。** 先找到 MSVC 的 `link.exe` 在哪：

```powershell
$link = Get-ChildItem "C:\Program Files\Microsoft Visual Studio\2022\*\VC\Tools\MSVC\*\bin\HostX64\x64\link.exe" |
    Sort-Object FullName -Descending | Select-Object -First 1
$env:PATH = "$($link.DirectoryName);$env:PATH"
```

改完执行 `link.exe /?`，应当打印 MSVC 链接器的版本信息。

**办法 4：确认工具链装全了：**

```powershell
rustup show
rustup target list --installed
```

`x86_64-pc-windows-msvc` 必须在已安装目标里。

---

## 13. 账号密码设置用不了

**症状：** 服务控制页左上角的账号密码卡里，账号下拉是空的、点「读取」报错、
或者「打开网页并登录」跳回登录页。

### 可能原因

| 现象 | 原因 | 怎么确认 |
| --- | --- | --- |
| 账号下拉是空的 | 找不到 `data/员工列表.xlsx`，或者表里没有工号列 / 密码列 | 面板提示「没有可用账号（未找到员工列表）」 |
| 账号少了几个 | 已离职的账号不会列出来（它们本来就登不进去） | 员工表里的「在职状态」列 |
| 来源字段下拉是空的 | `data/salary/` 下没有能解析出年月的工资表 | 工资表文件名要含年月，如 `2026 年 10 月员工工资.xlsx` |
| 「读取」报「没有这一列」 | 选中的字段在**最新一份**工资表里不存在（各月表头可能不同） | 换一个月份的表，或换一个字段 |
| 「读取」报「没有记录」 | 该员工在最新工资表里没有这一行（离职员工尤其常见） | 换一个在职账号 |
| 「打开网页并登录」提示连不上服务 | 服务没启动 | 先在「服务控制」页启动服务 |
| 「打开网页并登录」提示工号或密码错误 | 读出来的字段值不是真正的口令 | 换成员工表里登记口令的那一列，或手动输入口令 |
| 提示「解密失败」 | 凭据是在另一个 Windows 账号下保存的 | 删掉 `data/credentials.json` 重新保存 |

### 凭据存在哪

`data/credentials.json`，密码用 Windows DPAPI 加密，**只有当前 Windows 用户能解开**。
换用户或换机器都解不开，删掉文件重新保存即可。

### 想确认令牌有没有换到

托盘登录成功时，`data/audit.log` 里会多一条 `LOGIN`；浏览器打开网页后自动查了工资条，
还会多一条 `VIEW`：

```powershell
Get-Content data\audit.log -Tail 5 -Encoding UTF8
```

---

## 14. 其它常见问题

### 页面提示「前端尚未构建」

后端已启动但没找到前端产物。在 `src-tauri/` 目录执行：

```powershell
cd src-tauri
npm ci
npm run build
```

后端会按这个顺序找前端目录（`paths::web_dir`）：

1. `SALARY_WEB_DIR` 环境变量；
2. `cargo run` 时的 `src-tauri/dist/`；
3. 可执行文件上两级的 `src-tauri/dist/`（workspace 构建）；
4. 可执行文件同级的 `web/`（便携分发）。

### 前端改了但页面没变

- `assets/` 下的文件带内容哈希，浏览器会长期缓存 → 强制刷新（Ctrl+F5）；
- `index.html` 是 `no-cache`，每次回源校验，一般不用强刷；
- 改的是 Rust 代码 → 需要重启服务。

### 改了后端代码但行为没变

Excel 换了会自动失效（按 mtime），但**代码**不会 —— 改了解析逻辑要重启服务。

### `cargo test` 有 9 个用例「跳过」

集成测试依赖 `data/` 下的示例数据，数据不在时会**跳过而不是失败**。
重新生成：

```powershell
python scripts/generate_sample_data.py --data-dir data --months 3
cargo test --workspace
```

> 集成测试用的登录夹具是仓库里固定的明文样本
> `src-tauri/tests/fixtures/员工列表-测试用.xlsx`，不依赖 `data/` 下那一份，
> 所以把 `data/` 换成哈希口令不会影响测试。
> 重新生成样本时想一并更新它，加 `--employee-fixture` 参数。

### `cargo` 命令突然去下载另一个工具链，或者报 clippy 组件冲突

**症状：**

```
info: syncing channel updates for stable-x86_64-pc-windows-gnu
info: downloading 6 components
info: rolling back changes
error: failed to install component: 'clippy-preview-x86_64-pc-windows-gnu',
       detected conflict: 'bin\clippy-driver.exe'
```

或者任何 `cargo` / `cargo fmt` / `cargo clippy` 命令都在下载工具链后才开始干活。

### 可能原因

本仓库**不固定**工具链（没有 `rust-toolchain.toml`），所以更可能是本机自己的 rustup 配置：
装了多个 host（`x86_64-pc-windows-gnu` 与 `x86_64-pc-windows-msvc`）时，
rustup 的 `default_host_tuple` 与 `default_toolchain` 可能不是同一个 triple，
于是一条 `cargo` 命令会去解析、下载另一套工具链，并因为已存在的 `clippy-driver.exe` 报冲突。
本机若有 `rustup override set` 或目录级的 `rust-toolchain.toml`，也会覆盖默认工具链。

### 怎么确认

```powershell
rustup toolchain list
Get-Content "$env:USERPROFILE\.rustup\settings.toml"
```

看 `settings.toml` 里的 `default_host_tuple` 与 `default_toolchain` 是不是同一个 triple。

### 怎么修

本项目依赖 MSVC 链接器（见 [第 12 节](#12-cargo-build-链接失败)），
所以要显式用 MSVC 工具链。

**临时指定**（当前窗口有效，不写任何文件）：

```powershell
$env:RUSTUP_TOOLCHAIN = "stable-x86_64-pc-windows-msvc"
cargo fmt --all -- --check
```

**长期解决**：在**仓库目录**执行下面这条，只影响本目录，不改仓库里的任何文件：

```powershell
rustup override set stable-x86_64-pc-windows-msvc
```

取消这个 override 用 `rustup override unset`。

### 登录后过一会儿就跳回登录页

令牌有效期 30 分钟。正常过期会跳到 `/login?expired=1`，
登录页显示「登录状态已过期，请重新登录」。

如果**刚登录就掉线**，看是不是有第二个服务进程在跑——
两个进程用的是不同的 `data/secret.key`（各自生成的），令牌互相不认：

```powershell
Get-Process | Where-Object { $_.ProcessName -like "*salary*" } | Select-Object Id, ProcessName
```

### 托盘面板显示「服务已在运行」，但我明明没启动

可能有一个独立启动的服务进程（比如之前用 `start.bat` 起的）。
面板的「停止服务」会先 `netstat` 找 PID、再用 `tasklist` 确认进程名**含 `salary`**，
确认是自己家的人才结束它。端口上是别的程序时它会拒绝并告诉你那是谁。

### 端口改了以后连不上

端口是成对的：HTTP `P`、HTTPS `P + 1`。改端口后：

- 防火墙要放行**两个**端口；
- 面板里的访问地址会自动跟着变；
- 已在浏览器里的令牌不受影响（令牌与端口无关）。

### 托盘面板的「月份数」和网页里的月份列表对不上

面板 KPI 上的月份数只统计**文件名含「工资」**的表（`panel::count_months`），
而网页的月份列表统计 `data/salary/` 下所有 `.xlsx` / `.xls`。
命名不统一就会差几个。统一用 `2026 年 10 月员工工资.xlsx` 这种命名即可，
详见 [第 9 节](#9-字段页列出的字段和工资表对不上)。
