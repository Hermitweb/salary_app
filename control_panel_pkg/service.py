"""
ServiceManager: 服务管理器（HTTP + HTTPS + 看门狗 + 自签名证书）
"""
import os
import shutil
import socket
import ssl
import subprocess
import sys
import threading
import time
from pathlib import Path
from werkzeug.serving import make_server

from .constants import BASE_DIR, DATA_DIR, DEFAULT_PORT


class ServiceManager:
    """服务管理器：启动/停止/保活（同时支持 HTTP + HTTPS）"""

    # 保活参数
    _WATCHDOG_INTERVAL = 5      # 看门狗检测间隔（秒）
    _MAX_RESTART_COUNT = 5      # 最大自动重启次数（0=无限）
    _RESTART_DELAY = 3          # 重启前等待时间（秒）

    def __init__(self):
        self.flask_thread = None
        self.flask_app = None
        self.is_running = False
        self.port = self._load_port()
        self._server = None
        # HTTPS 相关
        self._https_server = None
        self._https_thread = None
        self.https_port = self.port + 1
        self.https_enabled = False
        # 保活相关
        self._watchdog_thread = None
        self._wd_stop_event = threading.Event()
        self._wd_generation = 0  # 世代计数：每次 _start_watchdog 递增，旧看门狗据此识别自己已过期
        self._restart_count = 0
        self._last_error = None
        self._auto_restart_enabled = True
        self._manual_stopping = False
        self._server_id = 0
        # 期望运行状态：start() 设 True，stop() 设 False
        # 线程 finally 不再修改 is_running，由看门狗根据此标志判断是否要重启
        self._expected_running = False

    def _load_port(self):
        from .constants import PORT_CONFIG
        try:
            if PORT_CONFIG.exists():
                with open(PORT_CONFIG, "r", encoding="utf-8") as f:
                    port_str = f.read().strip()
                    if port_str and port_str.isdigit():
                        port = int(port_str)
                        if 1 <= port <= 65535:
                            return port
        except Exception:
            pass
        return DEFAULT_PORT

    def _save_port(self, port):
        from .constants import PORT_CONFIG
        try:
            with open(PORT_CONFIG, "w", encoding="utf-8") as f:
                f.write(str(port))
            return True
        except Exception:
            return False

    def set_port(self, port):
        port = int(port)
        if port < 1 or port > 65534:
            return False, "端口号必须在 1-65534 之间（需要预留 HTTPS 端口 +1）"
        if self.is_running:
            return False, "请先停止服务再修改端口"
        self.port = port
        self.https_port = port + 1
        self._save_port(port)
        return True, f"端口已设置为 {port}"

    def start(self, force=False):
        """启动服务（同时启动 HTTP 和 HTTPS）"""
        if self.is_running:
            return False, "服务已在运行中"

        if not force and self._manual_stopping:
            return False, "服务已被手动停止，不再自动重启"

        if not self._bind_port_with_retry():
            return False, f"端口 {self.port} 被占用，请稍后重试或更换端口"

        try:
            sys.path.insert(0, str(BASE_DIR))
            from app import app as flask_app

            self.flask_app = flask_app

            # === 1. 启动 HTTP 服务 ===
            self._server = make_server("0.0.0.0", self.port, self.flask_app, threaded=True)

            # === 2. 启动 HTTPS 服务（端口+1）===
            self.https_enabled = False
            self._https_server = None
            self._https_thread = None
            https_port = self.port + 1
            if https_port <= 65535:
                ssl_ctx = self._create_ssl_context()
                if ssl_ctx is not None:
                    try:
                        self._https_server = make_server(
                            "0.0.0.0", https_port, self.flask_app,
                            threaded=True, ssl_context=ssl_ctx,
                        )
                        self.https_enabled = True
                        self.https_port = https_port
                    except Exception:
                        self._https_server = None

            if force:
                self._manual_stopping = False
                self._restart_count = 0
                self._last_error = None
            # 注意：自动重启（force=False）由 watchdog_loop 负责增加 _restart_count
            # 手动启动（force=True）由这里重置 _restart_count

            self._server_id += 1
            server_id = self._server_id
            self._expected_running = True

            def run_server():
                try:
                    self._server.serve_forever()
                except Exception:
                    pass
                finally:
                    # 线程退出时只清理资源引用，不再修改 is_running
                    # 真正该由 stop() 或 watchdog 决定 is_running
                    if self._server_id == server_id:
                        self._server = None
                        self._https_server = None
                        self.flask_app = None
                        self.flask_thread = None
                        self._https_thread = None

            self.flask_thread = threading.Thread(target=run_server, daemon=True)
            self.flask_thread.start()

            if self._https_server is not None:
                def run_https_server():
                    try:
                        self._https_server.serve_forever()
                    except Exception:
                        pass
                    finally:
                        # 清理 HTTPS 资源引用
                        if self._server_id == server_id:
                            self._https_server = None
                            self._https_thread = None
                self._https_thread = threading.Thread(
                    target=run_https_server, daemon=True
                )
                self._https_thread.start()

            self.is_running = True
            self._start_watchdog()
            if self.https_enabled:
                return True, f"服务已启动（HTTP {self.port} + HTTPS {self.https_port}）"
            return True, "服务已启动（HTTP）"
        except Exception as e:
            self.is_running = False
            self._server = None
            self._https_server = None
            self._https_thread = None
            self.flask_app = None
            self.flask_thread = None
            return False, f"启动失败: {e}"

    def stop(self):
        """手动停止服务"""
        if not self.is_running:
            return False, "服务未运行"

        # 1. 标记期望停止 + 手动停止，看门狗据此不再触发重启
        self._manual_stopping = True
        self._expected_running = False

        # 2. 停止看门狗
        self._stop_watchdog()

        # 3. 直接关闭底层 socket，让 serve_forever() 退出
        srv = self._server
        self._server = None
        if srv is not None:
            self._close_server_socket(srv)

        https_srv = self._https_server
        self._https_server = None
        if https_srv is not None:
            self._close_server_socket(https_srv)

        # 4. 等待工作线程退出
        worker = self.flask_thread
        if worker is not None and worker.is_alive():
            worker.join(timeout=5)
        https_worker = self._https_thread
        if https_worker is not None and https_worker.is_alive():
            https_worker.join(timeout=3)
        self._server_id += 1
        self.flask_thread = None
        self._https_thread = None
        self._https_server = None
        self.flask_app = None
        self.https_enabled = False
        self.is_running = False
        return True, "服务已停止"

    @staticmethod
    def _close_server_socket(srv):
        try:
            srv.socket.close()
        except Exception:
            pass
        try:
            srv.server_close()
        except Exception:
            pass

    def _bind_port_with_retry(self, max_retries=3, delay=0.5):
        """尝试绑定端口，失败则重试（应对 TIME_WAIT）"""
        for i in range(max_retries):
            sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            try:
                sock.bind(("0.0.0.0", self.port))
                sock.close()
                return True
            except OSError:
                sock.close()
                if i < max_retries - 1:
                    time.sleep(delay)
        return False

    def _start_watchdog(self):
        self._wd_stop_event.clear()
        self._wd_generation += 1
        self._watchdog_thread = threading.Thread(
            target=self._watchdog_loop,
            args=(self._wd_generation,),
            daemon=True,
        )
        self._watchdog_thread.start()

    def _stop_watchdog(self):
        self._wd_stop_event.set()
        # 递增世代，让所有旧看门狗在下一次 wait 返回时识别自己已过期
        self._wd_generation += 1
        wd = self._watchdog_thread
        if wd is not None and wd.is_alive():
            wd.join(timeout=2)
        self._watchdog_thread = None

    def _watchdog_loop(self, my_generation):
        """看门狗主循环：同时监控 HTTP 和 HTTPS 线程

        my_generation: 启动时的世代号。如果不再匹配 self._wd_generation，
                       说明本看门狗已被新看门狗取代，应立即退出。
        """
        while not self._wd_stop_event.is_set():
            if my_generation != self._wd_generation:
                # 已被新看门狗取代
                return
            self._wd_stop_event.wait(self._WATCHDOG_INTERVAL)
            if self._wd_stop_event.is_set():
                break
            if my_generation != self._wd_generation:
                return

            # 主动停止中，不做检测
            if self._manual_stopping or not self._expected_running:
                continue

            # 检测 HTTP 线程
            http_dead = (
                self.flask_thread is None or not self.flask_thread.is_alive()
            )
            # 检测 HTTPS 线程
            https_dead = False
            if self.https_enabled:
                https_dead = (
                    self._https_thread is None or not self._https_thread.is_alive()
                )

            crashed = http_dead or https_dead
            if not crashed:
                continue

            # 标记为停止，由 UI/外部感知
            self.is_running = False
            if http_dead:
                self._last_error = "HTTP 进程意外退出"
            else:
                self._last_error = "HTTPS 进程意外退出"
            print(f"[保活] 检测到 {self._last_error}")

            if not self._auto_restart_enabled:
                continue

            # 在 watchdog_loop 中递增计数（避免首次手动 start 被算作 1 次）
            self._restart_count += 1
            max_retries = self._MAX_RESTART_COUNT
            if max_retries != 0 and self._restart_count > max_retries:
                print(f"[保活] 已达最大重启次数({max_retries})，停止自动重启")
                continue

            # 重启前等待
            self._wd_stop_event.wait(self._RESTART_DELAY)
            if (
                self._wd_stop_event.is_set()
                or self._manual_stopping
                or not self._expected_running
                or my_generation != self._wd_generation
            ):
                continue

            ok, msg = self.start(force=False)
            if ok:
                print(f"[保活] 服务已自动重启 (第{self._restart_count}次)")
                # 重启期间被请求停止？立即关掉新启的服务
                if self._manual_stopping or not self._expected_running:
                    print(f"[保活] 重启期间被请求停止，立即停止新服务")
                    self.stop()
            else:
                print(f"[保活] 自动重启失败: {msg}")

    def check_status(self):
        """检查并修正 is_running 状态（UI 定时调用）"""
        if not self._expected_running:
            return False
        worker = self.flask_thread
        if worker is None or not worker.is_alive():
            return False
        if self.https_enabled:
            https_worker = self._https_thread
            if https_worker is None or not https_worker.is_alive():
                return False
        # 同步 is_running
        self.is_running = True
        return True

    def get_local_ip(self):
        """#8 使用 UDP socket 技巧获取 LAN IP（比 gethostbyname 准确）"""
        try:
            s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
            s.connect(("8.8.8.8", 80))
            ip = s.getsockname()[0]
            s.close()
            return ip
        except Exception:
            try:
                return socket.gethostbyname(socket.gethostname())
            except Exception:
                return "127.0.0.1"

    # ========== HTTPS 自签名证书相关 ==========
    def _cert_dir(self):
        return DATA_DIR / "certs"

    def _cert_file(self):
        return self._cert_dir() / "server.crt"

    def _key_file(self):
        return self._cert_dir() / "server.key"

    def _find_openssl(self):
        op = shutil.which("openssl")
        if op:
            return op
        for p in [
            r"C:\Program Files\Git\usr\bin\openssl.exe",
            r"C:\Program Files (x86)\Git\usr\bin\openssl.exe",
            r"C:\Program Files\OpenSSL-Win64\bin\openssl.exe",
            r"C:\Program Files (x86)\OpenSSL-Win32\bin\openssl.exe",
            r"C:\Windows\System32\openssl.exe",
        ]:
            if Path(p).exists():
                return p
        return None

    def _ensure_ssl_cert(self):
        """生成自签名证书（如已存在则复用）"""
        cf = self._cert_file()
        kf = self._key_file()
        if cf.exists() and kf.exists():
            return True
        try:
            self._cert_dir().mkdir(parents=True, exist_ok=True)
        except Exception:
            return False

        openssl = self._find_openssl()
        if not openssl:
            return False

        san_parts = ["DNS:localhost", "IP:127.0.0.1"]
        lan_ip = self.get_local_ip()
        if lan_ip and not lan_ip.startswith("127."):
            san_parts.append(f"IP:{lan_ip}")
        san_line = ",".join(san_parts)

        env = os.environ.copy()
        env.pop("OPENSSL_CONF", None)

        null_cfg = "NUL" if sys.platform == "win32" else "/dev/null"

        try:
            cmd = [
                openssl, "req", "-x509", "-newkey", "rsa:2048",
                "-keyout", str(kf), "-out", str(cf),
                "-days", "3650", "-nodes",
                "-subj", "/CN=SalarySystem Local",
                "-config", null_cfg,
                "-addext", f"subjectAltName={san_line}",
            ]
            r = subprocess.run(
                cmd, capture_output=True, text=True, timeout=30, env=env
            )
            if r.returncode != 0:
                cmd2 = [c for c in cmd if c not in ("-config", null_cfg)]
                r = subprocess.run(
                    cmd2, capture_output=True, text=True, timeout=30, env=env
                )
            return r.returncode == 0 and cf.exists() and kf.exists()
        except Exception:
            return False

    def _create_ssl_context(self):
        if not self._ensure_ssl_cert():
            return None
        try:
            ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            ctx.load_cert_chain(str(self._cert_file()), str(self._key_file()))
            return ctx
        except Exception:
            return None
