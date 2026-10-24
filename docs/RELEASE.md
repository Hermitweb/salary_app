# 发布与部署

本文覆盖：release 构建、产物位置、便携版目录结构、首次初始化、打包分发清单、
版本号位置、发布前检查、升级与回滚。

配套文档：[DEVELOPMENT.md](DEVELOPMENT.md)、[SECURITY.md](SECURITY.md)、
[ARCHITECTURE.md](ARCHITECTURE.md)。

---

## 1. 版本号在哪里

系统有两处版本号，**都要改**：

| 文件 | 字段 | 当前值 | 对应产物 |
| --- | --- | --- | --- |
| `src-tauri/Cargo.toml` | `[package] version` | `0.3.0` | `salary-app-backend.exe` |
| `tray/Cargo.toml` | `[package] version` | `0.3.0` | `salary-tray.exe` |

```toml
# src-tauri/Cargo.toml
[package]
name = "salary-app-backend"
version = "0.3.0"
edition = "2024"
rust-version = "1.85"
```

```toml
# tray/Cargo.toml
[package]
name = "salary-tray"
version = "0.3.0"
edition = "2024"
rust-version = "1.85"
```

> 前端的 `src-tauri/package.json` **没有 `version` 字段**（它是 `private: true` 的
> 内部包），所以不需要跟着改。前端版本由后端的版本号统一代表。

`Cargo.lock` 里也记着这两个包的版本，改完版本号后提交更新过的 `Cargo.lock`。

---

## 2. release 构建

### 2.1 命令

```powershell
# 1) 构建前端（产物在 src-tauri/dist/）
cd src-tauri
npm ci
npm run build
cd ..

# 2) 构建两个 release 可执行文件
cargo build --release --bin salary-app-backend --locked
cargo build --release -p salary-tray --locked
```

或者用脚本（会自动处理 MSVC 链接器路径与控制台编码）：

```powershell
scripts\build.ps1 -Frontend
scripts\build.ps1 -Release
scripts\build.ps1 -Tray
```

```bash
scripts/build.sh --frontend
scripts/build.sh --release
scripts/build.sh --tray
```

> `scripts/build.sh --release` 只构建后端，托盘要单独用 `--tray`。
> 要一次全构建，用 `--all`（debug）或分别跑前端 + `--release` + `--tray`。

### 2.2 产物位置

| 产物 | 路径 | 说明 |
| --- | --- | --- |
| 后端服务 | `target/release/salary-app-backend.exe` | 独立运行的 HTTP/HTTPS 服务 |
| 托盘面板 | `target/release/salary-tray.exe` | Windows 托盘控制面板 |
| 前端页面 | `src-tauri/dist/` | `index.html` + `assets/`（Vite 产物，带内容哈希） |

release profile 在 workspace 根的 `Cargo.toml` 里：

```toml
[profile.release]
opt-level = 3
lto = true
```

LTO 会让编译慢一些，但产物体积和性能都更好。

---

## 3. 部署目录结构

后端通过 `src-tauri/src/paths.rs` 定位数据目录与前端目录。定位规则是
**按可执行文件位置**算的，因此分发目录必须符合下面两种结构之一。

### 3.1 便携版（推荐：单目录拷走就能跑）

```
<部署目录>\
├── salary-app-backend.exe     ← 后端
├── salary-tray.exe            ← 托盘面板
├── web\                       ← 前端构建产物（src-tauri/dist/ 的内容）
│   ├── index.html
│   ├── app_icon.png           ← 应用图标（favicon 与页面 logo 共用）
│   └── assets\
│       ├── index-<hash>.js
│       └── index-<hash>.css
└── data\                      ← 数据目录（首次启动自动创建）
    ├── 员工列表.xlsx
    ├── salary\
    │   └── 2026 年 10 月员工工资.xlsx
    ├── fields.txt             ← 可选：字段显示配置
    ├── secret.key             ← 自动生成
    ├── certs\                 ← 自动生成
    │   ├── server.crt
    │   └── server.key
    ├── audit.log              ← 自动生成
    └── port.txt               ← 可选：端口配置
```

对应到 `paths.rs` 的规则：

