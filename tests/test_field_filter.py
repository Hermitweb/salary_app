"""测试 _col_should_show 和 _filtered_field"""
import os
import sys
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from excel_data import _col_should_show, _filtered_field, STATE_AUTO, STATE_SHOW, STATE_HIDE


class TestColShouldShow:
    """测试 _col_should_show 三态逻辑"""

    def test_no_source_fields_always_show(self):
        """未配置列名：默认显示"""
        assert _col_should_show("unknown_col", {}, {}) is True

    def test_show_state_always_show(self):
        """任一源字段 = show -> 强制显示"""
        config = {"应发基本工资": STATE_SHOW}
        assert _col_should_show("base_salary", {"应发基本工资": 0}, config) is True
        assert _col_should_show("base_salary", {}, config) is True

    def test_hide_state_with_no_data(self):
        """所有源字段 = hide 且无数据 -> 隐藏"""
        config = {"应发基本工资": STATE_HIDE}
        assert _col_should_show("base_salary", {}, config) is False

    def test_hide_state_with_data(self):
        """所有源字段 = hide 但有数据 -> 隐藏（hide 优先）"""
        config = {"应发基本工资": STATE_HIDE}
        assert _col_should_show("base_salary", {"应发基本工资": 5000}, config) is False

    def test_auto_state_with_data(self):
        """auto + 有数据 -> 显示"""
        config = {"应发基本工资": STATE_AUTO}
        assert _col_should_show("base_salary", {"应发基本工资": 5000}, config) is True

    def test_auto_state_no_data(self):
        """auto + 无数据 -> 隐藏"""
        config = {"应发基本工资": STATE_AUTO}
        assert _col_should_show("base_salary", {}, config) is False
        assert _col_should_show("base_salary", {"应发基本工资": 0}, config) is False
        assert _col_should_show("base_salary", {"应发基本工资": ""}, config) is False


class TestFilteredField:
    """测试 _filtered_field 包装函数"""

    def test_show_returns_value(self):
        """show 状态：直接返回值"""
        config = {"应发基本工资": STATE_SHOW}
        result = _filtered_field("base_salary", 5000, {}, config)
        assert result == 5000

    def test_auto_with_value_returns_value(self):
        """auto + 有值：返回值"""
        config = {"应发基本工资": STATE_AUTO}
        data = {"应发基本工资": 5000}
        result = _filtered_field("base_salary", 5000, data, config)
        assert result == 5000

    def test_auto_no_value_returns_none(self):
        """auto + 无值：返回 None"""
        config = {"应发基本工资": STATE_AUTO}
        result = _filtered_field("base_salary", "", {}, config)
        assert result is None

    def test_hide_with_value_keeps_value(self):
        """hide + 有值：旧行为保留（不强制置 None）"""
        config = {"应发基本工资": STATE_HIDE}
        result = _filtered_field("base_salary", 5000, {}, config)
        assert result == 5000

    def test_hide_no_value_returns_none(self):
        """hide + 无值：None"""
        config = {"应发基本工资": STATE_HIDE}
        result = _filtered_field("base_salary", "", {}, config)
        assert result is None
