# 员工工资查询系统

局域网内员工自助查询工资的轻量级 Web 应用。

## ✨ 特性

- 🔒 登录认证（工号 = 账号，紧急联系人号码 = 密码）
- 🛡 登录失败锁定：同一工号或同一 IP 15 分钟内失败 5 次即锁定 15 分钟
- 📊 工资条表格化展示（仿照传统工资条样式）
- 📷 一键截图导出 PNG
- 🖨️ 导出 / 打印动作记录到服务端审计日志
- 🏷️ 字段三态配置（auto / show / hide），保存即生效
- 📋 工资条列映射外置配置（`data/payslip_columns.json`），无需改代码即可适配新表
- 🌐 同时支持 HTTP 和 HTTPS（自签名证书，HTTP 自动 301 跳转 HTTPS）
- ⏱️ 会话 30 分钟超时，Cookie 启用 HttpOnly / SameSite
- 📋 访问审计日志（data/access.log，自动轮转，单文件 1MB × 3 份）
- 🖥️ Tkinter 控制面板（启动/停止服务、字段配置、开机自启、防火墙放行、查看日志、最小化到托盘）
- 🐕 看门狗：崩溃自动重启（含重启频率限制）+ `/healthz` 死锁探活
- 📦 打包为便携版（onedir 模式，低杀软误报率，含图标与版本信息）

## 🗂️ 目录结构

```
salary_app/
├── app.py                  # Flask 应用入口
├── excel_data.py           # Excel 数据处理（员工/工资/字段配置）
├── cache.py                # 数据缓存层
├── control_panel_pkg/      # Tkinter 控制面板（拆分为 5 个模块）
│   ├── __init__.py
│   ├── constants.py        # 路径/常量/工具函数
│   ├── service.py          # ServiceManager（HTTP/HTTPS + 看门狗 + 死锁探活）
│   ├── field_config.py     # FieldConfig + FieldItem
│   ├── tray.py             # 系统托盘
│   └── ui.py               # 主界面 + 事件处理
├── colors.py               # 颜色常量
├── build_portable.py       # 便携版打包脚本
├── md2docx.py              # 使用说明书 md → docx 转换
├── 使用说明书.md / .docx    # 终端用户文档
├── 工资查询系统.spec        # PyInstaller 打包配置（含图标/版本信息）
├── app_icon.ico            # EXE 图标
├── version_info.txt        # EXE 版本资源
├── requirements.txt        # Python 依赖（版本锁定）
├── templates/              # HTML 模板
│   ├── login.html
│   └── salary.html
├── static/                 # 静态资源
│   └── js/html2canvas.min.js
├── tests/                  # 单元测试
│   ├── test_field_filter.py
│   ├── test_salary_file.py
│   ├── test_secret_key.py
│   ├── test_utils.py
│   ├── test_watchdog.py
│   ├── test_multiuser.py
│   ├── test_p1_p2.py       # P1/P2 专项测试
│   └── smoke_test.py
└── data/                   # 数据目录（运行时由 EXE/源码同级管理）
    ├── 员工列表.xlsx       # 员工账号信息
    ├── salary/             # 工资表目录
    │   └── 2026年05月员工工资.xlsx
    ├── fields.txt          # 字段显示配置
    ├── payslip_columns.json # 工资条列映射配置（可选，覆盖默认值）
    ├── port.txt            # 服务端口
    ├── secret.key          # Flask session 密钥（首次启动生成）
    ├── access.log          # 访问审计日志（自动轮转）
    └── certs/              # HTTPS 证书（首次启动生成）
```

## 🚀 快速开始

### 开发模式

```bash
# 安装依赖（版本已锁定）
pip install -r requirements.txt

# 启动 Flask 应用
python app.py

# 启动控制面板（推荐）
python -m control_panel_pkg
```

### 访问

服务启动后，浏览器打开：
- HTTPS: `https://localhost:5001`（首次会提示证书不受信任，选择"继续访问"）
- HTTP: `http://localhost:5000` 会自动 301 跳转到 HTTPS（HTTPS 未启用时除外）

局域网其他设备访问：`https://<本机IP>:5001`

> 局域网访问若被拦截，在控制面板点击「🛡 防火墙一键放行」（需管理员权限）。

### 登录

- 账号：员工工号
- 密码：紧急联系人号码（在员工列表中设置）

**忘记密码 / 重置密码**：管理员编辑 `data/员工列表.xlsx`，将该员工的「紧急联系人号码」列改为新值并保存，员工下次登录即用新密码，无需重启服务。

