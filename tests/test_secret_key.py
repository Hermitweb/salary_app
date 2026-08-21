"""测试 secret_key 持久化逻辑（真实实现：app._load_or_create_secret_key）"""
import os
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

import app as app_mod


class TestSecretKeyPersistence:
    """验证 secret_key 的加载/生成/持久化（调用真实实现）"""

    def test_creates_key_file_when_missing(self, monkeypatch, tmp_path):
        """首次启动：生成随机 key 并落盘"""
        key_file = tmp_path / "secret.key"
        monkeypatch.setattr(app_mod, "_SECRET_KEY_FILE", key_file)
        key = app_mod._load_or_create_secret_key()
        assert key_file.exists(), "密钥文件应被创建"
        assert key == key_file.read_text(encoding="utf-8").strip()
        assert len(key) == 64  # 32 bytes hex

    def test_reuses_existing_key(self, monkeypatch, tmp_path):
        """已有 key：直接读取，不重新生成"""
        key_file = tmp_path / "secret.key"
        key_file.write_text("existing_key_abcdef", encoding="utf-8")
        monkeypatch.setattr(app_mod, "_SECRET_KEY_FILE", key_file)
        key = app_mod._load_or_create_secret_key()
        assert key == "existing_key_abcdef"

    def test_generated_keys_are_random(self, monkeypatch, tmp_path):
        """两次生成（不同目录）应得到不同的随机 key"""
        keys = []
        for sub in ("a", "b"):
            d = tmp_path / sub
            d.mkdir()
            monkeypatch.setattr(app_mod, "_SECRET_KEY_FILE", d / "secret.key")
            keys.append(app_mod._load_or_create_secret_key())
        assert keys[0] != keys[1]

    def test_readonly_dir_returns_ephemeral_key(self, monkeypatch, tmp_path):
        """写入失败（只读目录）时仍返回可用密钥"""
        ro = tmp_path / "ro"
        ro.mkdir()
        os.chmod(ro, 0o500)  # 只读
        try:
            monkeypatch.setattr(app_mod, "_SECRET_KEY_FILE", ro / "secret.key")
            key = app_mod._load_or_create_secret_key()
            assert isinstance(key, str) and len(key) >= 32
        finally:
            os.chmod(ro, 0o700)
