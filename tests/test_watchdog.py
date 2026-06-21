"""
看门狗保活机制测试
- T1: 模拟服务崩溃，验证看门狗能检测并自动重启
- T2: 验证自动重启次数限制
- T3: 验证手动 stop() 不会触发自动重启
- T4: 验证 HTTPS 线程死亡时也能被检测（潜在问题）
"""
import os
import sys
import time
import threading
from pathlib import Path

# 测试环境
ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
os.chdir(ROOT)

# 用临时数据目录避免污染
import tempfile
TMP_DATA = Path(tempfile.mkdtemp(prefix="watchdog_test_"))
print(f"[测试] 使用临时数据目录: {TMP_DATA}")

# 覆盖 DATA_DIR
from control_panel_pkg import constants
constants.DATA_DIR = TMP_DATA
constants.DATA_DIR.mkdir(exist_ok=True)
constants.BASE_DIR = ROOT
constants.PORT_CONFIG = TMP_DATA / "port.txt"
constants.FIELDS_CONFIG = TMP_DATA / "fields.txt"

# 准备假数据：让 app 可以导入
(TMP_DATA / "员工列表.xlsx").write_bytes(b"")  # 假文件，足够导入
(TMP_DATA / "salary").mkdir(exist_ok=True)

# 写测试工资表
from openpyxl import Workbook
wb = Workbook()
ws = wb.active
ws.append(["工号", "姓名", "部门", "基本工资"])
ws.append(["001", "测试", "测试部", 5000])
wb.save(TMP_DATA / "salary" / "2026年05月员工工资.xlsx")

# 写员工表
wb2 = Workbook()
ws2 = wb2.active
ws2.append(["工号", "姓名", "部门", "紧急联系人", "在职状态"])
ws2.append(["001", "测试", "测试部", "13800000000", "在职"])
wb2.save(TMP_DATA / "员工列表.xlsx")

# 设置端口避免冲突
constants.PORT_CONFIG.write_text("15001", encoding="utf-8")

from control_panel_pkg.service import ServiceManager


def banner(s):
    print(f"\n{'='*60}\n{s}\n{'='*60}")


# ============== T1: 检测崩溃并自动重启 ==============
def test_crash_detection():
    banner("T1: 模拟服务崩溃，验证看门狗能自动重启")
    sm = ServiceManager()
    ok, msg = sm.start()
    assert ok, f"启动失败: {msg}"
    print(f"[T1] 启动成功: {msg}")
    print(f"[T1] 初始 is_running={sm.is_running}, restart_count={sm._restart_count}")

    # 记录当前线程
    original_thread = sm.flask_thread
    original_server_id = sm._server_id
    print(f"[T1] original_thread={original_thread.name if original_thread else None}, server_id={original_server_id}")

    # 模拟崩溃：直接关闭底层 socket，触发 serve_forever 退出
    print("[T1] 模拟崩溃: 关闭 server socket")
    sm._close_server_socket(sm._server)
    # 给线程一点时间退出
    time.sleep(1)

    # 等待看门狗检测并重启（看门狗间隔 5 秒 + 重启延迟 3 秒 = 最多 8 秒）
    print("[T1] 等待看门狗检测并重启（最多 15 秒）...")
    deadline = time.time() + 15
    restarted = False
    while time.time() < deadline:
        if sm.flask_thread is not None and sm.flask_thread is not original_thread and sm.flask_thread.is_alive():
            restarted = True
            break
        time.sleep(0.5)

    if restarted:
        print(f"[T1] OK: auto-restart succeeded, new thread={sm.flask_thread.name}, restart_count={sm._restart_count}, is_running={sm.is_running}")
        result = True
    else:
        print(f"[T1] FAIL: auto-restart failed, is_running={sm.is_running}, restart_count={sm._restart_count}")
        result = False

    # 清理
    sm.stop()
    return result


# ============== T2: 重启次数限制 ==============
def test_restart_limit():
    banner("T2: 验证最大重启次数限制（设为 3 便于测试）")
    sm = ServiceManager()
    # 临时降低上限
    ServiceManager._MAX_RESTART_COUNT = 3
    ServiceManager._WATCHDOG_INTERVAL = 2
    ServiceManager._RESTART_DELAY = 1

    ok, msg = sm.start()
    assert ok, f"启动失败: {msg}"
    print(f"[T2] 启动成功: {msg}")

    # 持续触发崩溃直到达到上限
    initial_count = sm._restart_count  # 应该是 1（start 中 increment）
    print(f"[T2] 初始 restart_count={initial_count}")
    print(f"[T2] 最大重启次数: {sm._MAX_RESTART_COUNT}")

    # 让它一直崩溃：起一个后台线程周期性关闭 server
    def crash_loop():
        for _ in range(20):
            time.sleep(4)
            if sm._server is not None:
                try:
                    sm._close_server_socket(sm._server)
                    print(f"[T2-crash] 触发一次崩溃，当前 restart_count={sm._restart_count}")
                except Exception as e:
                    print(f"[T2-crash] error: {e}")

    crash_thread = threading.Thread(target=crash_loop, daemon=True)
    crash_thread.start()

    # 等待看门狗耗尽重启次数
    # 每次重启: 2秒检测 + 1秒延迟 + 服务运行时间（约2-4秒被crash）
    # 假设3次重启需要 ~ 20秒
    print("[T2] 等待看门狗耗尽重启次数（最多 40 秒）...")
    deadline = time.time() + 40
    exhausted = False
    while time.time() < deadline:
        # 当 restart_count 超过上限且 is_running 长时间为 False，说明已耗尽
        if sm._restart_count >= sm._MAX_RESTART_COUNT and not sm.is_running:
            # 再多等几秒确认不会自启
            time.sleep(3)
            if not sm.is_running:
                exhausted = True
                break
        time.sleep(0.5)

    if exhausted:
        print(f"[T2] OK: restart limit enforced: restart_count={sm._restart_count}, is_running={sm.is_running}")
        result = True
    else:
        print(f"[T2] FAIL: restart limit NOT enforced: restart_count={sm._restart_count}, is_running={sm.is_running}")
        result = False

    sm.stop()
    # 还原
    ServiceManager._MAX_RESTART_COUNT = 5
    ServiceManager._WATCHDOG_INTERVAL = 5
    ServiceManager._RESTART_DELAY = 3
    return result