| 资源 | 规则 | 便携版命中 |
| --- | --- | --- |
| 数据目录 | `SALARY_DATA_DIR` → `cargo run` 的 `CARGO_MANIFEST_DIR/../data` → 编译期路径 → **exe 上两级的 `data/`**（存在就用）→ **exe 同级的 `data/`** | exe 同级的 `data/` |
| 前端目录 | `SALARY_WEB_DIR` → `cargo run` 的 `dist/` → **exe 上两级的 `src-tauri/dist/`** → **exe 同级的 `web/`** | exe 同级的 `web/` |

打包方法：

```powershell
$stage = "D:\release\salary-app"
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $stage | Out-Null
New-Item -ItemType Directory -Path "$stage\web" | Out-Null

Copy-Item target\release\salary-app-backend.exe $stage
Copy-Item target\release\salary-tray.exe        $stage
Copy-Item src-tauri\dist\*                      "$stage\web" -Recurse
```

### 3.2 workspace 布局（就地运行，不拷贝）

保持仓库的结构不动，直接在仓库里跑 release 产物：

```
<仓库根>\
├── Cargo.toml
├── src-tauri\
│   ├── dist\                  ← 前端产物
│   │   ├── index.html
│   │   └── assets\
│   └── src\
├── target\release\
│   ├── salary-app-backend.exe ← exe 在 target/release/，距离仓库根两级
│   └── salary-tray.exe
└── data\
    ├── 员工列表.xlsx
    └── salary\
```

这时：

- 数据目录 → `target/release/` 上两级的 `data/` = 仓库根的 `data/`；
- 前端目录 → `target/release/` 上两级的 `src-tauri/dist/` = 仓库根的 `src-tauri/dist/`。

启动：

```powershell
.\target\release\salary-app-backend.exe
```

> 直接双击 exe 不会有终端窗口（托盘是 `windows` 子系统；后端是控制台程序，
> 双击会开一个控制台窗口，最小化即可；本机运维建议用 `start-tray.bat`）。

### 3.3 用环境变量覆盖（特殊情况）

```powershell
$env:SALARY_DATA_DIR = "D:\salary-data"
$env:SALARY_WEB_DIR  = "D:\salary-web"
.\target\release\salary-app-backend.exe
```

适合数据放在加密卷、前端放在共享目录的场景。

---

## 4. `data/` 目录的首次初始化

**后端进程**负责创建数据目录。首次启动时会：

1. 创建数据目录本身（`SALARY_DATA_DIR` 或按 3.1 规则定位的那一个）；
2. 创建 `data/salary/` 子目录；
3. 生成令牌签名密钥 `data/secret.key`（32 字节随机数的十六进制）；
4. 生成自签名证书 `data/certs/server.crt` 与 `data/certs/server.key`；
5. 首次登录 / 查询时创建 `data/audit.log`。

**托盘程序不会创建数据目录**（它只读）。所以第一次部署时要么先跑一次后端，
要么手工建好目录并放入数据。

手工初始化：

```powershell
$stage = "D:\release\salary-app"
New-Item -ItemType Directory -Path "$stage\data" | Out-Null
New-Item -ItemType Directory -Path "$stage\data\salary" | Out-Null
Copy-Item <你的员工表>.xlsx "$stage\data\员工列表.xlsx"
Copy-Item <你的工资表>*.xlsx "$stage\data\salary\"
```

首次部署的数据准备：

```powershell
# 生成示例数据先跑通流程
python scripts/generate_sample_data.py --data-dir data --months 3

# 口令升级成哈希
python scripts/hash_passwords.py --in-place
```

> **正式上线前务必替换掉示例数据**（示例账号 `001` / `salary2026`、
> `003` / `admin2026` 是公开的），并按 [SECURITY.md](SECURITY.md) 的
> 部署建议收紧防火墙与文件权限。

`data/` 目录应当**整目录拷贝**，不要只拷 `.xlsx`：

| 要带上的 | 说明 |
| --- | --- |
| `员工列表.xlsx` | 必需 |
| `salary/*.xlsx` | 必需 |
| `fields.txt` | 可选，但带上的话员工看到的字段布局就是熟悉的 |
| `port.txt` | 可选，端口配置 |
| `secret.key` | **升级时务必保留**（换了就全员重新登录） |
| `certs/` | 可选，删掉会自动重新生成自签证书 |
| `audit.log` | 可选，历史审计记录 |

