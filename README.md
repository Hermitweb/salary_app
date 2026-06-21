# 员工工资查询系统

局域网内员工自助查询工资的轻量级 Web 应用。

## ✨ 特性

- 🔒 登录认证（工号 = 账号，紧急联系人号码 = 密码）
- 📊 工资条表格化展示（仿照传统工资条样式）
- 📷 一键截图导出 PNG
- 🏷️ 字段三态配置（auto / show / hide）
- 🌐 同时支持 HTTP 和 HTTPS（自签名证书）
- 📋 访问审计日志（data/access.log）
- 🖥️ Tkinter 控制面板（启动/停止服务、字段配置、最小化到托盘）
- 📦 打包为便携版（onedir 模式，低杀软误报率）

## 🗂️ 目录结构

```
salary_app/
├── app.py                  # Flask 应用入口
├── excel_data.py           # Excel 数据处理（员工/工资/字段配置）
├── control_panel_pkg/      # Tkinter 控制面板（拆分为 5 个模块）
│   ├── __init__.py
│   ├── constants.py        # 路径/常量/工具函数
│   ├── service.py          # ServiceManager（HTTP/HTTPS + 看门狗）
│   ├── field_config.py     # FieldConfig + FieldItem
│   ├── tray.py             # 系统托盘
│   └── ui.py               # 主界面 + 事件处理
├── colors.py               # 颜色常量
├── build_portable.py       # 便携版打包脚本
├── requirements.txt        # Python 依赖
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
│   └── smoke_test.py
└── data/                   # 数据目录（运行时由 EXE/源码同级管理）
    ├── 员工列表.xlsx       # 员工账号信息
    ├── salary/             # 工资表目录
    │   └── 2026年05月员工工资.xlsx
    ├── fields.txt          # 字段显示配置
    ├── port.txt            # 服务端口
    ├── secret.key          # Flask session 密钥（首次启动生成）
    ├── access.log          # 访问审计日志
    └── certs/              # HTTPS 证书（首次启动生成）
```

## 🚀 快速开始

### 开发模式

```bash
# 安装依赖
pip install -r requirements.txt

# 启动 Flask 应用
python app.py

# 启动控制面板（推荐）
python -m control_panel_pkg
```

### 访问

服务启动后，浏览器打开：
- HTTP: `http://localhost:5000`
- HTTPS: `https://localhost:5001`（首次会提示证书不受信任，选择"继续访问"）

局域网其他设备访问：`http://<本机IP>:5000`

### 登录

- 账号：员工工号
- 密码：紧急联系人号码（在员工列表中设置）

## 📦 打包便携版

```bash
# onedir 模式（默认，推荐，杀毒软件误报率低）
python -m PyInstaller --noconfirm --onedir --windowed --clean ^
    --name "工资查询系统" ^
    --add-data "templates;templates" ^
    --add-data "app.py;." ^
    --add-data "excel_data.py;." ^
    --add-data "static;static" ^
    --add-data "control_panel_pkg;control_panel_pkg" ^
    -m control_panel_pkg
python build_portable.py
```

生成 `便携版/工资查询系统/工资查询系统.exe` + `启动.vbs` + `便携版.zip`。

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

修改后保存即生效，无需重启服务。

### 端口配置

编辑 `data/port.txt`：
```
5000
```

## 🧪 测试

```bash
python -m pytest tests/ -v
```

## 🔐 安全

- `secret_key` 首次启动随机生成并持久化到 `data/secret.key`
- 所有写操作（POST）需要 CSRF token
- 访问日志记录在 `data/access.log`

## 🛠️ 常见问题

**Q: 控制面板提示"启动异常"？**
A: 检查 `data/access.log` 和控制台输出，确认端口未被占用。

**Q: HTTPS 证书警告？**
A: 自签名证书是正常的，浏览器选择"高级 → 继续访问"即可。如需正式证书，替换 `data/certs/server.crt` 和 `server.key`。

**Q: 字段配置不生效？**
A: 字段名必须完全匹配工资表中的列名。修改后无需重启。

## 📝 License

Internal use only.
