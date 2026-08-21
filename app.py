import os
import sys
import json
import threading
from pathlib import Path
from flask import (
    Flask, request, render_template, render_template_string,
    redirect, url_for, session, jsonify,
)

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import excel_data
from excel_data import DATA_DIR

# app.py 内部使用 Path 风格的路径（兼容 pathlib API）
_DATA_DIR = Path(DATA_DIR)
app = Flask(__name__)

# 密钥管理：首次启动时生成随机密钥并持久化到 data/secret.key
# 避免硬编码（源码可见，session 可被伪造）
_SECRET_KEY_FILE = _DATA_DIR / "secret.key"


def _load_or_create_secret_key():
    """加载或创建持久化的 secret_key"""
    if _SECRET_KEY_FILE.exists():
        try:
            return _SECRET_KEY_FILE.read_text(encoding="utf-8").strip()
        except OSError:
            pass
    # 生成新的随机密钥
    import os as _os
    key = _os.urandom(32).hex()
    try:
        _SECRET_KEY_FILE.write_text(key, encoding="utf-8")
    except OSError:
        # 写入失败（只读目录）则用临时密钥（每次启动变化，登录态会丢）
        pass
    return key


app.secret_key = _load_or_create_secret_key()
app.config['TEMPLATES_AUTO_RELOAD'] = True
app.jinja_env.auto_reload = True

# === 会话加固（P1-10）===
# 30 分钟无操作过期；HttpOnly + SameSite=Lax 防 CSRF/脚本读取。
# Secure 标志由 ServiceManager 在确认 HTTPS 启用后再打开
# （纯 HTTP 模式下打开会导致 Cookie 无法发送，登录直接失效）。
from datetime import timedelta as _timedelta
app.config['PERMANENT_SESSION_LIFETIME'] = _timedelta(minutes=30)
app.config['SESSION_COOKIE_HTTPONLY'] = True
app.config['SESSION_COOKIE_SAMESITE'] = 'Lax'
app.config['SESSION_COOKIE_SECURE'] = False


# === CSRF 防护（轻量级实现，无额外依赖）===
import hmac as _hmac
import hashlib as _hashlib
import time as _time


def _generate_csrf_token():
    """生成或获取当前 session 的 CSRF token"""
    if "_csrf_token" not in session:
        import os as _os
        session["_csrf_token"] = _os.urandom(24).hex()
    return session["_csrf_token"]


def _verify_csrf_token(token):
    """验证表单提交的 CSRF token"""
    expected = session.get("_csrf_token")
    if not expected or not token:
        return False
    return _hmac.compare_digest(expected, token)


app.jinja_env.globals["csrf_token"] = _generate_csrf_token


@app.before_request
def _csrf_protect():
    """拦截所有写操作（POST/PUT/DELETE）进行 CSRF 校验"""
    if request.method in ("POST", "PUT", "DELETE", "PATCH"):
        # 登录/登出也需要校验
        token = request.form.get("_csrf_token", "")
        if not _verify_csrf_token(token):
            # 使用 make_response 显式设置 403 状态码（兼容不同 Flask 版本）
            from flask import make_response
            resp = make_response(
                render_template_string(
                    "<h3>CSRF 验证失败</h3><p>请刷新页面后重试</p>"
                )
            )
            resp.status_code = 403
            return resp


# === 登录失败锁定（内存实现：工号 + IP 双维度，重启清零可接受）===
_LOGIN_FAIL_LIMIT = 5          # 窗口内允许的失败次数
_LOGIN_LOCK_WINDOW = 15 * 60   # 统计窗口（秒）
_LOGIN_LOCK_DURATION = 15 * 60  # 锁定时长（秒）
_login_failures = {}   # key -> [失败时间戳...]
_login_locks = {}      # key -> 锁定到期时间戳
_login_mutex = threading.Lock()


def _client_ip():
    return request.remote_addr or "-"


def _login_locked(username):
    """返回 (是否锁定, 剩余秒数)"""
    now = _time.time()
    keys = (f"u:{username}", f"ip:{_client_ip()}")
    with _login_mutex:
        for key in keys:
            until = _login_locks.get(key, 0)
            if until > now:
                return True, int(until - now) + 1
        return False, 0


