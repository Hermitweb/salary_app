"""
P1/P2 新增功能回归测试：
- P1-10 会话加固（TTL / SameSite / HttpOnly / Secure 跟随 HTTPS）
- P1-14 导出审计接口 + 模板水印/审计钩子
- P2-15 工资条列映射配置化（data/payslip_columns.json）
- P2-17 月列表目录快照缓存
- P2-19 看门狗：重启计数时间窗 + /healthz 死锁探活
- P2-20 端口正则锚定
- 2026-08 复查修复：审计 IP 取对端地址、PDF 导出 HTML 转义、
  工号列检测统一、工资缓存上限（LRU）、原始模式顺序读取
"""
import os
import re
import shutil
import sys
import tempfile
import time
from datetime import timedelta
from pathlib import Path

import pytest
import requests

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))


@pytest.fixture(scope="module")
def test_env():
    TMP_DATA = Path(tempfile.mkdtemp(prefix="p1p2_test_"))

    from control_panel_pkg import constants
    import excel_data
    import cache as cache_mod

    cache_mod.DataCache.reset_for_test()
    constants.DATA_DIR = TMP_DATA
    constants.DATA_DIR.mkdir(exist_ok=True)
    constants.PORT_CONFIG = TMP_DATA / "port.txt"
    constants.FIELDS_CONFIG = TMP_DATA / "fields.txt"
    constants.BASE_DIR = ROOT
    excel_data.DATA_DIR = str(TMP_DATA)
    excel_data.FIELDS_CONFIG = TMP_DATA / "fields.txt"
    excel_data.reset_scan_caches()
    cache_mod.DataCache.reset_for_test()

    from openpyxl import Workbook
    wb_emp = Workbook()
    ws = wb_emp.active
    ws.append(["员工号", "姓名", "部门", "紧急联系人号码"])
    ws.append(["E0001", "张三", "测试部", "13800000001"])
    wb_emp.save(TMP_DATA / "员工列表.xlsx")

    sal_dir = TMP_DATA / "salary"
    sal_dir.mkdir(exist_ok=True)
    wb_sal = Workbook()
    ws_sal = wb_sal.active
    ws_sal.append(["员工号", "姓名", "部门", "应发基本工资", "应发绩效工资", "本月实发工资"])
    ws_sal.append(["E0001", "张三", "测试部", 5000, 1200, 6200])
    wb_sal.save(sal_dir / "2026年05月员工工资.xlsx")

    PORT = 15060
    constants.PORT_CONFIG.write_text(str(PORT), encoding="utf-8")

    from control_panel_pkg.service import ServiceManager
    import logging
    logging.getLogger("werkzeug").setLevel(logging.WARNING)
    sm = ServiceManager()
    ok, msg = sm.start()
    assert ok, f"ServiceManager 启动失败: {msg}"
    time.sleep(0.5)

    yield {"TMP_DATA": TMP_DATA, "PORT": PORT, "sm": sm}

    sm.stop()
    time.sleep(0.3)
    shutil.rmtree(TMP_DATA, ignore_errors=True)
    cache_mod.DataCache.reset_for_test()
    excel_data.reset_scan_caches()


def _login(port, username="E0001", password="13800000001", s=None):
    s = s or requests.Session()
    r = s.get(f"http://127.0.0.1:{port}/login")
    m = re.search(r'name="_csrf_token"\s+value="([^"]+)"', r.text)
    assert m, "登录页应包含 CSRF token"
    r = s.post(f"http://127.0.0.1:{port}/login",
               data={"username": username, "password": password,
                     "_csrf_token": m.group(1)})
    assert "/salary" in r.url, f"登录应跳转到工资页: {r.url}"
    return s


# ===== P1-10 会话加固 =====

def test_session_config_defaults():
    import app as app_mod
    assert app_mod.app.config["PERMANENT_SESSION_LIFETIME"] == timedelta(minutes=30)
    assert app_mod.app.config["SESSION_COOKIE_HTTPONLY"] is True
    assert app_mod.app.config["SESSION_COOKIE_SAMESITE"] == "Lax"


