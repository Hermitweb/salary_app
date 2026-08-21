"""
TrayIcon: 系统托盘图标（pystray 封装）
"""
import threading

from .constants import HAS_TRAY

if HAS_TRAY:
    from pystray import MenuItem as item, Menu, Icon
    from PIL import Image, ImageDraw


class TrayIcon:
    def __init__(self, app):
        self.app = app
        self.icon = None
        if HAS_TRAY:
            self._create_tray()

    def _create_image(self, color):
        img = Image.new("RGB", (64, 64), color="white")
        draw = ImageDraw.Draw(img)
        draw.ellipse((8, 8, 56, 56), fill=color)
        draw.text((22, 24), "¥", fill="white")
        return img

    def _create_tray(self):
        menu = Menu(
            item("显示面板", self._show_window, default=True),
            item("启动服务", self._start_service),
            item("停止服务", self._stop_service),
            item("退出", self._quit_app)
        )
        self.icon = Icon(
            "SalarySystem",
            self._create_image("#3b82f6"),
            title="员工工资查询系统",
            menu=menu
        )
        threading.Thread(target=self.icon.run, daemon=True).start()

    def _show_window(self, icon=None, item=None):
        self.app.root.after(0, self._restore_window)

    def _restore_window(self):
        self.app.root.deiconify()
        self.app.root.lift()
        self.app.root.focus_force()

    def _start_service(self, icon=None, item=None):
        self.app.root.after(0, self.app._start_service)

    def _stop_service(self, icon=None, item=None):
        self.app.root.after(0, self.app._stop_service)

    def _quit_app(self, icon=None, item=None):
        self.app.root.after(0, self._quit)

    def _quit(self):
        # P2-20 托盘退出确认：服务运行中直接退出会停掉所有人的查询
        running = False
        try:
            running = self.app.service.check_status()
        except Exception:
            pass
        if running:
            from tkinter import messagebox
            if not messagebox.askyesno(
                "退出确认",
                "工资查询服务正在运行，退出后员工将无法查询。\n\n确定要退出吗？",
            ):
                return
        try:
            self.app.service.stop()
        except Exception:
            pass
        self.app.root.destroy()
        if self.icon:
            self.icon.stop()

    def stop(self):
        if self.icon:
            self.icon.stop()
