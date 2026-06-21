"""
多人并发访问测试
模拟 N 个用户同时登录、查询、登出，验证：
1. Flask 不会因并发而崩溃
2. Excel 文件读取无冲突
3. session 隔离正常
4. 不会因为文件锁导致阻塞
"""
import os
import re
import sys
import time
import shutil
import tempfile
import threading
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor, as_completed

import pytest
import requests

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))


@pytest.fixture(scope="module")
def test_env():
    """模块级 fixture：启动服务并准备数据"""
    TMP_DATA = Path(tempfile.mkdtemp(prefix="multiuser_test_"))
    print(f"[setup] 临时数据目录: {TMP_DATA}")

    from control_panel_pkg import constants
    import excel_data
    import cache as cache_mod

    # 关键：先重置 DataCache 单例，再设置 DATA_DIR
    cache_mod.DataCache.reset_for_test()
    constants.DATA_DIR = TMP_DATA
    constants.DATA_DIR.mkdir(exist_ok=True)
    constants.PORT_CONFIG = TMP_DATA / "port.txt"
    constants.FIELDS_CONFIG = TMP_DATA / "fields.txt"
    constants.BASE_DIR = ROOT
    excel_data.DATA_DIR = str(TMP_DATA)
    excel_data.FIELDS_CONFIG = TMP_DATA / "fields.txt"
    cache_mod.DataCache.reset_for_test()

    # 准备测试数据：30 个员工
    from openpyxl import Workbook
    wb_emp = Workbook()
    ws_emp = wb_emp.active
    ws_emp.append(["员工号", "姓名", "部门", "手机号", "离职"])
    for i in range(1, 31):
        ws_emp.append([f"E{i:04d}", f"员工{i:02d}", "测试部", f"138{i:08d}", "否"])
    wb_emp.save(TMP_DATA / "员工列表.xlsx")

    # 工资表
    salary_dir = TMP_DATA / "salary"
    salary_dir.mkdir(exist_ok=True)
    wb_sal = Workbook()
    ws_sal = wb_sal.active
    headers = ["员工号", "姓名", "部门", "应发基本工资", "应发绩效工资",
               "应发岗位工资", "本月社会保险", "本月住房公积金",
               "本月个人所得税", "本月实发工资"]
    ws_sal.append(headers)
    for i in range(1, 31):
        ws_sal.append([f"E{i:04d}", f"员工{i:02d}", "测试部",
                       5000 + i*100, 1000 + i*50, 800 + i*20,
                       500, 300, 100, 5000 + i*100 + 1000 + i*50 + 800 + i*20 - 900])
    wb_sal.save(salary_dir / "2026年05月员工工资.xlsx")

    PORT = 15010
    constants.PORT_CONFIG.write_text(str(PORT), encoding="utf-8")

    from control_panel_pkg.service import ServiceManager
    import logging
    logging.getLogger("werkzeug").setLevel(logging.WARNING)
    sm = ServiceManager()
    ok, msg = sm.start()
    assert ok, f"ServiceManager 启动失败: {msg}"
    print(f"[start] {msg}")
    time.sleep(0.5)

    yield {"TMP_DATA": TMP_DATA, "PORT": PORT, "sm": sm}

    sm.stop()
    time.sleep(0.3)
    shutil.rmtree(TMP_DATA, ignore_errors=True)
    cache_mod.DataCache.reset_for_test()


def _login(username, password, port):
    s = requests.Session()
    r = s.get(f"http://127.0.0.1:{port}/login")
    m = re.search(r'name="_csrf_token"\s+value="([^"]+)"', r.text)
    if not m:
        return None, f"找不到 CSRF token"
    csrf = m.group(1)
    r = s.post(f"http://127.0.0.1:{port}/login",
               data={"username": username, "password": password, "_csrf_token": csrf})
    if "/salary" in r.url or "salary" in r.text:
        return s, "OK"
    return None, f"登录失败: {r.text[:200]}"


def _query_salary(session, port, year=2026, month=5):
    r = session.get(f"http://127.0.0.1:{port}/salary?year={year}&month={month}")
    return r.status_code, len(r.content), r.elapsed.total_seconds()


def _logout(session, port):
    session.get(f"http://127.0.0.1:{port}/logout")


def _user_scenario(user_id, port, do_logout=True):
    username = f"E{user_id:04d}"
    password = f"138{user_id:08d}"
    try:
        s, msg = _login(username, password, port)
        if s is None:
            return {"user": username, "ok": False, "error": msg}

        results = []
        for _ in range(3):
            sc, size, dt = _query_salary(s, port)
            results.append((sc, size, dt))

        r = s.get(f"http://127.0.0.1:{port}/api/salary?year=2026&month=5")
        api_ok = r.status_code == 200 and r.json().get("ok")

        if do_logout:
            _logout(s, port)

        all_ok = all(sc == 200 for sc, _, _ in results) and api_ok
        avg_time = sum(dt for _, _, dt in results) / len(results)
        max_size = max(size for _, size, _ in results)

        return {
            "user": username, "ok": all_ok,
            "queries": len(results),
            "avg_time": avg_time,
            "max_size": max_size,
            "api_ok": api_ok,
        }
    except Exception as e:
        return {"user": username, "ok": False, "error": str(e)}