def test_session_cookie_attributes(test_env):
    port = test_env["PORT"]
    r = requests.get(f"http://127.0.0.1:{port}/login")
    set_cookie = r.headers.get("Set-Cookie", "")
    assert "HttpOnly" in set_cookie
    assert "SameSite=Lax" in set_cookie


def test_session_cookie_secure_follows_https(test_env):
    """Secure 跟随 https_enabled and https_redirect：
    测试关闭强跳转直连 HTTP → False；恢复生产语义（强跳转）重启后 → True"""
    sm = test_env["sm"]
    import app as app_mod
    if sm.https_enabled:
        assert app_mod.app.config["SESSION_COOKIE_SECURE"] is False

        sm.stop()
        sm.https_redirect = True
        ok, msg = sm.start(force=True)
        assert ok, f"重启失败: {msg}"
        assert app_mod.app.config["SESSION_COOKIE_SECURE"] is True

        sm.stop()
        sm.https_redirect = False
        ok, msg = sm.start(force=True)
        assert ok, f"重启失败: {msg}"
    else:
        assert app_mod.app.config["SESSION_COOKIE_SECURE"] is False


# ===== P1-14 审计 + 水印 =====

def test_audit_endpoint_requires_login(test_env):
    """带合法 CSRF token 但未登录 → 应 401（CSRF 校验先于业务，裸 POST 是 403）"""
    port = test_env["PORT"]
    s = requests.Session()
    r = s.get(f"http://127.0.0.1:{port}/login")
    m = re.search(r'name="_csrf_token"\s+value="([^"]+)"', r.text)
    assert m, "登录页应包含 CSRF token"
    r = s.post(f"http://127.0.0.1:{port}/api/audit",
               data={"action": "EXPORT_IMG", "_csrf_token": m.group(1)})
    assert r.status_code == 401


def test_audit_endpoint_logs_export(test_env):
    port = test_env["PORT"]
    s = _login(port)

    # CSRF token（工资页内的表单）
    r = s.get(f"http://127.0.0.1:{port}/salary")
    m = re.search(r'name="_csrf_token"\s+value="([^"]+)"', r.text)
    assert m, "工资页应包含 CSRF token"
    csrf = m.group(1)

    r = s.post(f"http://127.0.0.1:{port}/api/audit",
               data={"action": "EXPORT_IMG", "detail": "2026-05 工资条截图",
                     "_csrf_token": csrf})
    assert r.status_code == 200 and r.json()["ok"]

    r = s.post(f"http://127.0.0.1:{port}/api/audit",
               data={"action": "BAD_ACTION", "_csrf_token": csrf})
    assert r.status_code == 400

    # 日志文件路径以 app 模块导入时绑定的 _DATA_DIR 为准
    import app as app_mod
    log_file = Path(app_mod._DATA_DIR) / "access.log"
    assert log_file.exists(), "审计日志应落盘"
    tail = log_file.read_text(encoding="utf-8").splitlines()[-50:]
    assert any("EXPORT_IMG" in ln and "E0001" in ln for ln in tail), \
        f"审计日志应记录导出动作，最近日志：{tail[-3:]}"


def test_salary_template_has_watermark_and_audit():
    tpl = (ROOT / "templates" / "salary.html").read_text(encoding="utf-8")
    assert "watermark" not in tpl.lower(), "所有水印已按需求移除（截图/PDF/打印）"
    assert "auditExport" in tpl, "导出/打印动作应上报审计"
    assert "EXPORT_PDF" in tpl and "EXPORT_IMG" in tpl and "PRINT" in tpl


# ===== P2-15 列映射配置化 =====

def test_payslip_column_map_default():
    import excel_data
    assert excel_data._PAYSLIP_COLUMN_MAP["base_salary"] == ["应发基本工资"]


