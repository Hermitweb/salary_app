"""把 Python 原版的应用图标（app_icon.ico）复制到托盘 crate。

Rust 版要沿用同一份品牌资产：标题栏、任务栏、托盘、侧边栏 logo
四处都用它，避免「同一个应用两套图标」。
"""
import shutil
from pathlib import Path

SOURCE = Path(r"E:\workspace\salary_app\app_icon.ico")
TARGET = Path(__file__).resolve().parents[1] / "tray" / "resources" / "app_icon.ico"

if not SOURCE.is_file():
    raise SystemExit(f"找不到原版图标：{SOURCE}")

TARGET.parent.mkdir(parents=True, exist_ok=True)
shutil.copyfile(SOURCE, TARGET)
print(f"已复制：{SOURCE} -> {TARGET}")