---

## 5. 打包分发清单

### 5.1 必带

| 项 | 来源 |
| --- | --- |
| `salary-app-backend.exe` | `target/release/` |
| `salary-tray.exe` | `target/release/` |
| `web/index.html` + `web/assets/` | `src-tauri/dist/` |
| `data/员工列表.xlsx` | 你自己的（**不是示例数据**） |
| `data/salary/*.xlsx` | 你自己的 |

### 5.2 选带

| 项 | 什么时候带 |
| --- | --- |
| `data/fields.txt` | 想保留既有的字段显示布局 |
| `data/port.txt` | 想固定端口 |
| `data/certs/` | 想沿用同一张自签证书（否则会自动重新生成，浏览器要重新确认） |
| `data/audit.log` | 想带上历史审计记录 |
| `data/secret.key` | **多台机器统一部署时**，保证令牌互通；单台机器首次部署不需要 |

### 5.3 绝对不能带

| 项 | 原因 |
| --- | --- |
| `data/员工列表-哈希版.xlsx` | 脚本生成的中间产物 |
| `data/员工列表.xlsx.bak` | **明文口令备份** |
| `data/secret.key`（对外分发时） | 令牌签名密钥，泄露等于任意伪造令牌 |
| `data/certs/server.key` | HTTPS 私钥 |
| `data/audit.log` | 含工号、姓名、查询记录 |
| 任何 `.xlsx` 工资表明细（对内网外分发时） | 全部员工的工资数据 |
| `target/`、`node_modules/`、`src-tauri/dist/` 以外的源码 | 不需要 |
| `scripts/__pycache__/` | Python 字节码缓存 |

### 5.4 打包后的自检

在**打包目录**里做，不要在仓库里做：

```powershell
cd D:\release\salary-app

# 1) 目录里不该有任何 xlsx（数据目录要空或只有你打算放的东西）
Get-ChildItem -Recurse -Include *.xlsx, *.xls, *.bak, secret.key, *.key

# 2) 前端产物齐全
Test-Path .\web\index.html
Get-ChildItem .\web\assets

# 3) 起一个实例验证
.\salary-app-backend.exe
```

第 3 步的验证项：

- 启动横幅里的「数据目录」指向你放数据的那个目录；
- 浏览器打开 `http://localhost:5001` 能看到登录页（不是「前端尚未构建」）；
- 登录一次能查到工资条；
- `data/audit.log` 里出现 `LOGIN` 与 `VIEW` 记录。

> **在构建机上自检要先设环境变量。** `data/` 与 `web/` 的定位规则里，
> 编译期写进 exe 的仓库路径排在 exe 相对路径前面（见 3.1 的表格）。
> 在构建机上直接跑打包目录里的 exe，横幅会显示仓库的 `data/`，
> 而不是打包目录的——这不是打包坏了。用环境变量指过去即可模拟干净机器：
>
> ```powershell
> $env:SALARY_DATA_DIR = "D:\release\salary-app\data"
> $env:SALARY_WEB_DIR  = "D:\release\salary-app\web"
> .\salary-app-backend.exe
> ```
>
> 真正的部署机上没有那些编译期路径，exe 会自动用同级目录，无需设置。

### 5.5 打成压缩包

```powershell
$stage = "D:\release\salary-app"
$zip = "D:\release\salary-app-0.3.0.zip"
Remove-Item $zip -Force -ErrorAction SilentlyContinue
Compress-Archive -Path "$stage\*" -DestinationPath $zip
```

> 压缩包**不要包含 `data/` 里的敏感文件**。如果需要带 `data/`，
> 建议先打成一个内部包（仅内网传递），或者带 `data/` 但把 `secret.key`
> 与 `certs/` 排除掉，让目标机器首次启动时自己生成。

---

## 6. 发布前检查清单

### 6.1 必做

一条命令跑完格式、lint、测试与前端构建：

```powershell
scripts\verify.ps1
```

或者手动逐步来：