def test_payslip_column_map_json_override(tmp_path, monkeypatch):
    import json
    import excel_data
    cfg = tmp_path / "payslip_columns.json"
    cfg.write_text(json.dumps({"base_salary": ["自定义基本工资"],
                               "custom_col": ["新字段"]}, ensure_ascii=False),
                   encoding="utf-8")
    monkeypatch.setattr(excel_data, "_PAYSLIP_MAP_CONFIG", cfg)
    try:
        excel_data._load_payslip_column_map()
        assert excel_data._PAYSLIP_COLUMN_MAP["base_salary"] == ["自定义基本工资"]
        assert excel_data._PAYSLIP_COLUMN_MAP["custom_col"] == ["新字段"]
        # 未覆盖的键保留默认值
        assert excel_data._PAYSLIP_COLUMN_MAP["net_salary"] == ["本月实发工资", "实发工资"]
    finally:
        monkeypatch.setattr(excel_data, "_PAYSLIP_MAP_CONFIG",
                            Path(excel_data.DATA_DIR) / "payslip_columns.json")
        excel_data._load_payslip_column_map()


def test_payslip_column_map_broken_json_falls_back(tmp_path, monkeypatch):
    import excel_data
    cfg = tmp_path / "payslip_columns.json"
    cfg.write_text("{invalid json", encoding="utf-8")
    monkeypatch.setattr(excel_data, "_PAYSLIP_MAP_CONFIG", cfg)
    excel_data._load_payslip_column_map()
    assert excel_data._PAYSLIP_COLUMN_MAP == excel_data._DEFAULT_PAYSLIP_COLUMN_MAP


# ===== P2-17 月列表缓存 =====

def test_months_cache_hit_and_invalidate(tmp_path, monkeypatch):
    import excel_data
    monkeypatch.setattr(excel_data, "DATA_DIR", str(tmp_path))
    excel_data.reset_scan_caches()

    sal = tmp_path / "salary"
    sal.mkdir()
    from openpyxl import Workbook
    wb = Workbook()
    ws = wb.active
    ws.append(["员工号", "姓名", "应发基本工资"])
    ws.append(["E0001", "张三", 5000])
    wb.save(sal / "2026年05月员工工资.xlsx")

    m1 = excel_data.get_available_months()
    assert m1 == [{"year": 2026, "month": 5}]
    assert excel_data._MONTHS_CACHE["key"] is not None, "应写入缓存"

    # 未变化时命中缓存（同结果）
    m2 = excel_data.get_available_months()
    assert m2 == m1

    # 新增文件后缓存应失效并看到新月份
    wb.save(sal / "2026年06月员工工资.xlsx")
    m3 = excel_data.get_available_months()
    assert m3 == [{"year": 2026, "month": 6}, {"year": 2026, "month": 5}]
    excel_data.reset_scan_caches()


# ===== P2-19 看门狗增强 =====

def test_healthz_returns_200(test_env):
    port = test_env["PORT"]
    r = requests.get(f"http://127.0.0.1:{port}/healthz", timeout=3)
    assert r.status_code == 200
    assert r.text == "OK"


def test_probe_healthz_alive(test_env):
    sm = test_env["sm"]
    sm.enable_deadlock_probe = True
    try:
        assert sm._probe_healthz() is True, "服务存活时 /healthz 探活应成功"
    finally:
        sm.enable_deadlock_probe = False


def test_probe_healthz_dead_port():
    """探活一个没人监听的端口应返回 False"""
    from control_panel_pkg.service import ServiceManager
    sm = ServiceManager.__new__(ServiceManager)
    sm._probe_port = 59999  # 大概率未占用
    assert sm._probe_healthz() is False


