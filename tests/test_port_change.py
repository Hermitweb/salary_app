"""
测试：修改端口后启动服务是否正常
复现：控制面板更换端口保存之后点击启动服务直接闪退
"""
import os
import sys
import time
import shutil
import tempfile
import threading
from pathlib import Path

import pytest
import requests

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))


@pytest.fixture(scope="module")
def port_change_env():
    """独立的测试环境"""
    TMP_DATA = Path(tempfile.mkdtemp(prefix="port_change_"))

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
    cache_mod.DataCache.reset_for_test()

    # 员工
    from openpyxl import Workbook
    wb = Workbook()
    ws = wb.active
    ws.append(["员工号", "姓名", "部门", "紧急联系人号码"])
    ws.append(["E0001", "张三", "测试部", "13800000001"])
    wb.save(TMP_DATA / "员工列表.xlsx")

    # 工资表
    salary_dir = TMP_DATA / "salary"
    salary_dir.mkdir(exist_ok=True)
    wb_sal = Workbook()
    ws_sal = wb_sal.active
    ws_sal.append(["员工号", "姓名", "部门", "应发基本工资"])
    ws_sal.append(["E0001", "张三", "测试部", 5000])
    wb_sal.save(salary_dir / "2026年05月员工工资.xlsx")

    from control_panel_pkg.service import ServiceManager
    import logging
    logging.getLogger("werkzeug").setLevel(logging.WARNING)

    sm = ServiceManager()
    yield {"TMP_DATA": TMP_DATA, "sm": sm}

    try:
        sm.stop()
    except Exception:
        pass
    time.sleep(0.3)
    shutil.rmtree(TMP_DATA, ignore_errors=True)
    cache_mod.DataCache.reset_for_test()


def test_change_port_then_start(port_change_env):
    """场景：默认端口启动 → 停止 → 改端口 → 再启动"""
    sm = port_change_env["sm"]

    # 第一次：默认 5001
    ok, msg = sm.start(force=True)
    assert ok, f"首次启动失败: {msg}"
    print(f"[T1] 首次启动: {msg} (port={sm.port}, https_port={sm.https_port})")

    # 验证 HTTP 端口
    r = requests.get(f"http://127.0.0.1:{sm.port}/login", timeout=3)
    assert r.status_code == 200
    print(f"[T1] HTTP {sm.port} 可访问")

    # 停止
    ok, msg = sm.stop()
    assert ok, f"停止失败: {msg}"
    time.sleep(0.5)

    # 改端口为 5099
    print("\n[T2] 改端口为 5099")
    ok, msg = sm.set_port(5099)
    assert ok, f"改端口失败: {msg}"
    print(f"[T2] set_port(5099): {msg}")

    # 再次启动
    print("\n[T3] 改端口后启动")
    try:
        ok, msg = sm.start(force=True)
    except Exception as e:
        pytest.fail(f"start 异常: {e}")
    assert ok, f"改端口后启动失败: {msg}"
    print(f"[T3] start: {msg} (port={sm.port}, https_port={sm.https_port})")
    assert sm.port == 5099, f"端口不对: {sm.port}"
    assert sm.is_running, "is_running 应为 True"

    # 验证 HTTP 端口
    r = requests.get(f"http://127.0.0.1:{sm.port}/login", timeout=3)
    assert r.status_code == 200
    print(f"[T3] HTTP {sm.port} 可访问")


def test_change_port_directly(port_change_env):
    """场景：不通过 set_port，直接修改 self.port 后启动"""
    sm = port_change_env["sm"]

    # 第一次启动
    ok, msg = sm.start(force=True)
    if not ok:
        # 可能上一个测试残留，先 stop
        sm.stop()
        time.sleep(0.5)
        ok, msg = sm.start(force=True)
    assert ok
    sm.stop()
    time.sleep(0.5)

    # 模拟 UI 行为：直接修改属性
    print("\n[T4] 模拟 UI: 直接修改属性 + _save_port")
    sm.port = 5199
    sm.https_port = 5200
    sm._save_port(5199)
    print(f"[T4] 修改后: port={sm.port}, https_port={sm.https_port}")

    # 启动
    try:
        ok, msg = sm.start(force=True)
    except Exception as e:
        pytest.fail(f"start 异常: {e}")
    assert ok, f"启动失败: {msg}"
    print(f"[T4] start: {msg}")
    assert sm.port == 5199
    assert sm.is_running

    r = requests.get(f"http://127.0.0.1:{sm.port}/login", timeout=3)
    assert r.status_code == 200
    print(f"[T4] HTTP {sm.port} 可访问")

    sm.stop()
    time.sleep(0.3)


