"""
常量与工具函数
- 路径解析
- 状态常量
- 字段动态加载（带缓存）
- 进程互斥体
"""
import ctypes
import os
import re
import sys
from pathlib import Path


def _get_code_dir():
    """获取代码所在目录（打包后在 _MEIPASS 内）"""
    if getattr(sys, 'frozen', False):
        return Path(os.path.dirname(os.path.abspath(__file__)))
    return Path(__file__).parent.parent


def _get_data_dir():
    """获取数据目录：开发时用源码同级 data，打包后用 exe 同级 data（唯一位置）"""
    if getattr(sys, 'frozen', False):
        return Path(os.path.dirname(os.path.abspath(sys.executable))) / "data"
    return Path(__file__).parent.parent / "data"


BASE_DIR = _get_code_dir()
DATA_DIR = _get_data_dir()
FIELDS_CONFIG = DATA_DIR / "fields.txt"
PORT_CONFIG = DATA_DIR / "port.txt"
APP_PY = BASE_DIR / "app.py"
DEFAULT_PORT = 5000


def _resolve_default_port():
    """#26 端口默认值共享：尝试从 app.py 读取默认值
    正则锚定行首，避免误匹配 https_port = ... 之类的其他赋值行"""
    try:
        if APP_PY.exists():
            with open(APP_PY, "r", encoding="utf-8") as f:
                content = f.read()
            m = re.search(r"^port\s*=\s*(\d+)", content, re.M)
            if m:
                return int(m.group(1))
    except (OSError, ValueError):
        pass
    return DEFAULT_PORT


# 运行时确定最终默认值
DEFAULT_PORT = _resolve_default_port()

# 全局互斥体句柄
_MUTEX_HANDLE = None


def _acquire_lock():
    """使用 Windows 命名互斥体检测重复运行"""
    global _MUTEX_HANDLE
    try:
        name = "SalaryControlPanel-7F3A2B1C"
        _MUTEX_HANDLE = ctypes.windll.kernel32.CreateMutexW(None, False, name)
        err = ctypes.windll.kernel32.GetLastError()
        if err == 183:  # ERROR_ALREADY_EXISTS
            if _MUTEX_HANDLE:
                ctypes.windll.kernel32.CloseHandle(_MUTEX_HANDLE)
                _MUTEX_HANDLE = None
            return False
        return True
    except Exception:
        return True


def _release_lock():
    """释放互斥体"""
    global _MUTEX_HANDLE
    if _MUTEX_HANDLE:
        try:
            ctypes.windll.kernel32.CloseHandle(_MUTEX_HANDLE)
        except Exception:
            pass
        _MUTEX_HANDLE = None


# 三态常量
STATE_AUTO = "auto"   # 跟随全局规则：空/0 不显示
STATE_SHOW = "show"   # 强制显示
STATE_HIDE = "hide"   # 强制隐藏

STATE_LABELS = {
    STATE_AUTO: "跟随",
    STATE_SHOW: "显示",
    STATE_HIDE: "隐藏",
}

STATE_COLORS = {
    STATE_AUTO: {
        "active_bg": "#e5e7eb", "bg": "#f3f4f6", "fg": "#6b7280",
        "select_bg": "#9ca3af",
    },
    STATE_SHOW: {
        "active_bg": "#dbeafe", "bg": "#eff6ff", "fg": "#1e40af",
        "select_bg": "#3b82f6",
    },
    STATE_HIDE: {
        "active_bg": "#fee2e2", "bg": "#fef2f2", "fg": "#991b1b",
        "select_bg": "#ef4444",
    },
}

STATE_OPTIONS = [
    (STATE_SHOW, "显示"),
    (STATE_HIDE, "隐藏"),
    (STATE_AUTO, "全局"),
]


_DYNAMIC_FIELDS_CACHE = None


def get_dynamic_fields():
    """从工资表动态读取所有字段（带缓存，只读取一次）"""
    global _DYNAMIC_FIELDS_CACHE
    if _DYNAMIC_FIELDS_CACHE is not None:
        return _DYNAMIC_FIELDS_CACHE
    sys.path.insert(0, str(BASE_DIR))
    try:
        from excel_data import get_all_salary_fields
        _DYNAMIC_FIELDS_CACHE = get_all_salary_fields()
    except Exception:
        _DYNAMIC_FIELDS_CACHE = []
    return _DYNAMIC_FIELDS_CACHE


# pystray 可用性
try:
    from pystray import MenuItem as item, Menu, Icon
    from PIL import Image, ImageDraw
    HAS_TRAY = True
except ImportError:
    HAS_TRAY = False