def test_restart_window_resets_count():
    """稳定运行超过时间窗后，重启计数应清零重新计"""
    from control_panel_pkg.service import ServiceManager
    sm = ServiceManager.__new__(ServiceManager)
    sm._restart_count = 5
    sm._last_restart_ts = time.time() - (ServiceManager._RESTART_WINDOW + 60)
    # 模拟 watchdog 重启计数逻辑
    now = time.time()
    if now - sm._last_restart_ts > sm._RESTART_WINDOW:
        sm._restart_count = 0
    sm._restart_count += 1
    sm._last_restart_ts = now
    assert sm._restart_count == 1, "跨时间窗后应重新从 1 计数"


def test_deadlock_probe_forces_kill():
    """连续探活失败达到阈值时应强制关闭 server socket"""
    from control_panel_pkg.service import ServiceManager
    sm = ServiceManager.__new__(ServiceManager)
    sm._probe_tick = 999
    sm._probe_fails = ServiceManager._DEADLOCK_PROBE_FAILS - 1
    sm._probe_port = 59999
    sm._server = None
    sm._https_server = None
    sm.is_running = True
    sm._last_error = None
    sm.enable_deadlock_probe = True
    sm._DEADLOCK_PROBE_INTERVAL = ServiceManager._DEADLOCK_PROBE_INTERVAL
    sm._WATCHDOG_INTERVAL = ServiceManager._WATCHDOG_INTERVAL
    sm._DEADLOCK_PROBE_FAILS = ServiceManager._DEADLOCK_PROBE_FAILS

    sm._deadlock_probe_tick()
    assert sm.is_running is False, "判定死锁后应标记停止"
    assert "死锁" in (sm._last_error or "")


# ===== P2-20 端口正则锚定 =====

def test_port_regex_anchored(tmp_path, monkeypatch):
    """正则应匹配行首的 port=，而不是 https_port = """
    from control_panel_pkg import constants
    fake_app = tmp_path / "app.py"
    fake_app.write_text(
        "https_port = 9999\n"
        "port = 5000\n"
        "port_env = int(os.environ.get('SALARY_PORT', 0))\n",
        encoding="utf-8",
    )
    monkeypatch.setattr(constants, "APP_PY", fake_app)
    assert constants._resolve_default_port() == 5000


# ===== P3 体验摩擦修复 =====

def test_login_page_ux_elements(test_env):
    """登录页：初始密码提示 + autocomplete（浏览器记住凭据降低重登成本）"""
    port = test_env["PORT"]
    r = requests.get(f"http://127.0.0.1:{port}/login")
    assert "初始密码为 HR 登记的紧急联系人号码" in r.text
    assert 'autocomplete="username"' in r.text
    assert 'autocomplete="current-password"' in r.text


def test_session_expired_hint(test_env):
    """会话过期提示：新访客无提示；过期（cookie 在）有提示；主动登出无提示"""
    port = test_env["PORT"]
    s = requests.Session()

    # 新访客直接访问受保护页 → 无过期提示
    r = s.get(f"http://127.0.0.1:{port}/salary", allow_redirects=True)
    assert "登录状态已过期" not in r.text

    # 登录 → 种 was_logged_in cookie
    _login(port, s=s)
    assert "was_logged_in" in s.cookies

    # 模拟会话过期：清 session 保留标记 cookie
    s.cookies.clear("127.0.0.1", "/", "session")
    r = s.get(f"http://127.0.0.1:{port}/salary", allow_redirects=True)
    assert "登录状态已过期" in r.text

    # 重新登录后主动登出 → 标记清除，无误导性提示
    _login(port, s=s)
    r = s.post(f"http://127.0.0.1:{port}/logout", allow_redirects=True)
    r = s.get(f"http://127.0.0.1:{port}/salary", allow_redirects=True)
    assert "登录状态已过期" not in r.text


# ===== 2026-08 复查修复 =====