## 📦 打包便携版

```bash
# 推荐使用 spec 文件（自动携带图标与版本信息）
python -m PyInstaller --noconfirm --clean 工资查询系统.spec

# 或一键打包发布
一键打包发布.bat
```

生成 `便携版/工资查询系统/工资查询系统.exe` + `启动.vbs` + `便携版.zip`。

## 📄 数据文件规范

### 员工列表.xlsx（标准模式，推荐）

第 1 行为表头，第 2 行起为员工数据。系统识别的列名：

| 列名 | 必填 | 说明 |
|------|------|------|
| 员工号 / 工号 | ✅ | 登录账号，也是工资表关联键 |
| 密码 / 紧急联系人 / 紧急联系人号码 / 手机号 | ✅（至少其一） | 登录密码 |
| 姓名 | 建议 | 显示用途 |
| 部门、管理员、离职、离职否 | 可选 | 部门显示；管理员标记；离职过滤 |

> ⚠️ 若找不到密码列，登录功能会**整体禁用并显式报错**（不会把其他列误当密码），请在控制面板查看错误提示并修正表头。

也支持**原始模式**（第 1 行账号、第 2 行密码、第 3 行姓名……逐行布局），标准模式优先。

### 工资表

放入 `data/salary/`，文件名包含年月即可自动识别（推荐 `2026年05月员工工资.xlsx`）。表内需含「员工号」列用于匹配员工。

### 工资条列映射（可选）

工资条各项与 Excel 列名的对应关系默认内置，如需适配新表头，编辑 `data/payslip_columns.json`：

```json
{
  "base_salary": ["应发基本工资"],
  "net_salary": ["本月实发工资", "实发工资"]
}
```

键为工资条项目（base_salary / net_salary / social_ins 等），值为候选列名列表（按顺序匹配第一个命中的列）。文件不存在时使用内置默认值，修改保存后刷新页面即生效，无需重启。

## ⚙️ 配置

### 字段显示配置

编辑 `data/fields.txt`，每行一个字段：
```
# 字段名 = 状态
# 状态: show 强制显示 | hide 强制隐藏 | auto 自动（空/0 不显示）
员工号 = show
应发基本工资 = auto
本月实发工资 = show
```

修改后保存即生效，无需重启服务。控制面板「字段配置」页也可图形化修改，点「🔄 刷新字段」重新读取。

### 端口配置

编辑 `data/port.txt`（或控制面板「服务端口」输入框）：
```
5000
```

HTTPS 端口自动为 HTTP 端口 +1。

## 🧪 测试

```bash
python -m pytest tests/ -v
```

## 🔐 安全

- `secret_key` 首次启动随机生成并持久化到 `data/secret.key`
- 所有写操作（POST）需要 CSRF token（含退出登录）
- 登录失败锁定：同一工号或同一 IP 15 分钟内失败 5 次即锁定 15 分钟
- 密码比对使用恒定时间比较（`hmac.compare_digest`），防时序侧信道
- 会话 TTL 30 分钟；Cookie HttpOnly + SameSite=Lax；启用 HTTPS 时自动加 Secure
- HTTPS 启用时，HTTP 端口仅做 301 跳转，凭据不走明文
- 员工表缺少工号/密码列时禁用登录并显式报错（不回落到其他列）
- 截图 / PDF / 打印动作均写入审计日志（含操作人 IP 与工号），外泄可溯源
- 访问日志记录在 `data/access.log`（RotatingFileHandler，1MB × 3 份）
- 看门狗崩溃自动重启（时间窗内限次），`/healthz` 探活失败强制重启疑似死锁进程

## 🛠️ 常见问题

**Q: 控制面板提示"启动异常"？**
A: 点「📋 查看日志」按钮打开 `data/access.log` 排查，常见原因是端口被占用。

**Q: HTTPS 证书警告？**
A: 自签名证书是正常的，浏览器选择"高级 → 继续访问"即可。如需正式证书，替换 `data/certs/server.crt` 和 `server.key`。

**Q: 字段配置不生效？**
A: 字段名必须完全匹配工资表中的列名。修改后无需重启，可点「🔄 刷新字段」确认加载。

**Q: 如何重置员工密码？**
A: 编辑 `data/员工列表.xlsx` 中该员工的「紧急联系人号码」列，保存即生效。

**Q: 开机自动启动？**
A: 控制面板「系统设置」勾选「开机自动启动」（写入当前用户注册表 Run 键）。

## 📝 License

Internal use only.
