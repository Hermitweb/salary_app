"""测试 _is_empty_or_zero 工具函数"""
import os
import sys
import pytest

# 把 salary_app 目录加入路径
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from excel_data import _is_empty_or_zero


class TestIsEmptyOrZero:
    def test_none(self):
        assert _is_empty_or_zero(None) is True

    def test_empty_string(self):
        assert _is_empty_or_zero("") is True
        assert _is_empty_or_zero("   ") is True

    def test_zero(self):
        assert _is_empty_or_zero(0) is True
        assert _is_empty_or_zero(0.0) is True
        assert _is_empty_or_zero("0") is True
        assert _is_empty_or_zero("0.0") is True
        assert _is_empty_or_zero("0.00") is True

    def test_currency_zero(self):
        assert _is_empty_or_zero("¥0") is True
        assert _is_empty_or_zero("￥0") is True
        assert _is_empty_or_zero("¥0.00") is True

    def test_non_zero_numbers(self):
        assert _is_empty_or_zero(1) is False
        assert _is_empty_or_zero(0.01) is False
        assert _is_empty_or_zero("1") is False
        assert _is_empty_or_zero("100") is False
        assert _is_empty_or_zero("¥1") is False
        assert _is_empty_or_zero("1,000") is False

    def test_text(self):
        assert _is_empty_or_zero("hello") is False
        assert _is_empty_or_zero("员工号") is False
