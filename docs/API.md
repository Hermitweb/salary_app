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
| 404 | 资源不存在（员工表、工资表、记录） |
| 429 | 登录失败次数过多，已被临时锁定 |
| 500 | 服务端内部错误 |

## 跨域

默认不返回任何 CORS 响应头，浏览器只能同源访问。

需要跨域调用时设置 `SALARY_ALLOW_ORIGIN`：

| 取值 | 效果 |
| --- | --- |
| 不设置 / 空 | 只允许同源 |
| `https://hr.example.com` | 只放行这一个来源 |
| `*` 或任意非空值 | 放行任意来源 |

> 令牌放在 `Authorization` 头里而不是 Cookie，因此放开跨域不会让别的网站
> 自动带上用户的令牌。但仍建议指定具体来源，别把服务暴露给任意网站。

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
    "dept": "工程部"
  }
}
```

**响应 401**：工号或密码错误、或账号已停用。两种情况返回同一句提示，避免暴露工号是否存在。

**响应 429**：15 分钟内失败 5 次后触发锁定，锁定时长 15 分钟。

> 令牌默认 30 分钟有效，过期后重新登录即可。

---

## GET /api/auth/me

用会话令牌换取当前员工信息（工号、姓名、部门）。

托盘控制面板把凭据交给浏览器时，地址里带的是**令牌**而不是密码；
前端拿到令牌后调这个接口补齐员工信息，姓名与部门才能正常显示。

**响应 200**

```json
{
  "ok": true,
  "employee": {
    "employee_id": "001",
    "name": "张三",
    "dept": "工程部"
  }
}
```

**响应 401**：缺少令牌、令牌无效或已过期。

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
  },
  "notice": null
}
```

`headers` 与 `values` 一一对应，`data` 是同一份内容的键值对形式。

**查不到指定月份时会回落到最新的一份工资表**（与 Python 原版一致），
这时 `year` / `month` 是**实际返回**的那个月，`notice` 里会写明实际情况：

```json
{
  "ok": true,
  "year": 2026,
  "month": 10,
  "notice": "未找到 2026 年 01 月的工资表，已改为返回 2026 年 10 月的数据。",
  "headers": ["…"],
  "values": ["…"]
}
```

前端会把这个 `notice` 显示成一条提示条，不会让人误以为数据错乱。

**响应 400**：月份不在 1–12 范围内。
**响应 404**：没有任何工资表可查，或该员工在当月没有工资记录。

### GET /api/salary?year=&month=

同样效果的查询参数形式，保留是为了兼容 Python 原版（`/api/salary?year=2026&month=10`）。
两种形式返回完全一致的结果。

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

> 审计日志只写进 `data/audit.log`（JSON Lines），运维直接看文件；
> 网页端不再提供查阅接口。
> 日志文件超过 5 MB 会自动轮转：当前日志改名为 `data/audit.log.1`，只保留最近一份备份。

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