# ============== 测试 1: 10 用户同时访问 ==============
def test_concurrent_10(test_env):
    port = test_env["PORT"]
    print("\n" + "="*60)
    print("T1: 10 个用户并发登录 + 查询")
    print("="*60)
    n_users = 10
    start = time.time()
    with ThreadPoolExecutor(max_workers=n_users) as pool:
        futures = [pool.submit(_user_scenario, i, port) for i in range(1, n_users + 1)]
        results = [f.result() for f in as_completed(futures)]
    elapsed = time.time() - start

    ok_count = sum(1 for r in results if r["ok"])
    avg_times = [r.get("avg_time", 0) for r in results if r.get("ok")]
    if avg_times:
        avg_str = f"{sum(avg_times)/len(avg_times)*1000:.1f}ms"
    else:
        avg_str = "N/A"
    print(f"success: {ok_count}/{n_users}, total time: {elapsed:.2f}s, "
          f"avg query time: {avg_str}")
    assert ok_count == n_users


# ============== 测试 2: 30 用户同时访问 ==============
def test_concurrent_30(test_env):
    port = test_env["PORT"]
    print("\n" + "="*60)
    print("T2: 30 个用户并发登录 + 查询（压力测试）")
    print("="*60)
    n_users = 30
    start = time.time()
    with ThreadPoolExecutor(max_workers=n_users) as pool:
        futures = [pool.submit(_user_scenario, i, port) for i in range(1, n_users + 1)]
        results = [f.result() for f in as_completed(futures)]
    elapsed = time.time() - start

    ok_count = sum(1 for r in results if r["ok"])
    print(f"success: {ok_count}/{n_users}, total time: {elapsed:.2f}s")
    assert ok_count == n_users


# ============== 测试 3: 同一用户多 session 隔离 ==============
def test_same_user_multiple_sessions(test_env):
    port = test_env["PORT"]
    print("\n" + "="*60)
    print("T3: 同一用户 5 个并发 session 隔离")
    print("="*60)
    username = "E0001"
    password = "13800000001"

    def one_session():
        s, msg = _login(username, password, port)
        if s is None:
            return False
        sc, _, _ = _query_salary(s, port)
        return sc == 200

    with ThreadPoolExecutor(max_workers=5) as pool:
        results = [f.result() for f in [pool.submit(one_session) for _ in range(5)]]
    ok = sum(1 for r in results if r)
    print(f"success: {ok}/5")
    assert ok == 5


# ============== 测试 4: 并发登录失败（错误密码） ==============
def test_concurrent_login_failures(test_env):
    port = test_env["PORT"]
    print("\n" + "="*60)
    print("T4: 并发错误登录（防止文件锁死）")
    print("="*60)
    def bad_login():
        s = requests.Session()
        r = s.get(f"http://127.0.0.1:{port}/login")
        m = re.search(r'name="_csrf_token"\s+value="([^"]+)"', r.text)
        if not m:
            return False
        r = s.post(f"http://127.0.0.1:{port}/login",
                   data={"username": "E0001", "password": "wrong", "_csrf_token": m.group(1)})
        return r.status_code == 200

    with ThreadPoolExecutor(max_workers=20) as pool:
        results = [f.result() for f in [pool.submit(bad_login) for _ in range(20)]]
    ok = sum(1 for r in results if r)
    print(f"success: {ok}/20")
    assert ok == 20


# ============== 测试 5: 持续高负载 ==============
def test_sustained_load(test_env):
    port = test_env["PORT"]
    print("\n" + "="*60)
    print("T5: 持续高负载 5 秒（模拟办公时间）")
    print("="*60)
    import random
    stop_flag = threading.Event()
    errors = []
    total_requests = [0]
    success_requests = [0]

    def worker():
        while not stop_flag.is_set():
            try:
                uid = random.randint(1, 30)
                username = f"E{uid:04d}"
                password = f"138{uid:08d}"
                s, _ = _login(username, password, port)
                if s:
                    _query_salary(s, port)
                    success_requests[0] += 1
                total_requests[0] += 1
            except Exception as e:
                errors.append(str(e))

    threads = [threading.Thread(target=worker, daemon=True) for _ in range(15)]
    start = time.time()
    for t in threads:
        t.start()
    time.sleep(5)
    stop_flag.set()
    for t in threads:
        t.join(timeout=2)
    elapsed = time.time() - start

    rps = total_requests[0] / elapsed
    print(f"total: {total_requests[0]}, success: {success_requests[0]}, "
          f"time: {elapsed:.1f}s, RPS: {rps:.1f}")
    assert len(errors) == 0 and success_requests[0] > 0
