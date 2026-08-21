"""测试工具函数：准备临时数据目录"""
import os
import sys
import tempfile
import shutil
from pathlib import Path

import pytest

# 测试直连 HTTP 端口，自签证书无法通过 requests 验证：
# 必须在模块导入期关闭强跳转（模块级 fixture 启动服务早于函数级 fixture）
_PROJECT_ROOT = Path(__file__).resolve().parent.parent
if str(_PROJECT_ROOT) not in sys.path:
    sys.path.insert(0, str(_PROJECT_ROOT))
from control_panel_pkg.service import ServiceManager as _SM  # noqa: E402
_SM.https_redirect = False
# 看门狗死锁探活在专项测试中单独覆盖，其余测试关闭以保证确定性
_SM.enable_deadlock_probe = False


@pytest.fixture(autouse=True)
def _isolate_security_state():
    """每个测试结束后清空登录锁定状态，避免用例间相互影响"""
    yield
    app_mod = sys.modules.get("app")
    if app_mod is not None and hasattr(app_mod, "_reset_login_limits"):
        app_mod._reset_login_limits()


def setup_temp_data_dir():
    """创建临时 data 目录，模拟 frozen 环境"""
    tmp = Path(tempfile.mkdtemp(prefix="salary_test_"))
    sub = tmp / "data"
    sub.mkdir()
    # 写入空 fields.txt
    (sub / "fields.txt").write_text("", encoding="utf-8")
    # 写入默认 port
    (sub / "port.txt").write_text("0", encoding="utf-8")
    # 把临时 data 路径注入到 excel_data 模块
    sys.modules.pop("excel_data", None)
    return tmp, sub


def cleanup_temp_data_dir(tmp):
    shutil.rmtree(tmp, ignore_errors=True)