```powershell
# 1) 格式
cargo fmt --all -- --check

# 2) Lint（必须零告警）
cargo clippy --workspace --all-targets -- -D warnings

# 3) 测试
cargo test --workspace

# 4) 前端类型检查与构建
cd src-tauri
npm ci
npm run typecheck
npm run build
cd ..

# 5) release 构建
cargo build --release --bin salary-app-backend --locked
cargo build --release -p salary-tray --locked
```

CI（`.github/workflows/ci.yml`）分成两个 job：Linux 上的 `backend` job 跑后端
（`cargo clippy -p salary-app-backend` + `cargo test -p salary-app-backend`
+ 前端类型检查与构建），Windows 上的 `windows` job 跑整仓
（`cargo clippy --workspace` + `cargo test --workspace` + `cargo bench --workspace --no-run`
+ 前端构建）。托盘面板只能在 Windows 上构建，所以整仓检查必须在 Windows 上跑。

> CI 的两个 job 都会先跑 `python scripts/generate_sample_data.py` 生成示例数据，
> 所以依赖 `data/salary/` 的集成测试在 CI 上**会真正跑到**而不是静默跳过。
> 本地跑测试前也要先生成，否则那 9 个用例会直接跳过：

### 6.2 逐条确认

**代码**

- [ ] `cargo fmt --all -- --check` 无输出
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` 无告警
- [ ] `cargo test --workspace --locked` 全部通过，**没有用例被静默跳过**
      （集成测试跳过时会打印「示例数据缺失，跳过」）
- [ ] `npm run typecheck` 与 `npm run build` 成功

**版本与产物**

- [ ] `src-tauri/Cargo.toml` 的 `version` 已更新
- [ ] `tray/Cargo.toml` 的 `version` 已更新
- [ ] `Cargo.lock` 里两个包的版本已更新并已提交
- [ ] `target/release/salary-app-backend.exe` 与 `target/release/salary-tray.exe` 都已生成
- [ ] `src-tauri/dist/index.html` 与 `src-tauri/dist/assets/` 都存在

**打包内容**

- [ ] 打包目录里**没有** `data/员工列表.xlsx.bak`（明文口令备份）
- [ ] 打包目录里**没有** `data/secret.key`
- [ ] 打包目录里**没有** `data/certs/server.key`
- [ ] 打包目录里**没有**任何 `*.xlsx` / `*.xls`（除非是有意内部分发）
- [ ] 打包目录里**没有** `target/`、`node_modules/`、`__pycache__/`

**文档**

- [ ] 接口有增减时，`docs/API.md` 已同步
- [ ] 数据格式有变化时，`docs/DATA.md` 已同步
- [ ] `CHANGELOG.md` 的 `## [Unreleased]` 段落已整理，必要时分出新的版本号段落

**安全**

- [ ] 示例账号（`001` / `salary2026`、`003` / `admin2026`）已删除或改口令
- [ ] 员工表口令已升级成哈希（`python scripts/hash_passwords.py --in-place`）
- [ ] `data/员工列表.xlsx.bak` 已删除
- [ ] 防火墙规则限定到内网网段，不是「所有来源」
- [ ] 确认 `git ls-files data/` 没有敏感文件被跟踪

---

## 7. 升级步骤

### 7.1 标准升级

1. **停服务。** 托盘面板点「停止服务」，或结束进程：

   ```powershell
   taskkill /IM salary-app-backend.exe /T /F
   taskkill /IM salary-tray.exe /T /F
   ```

2. **备份当前版本。** 至少保留可执行文件 + 前端产物：

   ```powershell
   Copy-Item -Recurse D:\release\salary-app D:\release\salary-app-0.1.0-backup
   ```

3. **替换可执行文件与前端产物。** 替换 `salary-app-backend.exe`、`salary-tray.exe`
   和整个 `web/` 目录。**不要动 `data/`。**

4. **确认 `data/secret.key` 还在。** 换了密钥所有已登录员工的令牌会立即失效，
   都要重新登录。升级不是换密钥的时机。

5. **启动。**

   ```powershell
   D:\release\salary-app\salary-app-backend.exe
   ```

   或者用托盘：双击 `salary-tray.exe`，面板里点「启动服务」。

