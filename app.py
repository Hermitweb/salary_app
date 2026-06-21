import os
import sys
import json
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


# === 访问审计日志（记录谁在什么时间查了什么）===
import logging as _logging
_access_logger = _logging.getLogger("access_audit")
_access_logger.setLevel(_logging.INFO)
_access_logger.propagate = False  # 不传到根 logger，避免重复
try:
    _access_handler = _logging.FileHandler(
        str(_DATA_DIR / "access.log"), encoding="utf-8"
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
    ip = request.headers.get("X-Forwarded-For", request.remote_addr or "-")
    emp_id = employee.get("id", "-") if employee else "-"
    emp_name = employee.get("name", "-") if employee else "-"
    _access_logger.info(f"{ip}|{action}|{emp_id}|{emp_name}|{detail}")


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

        result = excel_data.authenticate(username, password)

        if result["ok"]:
            session["employee"] = result["employee"]
            _log_access("LOGIN_OK", employee=result["employee"])
            return redirect(url_for("salary_view"))
        else:
            _log_access("LOGIN_FAIL", detail=f"账号={username} 原因={result['error']}")
            return render_template("login.html", error=result["error"])

    return render_template("login.html")


@app.route("/logout")
def logout():
    employee = session.get("employee")
    _log_access("LOGOUT", employee=employee)
    session.clear()
    return redirect(url_for("login"))


@app.route("/salary", methods=["GET", "POST"])
def salary_view():
    employee = session.get("employee")
    if not employee:
        return redirect(url_for("login"))

    available_months = excel_data.get_available_months()

    year = request.args.get("year") or request.form.get("year")
    month = request.args.get("month") or request.form.get("month")

    # 如果没有找到任何工资表，尝试用文件 mtime 找最新文件
    if not available_months:
        from pathlib import Path
        import datetime
        import openpyxl
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
            latest_path = candidates[0][1]
            # 读取第一行的工号列，尝试推断年月
            try:
                wb = openpyxl.load_workbook(latest_path, data_only=True, read_only=True)
                try:
                    ws = wb.active
                    dt = datetime.datetime.fromtimestamp(candidates[0][0])
                    year_guess, month_guess = dt.year, dt.month
                finally:
                    try:
                        wb.close()
                    except Exception:
                        pass
                available_months = [{"year": year_guess, "month": month_guess}]
                year = str(year_guess)
                month = str(month_guess)
            except Exception:
                pass

    if not available_months:
        return render_template("salary.html", employee=employee, error="没有可用的工资数据",
                               available_months=[], available_years=[],
                               months_for_year=[], salary=None,
                               selected_year="", selected_month="")

    # 解析可用的年份（去重，倒序）和月份
    available_years = sorted({m["year"] for m in available_months}, reverse=True)

    # 如果没选年份或年份不存在，默认选最新年份
    if not year or int(year) not in available_years:
        year = str(available_years[0])
    # 如果月份不在该年可用列表中，默认选该年最新月份
    months_for_year = sorted(
        [m["month"] for m in available_months if m["year"] == int(year)],
        reverse=True,
    )
    if not month or int(month) not in months_for_year:
        month = str(months_for_year[0]) if months_for_year else ""

    salary = None
    payslip = None
    error_msg = None
    result = excel_data.get_salary(employee["employee_id"], int(year), int(month))
    if result["ok"]:
        salary = {
            "headers": result["headers"],
            "values": result["values"],
            "data": result["data"],
        }
        payslip = excel_data.build_payslip_table(
            result["data"], employee, int(year), int(month)
        )
        _log_access("VIEW", employee=employee, detail=f"{year}-{int(month):02d}")
    else:
        error_msg = result["error"]
        _log_access("VIEW_FAIL", employee=employee,
                    detail=f"{year}-{int(month):02d} 原因={error_msg}")

    return render_template(
        "salary.html",
        employee=employee,
        available_months=available_months,
        available_years=available_years,
        months_for_year=months_for_year,
        salary=salary,
        payslip=payslip,
        selected_year=year,
        selected_month=month,
        error=error_msg,
    )


@app.route("/api/available_months")
def api_available_months():
    return jsonify(excel_data.get_available_months())


@app.route("/api/salary")
def api_salary():
    employee = session.get("employee")
    if not employee:
        return jsonify({"ok": False, "error": "未登录"}), 401

    year = request.args.get("year")
    month = request.args.get("month")
    if not year or not month:
        return jsonify({"ok": False, "error": "缺少年月参数"})

    result = excel_data.get_salary(employee["employee_id"], int(year), int(month))
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
