"""
control_panel 包：拆分后的桌面控制面板
模块化结构：
  - constants: 路径/常量/工具函数
  - service: ServiceManager (HTTP/HTTPS + 看门狗)
  - field_config: FieldConfig + FieldItem (字段配置)
  - tray: TrayIcon (系统托盘)
  - ui: ControlPanel (主界面)

#24 拆分动机：原 control_panel.py 1500+ 行，难以维护
"""
from .service import ServiceManager
from .field_config import FieldConfig, FieldItem
from .tray import TrayIcon
from .ui import ControlPanel
from .constants import (
    HAS_TRAY, BASE_DIR, DATA_DIR, FIELDS_CONFIG, PORT_CONFIG, APP_PY,
    DEFAULT_PORT, _get_data_dir, _get_code_dir,
    STATE_AUTO, STATE_SHOW, STATE_HIDE,
    STATE_LABELS, STATE_COLORS, STATE_OPTIONS,
    get_dynamic_fields,
)

__all__ = [
    "ServiceManager", "FieldConfig", "FieldItem", "TrayIcon", "ControlPanel",
    "HAS_TRAY", "DATA_DIR", "FIELDS_CONFIG", "PORT_CONFIG", "APP_PY", "DEFAULT_PORT",
    "STATE_AUTO", "STATE_SHOW", "STATE_HIDE",
    "STATE_LABELS", "STATE_COLORS", "STATE_OPTIONS",
    "get_dynamic_fields", "main",
]


def main():
    """包级入口"""
    from .ui import main as _ui_main
    return _ui_main()
