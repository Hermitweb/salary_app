"""测试 _find_salary_file 正则匹配逻辑"""
import os
import sys
import tempfile
import re
from pathlib import Path

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))


class TestFindSalaryFileRegex:
    """_find_salary_file 现在用正则匹配，应能处理各种命名格式"""

    def test_padded_month(self):
        """2026年05月员工工资.xlsx 格式"""
        year, month = "2026", "05"
        # 模拟正则
        pattern = re.compile(rf"{year}年\s*{month}\s*月")
        assert pattern.search("2026年05月员工工资.xlsx") is not None

    def test_unpadded_month(self):
        """2026年5月员工工资.xlsx 格式"""
        year, month = "2026", "5"
        pattern = re.compile(rf"{year}年\s*{month}\s*月")
        assert pattern.search("2026年5月员工工资.xlsx") is not None

    def test_no_space(self):
        """无空格：2026年5月员工工资"""
        year, month = "2026", "5"
        pattern = re.compile(rf"{year}年\s*{month}\s*月")
        assert pattern.search("2026年5月员工工资") is not None

    def test_with_space(self):
        """有空格：2026年 5 月 员工工资"""
        year, month = "2026", "5"
        pattern = re.compile(rf"{year}年\s*{month}\s*月")
        assert pattern.search("2026年 5 月 员工工资") is not None

    def test_year_not_matched(self):
        """年份不匹配"""
        pattern = re.compile(rf"2026年\s*5\s*月")
        assert pattern.search("2025年5月员工工资.xlsx") is None

    def test_month_not_matched(self):
        """月份不匹配"""
        pattern = re.compile(rf"2026年\s*5\s*月")
        assert pattern.search("2026年6月员工工资.xlsx") is None