6. **验证。**

   - 启动横幅里的「数据目录」指向正确的目录；
   - 浏览器能打开登录页；
   - 拿一个真实账号登录并查一次工资条；
   - `data/audit.log` 里有新的 `LOGIN` / `VIEW` 记录；
   - 托盘面板的「字段显示」页能看到字段列表。

### 7.2 数据格式有变化时

如果新版本改了数据格式（读法变了），升级步骤之外还要注意：

- **先在副本上验证。** 把 `data/` 复制一份，用 `SALARY_DATA_DIR` 指过去先试：

  ```powershell
  $env:SALARY_DATA_DIR = "D:\release\salary-app\data-测试"
  .\salary-app-backend.exe
  ```

- **先备份 `data/`。** 升级前把整个 `data/` 复制一份留底。

- **升级后检查审计日志与查询结果。** 字段显示逻辑变了的话，
  对照 `data/fields.txt` 检查工资条上的字段是否符合预期。

### 7.3 多台机器升级

每台机器的 `data/secret.key` 是各自生成的（令牌不跨机器通用）。
升级时**逐台操作**，不要批量覆盖 `data/` 目录——那会把一台机器的密钥带到另一台，
或者把某台机器独有的工资数据覆盖掉。

正确做法是逐台：备份 → 换 exe 与 `web/` → 保留本机的 `data/` → 验证 → 下一台。

---

## 8. 回滚

### 8.1 回滚可执行文件与前端

```powershell
# 1) 停服务
taskkill /IM salary-app-backend.exe /T /F
taskkill /IM salary-tray.exe /T /F

# 2) 把备份的 exe 与 web/ 换回来
Copy-Item D:\release\salary-app-0.1.0-backup\salary-app-backend.exe D:\release\salary-app\ -Force
Copy-Item D:\release\salary-app-0.1.0-backup\salary-tray.exe        D:\release\salary-app\ -Force
Remove-Item D:\release\salary-app\web -Recurse -Force
Copy-Item D:\release\salary-app-0.1.0-backup\web D:\release\salary-app\web -Recurse

# 3) 启动并验证
D:\release\salary-app\salary-app-backend.exe
```

**`data/` 目录在整个升级与回滚过程中都不要动。** 只要数据格式没变，
旧版本读得懂新数据，新版本也读得懂旧数据。

### 8.2 回滚数据

只有在数据被写坏时才需要。回滚数据意味着**丢掉这段时间的所有审计记录与
工资表变更**，所以先确认：

```powershell
# 看看新数据有没有问题
Get-Content D:\release\salary-app\data\audit.log -Tail 20 -Encoding UTF8
```

确认要回滚后：

```powershell
taskkill /IM salary-app-backend.exe /T /F
Copy-Item D:\release\salary-app\data-backup\* D:\release\salary-app\data\ -Recurse -Force
```

> 回滚数据时**优先保留当前的 `data/secret.key`**：换了密钥会让所有员工的
> 登录状态失效，而那是比数据回滚更麻烦的问题。

### 8.3 回滚后的检查

- [ ] 服务能正常启动，启动横幅里的「数据目录」正确
- [ ] 登录页能打开，前端产物是旧版（`web/index.html` 的时间戳对得上备份）
- [ ] 真实账号能登录并查到工资条
- [ ] 托盘面板能打开，字段显示页正常
- [ ] `data/audit.log` 恢复写入

---

## 9. 部署形态速查

| 场景 | 用哪个 | 启动方式 |
| --- | --- | --- |
| 内网服务器，长期运行 | `salary-app-backend.exe` + `web/` | 计划任务 / 服务包装器 / 手工双击 |
| 本机运维，需要图形化启停 | `salary-tray.exe` + `web/` | 双击 `salary-tray.exe`，面板里点「启动服务」 |
| 开发调试 | `cargo run --bin salary-app-backend` | 终端 |
| 前端热更新 | `cargo run --bin salary-app-backend` + `npm run dev` | 两个终端 |

> 托盘面板里的「开机自启」会在注册表
> `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run`
> 下写入 `SalaryApp` 值，指向托盘 exe 的完整路径。托盘进程启动时**不会**
> 自动拉起服务——如果需要开机即开服务，在面板里点「启动服务」后服务会一直在，
> 或者用计划任务启动后端。
