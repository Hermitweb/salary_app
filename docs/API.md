# REST API

服务默认监听 `http://localhost:5001`（HTTPS 为 `https://localhost:5002`）。

除 `GET /healthz` 外，所有接口都要求在请求头中带上登录后获得的令牌：

```
Authorization: Bearer <token>
```

## 通用约定

成功响应：

```json
{ "ok": true, ... }
```

失败响应：

```json
{ "ok": false, "error": "面向用户的错误提示" }
```

| 状态码 | 含义 |
| --- | --- |
| 200 | 成功 |
| 400 | 请求参数错误（例如月份不是 1–12） |
| 401 | 未登录、令牌无效或已过期 |
| 403 | 需要管理员权限 |
| 404 | 资源不存在（员工表、工资表、记录） |
| 429 | 登录失败次数过多，已被临时锁定 |
| 500 | 服务端内部错误 |

---

## POST /api/auth/login

登录并签发会话令牌。

**请求**

```json
{ "username": "001", "password": "••••••" }
```

**响应 200**

```json
{
  "ok": true,
  "token": "<会话令牌>",
  "employee": {
    "employee_id": "001",
    "name": "张三",
    "dept": "工程部",
    "is_admin": false
  }
}
```

**响应 401**：工号或密码错误、或账号已停用。两种情况返回同一句提示，避免暴露工号是否存在。

**响应 429**：15 分钟内失败 5 次后触发锁定，锁定时长 15 分钟。

> 令牌默认 30 分钟有效，过期后重新登录即可。

---

## POST /api/logout

退出登录。令牌是无状态的，服务端只记录一条审计事件，由前端清除本地令牌。

**响应 200**

```json
{ "ok": true }
```

---

## GET /api/available_months

列出所有可查询的工资月份，按年月倒序。

**响应 200**

```json
{
  "ok": true,
  "months": [
    { "year": 2026, "month": 10 },
    { "year": 2026, "month": 9 },
    { "year": 2026, "month": 8 }
  ]
}
```

---

## GET /api/salary/{year}/{month}

查询**当前登录员工**在指定月份的工资条。

路径参数 `year` 为四位年份，`month` 为 1–12。接口不接受工号参数——员工身份完全来自会话令牌，因此无法越权查询他人工资。

**响应 200**

```json
{
  "ok": true,
  "year": 2026,
  "month": 10,
  "headers": ["员工号", "姓名", "部门", "基本工资", "本月实发工资"],
  "values": ["001", "张三", "工程部", "10280", "12610.54"],
  "data": {
    "员工号": "001",
    "姓名": "张三",
    "部门": "工程部",
    "基本工资": "10280",
    "本月实发工资": "12610.54"
  }
}
```

`headers` 与 `values` 一一对应，`data` 是同一份内容的键值对形式。

**响应 400**：月份不在 1–12 范围内。
**响应 404**：该月份没有工资表，或该员工在当月没有工资记录。

### GET /api/salary?year=&month=

同样效果的查询参数形式，保留是为了兼容 Python 原版（`/api/salary?year=2026&month=10`）。
两种形式返回完全一致的结果。

---

## GET /api/cache/stats

缓存统计。**仅管理员**可用。

**响应 200**

```json
{
  "ok": true,
  "stats": {
    "hits": 12,
    "misses": 3,
    "salary_files_cached": 1
  }
}
```

**响应 403**：当前账号不是管理员。

---

## POST /api/cache/invalidate

清空所有缓存。**仅管理员**可用。

**响应 200**

```json
{ "ok": true, "message": "缓存已清空" }
```

---

## POST /api/audit

上报一条操作事件（导出图片、打印等）。服务端写入 `data/audit.log`（JSON Lines）。

**请求**

```json
{ "action": "EXPORT_IMG", "detail": "2026-10" }
```

`action` 常用取值：`VIEW`、`LOGIN`、`LOGIN_FAIL`、`LOGOUT`、`EXPORT_IMG`、`EXPORT_PDF`、`PRINT`。

**响应 200**

```json
{ "ok": true }
```

---

## GET /api/audit/log

查看审计日志的最近若干条记录。**仅管理员**可用。

审计日志直接写进 `data/audit.log`（JSON Lines），这个接口把最近若干条解析出来，
方便管理员在网页上直接核对「谁在什么时候查看了谁的工资」。

**查询参数**

| 参数 | 默认值 | 说明 |
| --- | --- | --- |
| `limit` | `100` | 返回最近多少条，取值范围 1–1000，超出按边界截断 |

**响应 200**

```json
{
  "ok": true,
  "events": [
    {
      "timestamp": "2026-10-21T02:31:05.123456+00:00",
      "ip": "192.168.1.23",
      "action": "VIEW",
      "employee_id": "001",
      "employee_name": "张三",
      "detail": "2026-10"
    }
  ]
}
```

**响应 403**：当前账号不是管理员。

> 日志文件超过 5 MB 会自动轮转：当前日志改名为 `data/audit.log.1`，只保留最近一份备份。
> 解析不了的行会被跳过——日志被外部工具改过也不会导致整个接口不可用。

---

## GET /healthz

健康检查，不需要认证，供负载均衡器或监控探活使用。

**响应 200**：`OK`（纯文本）

---

## 用 curl 调试

```bash
# 登录并保存令牌
TOKEN=$(curl -s -X POST http://localhost:5001/api/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"001","password":"salary2026"}' \
  | python -c "import sys,json; print(json.load(sys.stdin)['token'])")

# 可查询月份
curl -s http://localhost:5001/api/available_months -H "Authorization: Bearer $TOKEN"

# 工资条
curl -s http://localhost:5001/api/salary/2026/10 -H "Authorization: Bearer $TOKEN"
```