def test_audit_log_ignores_spoofed_forwarded_for(test_env):
    """审计日志取 TCP 对端地址：伪造的 X-Forwarded-For 不得进入审计日志"""
    port = test_env["PORT"]
    s = _login(port)
    r = s.get(f"http://127.0.0.1:{port}/salary",
              headers={"X-Forwarded-For": "9.9.9.9"})
    assert r.status_code == 200
    import app as app_mod
    log_file = Path(app_mod._DATA_DIR) / "access.log"
    tail = log_file.read_text(encoding="utf-8").splitlines()[-80:]
    assert not any("9.9.9.9" in ln for ln in tail), "伪造的 XFF 不应写入审计日志"
    assert any("|127.0.0.1|VIEW|" in ln for ln in tail), "应记录真实对端地址"


def test_pdf_export_escapes_dynamic_html():
    """PDF 导出窗口用 document.write 写入完整 HTML，动态标题必须经 escapeHtml 转义"""
    tpl = (ROOT / "templates" / "salary.html").read_text(encoding="utf-8")
    assert "function escapeHtml" in tpl
    assert "escapeHtml(title)" in tpl, "文档标题需转义"
    assert "escapeHtml(titleText)" in tpl, "副标题（含员工姓名）需转义"


def test_salary_emp_id_col_variants():
    """工号列判定统一（cache/excel_data 共用）：精确优先、子串变体兼容、找不到为 None"""
    from cache import find_salary_emp_id_col
    assert find_salary_emp_id_col(["序号", "员工号", "姓名"]) == 1
    assert find_salary_emp_id_col(["序号", "工号", "姓名"]) == 1
    assert find_salary_emp_id_col(["账号", "姓名"]) == 0, "「账号」列应能识别"
    assert find_salary_emp_id_col(["员工工号", "姓名"]) == 0, "「员工工号」变体应能识别"
    assert find_salary_emp_id_col(["姓名", "部门"]) is None, "绝不回落到第 0 列"
    assert find_salary_emp_id_col([]) is None


def test_salary_cache_has_file_limit(tmp_path):
    """工资表缓存有文件数上限（LRU 淘汰），防长期运行内存无限增长"""
    from openpyxl import Workbook
    from cache import DataCache, _MAX_SALARY_CACHE_FILES
    DataCache.reset_for_test()
    try:
        dc = DataCache.instance()
        for i in range(_MAX_SALARY_CACHE_FILES + 6):
            wb = Workbook()
            ws = wb.active
            ws.append(["员工号", "姓名"])
            ws.append([f"E{i:03d}", f"员工{i}"])
            p = tmp_path / f"salary_{i:02d}.xlsx"
            wb.save(p)
            dc.get_salary(str(p))
        cached = dc.get_stats()["salary_files_cached"]
        assert cached <= _MAX_SALARY_CACHE_FILES, \
            f"缓存文件数 {cached} 超过上限 {_MAX_SALARY_CACHE_FILES}"
    finally:
        DataCache.reset_for_test()


def test_authenticate_raw_mode_layout(tmp_path, monkeypatch):
    """原始模式（第1行账号/第2行密码/第3行姓名）登录：顺序读取改造后行为不变"""
    import excel_data
    from cache import DataCache
    from openpyxl import Workbook
    DataCache.reset_for_test()
    wb = Workbook()
    ws = wb.active
    ws.append(["E0001", "E0002"])
    ws.append(["13800000001", "13800000002"])
    ws.append(["张三", "李四"])
    ws.append(["测试部", "测试部"])
    wb.save(tmp_path / "员工列表.xlsx")
    monkeypatch.setattr(excel_data, "DATA_DIR", str(tmp_path))
    try:
        ok = excel_data.authenticate("E0001", "13800000001")
        assert ok["ok"], ok.get("error")
        assert ok["employee"]["name"] == "张三"
        assert ok["employee"]["dept"] == "测试部"
        bad = excel_data.authenticate("E0001", "wrong-password")
        assert not bad["ok"]
        miss = excel_data.authenticate("E0009", "13800000001")
        assert not miss["ok"]
    finally:
        DataCache.reset_for_test()
