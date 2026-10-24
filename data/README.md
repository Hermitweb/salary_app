# 数据目录

运行时数据目录。可以通过 `SALARY_DATA_DIR` 环境变量指向别处。

## 需要放置的文件

| 路径 | 说明 |
| --- | --- |
| `员工列表.xlsx` | 必需。工号、密码为必需列（没有「密码」列时依次回落到手机号 / 联系电话 / 紧急联系人号码）；姓名、部门、离职状态可选。网页端不区分管理员，「管理员 / 角色」列会被忽略。支持标准模式（首行表头）与原始模式（每列一名员工） |
| `salary/*.xlsx` | 必需。每月一个工资表，文件名需包含年月 |
| `fields.txt` | 可选。控制工资条里哪些字段显示 |
| `networks.txt` | 可选。控制面板「访问地址」卡片里列出的「支持访问的网络」，用来回答同事「在哪个 WiFi 能打开」 |

完整的格式说明见 [../docs/DATA.md](../docs/DATA.md)。

## 自动生成的文件

| 路径 | 说明 |
| --- | --- |
| `secret.key` | 令牌签名密钥（十六进制），首次启动时生成 |
| `certs/server.crt`、`certs/server.key` | HTTPS 自签名证书 |
| `audit.log` | 审计日志，JSON Lines 格式。超过 5 MB 自动轮转为 `audit.log.1`（只保留最近一份） |
| `credentials.json` | 控制面板「服务控制」页账号密码卡保存的网页登录凭据。密码用 Windows DPAPI 加密，只有当前 Windows 用户能解开 |

`port.txt` 可手动写入端口号（控制面板在 Python 版里也是这么存的），
优先级：`SALARY_PORT` 环境变量 > `port.txt` > 默认 5001。

`networks.txt` 每行一条，用竖线分隔：

```
有线 | 办公区域网络 | yes
WiFi | JTOFFICEWIFI、JTOFFICEWIFI_2.4G | yes
宿舍网络 | 暂不支持访问 | no
```

第三段填 `yes` / `no`（也认 `是` / `否` / `true` / `1`），
控制面板会用 ✓ / ✗ 标出来。

## 口令保护

员工表里的「密码」列可以存明文，也可以存 PBKDF2 哈希，两种格式可以混用。
批量升级见 [../docs/DATA.md](../docs/DATA.md) 或直接跑：

```bash
python scripts/hash_passwords.py --dry-run   # 先看会改什么
python scripts/hash_passwords.py             # 生成带哈希口令的新表
```

## 示例数据

```bash
python scripts/generate_sample_data.py --data-dir data
```

## 注意

这个目录下的 Excel 文件包含敏感信息，已被 `.gitignore` 排除，不要提交到版本库。

`credentials.json` 同样是敏感文件（密码密文 + 账号），已在 `.gitignore` 中排除。