def test_https_port_overflow(port_change_env):
    """场景：端口设为 65535（HTTPS 端口会超 65535）"""
    sm = port_change_env["sm"]

    # 先停止
    try:
        sm.stop()
    except Exception:
        pass
    time.sleep(0.3)

    # 65535 是 OK 的（HTTPS=65536 超界会被禁用）
    # 65534：HTTPS=65535 是 OK 的
    print("\n[T5] 测试端口 65534")
    ok, msg = sm.set_port(65534)
    assert ok
    sm.port = 65534
    sm.https_port = 65535

    try:
        ok, msg = sm.start(force=True)
    except Exception as e:
        pytest.fail(f"start 异常: {e}")
    assert ok
    print(f"[T5] start: {msg}")
    sm.stop()
    time.sleep(0.3)


def test_ui_save_port_then_start(port_change_env):
    """精确模拟 UI 流程：保存端口 → 启动服务"""
    sm = port_change_env["sm"]

    # 先确保停止
    try:
        sm.stop()
    except Exception:
        pass
    time.sleep(0.3)

    print("\n[T6] 模拟 UI：先启动 → 停止 → 保存新端口 → 启动")

    # 1. 第一次启动（用默认端口）
    sm.port = 5001
    sm.https_port = 5002
    sm._save_port(5001)
    ok, msg = sm.start(force=True)
    assert ok, f"首次启动失败: {msg}"
    print(f"[T6-1] 启动: {msg}")

    # 验证 HTTP 可访问
    r = requests.get(f"http://127.0.0.1:{sm.port}/login", timeout=3)
    assert r.status_code == 200

    # 2. 停止
    ok, msg = sm.stop()
    assert ok
    print(f"[T6-2] 停止: {msg}")
    time.sleep(0.5)

    # 3. 保存新端口（模拟点击"保存端口"按钮）
    new_port = 5099
    print(f"[T6-3] 保存新端口 {new_port}")
    ok, msg = sm.set_port(new_port)
    assert ok, f"set_port 失败: {msg}"
    print(f"[T6-3] set_port: {msg}")
    # 验证文件保存
    port_file = port_change_env["TMP_DATA"] / "port.txt"
    saved_port = int(port_file.read_text().strip())
    assert saved_port == new_port, f"文件端口不对: {saved_port}"
    print(f"[T6-3] 端口文件内容: {saved_port}")

    # 4. 启动（模拟点击"启动服务"按钮）
    print(f"[T6-4] 启动服务 port={new_port}")
    try:
        # 模拟 UI 行为：先设值
        sm.port = new_port
        sm.https_port = new_port + 1
        sm._save_port(new_port)
        ok, msg = sm.start(force=True)
    except Exception as e:
        pytest.fail(f"start 异常: {e}")
    assert ok, f"start 失败: {msg}"
    print(f"[T6-4] start: {msg}")
    assert sm.port == new_port
    assert sm.is_running

    # 5. 验证 HTTP 可访问
    r = requests.get(f"http://127.0.0.1:{sm.port}/login", timeout=3)
    assert r.status_code == 200, f"HTTP {sm.port} 不可访问"
    print(f"[T6-5] HTTP {sm.port} 可访问")


def test_start_while_watchdog_active(port_change_env):
    """场景：看门狗运行中时手动启动（应被 is_running 拦截）"""
    sm = port_change_env["sm"]

    try:
        sm.stop()
    except Exception:
        pass
    time.sleep(0.3)

    print("\n[T7] 看门狗运行中重复启动")

    sm.port = 5301
    sm.https_port = 5302
    sm._save_port(5301)
    ok, msg = sm.start(force=True)
    assert ok
    print(f"[T7-1] 启动成功: {msg}")

    # 立即再次启动
    ok2, msg2 = sm.start(force=True)
    assert not ok2
    assert "已在运行" in msg2
    print(f"[T7-2] 重复启动被拦截: {msg2}")

    sm.stop()
    time.sleep(0.3)
