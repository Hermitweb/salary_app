"""
包入口：python -m control_panel_pkg
效果等同于 python -c "from control_panel_pkg import main; main()"
注意：必须用绝对 import（不能 from .ui import main），
否则 PyInstaller 打包后会报 "attempted relative import with no known parent package"
"""
from control_panel_pkg.ui import main

if __name__ == "__main__":
    main()