def _record_login_failure(username):
    """记录一次失败（工号与 IP 两个维度都记账）；触发锁定时返回第一个锁定的 key"""
    now = _time.time()
    cutoff = now - _LOGIN_LOCK_WINDOW
    keys = (f"u:{username}", f"ip:{_client_ip()}")
    locked_key = None
    with _login_mutex:
        for key in keys:
            fails = _login_failures.setdefault(key, [])
            fails.append(now)
            while fails and fails[0] < cutoff:
                fails.pop(0)
            if len(fails) >= _LOGIN_FAIL_LIMIT:
                _login_locks[key] = now + _LOGIN_LOCK_DURATION
                _login_failures.pop(key, None)
                if locked_key is None:
                    locked_key = key
        # 顺带清理过期锁与陈旧失败计数（攻击者枚举大量工号时 u: 键会不断新增，
        # 若不清理字典会随时间无限增长）
        for key in [k for k, v in _login_locks.items() if v <= now]:
            _login_locks.pop(key, None)
        for key in [k for k, v in _login_failures.items() if not v or v[-1] < cutoff]:
            _login_failures.pop(key, None)
    return locked_key


def _clear_login_failures(username):
    """登录成功后清除该工号与该 IP 的失败计数"""
    with _login_mutex:
        _login_failures.pop(f"u:{username}", None)
        _login_failures.pop(f"ip:{_client_ip()}", None)
        # 顺带清理已过期的锁，避免字典无限增长
        now = _time.time()
        for key in [k for k, v in _login_locks.items() if v <= now]:
            _login_locks.pop(key, None)


def _lock_kind(lock_key):
    return "工号" if lock_key.startswith("u:") else "IP"


def _reset_login_limits():
    """清空登录失败计数与锁定状态（测试隔离用）"""
    with _login_mutex:
        _login_failures.clear()
        _login_locks.clear()


# === 访问审计日志（记录谁在什么时间查了什么）===
import logging as _logging
from logging.handlers import RotatingFileHandler as _RotatingFileHandler
_access_logger = _logging.getLogger("access_audit")
_access_logger.setLevel(_logging.INFO)
_access_logger.propagate = False  # 不传到根 logger，避免重复
try:
    _access_handler = _RotatingFileHandler(
        str(_DATA_DIR / "access.log"), encoding="utf-8",
        maxBytes=1024 * 1024, backupCount=3,  # 1MB x 3 个备份，防止日志撑爆磁盘
    )
    _access_handler.setFormatter(
        _logging.Formatter("%(asctime)s|%(message)s", datefmt="%Y-%m-%d %H:%M:%S")
    )
    _access_logger.addHandler(_access_handler)
except OSError:
    # 写日志失败（只读）则跳过，不影响主功能
    pass


def _log_access(action, employee=None, detail=""):
    """记录访问日志：IP|动作|工号|姓名|详情"""
    # 本系统为局域网直连部署（无反向代理），统一取 TCP 对端地址；
    # X-Forwarded-For 头可被客户端任意伪造，不作为审计依据
    ip = request.remote_addr or "-"
    emp_id = employee.get("employee_id", "-") if employee else "-"
    emp_name = employee.get("name", "-") if employee else "-"
    _access_logger.info(f"{ip}|{action}|{emp_id}|{emp_name}|{detail}")


def _parse_int_range(raw, lo, hi):
    """解析整数并限定范围；非法/缺失返回 None，绝不抛异常"""
    try:
        v = int(str(raw).strip())
    except (TypeError, ValueError):
        return None
    return v if lo <= v <= hi else None


# 会话过期感知（P3 体验）：session 过期后整体清空，无法区分"从未登录"与"刚过期"。
# 登录成功时种一个与 session 同 TTL（30 分钟）的普通 cookie，
# 未登录访问受保护页时若它还在 → 是刚过期，跳登录页并提示
_WAS_LOGGED_COOKIE = "was_logged_in"


def _mark_logged_in(resp):
    try:
        resp.set_cookie(
            _WAS_LOGGED_COOKIE, "1",
            max_age=int(app.config["PERMANENT_SESSION_LIFETIME"].total_seconds()),
            httponly=True, samesite="Lax",
        )
    except Exception:
        pass
    return resp


def _clear_logged_mark(resp):
    try:
        resp.delete_cookie(_WAS_LOGGED_COOKIE)
    except Exception:
        pass
    return resp


def _login_redirect():
    """未登录统一跳转：刚过期（cookie 还在）则带 expired 提示"""
    expired = 1 if request.cookies.get(_WAS_LOGGED_COOKIE) else None
    return redirect(url_for("login", expired=expired))


def _parse_year_month(year_raw, month_raw):
    """分别校验年/月参数；缺失或非法的返回 None（互不影响）"""
    return _parse_int_range(year_raw, 1990, 2100), _parse_int_range(month_raw, 1, 12)


@app.route("/")
def index():
    if "employee" in session:
        return redirect(url_for("salary_view"))
    return redirect(url_for("login"))