# ============== T3: 手动 stop 不触发自动重启 ==============
def test_manual_stop_no_restart():
    banner("T3: 手动 stop() 后，看门狗不应自动重启")
    sm = ServiceManager()
    ok, msg = sm.start()
    assert ok, f"启动失败: {msg}"
    print(f"[T3] 启动成功: {msg}")

    ok, msg = sm.stop()
    assert ok, msg
    print(f"[T3] 手动停止: {msg}, _manual_stopping={sm._manual_stopping}")

    # 等待 10 秒，看门狗间隔 5 秒，应该触发 2 次
    print("[T3] 等待 10 秒，看是否有意外重启...")
    time.sleep(10)

    if not sm.is_running and sm.flask_thread is None:
        print(f"[T3] OK: manual stop did not auto-restart, is_running={sm.is_running}")
        result = True
    else:
        print(f"[T3] FAIL: manual stop triggered restart, is_running={sm.is_running}, thread={sm.flask_thread}")
        result = False

    # 此时服务已被手动停止，再次 start 应该要 force=True
    print("[T3] Trying start() (without force)")
    ok, msg = sm.start()
    if not ok and "手动停止" in msg:
        print(f"[T3] OK: start() correctly rejected: {msg}")
    else:
        print(f"[T3] ?: start() behavior unexpected: ok={ok}, msg={msg}")

    return result


# ============== T4: HTTPS 线程死亡检测 ==============
def test_https_thread_not_watched():
    banner("T4: HTTPS 线程死亡是否能被检测（潜在问题）")
    sm = ServiceManager()
    ok, msg = sm.start()
    assert ok, f"启动失败: {msg}"
    if not sm.https_enabled:
        print("[T4] ! HTTPS 未启用，跳过测试")
        sm.stop()
        return None  # 跳过

    https_thread = sm._https_thread
    print(f"[T4] HTTPS 线程: {https_thread.name if https_thread else None}")

    # 直接关闭 HTTPS server socket，模拟 HTTPS 线程死亡
    if sm._https_server is not None:
        sm._close_server_socket(sm._https_server)
        time.sleep(1)
        https_alive = sm._https_thread.is_alive() if sm._https_thread else False
        print(f"[T4] 关闭 HTTPS server socket 后，线程仍 alive: {https_alive}")

    # 等待看门狗 10 秒
    print("[T4] 等待 10 秒，看 HTTPS 线程是否被自动重启...")
    time.sleep(10)

    # HTTPS 线程是否被重启？
    if sm._https_thread is not None and sm._https_thread is not https_thread and sm._https_thread.is_alive():
        print(f"[T4] OK: HTTPS thread auto-restarted")
        result = True
    else:
        print(f"[T4] FAIL: HTTPS thread not auto-restarted")
        result = False

    sm.stop()
    return result


# ============== T5: stop 后再次 start（force=True）能正常启动 ==============
def test_restart_after_stop():
    banner("T5: stop 后用 force=True 重新 start")
    sm = ServiceManager()
    ok, msg = sm.start()
    assert ok, f"启动失败: {msg}"
    sm.stop()
    print(f"[T5] 已 stop, _manual_stopping={sm._manual_stopping}")

    ok, msg = sm.start(force=True)
    if ok:
        print(f"[T5] OK: force=True start succeeded: {msg}, restart_count={sm._restart_count}")
        result = True
    else:
        print(f"[T5] FAIL: force=True start failed: {msg}")
        result = False

    sm.stop()
    return result


if __name__ == "__main__":
    results = []
    results.append(("T1 崩溃检测+自动重启", test_crash_detection()))
    results.append(("T2 重启次数限制", test_restart_limit()))
    results.append(("T3 手动 stop 不重启", test_manual_stop_no_restart()))
    results.append(("T4 HTTPS 线程监控", test_https_thread_not_watched()))
    results.append(("T5 stop 后 force restart", test_restart_after_stop()))

    print("\n" + "="*60)
    print("Test Summary")
    print("="*60)
    for name, r in results:
        if r is True:
            print(f"  PASS {name}")
        elif r is False:
            print(f"  FAIL {name}")
        else:
            print(f"  SKIP {name}")
