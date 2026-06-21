"""测试工具函数：准备临时数据目录"""
import os
import sys
import tempfile
import shutil
from pathlib import Path


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
