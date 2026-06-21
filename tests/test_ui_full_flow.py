"""
完整复现用户操作流程：启动 → 改端口 → 保存 → 启动
捕获所有异常，确保不会崩溃
"""
import os
import sys
import time
import shutil
import tempfile
import traceback
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))


def setup_env():
    TMP_DATA = Path(tempfile.mkdtemp(prefix="ui_full_"))

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


def test_full_flow():
    """完整流程测试"""
    import tkinter as tk
    TMP_DATA = setup_env()

    # 拦截 messagebox 防止阻塞测试
    from tkinter import messagebox
    original_showerror = messagebox.showerror
    original_showinfo = messagebox.showinfo
    original_askyesno = messagebox.askyesno
    logs = []
    messagebox.showerror = lambda title, msg: logs.append(f"ERROR: {title} - {msg}")
    messagebox.showinfo = lambda title, msg: logs.append(f"INFO: {title} - {msg}")
    messagebox.askyesno = lambda title, msg: (logs.append(f"ASK: {title} - {msg} -> YES"), True)[1]

    try:
        from control_panel_pkg.ui import ControlPanel
        import logging
        logging.getLogger("werkzeug").setLevel(logging.WARNING)

        root = tk.Tk()
        root.withdraw()  # 不显示窗口

        app = ControlPanel(root)

        print("=" * 60)
        print("[场景] 完整复现：启动 → 改端口 → 保存 → 启动")
        print("=" * 60)

        # 1. 第一次启动
        print("\n[1] 第一次启动（默认端口）")
        app.port_var.set(str(app.service.port))
        app._start_service()
        time.sleep(1.5)
        assert app.service.check_status(), f"首次启动失败: {logs}"
        print(f"  ✓ 启动成功: port={app.service.port}")

        # 2. 停止
        print("\n[2] 停止服务")
        app._stop_service()
        time.sleep(0.5)
        assert not app.service.check_status()
        print("  ✓ 停止成功")

        # 3. 修改端口
        print("\n[3] 修改端口到 5099")
        new_port = 5099
        app.port_var.set(str(new_port))
        # 模拟点击"保存端口"按钮
        try:
            app._save_port()
        except Exception as e:
            print(f"  ✗ _save_port 异常: {e}")
            traceback.print_exc()
            assert False
        time.sleep(0.5)
        print(f"  ✓ 保存成功: port={app.service.port}")
        print(f"  消息: {logs[-1] if logs else '(no msg)'}")

        # 4. 启动
        print("\n[4] 点击启动服务")
        try:
            app._start_service()
        except Exception as e:
            print(f"  ✗ _start_service 异常: {e}")
            traceback.print_exc()
            assert False, f"启动异常: {e}"
        time.sleep(1.5)
        if not app.service.check_status():
            print(f"  ✗ 启动失败: {logs}")
            assert False
        print(f"  ✓ 启动成功: port={app.service.port}, is_running={app.service.is_running}")

        # 5. 验证 HTTP
        import requests
        r = requests.get(f"http://127.0.0.1:{app.service.port}/login", timeout=3)
        assert r.status_code == 200
        print(f"  ✓ HTTP {app.service.port} 200 OK")

        # 6. 再次改端口（测试连续改端口）
        print("\n[5] 再次修改端口到 5199")
        new_port2 = 5199
        app.port_var.set(str(new_port2))
        try:
            app._save_port()
        except Exception as e:
            print(f"  ✗ _save_port 异常: {e}")
            assert False
        time.sleep(0.5)

        try:
            app._start_service()
        except Exception as e:
            print(f"  ✗ _start_service 异常: {e}")
            assert False
        time.sleep(1.5)
        assert app.service.check_status()
        assert app.service.port == new_port2
        print(f"  ✓ 再次启动成功: port={app.service.port}")

        r = requests.get(f"http://127.0.0.1:{app.service.port}/login", timeout=3)
        assert r.status_code == 200
        print(f"  ✓ HTTP {app.service.port} 200 OK")

        # 7. 清理
        print("\n[6] 清理")
        app._stop_service()
        time.sleep(0.5)
        root.destroy()

        print("\n" + "=" * 60)
        print("✓ 全部通过")
        print("=" * 60)
        print(f"\n所有 messagebox 消息:")
        for msg in logs:
            print(f"  {msg}")

    finally:
        messagebox.showerror = original_showerror
        messagebox.showinfo = original_showinfo
        messagebox.askyesno = original_askyesno
        shutil.rmtree(TMP_DATA, ignore_errors=True)
        try:
            from control_panel_pkg import cache as cache_mod
            cache_mod.DataCache.reset_for_test()
        except ImportError:
            import cache as cache_mod
            cache_mod.DataCache.reset_for_test()


if __name__ == "__main__":
    test_full_flow()
