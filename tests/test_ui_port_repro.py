"""
真实复现：控制面板更换端口保存之后点击启动服务
在 Tkinter 环境下模拟 UI 操作
"""
import os
import sys
import time
import shutil
import tempfile
from pathlib import Path
import threading

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))


def setup_env():
    TMP_DATA = Path(tempfile.mkdtemp(prefix="ui_port_"))

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

    # 固定测试端口：不写 port.txt 会回落到 DEFAULT_PORT（导入时解析的真实端口 5001），
    # 与生产服务冲突
    constants.PORT_CONFIG.write_text("16201", encoding="utf-8")

    from openpyxl import Workbook
    wb = Workbook()
    ws = wb.active
    ws.append(["员工号", "姓名", "部门", "紧急联系人号码"])
    ws.append(["E0001", "张三", "测试部", "13800000001"])
    wb.save(TMP_DATA / "员工列表.xlsx")

    salary_dir = TMP_DATA / "salary"
    salary_dir.mkdir(exist_ok=True)
    wb_sal = Workbook()
    ws_sal = wb_sal.active
    ws_sal.append(["员工号", "姓名", "部门", "应发基本工资"])
    ws_sal.append(["E0001", "张三", "测试部", 5000])
    wb_sal.save(salary_dir / "2026年05月员工工资.xlsx")

    return TMP_DATA


def _wait_not_busy(app, root, timeout=30):
    """P1-9 启停异步化后，完成回调经 root.after 投递：
    测试必须驱动 Tk 事件循环，等待 _busy 清空（服务真正启停完成）"""
    deadline = time.time() + timeout
    while time.time() < deadline:
        root.update()
        if app._busy is None:
            return True
        time.sleep(0.05)
    return False


def test_ui_port_change():
    """测试完整 UI 流程：启动 → 改端口 → 保存 → 启动"""
    import tkinter as tk
    TMP_DATA = setup_env()

    try:
        from control_panel_pkg.ui import ControlPanel
        import logging
        logging.getLogger("werkzeug").setLevel(logging.WARNING)

        root = tk.Tk()
        # 不显示窗口，测试逻辑
        # root.withdraw()

        app = ControlPanel(root)

        # 模拟：直接调用 _start_service 启动默认端口
        print("[1] 第一次启动（默认端口）")
        app.port_var.set(str(app.service.port))
        app._start_service()
        assert _wait_not_busy(app, root), "启动操作超时"

        is_running = app.service.check_status()
        print(f"  is_running = {is_running}, port = {app.service.port}")
        assert is_running, "首次启动失败"

        # 模拟：停止服务
        print("\n[2] 停止服务")
        app._stop_service()
        assert _wait_not_busy(app, root), "停止操作超时"
        is_running = app.service.check_status()
        print(f"  is_running = {is_running}")
        assert not is_running

        # 模拟：修改端口
        print("\n[3] 修改端口到 15099")
        new_port = 15099
        app.port_var.set(str(new_port))
        # 直接调用 set_port（绕过 askyesno 弹窗）
        ok, msg = app.service.set_port(new_port)
        print(f"  set_port: ok={ok}, msg={msg}")
        assert ok

        # 模拟：点击 启动服务
        print("\n[4] 点击启动服务")
        try:
            app._start_service()
        except Exception as e:
            print(f"  _start_service 异常: {e}")
            import traceback
            traceback.print_exc()
            assert False, f"启动异常: {e}"
        assert _wait_not_busy(app, root), "改端口后启动操作超时"

        is_running = app.service.check_status()
        print(f"  is_running = {is_running}, port = {app.service.port}")
        if not is_running:
            print(f"  ✗ 启动失败！")
            assert False, "改端口后启动失败"

        # 验证 HTTP 可访问
        import requests
        r = requests.get(f"http://127.0.0.1:{app.service.port}/login", timeout=3)
        print(f"  HTTP {app.service.port} 状态: {r.status_code}")
        assert r.status_code == 200

        print("\n✓ 全部通过")
        app._stop_service()
        assert _wait_not_busy(app, root, timeout=10), "收尾停止操作超时"
        time.sleep(0.3)
        root.destroy()
    finally:
        shutil.rmtree(TMP_DATA, ignore_errors=True)
        try:
            from control_panel_pkg import cache as cache_mod
            cache_mod.DataCache.reset_for_test()
        except ImportError:
            import cache as cache_mod
            cache_mod.DataCache.reset_for_test()


if __name__ == "__main__":
    test_ui_port_change()
