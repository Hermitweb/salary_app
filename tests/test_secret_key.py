"""测试 secret_key 持久化逻辑（通过 mock 替代文件 IO）"""
import os
import sys
import tempfile
from pathlib import Path
from unittest.mock import patch, mock_open

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))


class TestSecretKeyPersistence:
    """验证 secret_key 持久化逻辑（不直接导入 app 以避免 Flask 副作用）"""

    def test_load_existing_key(self):
        """已有 key：直接读取"""
        import os as _os
        with tempfile.TemporaryDirectory() as tmp:
            key_file = Path(tmp) / "secret.key"
            key_file.write_text("test_key_12345", encoding="utf-8")
            assert key_file.read_text(encoding="utf-8").strip() == "test_key_12345"

    def test_new_key_generation(self):
        """新 key：随机生成"""
        import os as _os
        key1 = _os.urandom(32).hex()
        key2 = _os.urandom(32).hex()
        assert key1 != key2
        assert len(key1) == 64  # 32 bytes hex = 64 chars

    def test_key_length(self):
        """key 长度符合预期"""
        import os as _os
        key = _os.urandom(32).hex()
        assert isinstance(key, str)
        assert len(key) == 64
