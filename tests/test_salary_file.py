"""测试 _find_salary_file 正则匹配逻辑（真实实现，临时数据目录）"""
import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

import excel_data


def _make_file(base: Path, name: str):
    base.mkdir(parents=True, exist_ok=True)
    (base / name).write_bytes(b"")
    return str(base / name)


class TestFindSalaryFile:
    """_find_salary_file 用正则匹配多种命名格式（调用真实实现）"""

    def test_padded_month(self, tmp_path, monkeypatch):
        """2026年05月员工工资.xlsx 格式"""
        expected = _make_file(tmp_path, "2026年05月员工工资.xlsx")
        monkeypatch.setattr(excel_data, "DATA_DIR", str(tmp_path))
        assert excel_data._find_salary_file(2026, 5) == expected

    def test_unpadded_month(self, tmp_path, monkeypatch):
        """2026年5月员工工资.xlsx 格式"""
        expected = _make_file(tmp_path, "2026年5月员工工资.xlsx")
        monkeypatch.setattr(excel_data, "DATA_DIR", str(tmp_path))
        assert excel_data._find_salary_file(2026, 5) == expected

    def test_dash_format(self, tmp_path, monkeypatch):
        """2026-05 工资.xlsx / 2026_05_工资.xlsx 格式"""
        expected = _make_file(tmp_path, "2026-05 工资.xlsx")
        monkeypatch.setattr(excel_data, "DATA_DIR", str(tmp_path))
        assert excel_data._find_salary_file(2026, 5) == expected

    def test_compact_format(self, tmp_path, monkeypatch):
        """202605工资.xlsx 格式"""
        expected = _make_file(tmp_path, "202605工资.xlsx")
        monkeypatch.setattr(excel_data, "DATA_DIR", str(tmp_path))
        assert excel_data._find_salary_file(2026, 5) == expected

    def test_salary_subdir_preferred(self, tmp_path, monkeypatch):
        """salary/ 子目录优先于 data 根目录"""
        _make_file(tmp_path, "2026年05月员工工资.xlsx")  # 根目录的干扰文件
        expected = _make_file(tmp_path / "salary", "2026年05月员工工资.xlsx")
        monkeypatch.setattr(excel_data, "DATA_DIR", str(tmp_path))
        assert excel_data._find_salary_file(2026, 5) == expected

    def test_month_must_match(self, tmp_path, monkeypatch):
        """月份不匹配时不应返回其他月份的文件"""
        _make_file(tmp_path, "2026年06月员工工资.xlsx")
        monkeypatch.setattr(excel_data, "DATA_DIR", str(tmp_path))
        assert excel_data._find_salary_file(2026, 5) is None

    def test_not_found(self, tmp_path, monkeypatch):
        """空目录返回 None"""
        monkeypatch.setattr(excel_data, "DATA_DIR", str(tmp_path))
        assert excel_data._find_salary_file(2026, 5) is None