@app.route("/login", methods=["GET", "POST"])
def login():
    if request.method == "POST":
        username = request.form.get("username", "").strip()
        password = request.form.get("password", "").strip()

        if not username or not password:
            _log_access("LOGIN_FAIL", detail=f"账号={username} 空字段")
            return render_template("login.html", error="请输入工号和密码")

        # 登录失败锁定检查（工号 + IP 双维度）
        locked, wait = _login_locked(username)
        if locked:
            mins = max(1, wait // 60)
            _log_access("LOGIN_BLOCKED", detail=f"账号={username} IP={_client_ip()} 剩余{wait}秒")
            return render_template(
                "login.html",
                error=f"失败次数过多，账号或所在网络已临时锁定，请约 {mins} 分钟后再试",
            )

        result = excel_data.authenticate(username, password)

        if result["ok"]:
            _clear_login_failures(username)
            session["employee"] = result["employee"]
            session.permanent = True  # 使 PERMANENT_SESSION_LIFETIME（30 分钟）生效
            _log_access("LOGIN_OK", employee=result["employee"])
            return _mark_logged_in(redirect(url_for("salary_view")))
        else:
            lock_key = _record_login_failure(username)
            if lock_key:
                _log_access("LOGIN_LOCKED", detail=f"账号={username} IP={_client_ip()} 触发{_lock_kind(lock_key)}锁定")
                return render_template(
                    "login.html",
                    error="失败次数过多，已临时锁定 15 分钟，请稍后再试",
                )
            _log_access("LOGIN_FAIL", detail=f"账号={username} IP={_client_ip()} 原因={result['error']}")
            return render_template("login.html", error=result["error"])

    return render_template("login.html", expired=request.args.get("expired"))


@app.route("/logout", methods=["GET", "POST"])
def logout():
    if request.method == "GET":
        # GET 不执行登出（防跨站触发），仅跳转首页
        return redirect(url_for("index"))
    employee = session.get("employee")
    _log_access("LOGOUT", employee=employee)
    session.clear()
    return _clear_logged_mark(redirect(url_for("login")))


@app.route("/salary", methods=["GET", "POST"])
def salary_view():
    employee = session.get("employee")
    if not employee:
        return _login_redirect()

    available_months = excel_data.get_available_months()

    year = request.args.get("year") or request.form.get("year")
    month = request.args.get("month") or request.form.get("month")
    year_i, month_i = _parse_year_month(year, month)

    # 如果没有找到任何工资表，尝试用文件 mtime 找最新文件
    if not available_months:
        import datetime
        candidates = []
        for d in [os.path.join(excel_data.DATA_DIR, "salary"),
                  excel_data.DATA_DIR]:
            if not os.path.exists(d):
                continue
            try:
                for fname in os.listdir(d):
                    fpath = os.path.join(d, fname)
                    if not (fname.endswith(".xlsx") or fname.endswith(".xls")):
                        continue
                    if not os.path.isfile(fpath):
                        continue
                    try:
                        mtime = os.path.getmtime(fpath)
                        candidates.append((mtime, fpath, fname))
                    except OSError:
                        pass
            except OSError:
                pass
        # 按 mtime 倒序排，取最新的
        if candidates:
            candidates.sort(reverse=True)
            dt = datetime.datetime.fromtimestamp(candidates[0][0])
            available_months = [{"year": dt.year, "month": dt.month}]
            year_i, month_i = dt.year, dt.month

    if not available_months:
        return render_template("salary.html", employee=employee, error="没有可用的工资数据",
                               available_months=[], available_years=[],
                               months_for_year=[], salary=None,
                               selected_year="", selected_month="")

    # 解析可用的年份（去重，倒序）和月份
    available_years = sorted({m["year"] for m in available_months}, reverse=True)

    # 如果没选年份或年份不合法/不存在，默认选最新年份
    if year_i is None or year_i not in available_years:
        year_i = available_years[0]
    # 如果月份不合法或不在该年可用列表中，默认选该年最新月份
    months_for_year = sorted(
        [m["month"] for m in available_months if m["year"] == year_i],
        reverse=True,
    )
    if month_i is None or month_i not in months_for_year:
        month_i = months_for_year[0] if months_for_year else None

    salary = None
    payslip = None
    error_msg = None
    result = excel_data.get_salary(employee["employee_id"], year_i, month_i)
    if result["ok"]:
        salary = {
            "headers": result["headers"],
            "values": result["values"],
            "data": result["data"],
        }
        payslip = excel_data.build_payslip_table(
            result["data"], employee, year_i, month_i
        )
        _log_access("VIEW", employee=employee, detail=f"{year_i}-{month_i:02d}")
    else:
        error_msg = result["error"]
        _log_access("VIEW_FAIL", employee=employee,
                    detail=f"{year_i}-{month_i:02d} 原因={error_msg}")

    return render_template(
        "salary.html",
        employee=employee,
        available_months=available_months,
        available_years=available_years,
        months_for_year=months_for_year,
        salary=salary,
        payslip=payslip,
        selected_year=str(year_i),
        selected_month=str(month_i) if month_i is not None else "",
        error=error_msg,
    )


@app.route("/api/available_months")
def api_available_months():
    # 月份列表可反推发薪节奏，属于内部信息：要求登录后才可获取
    employee = session.get("employee")
    if not employee:
        return jsonify({"ok": False, "error": "未登录"}), 401
    return jsonify(excel_data.get_available_months())


@app.route("/api/salary")
def api_salary():
    employee = session.get("employee")
    if not employee:
        return jsonify({"ok": False, "error": "未登录"}), 401

    year = request.args.get("year")
    month = request.args.get("month")
    if not year or not month:
        return jsonify({"ok": False, "error": "缺少年月参数"}), 400
    year_i, month_i = _parse_year_month(year, month)
    if year_i is None or month_i is None:
        return jsonify({"ok": False, "error": "年月参数无效"}), 400

    result = excel_data.get_salary(employee["employee_id"], year_i, month_i)
    return jsonify(result)


@app.route("/api/cache/stats")
def api_cache_stats():
    """缓存统计（管理员可访问）"""
    employee = session.get("employee")
    if not employee or not employee.get("is_admin"):
        return jsonify({"ok": False, "error": "无权访问"}), 403
    from cache import DataCache
    return jsonify({"ok": True, "stats": DataCache.instance().get_stats()})


@app.route("/api/cache/invalidate", methods=["POST"])
def api_cache_invalidate():
    """手动失效缓存（管理员）"""
    employee = session.get("employee")
    if not employee or not employee.get("is_admin"):
        return jsonify({"ok": False, "error": "无权访问"}), 403
    from cache import DataCache
    DataCache.instance().invalidate_all()
    return jsonify({"ok": True, "message": "缓存已清空"})


@app.route("/healthz")
def healthz():
    """看门狗死锁探活端点（公开、无副作用，供本机探活）"""
    return "OK", 200


@app.route("/api/audit", methods=["POST"])
def api_audit():
    """导出/打印动作审计（P1-14：前端导出截图、导出PDF、打印时上报）"""
    employee = session.get("employee")
    if not employee:
        return jsonify({"ok": False, "error": "未登录"}), 401
    action = request.form.get("action", "").strip().upper()
    if action not in ("EXPORT_IMG", "EXPORT_PDF", "PRINT"):
        return jsonify({"ok": False, "error": "未知动作"}), 400
    detail = request.form.get("detail", "")[:100]
    _log_access(action, employee=employee, detail=detail)
    return jsonify({"ok": True})


@app.errorhandler(404)
def _error_404(e):
    if request.path.startswith("/api/"):
        return jsonify({"ok": False, "error": "接口不存在"}), 404
    return render_template("error.html", code=404,
                           message="页面不存在或链接已失效"), 404


@app.errorhandler(500)
def _error_500(e):
    if request.path.startswith("/api/"):
        return jsonify({"ok": False, "error": "服务器内部错误"}), 500
    return render_template("error.html", code=500,
                           message="服务器开小差了，请稍后重试"), 500


if __name__ == "__main__":
    import socket
    from pathlib import Path

    if getattr(sys, 'frozen', False):
        base_dir = Path(os.path.dirname(os.path.abspath(sys.executable)))
    else:
        base_dir = Path(__file__).parent
    port_config = base_dir / "data" / "port.txt"

    port = int(os.environ.get("SALARY_PORT", 0))
    if port == 0:
        if port_config.exists():
            try:
                with open(port_config, "r", encoding="utf-8") as f:
                    port = int(f.read().strip())
            except Exception:
                port = 5000
        else:
            port = 5000
    
    host = "0.0.0.0"
    # 使用 UDP socket 技巧获取本机 LAN IP（比 gethostbyname 准确）
    local_ip = "127.0.0.1"
    try:
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.connect(("8.8.8.8", 80))
        local_ip = s.getsockname()[0]
        s.close()
    except OSError:
        pass
    print("=" * 50)
    print("  员工工资查询系统 已启动")
    print("=" * 50)
    print(f"  本机访问: http://localhost:{port}")
    print(f"  局域网访问: http://{local_ip}:{port}")
    print(f"  监听地址: 0.0.0.0:{port}")
    print("=" * 50)
    app.run(host=host, port=port, debug=False)
