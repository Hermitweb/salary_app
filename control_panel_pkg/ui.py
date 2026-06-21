"""
ControlPanel: 主界面 + 事件处理
负责构建 UI（服务管理 / 字段配置 两个标签页）并响应用户操作
"""
import os
import sys
import tkinter as tk
from tkinter import ttk, messagebox


from .constants import (
    HAS_TRAY, DATA_DIR, _get_data_dir,
    _acquire_lock, _release_lock,
)
from .service import ServiceManager
from .field_config import FieldConfig, FieldItem
from .tray import TrayIcon


class ControlPanel:
    def __init__(self, root):
        self.root = root
        self.service = ServiceManager()
        self.field_config = FieldConfig()
        self.tray = None

        self.root.title("员工工资查询系统 - 控制面板")
        # #13 自适应屏幕分辨率：根据屏幕尺寸调整窗口（高 DPI 占 70%）
        try:
            screen_w = self.root.winfo_screenwidth()
            screen_h = self.root.winfo_screenheight()
            # 桌面缩放因子（DPI 缩放）
            try:
                dpi_scale = self.root.winfo_fpixels("1i") / 96.0
            except tk.TclError:
                dpi_scale = 1.0
            win_w = min(820, int(screen_w * 0.55 / dpi_scale))
            win_h = min(740, int(screen_h * 0.78 / dpi_scale))
            self.root.geometry(f"{win_w}x{win_h}")
            self.root.minsize(720, 600)
        except tk.TclError:
            # 极端情况下 fallback 到固定值
            self.root.geometry("760x680")
            self.root.minsize(720, 600)
        self.root.configure(bg="#f5f7fa")

        # #确保窗口从 VBS / 启动.vbs 启动时也能立即显示并置顶
        self.root.lift()
        self.root.attributes("-topmost", True)
        self.root.after(300, lambda: self.root.attributes("-topmost", False))

        self.root.protocol("WM_DELETE_WINDOW", self._on_close)
        self.root.bind("<Unmap>", self._on_minimize)
        self.root.bind("<Control-m>", lambda e: self._hide_to_tray())
        self.root.bind("<Control-M>", lambda e: self._hide_to_tray())

        self._build_ui()
        self._refresh_field_tags()
        self._update_status()
        self._start_status_check()

        if HAS_TRAY:
            self.tray = TrayIcon(self)
        else:
            # #16 pystray 不可用时显示一次性提示
            self.root.after(
                500,
                lambda: messagebox.showinfo(
                    "托盘功能不可用",
                    "pystray 库未安装或不可用。\n\n"
                    "将无法最小化到系统托盘，关闭窗口时会询问是否退出。\n"
                    "如需托盘功能，请运行：pip install pystray pillow pywin32"
                ),
            )

    # ========== 窗口事件 ==========
    def _show_close_prompt(self, service_running):
        """关闭提示框：3 按钮 - 隐藏到托盘 / 退出 / 取消

        返回："hide" / "exit" / "cancel"
        """
        dlg = tk.Toplevel(self.root)
        dlg.title("关闭")
        dlg.transient(self.root)
        dlg.resizable(False, False)
        dlg.configure(bg="#f5f7fa")
        dlg.attributes("-topmost", True)
        dlg.after(200, lambda: dlg.attributes("-topmost", False))

        result = {"choice": "cancel"}

        # 图标 + 标题
        header = tk.Frame(dlg, bg="#1a2332", height=56)
        header.pack(fill="x", side="top")
        header.pack_propagate(False)
        tk.Label(
            header, text="✕", bg="#1a2332", fg="#ef4444",
            font=("Microsoft YaHei", 18, "bold"),
        ).pack(side="left", padx=(18, 8), pady=10)
        tk.Label(
            header, text="关闭工资查询系统", bg="#1a2332", fg="white",
            font=("Microsoft YaHei", 11, "bold"),
        ).pack(side="left", pady=10)

        # 内容
        body = tk.Frame(dlg, bg="#f5f7fa")
        body.pack(fill="both", expand=True, padx=20, pady=(18, 8))

        if service_running:
            warn = (
                "服务正在运行中。\n\n"
                "• 选择「隐藏到托盘」：服务继续在后台运行\n"
                "• 选择「退出」：会先停止服务再退出程序"
            )
        else:
            warn = (
                "选择操作：\n\n"
                "• 「隐藏到托盘」：程序在后台继续运行\n"
                "• 「退出」：完全关闭程序"
            )

        tk.Label(
            body, text=warn, bg="#f5f7fa", fg="#1a2332",
            font=("Microsoft YaHei", 9), justify="left", anchor="w",
        ).pack(fill="x")

        # 按钮区
        btn_frame = tk.Frame(dlg, bg="#f5f7fa")
        btn_frame.pack(fill="x", padx=20, pady=(8, 18))

        def on_hide():
            result["choice"] = "hide"
            dlg.destroy()

        def on_exit():
            result["choice"] = "exit"
            dlg.destroy()

        def on_cancel():
            result["choice"] = "cancel"
            dlg.destroy()

        # 隐藏到托盘（主推荐按钮）
        tk.Button(
            btn_frame, text="  隐藏到托盘  ", command=on_hide,
            bg="#3b82f6", fg="white", activebackground="#2563eb",
            relief="flat", font=("Microsoft YaHei", 9, "bold"),
            cursor="hand2", padx=4, pady=6,
        ).pack(side="left", padx=(0, 6))

        # 退出（红色警示色）
        exit_btn = tk.Button(
            btn_frame, text="  退  出  ", command=on_exit,
            bg="#ef4444", fg="white", activebackground="#dc2626",
            relief="flat", font=("Microsoft YaHei", 9),
            cursor="hand2", padx=4, pady=6,
        )
        exit_btn.pack(side="left", padx=(0, 6))

        # 取消
        tk.Button(
            btn_frame, text="  取  消  ", command=on_cancel,
            bg="#e5e7eb", fg="#1a2332", activebackground="#d1d5db",
            relief="flat", font=("Microsoft YaHei", 9),
            cursor="hand2", padx=4, pady=6,
        ).pack(side="right")

        # 快捷键
        dlg.bind("<Escape>", lambda e: on_cancel())
        dlg.bind("<Return>", lambda e: on_hide())
        dlg.bind("<Control-q>", lambda e: on_exit())

        # 居中
        dlg.update_idletasks()
        w, h = 420, 240 if service_running else 200
        x = self.root.winfo_rootx() + (self.root.winfo_width() - w) // 2
        y = self.root.winfo_rooty() + (self.root.winfo_height() - h) // 2
        dlg.geometry(f"{w}x{h}+{x}+{y}")

        dlg.grab_set()
        dlg.protocol("WM_DELETE_WINDOW", on_cancel)
        self.root.wait_window(dlg)
        return result["choice"]

    def _on_close(self):
        # #18 检测服务是否在运行（带异常保护）
        try:
            service_running = self.service.check_status()
            choice = self._show_close_prompt(service_running)
        except Exception as e:
            messagebox.showerror("错误", f"退出时发生异常: {e}")
            return

        if choice == "cancel":
            return

        if choice == "hide":
            # 隐藏到托盘，服务继续运行
            self._hide_to_tray()
            return

        # choice == "exit"：先停止服务再退出
        try:
            if service_running:
                self.service.stop()
                self._update_status()
        except Exception as e:
            messagebox.showerror("错误", f"停止服务时发生异常: {e}")
            return
        # 关闭托盘图标
        if self.tray:
            self.tray.stop()
        _release_lock()
        self.root.destroy()

    def _on_minimize(self, event):
        if self.root.state() == "iconic":
            self.root.after(100, self._hide_to_tray)

    def _hide_to_tray(self):
        if self.tray:
            self.root.withdraw()
        else:
            self.root.iconify()  # #16 托盘不可用时退化为普通最小化

    def _open_data_folder(self):
        """#22 打开数据目录（在资源管理器中定位 data 文件夹）"""
        try:
            data_dir = str(_get_data_dir())
            if not os.path.exists(data_dir):
                messagebox.showwarning("目录不存在", f"数据目录不存在：\n{data_dir}")
                return
            # Windows：用 explorer.exe 打开
            if sys.platform == "win32":
                os.startfile(data_dir)
            elif sys.platform == "darwin":
                os.system(f'open "{data_dir}"')
            else:
                os.system(f'xdg-open "{data_dir}"')
        except Exception as e:
            messagebox.showerror("打开失败", f"无法打开数据目录：\n{e}")

    def show_window(self):
        self.root.deiconify()
        self.root.lift()
        self.root.focus_force()

    # ========== UI 构建 ==========
    def _build_ui(self):
        title_frame = tk.Frame(self.root, bg="#1a2332", height=60)
        title_frame.pack(fill="x", side="top")
        title_frame.pack_propagate(False)

        tk.Label(
            title_frame,
            text="员工工资查询系统",
            bg="#1a2332",
            fg="white",
            font=("Microsoft YaHei", 14, "bold")
        ).pack(side="left", padx=20, pady=15)

        tk.Label(
            title_frame,
            text="控制面板",
            bg="#1a2332",
            fg="#9ca3af",
            font=("Microsoft YaHei", 10)
        ).pack(side="left", pady=18)

        notebook = ttk.Notebook(self.root)
        notebook.pack(fill="both", expand=True, padx=12, pady=12)

        service_tab = tk.Frame(notebook, bg="#f5f7fa")
        notebook.add(service_tab, text="  服务管理  ")
        self._build_service_tab(service_tab)

        fields_tab = tk.Frame(notebook, bg="#f5f7fa")
        notebook.add(fields_tab, text="  字段配置  ")
        self._build_fields_tab(fields_tab)

    def _build_service_tab(self, parent):
        card = tk.Frame(parent, bg="white", highlightbackground="#e8ecf1", highlightthickness=1)
        card.pack(fill="x", padx=16, pady=(12, 6))

        tk.Label(
            card, text="服务状态", bg="white", fg="#1a2332",
            font=("Microsoft YaHei", 10, "bold")
        ).pack(anchor="w", padx=16, pady=(8, 4))

        self.status_frame = tk.Frame(card, bg="white")
        self.status_frame.pack(fill="x", padx=16, pady=(0, 8))

        self.status_indicator = tk.Label(
            self.status_frame, text="●", bg="white", fg="#9ca3af",
            font=("Microsoft YaHei", 12)
        )
        self.status_indicator.pack(side="left")

        self.status_label = tk.Label(
            self.status_frame, text="已停止", bg="white", fg="#6b7280",
            font=("Microsoft YaHei", 10)
        )
        self.status_label.pack(side="left", padx=(6, 0))

        port_card = tk.Frame(parent, bg="white", highlightbackground="#e8ecf1", highlightthickness=1)
        port_card.pack(fill="x", padx=16, pady=6)

        tk.Label(
            port_card, text="端口设置", bg="white", fg="#1a2332",
            font=("Microsoft YaHei", 10, "bold")
        ).pack(anchor="w", padx=16, pady=(8, 4))

        port_frame = tk.Frame(port_card, bg="white")
        port_frame.pack(fill="x", padx=16, pady=(0, 4))

        tk.Label(
            port_frame, text="服务端口:", bg="white", fg="#5a6a7a",
            font=("Microsoft YaHei", 9)
        ).pack(side="left")

        self.port_var = tk.StringVar(value=str(self.service.port))
        self.port_entry = tk.Entry(
            port_frame, textvariable=self.port_var, width=8,
            font=("Microsoft YaHei", 9), justify="center"
        )
        self.port_entry.pack(side="left", padx=(6, 6))

        self.save_port_btn = tk.Button(
            port_frame, text="保存端口", command=self._save_port,
            bg="#10b981", fg="white", activebackground="#059669",
            relief="flat", font=("Microsoft YaHei", 9),
            width=8, cursor="hand2"
        )
        self.save_port_btn.pack(side="left", padx=6)

        # 端口提示换行显示，节省水平空间
        self.port_hint = tk.Label(
            port_card, text="建议使用 1024-65534 之间的端口（HTTPS = 端口+1）",
            bg="white", fg="#9ca3af",
            font=("Microsoft YaHei", 8)
        )
        self.port_hint.pack(anchor="w", padx=16, pady=(0, 8))

        btn_frame = tk.Frame(parent, bg="white", highlightbackground="#e8ecf1", highlightthickness=1)
        btn_frame.pack(fill="x", padx=16, pady=6)

        tk.Label(
            btn_frame, text="操作", bg="white", fg="#1a2332",
            font=("Microsoft YaHei", 10, "bold")
        ).pack(anchor="w", padx=16, pady=(8, 4))

        self.start_btn = tk.Button(
            btn_frame, text="▶ 启动服务", command=self._start_service,
            bg="#3b82f6", fg="white", activebackground="#2563eb",
            relief="flat", font=("Microsoft YaHei", 9),
            width=14, height=1, cursor="hand2"
        )
        self.start_btn.pack(side="left", padx=(16, 6), pady=(0, 8))

        self.stop_btn = tk.Button(
            btn_frame, text="■ 停止服务", command=self._stop_service,
            bg="#9ca3af", fg="white", activebackground="#6b7280",
            relief="flat", font=("Microsoft YaHei", 9),
            width=14, height=1, cursor="hand2",
            state="disabled"
        )
        self.stop_btn.pack(side="left", padx=6, pady=(0, 8))

        self.min_btn = tk.Button(
            btn_frame, text="— 最小化到托盘", command=self._hide_to_tray,
            bg="#6b7280", fg="white", activebackground="#4b5563",
            relief="flat", font=("Microsoft YaHei", 9),
            width=14, height=1, cursor="hand2"
        )
        self.min_btn.pack(side="left", padx=6, pady=(0, 8))

        # #22 打开数据目录按钮
        self.open_folder_btn = tk.Button(
            btn_frame, text="📁 数据目录", command=self._open_data_folder,
            bg="#6b7280", fg="white", activebackground="#4b5563",
            relief="flat", font=("Microsoft YaHei", 9),
            width=12, height=1, cursor="hand2"
        )
        self.open_folder_btn.pack(side="left", padx=6, pady=(0, 8))

        info_frame = tk.Frame(parent, bg="white", highlightbackground="#e8ecf1", highlightthickness=1)
        info_frame.pack(fill="both", expand=True, padx=16, pady=(6, 12))

        tk.Label(
            info_frame, text="访问信息", bg="white", fg="#1a2332",
            font=("Microsoft YaHei", 10, "bold")
        ).pack(anchor="w", padx=16, pady=(8, 4))

        # 访问信息区域（Canvas + 滚动条，支持滚轮滑动）
        scroll_outer = tk.Frame(info_frame, bg="white")
        scroll_outer.pack(fill="both", expand=True, padx=16, pady=(0, 16))

        self.info_canvas = tk.Canvas(
            scroll_outer, bg="white", highlightthickness=0,
            height=340, bd=0,
        )
        self.info_scrollbar = ttk.Scrollbar(
            scroll_outer, orient="vertical", command=self.info_canvas.yview
        )
        self.info_scrollable_frame = tk.Frame(self.info_canvas, bg="white")

        # 把内部 frame 嵌入 canvas
        self._info_canvas_window = self.info_canvas.create_window(
            (0, 0), window=self.info_scrollable_frame, anchor="nw"
        )

        # 内部 frame 大小变化时更新 scrollregion
        self.info_scrollable_frame.bind(
            "<Configure>",
            lambda e: self.info_canvas.configure(
                scrollregion=self.info_canvas.bbox("all")
            )
        )

        # canvas 大小变化时让内部 frame 跟随宽度
        self.info_canvas.bind(
            "<Configure>",
            lambda e: self.info_canvas.itemconfig(
                self._info_canvas_window, width=e.width
            )
        )

        self.info_canvas.configure(yscrollcommand=self.info_scrollbar.set)
        self.info_canvas.pack(side="left", fill="both", expand=True)
        self.info_scrollbar.pack(side="right", fill="y")

        # 滚轮事件绑定（全局 + 区域判断，避免干扰其他控件）
        self.root.bind_all("<MouseWheel>", self._on_mousewheel)
        self.root.bind_all("<Button-4>", self._on_mousewheel)   # Linux
        self.root.bind_all("<Button-5>", self._on_mousewheel)   # Linux

        # 用可滚动 frame 替代原来的 info_container，后续添加的子控件都放这里
        self.info_container = self.info_scrollable_frame

        self.info_text = tk.Text(
            self.info_container, height=6, bg="#fafbfc", fg="#374151",
            relief="flat", font=("Consolas", 10),
            padx=12, pady=8, wrap="word"
        )
        self.info_text.pack(fill="x", padx=4, pady=(0, 4))
        self.info_text.insert("1.0", "等待启动服务...")
        self.info_text.config(state="disabled")

    def _on_mousewheel(self, event):
        """鼠标滚轮事件：仅在访问信息区域内才滚动 Canvas"""
        # 找到当前鼠标位置下的组件
        try:
            x, y = self.root.winfo_pointerxy()
            widget = self.root.winfo_containing(x, y)
        except Exception:
            return
        if widget is None:
            return
        # 向上遍历父组件，判断鼠标是否在 info 区域
        w = widget
        in_info = False
        while w is not None:
            if w in (self.info_canvas, self.info_scrollable_frame):
                in_info = True
                break
            try:
                w = w.master
            except Exception:
                break
        if not in_info:
            return  # 不在 info 区域，不响应
        # 计算滚动量
        if event.num == 4:      # Linux up
            delta = -1
        elif event.num == 5:    # Linux down
            delta = 1
        else:                   # Windows / macOS
            delta = -1 if event.delta > 0 else 1
        self.info_canvas.yview_scroll(delta, "units")
        return "break"  # 阻止事件冒泡

    def _build_fields_tab(self, parent):
        desc_frame = tk.Frame(parent, bg="#f5f7fa")
        desc_frame.pack(fill="x", padx=16, pady=(16, 4))

        tk.Label(
            desc_frame,
            text="字段显示配置：",
            bg="#f5f7fa", fg="#5a6a7a",
            font=("Microsoft YaHei", 10, "bold"),
            anchor="w"
        ).pack(anchor="w")

        hint_frame = tk.Frame(parent, bg="#f5f7fa")
        hint_frame.pack(fill="x", padx=16, pady=(0, 8))

        tk.Label(
            hint_frame,
            text="● 显示 - 强制显示，即使值为 0/空    ● 隐藏 - 强制不显示    ● 全局 - 跟随全局规则（值为 0/空 时不显示）",
            bg="#f5f7fa", fg="#6b7280",
            font=("Microsoft YaHei", 9),
            anchor="w"
        ).pack(anchor="w")

        self._fields_canvas_frame = tk.Frame(parent, bg="#f5f7fa")
        self._fields_canvas_frame.pack(fill="both", expand=True, padx=16, pady=8)

        self._fields_canvas = tk.Canvas(
            self._fields_canvas_frame, bg="#f5f7fa",
            highlightthickness=0, bd=0, height=100
        )
        self._fields_vbar = ttk.Scrollbar(
            self._fields_canvas_frame, orient="vertical",
            command=self._fields_canvas.yview
        )
        self._fields_canvas.configure(
            yscrollcommand=self._fields_vbar.set,
        )

        self._fields_inner = tk.Frame(self._fields_canvas, bg="white")
        self._fields_canvas_window = self._fields_canvas.create_window(
            (0, 0), window=self._fields_inner, anchor="nw"
        )

        self._fields_inner.bind(
            "<Configure>",
            lambda e: self._fields_canvas.configure(scrollregion=self._fields_canvas.bbox("all"))
        )
        self._fields_canvas.bind(
            "<Configure>",
            self._on_fields_canvas_configure
        )
        self._fields_canvas.bind(
            "<Enter>",
            lambda e: self._fields_canvas.bind_all("<MouseWheel>", self._on_fields_mousewheel)
        )
        self._fields_canvas.bind(
            "<Leave>",
            lambda e: self._fields_canvas.unbind_all("<MouseWheel>")
        )

        self._fields_canvas.pack(side="left", fill="both", expand=True)
        self._fields_vbar.pack(side="right", fill="y")

        self._field_items = []

    def _on_fields_canvas_configure(self, event):
        # 内容变化时更新滚动区域即可
        self._fields_canvas.configure(scrollregion=self._fields_canvas.bbox("all"))

    def _on_fields_mousewheel(self, event):
        self._fields_canvas.yview_scroll(int(-1 * (event.delta / 120)), "units")

    def _refresh_field_tags(self):
        # #8 销毁旧控件改为 pack_forget + 清空列表，避免控件 ID 失效
        for item in self._field_items:
            try:
                item.lbl.pack_forget()
                item.rb_frame.pack_forget()
            except tk.TclError:
                pass
            try:
                item.lbl.destroy()
                item.rb_frame.destroy()
            except tk.TclError:
                pass
        self._field_items = []

        # 4 列布局：标签 | 单选框 | 标签 | 单选框
        self._fields_inner.grid_columnconfigure(0, weight=1, uniform="col")
        self._fields_inner.grid_columnconfigure(1, weight=0)
        self._fields_inner.grid_columnconfigure(2, weight=1, uniform="col")
        self._fields_inner.grid_columnconfigure(3, weight=0)

        fields = self.field_config.fields
        for i in range(0, len(fields), 2):
            row = i // 2

            # 左列
            item_l = FieldItem(self._fields_inner, fields[i],
                               self.field_config.get_state(fields[i]),
                               self._on_field_change)
            item_l.lbl.grid(row=row, column=0, sticky="w", padx=(4, 2), pady=3)
            item_l.rb_frame.grid(row=row, column=1, sticky="e", padx=(2, 8), pady=3)
            self._field_items.append(item_l)

            # 右列
            if i + 1 < len(fields):
                item_r = FieldItem(self._fields_inner, fields[i + 1],
                                   self.field_config.get_state(fields[i + 1]),
                                   self._on_field_change)
                item_r.lbl.grid(row=row, column=2, sticky="w", padx=(8, 2), pady=3)
                item_r.rb_frame.grid(row=row, column=3, sticky="e", padx=(2, 4), pady=3)
                self._field_items.append(item_r)

        self._fields_inner.update_idletasks()
        self._fields_canvas.configure(scrollregion=self._fields_canvas.bbox("all"))

    def _on_field_change(self, field, state):
        self.field_config.set_state(field, state)
        for item in self._field_items:
            if item.field == field:
                item.refresh(state)
                break

    # ========== 业务事件 ==========
    def _save_port(self):
        try:
            port_str = self.port_var.get().strip()
            try:
                port = int(port_str)
            except ValueError:
                messagebox.showerror("错误", "请输入有效的端口号（数字）")
                return
            if port < 1 or port > 65534:
                messagebox.showerror("错误", "端口号必须在 1-65534 之间（需要预留 HTTPS 端口 +1）")
                return
            # #18 修改端口时如果服务在跑，先停服务
            if self.service.check_status():
                if not messagebox.askyesno("提示", "修改端口需要先停止服务，是否继续？"):
                    return
                try:
                    success, msg = self.service.stop()
                except Exception as e:
                    messagebox.showerror("错误", f"停止服务时发生异常: {e}")
                    return
                if not success:
                    messagebox.showerror("错误", msg)
                    return
            # 如果新端口和当前端口相同，跳过
            if port == self.service.port:
                messagebox.showinfo("提示", f"端口已是 {port}，无需修改")
                return
            try:
                success, msg = self.service.set_port(port)
            except Exception as e:
                messagebox.showerror("错误", f"设置端口时发生异常: {e}")
                return
            if success:
                self.port_var.set(str(port))
                try:
                    self._update_status()
                except tk.TclError:
                    pass
                messagebox.showinfo("成功", f"端口已设置为 {port}（HTTPS 将使用 {port+1}）\n请点击「启动服务」按钮启动")
            else:
                messagebox.showerror("错误", msg)
        except Exception as e:
            messagebox.showerror("异常", f"保存端口时发生未预期错误: {e}")
            import traceback
            traceback.print_exc()

    def _start_service(self):
        # #18 异常保护：捕获未知错误，避免崩溃
        try:
            port_str = self.port_var.get().strip()
            try:
                port = int(port_str)
            except ValueError:
                messagebox.showerror("错误", "请输入有效的端口号（数字）")
                return
            if port < 1 or port > 65534:
                messagebox.showerror("错误", "端口号必须在 1-65534 之间（需要预留 HTTPS 端口 +1）")
                return
            # 同步端口（如果和 self.service.port 不一致）
            if port != self.service.port:
                self.service.port = port
                self.service.https_port = port + 1
                try:
                    self.service._save_port(port)
                except Exception as e:
                    messagebox.showerror("错误", f"保存端口失败: {e}")
                    return
            self.port_var.set(str(port))
            # 启动前更新一次状态（防止 UI 显示错误）
            try:
                self._update_status()
            except tk.TclError:
                pass
            # 启动服务
            try:
                success, msg = self.service.start(force=True)
            except Exception as e:
                import traceback
                traceback.print_exc()
                messagebox.showerror("启动异常", f"启动服务时发生异常: {e}")
                try:
                    self._update_status()
                except tk.TclError:
                    pass
                return
            try:
                self._update_status()
            except tk.TclError:
                pass
            if not success:
                messagebox.showerror("错误", msg)
        except Exception as e:
            import traceback
            traceback.print_exc()
            messagebox.showerror("异常", f"启动服务时发生未预期错误: {e}")
            try:
                self._update_status()
            except tk.TclError:
                pass

    def _stop_service(self):
        # #18 异常保护
        try:
            success, msg = self.service.stop()
        except Exception as e:
            messagebox.showerror("停止异常", f"发生未知错误: {e}")
            self._update_status()
            return
        if success:
            self._update_status()
        else:
            messagebox.showerror("错误", msg)

    def _update_status(self):
        is_running = self.service.check_status()
        if is_running:
            self.status_indicator.config(fg="#10b981")
            # 显示保活重启次数
            restart_info = ""
            if self.service._restart_count > 0:
                restart_info = f" (已自动恢复{self.service._restart_count}次)"
            self.status_label.config(text=f"运行中{restart_info}", fg="#10b981")
            self.start_btn.config(state="disabled", bg="#9ca3af", activebackground="#6b7280")
            self.stop_btn.config(state="normal", bg="#ef4444", activebackground="#dc2626")
            ip = self.service.get_local_ip()
            local_url = f"http://localhost:{self.service.port}"
            lan_url = f"http://{ip}:{self.service.port}"
            # 构建带复制按钮的访问信息（首次构建 / 后续只更新内容，避免闪烁）
            self._build_info_running(local_url, lan_url)
        else:
            self.status_indicator.config(fg="#9ca3af")
            status_text = "已停止"
            # 显示上次错误原因
            if self.service._last_error:
                status_text = f"已停止 ({self.service._last_error})"
            self.status_label.config(text=status_text, fg="#6b7280")
            self.start_btn.config(state="normal", bg="#3b82f6", activebackground="#2563eb")
            self.stop_btn.config(state="disabled", bg="#9ca3af", activebackground="#6b7280")
            # 重置构建标志，允许下次启动时重建 UI
            self._info_built = False
            # 先清理运行态创建的子组件（保留 info_text），再显示 info_text
            for widget in self.info_container.winfo_children():
                if widget != self.info_text:
                    try:
                        widget.destroy()
                    except tk.TclError:
                        pass
            try:
                # 确保 info_text 已 pack 显示（如果之前被 pack_forget）
                if not self.info_text.winfo_ismapped():
                    self.info_text.pack(fill="x")
                self.info_text.config(state="normal")
                self.info_text.delete("1.0", "end")
                self.info_text.insert(
                    "1.0",
                    f"当前端口: {self.service.port}\n\n服务未启动，请点击「启动服务」按钮启动\n\n提示：修改端口后需重新启动服务生效"
                )
                self.info_text.config(state="disabled")
            except tk.TclError:
                # 万一 info_text 已被销毁，重新创建
                self._recreate_info_text()

    def _recreate_info_text(self):
        """安全网：万一 info_text 已被销毁，重新创建一个"""
        try:
            self.info_text = tk.Text(
                self.info_container, height=6, bg="#fafbfc", fg="#374151",
                relief="flat", font=("Consolas", 10),
                padx=12, pady=8, wrap="word"
            )
            self.info_text.pack(fill="x")
            self.info_text.insert(
                "1.0",
                f"当前端口: {self.service.port}\n\n服务未启动，请点击「启动服务」按钮启动"
            )
            self.info_text.config(state="disabled")
        except tk.TclError:
            pass

    def _build_info_running(self, local_url, lan_url):
        """构建运行状态的访问信息（HTTP/HTTPS 左右分布 + 一键复制 + 网络信息 + 登录提示）
        首次构建完整 UI；后续只更新 URL 等可变内容，避免闪烁。
        """
        # #10 如果已经构建过访问信息区，只更新 URL 即可（避免每 3 秒重建导致闪烁）
        if getattr(self, "_info_built", False):
            self._update_info_urls(local_url, lan_url)
            return
        self._info_built = True
        # 初始化 URL 标签字典（用于后续更新）
        self._url_labels = {}

        # 先隐藏原始 info_text（不销毁，保留引用以便停止后复用）
        try:
            self.info_text.pack_forget()
        except tk.TclError:
            pass
        # 清空运行态上次创建的子组件
        for widget in self.info_container.winfo_children():
            if widget != self.info_text:
                try:
                    widget.destroy()
                except tk.TclError:
                    pass

        # HTTPS URL（如启用）
        local_https = ""
        lan_https = ""
        if self.service.https_enabled:
            local_https = f"https://localhost:{self.service.https_port}"
            lan_https = f"https://{self.service.get_local_ip()}:{self.service.https_port}"

        # === 一键复制所有信息按钮（置顶）===
        self.copy_all_btn = tk.Button(
            self.info_container,
            text="一键复制所有信息（IP链接+网络信息）",
            command=self._copy_all_info,
            bg="#3b82f6", fg="white",
            activebackground="#2563eb", activeforeground="white",
            relief="flat", cursor="hand2",
            font=("Microsoft YaHei", 10, "bold"),
            padx=16, pady=8,
        )
        self.copy_all_btn.pack(fill="x", pady=(4, 12), padx=4)

        def _make_row(parent, label_text, url, bg_color, key):
            """在指定 panel 内创建一行（标签 + URL + 复制按钮）"""
            row = tk.Frame(parent, bg=bg_color)
            row.pack(fill="x", pady=1, padx=4)
            tk.Label(row, text=label_text, bg=bg_color, fg="#6b7280",
                     font=("Microsoft YaHei", 8), width=6, anchor="w").pack(side="left")
            url_lbl = tk.Label(row, text=url, bg=bg_color, fg="#3b82f6",
                               font=("Consolas", 8), cursor="hand2",
                               wraplength=160, justify="left")
            url_lbl.pack(side="left", padx=(2, 4), fill="x", expand=True)
            btn = tk.Button(row, text="复制", font=("Microsoft YaHei", 7),
                            bg="#e5e7eb", fg="#374151", relief="flat",
                            cursor="hand2", padx=6,
                            command=lambda u=url: self._copy_url(u))
            btn.pack(side="right")
            # 保存引用以便后续更新
            self._url_labels[key + "_lbl"] = url_lbl
            self._url_labels[key + "_btn"] = btn
            return row

        # === HTTP 和 HTTPS 左右分布 ===
        panels_frame = tk.Frame(self.info_container, bg="#fafbfc")
        panels_frame.pack(fill="x", pady=(0, 0), padx=4)

        # HTTP 面板（左侧，蓝色调）
        http_panel = tk.Frame(panels_frame, bg="#eff6ff",
                              highlightbackground="#93c5fd", highlightthickness=1)
        http_panel.pack(side="left", fill="both", expand=True, padx=(0, 3))
        tk.Label(http_panel, text="HTTP", bg="#eff6ff", fg="#1e40af",
                 font=("Microsoft YaHei", 10, "bold"),
                 anchor="center").pack(fill="x", pady=(6, 4))
        _make_row(http_panel, "本机:", local_url, "#eff6ff", "http_local")
        _make_row(http_panel, "局域网:", lan_url, "#eff6ff", "http_lan")

        # HTTPS 面板（右侧，绿色调）
        if self.service.https_enabled:
            https_panel = tk.Frame(panels_frame, bg="#f0fdf4",
                                   highlightbackground="#86efac", highlightthickness=1)
            https_panel.pack(side="left", fill="both", expand=True, padx=(3, 0))
            tk.Label(https_panel, text="HTTPS", bg="#f0fdf4", fg="#166534",
                     font=("Microsoft YaHei", 10, "bold"),
                     anchor="center").pack(fill="x", pady=(6, 4))
            _make_row(https_panel, "本机:", local_https, "#f0fdf4", "https_local")
            _make_row(https_panel, "局域网:", lan_https, "#f0fdf4", "https_lan")
            # HTTPS 警告放在面板内
            tk.Label(https_panel,
                     text="⚠ 首次访问提示证书不受信任，请选「继续访问」",
                     bg="#f0fdf4", fg="#a16207",
                     font=("Microsoft YaHei", 7), anchor="w",
                     wraplength=180, justify="left").pack(fill="x", padx=6, pady=(4, 6))
        else:
            # HTTPS 未启用占位
            placeholder = tk.Frame(panels_frame, bg="#f3f4f6",
                                    highlightbackground="#d1d5db", highlightthickness=1)
            placeholder.pack(side="left", fill="both", expand=True, padx=(3, 0))
            tk.Label(placeholder, text="HTTPS", bg="#f3f4f6", fg="#9ca3af",
                     font=("Microsoft YaHei", 10, "bold"),
                     anchor="center").pack(fill="x", pady=(6, 4))
            tk.Label(placeholder, text="未启用", bg="#f3f4f6", fg="#9ca3af",
                     font=("Microsoft YaHei", 9)).pack(pady=10)

        # === 网络信息卡片 ===
        self._build_network_info(self.info_container)

        # === 登录提示卡片 ===
        self._build_login_hint(self.info_container)

        # 提示信息
        hint = "提示：其他设备需在同一WiFi/局域网内访问"
        if self.service._restart_count > 0:
            hint += f"\n[保活] 服务曾异常退出，已自动恢复"
        hint_label = tk.Label(self.info_container, text=hint,
                               bg="#fafbfc", fg="#9ca3af",
                               font=("Microsoft YaHei", 8),
                               justify="left", anchor="w")
        hint_label.pack(fill="x", pady=(4, 0))

    def _update_info_urls(self, local_url, lan_url):
        """只更新访问链接内容，不重建 UI（避免定时刷新导致的闪烁）"""
        if not hasattr(self, "_url_labels"):
            return
        # 更新 HTTP URL
        if "http_local_lbl" in self._url_labels:
            self._url_labels["http_local_lbl"].config(text=local_url)
            self._url_labels["http_local_btn"].config(
                command=lambda u=local_url: self._copy_url(u))
        if "http_lan_lbl" in self._url_labels:
            self._url_labels["http_lan_lbl"].config(text=lan_url)
            self._url_labels["http_lan_btn"].config(
                command=lambda u=lan_url: self._copy_url(u))
        # 更新 HTTPS URL（如启用）
        if self.service.https_enabled:
            local_https = f"https://localhost:{self.service.https_port}"
            lan_https = f"https://{self.service.get_local_ip()}:{self.service.https_port}"
            if "https_local_lbl" in self._url_labels:
                self._url_labels["https_local_lbl"].config(text=local_https)
                self._url_labels["https_local_btn"].config(
                    command=lambda u=local_https: self._copy_url(u))
            if "https_lan_lbl" in self._url_labels:
                self._url_labels["https_lan_lbl"].config(text=lan_https)
                self._url_labels["https_lan_btn"].config(
                    command=lambda u=lan_https: self._copy_url(u))

    def _build_login_hint(self, parent):
        """构建登录提示卡片（账号=工号, 密码=紧急联系人号码）"""
        login_frame = tk.Frame(parent, bg="#fef3c7",
                               highlightbackground="#fcd34d", highlightthickness=1)
        login_frame.pack(fill="x", pady=(8, 0), padx=4)
        inner = tk.Frame(login_frame, bg="#fef3c7")
        inner.pack(fill="x", padx=12, pady=8)
        # 标题行
        title_row = tk.Frame(inner, bg="#fef3c7")
        title_row.pack(fill="x", pady=(0, 6))
        tk.Label(title_row, text="●", bg="#fef3c7", fg="#d97706",
                 font=("Microsoft YaHei", 9)).pack(side="left", padx=(0, 4))
        tk.Label(title_row, text="登录提示", bg="#fef3c7", fg="#92400e",
                 font=("Microsoft YaHei", 9, "bold")).pack(side="left")
        # 账号/密码信息
        info_row = tk.Frame(inner, bg="#fef3c7")
        info_row.pack(fill="x")
        tk.Label(info_row, text="账号 = 工号", bg="#fef3c7", fg="#78350f",
                 font=("Microsoft YaHei", 9)).pack(side="left", padx=(0, 16))
        tk.Label(info_row, text="密码 = 紧急联系人号码", bg="#fef3c7", fg="#78350f",
                 font=("Microsoft YaHei", 9)).pack(side="left")

    def _build_network_info(self, parent):
        """构建支持访问的网络信息卡片（办公/WiFi/宿舍）"""
        net_frame = tk.Frame(parent, bg="#f0fdf4",
                             highlightbackground="#86efac", highlightthickness=1)
        net_frame.pack(fill="x", pady=(12, 0), padx=4)

        inner = tk.Frame(net_frame, bg="#f0fdf4")
        inner.pack(fill="x", padx=12, pady=8)

        # 标题
        title_row = tk.Frame(inner, bg="#f0fdf4")
        title_row.pack(fill="x", pady=(0, 6))
        tk.Label(title_row, text="●", bg="#f0fdf4", fg="#10b981",
                 font=("Microsoft YaHei", 9)).pack(side="left", padx=(0, 4))
        tk.Label(title_row, text="支持访问的网络", bg="#f0fdf4", fg="#065f46",
                 font=("Microsoft YaHei", 9, "bold")).pack(side="left")

        # 网络条目：(类型, 名称, 是否支持)
        rows = [
            ("有线", "办公区域网络", True),
            ("WiFi", "JTOFFICEWIFI、JTOFFICEWIFI_2.4G", True),
            ("宿舍网络", "暂不支持访问", False),
        ]
        for net_type, net_desc, supported in rows:
            self._make_net_row(inner, net_type, net_desc, supported)

    def _make_net_row(self, parent, net_type, net_desc, supported):
        """构建单个网络条目"""
        row = tk.Frame(parent, bg="#f0fdf4")
        row.pack(fill="x", pady=2)
        mark = "✓" if supported else "✗"
        mark_color = "#10b981" if supported else "#ef4444"
        tk.Label(row, text=mark, bg="#f0fdf4", fg=mark_color,
                 font=("Microsoft YaHei", 10, "bold"),
                 width=2, anchor="center").pack(side="left")
        tk.Label(row, text=f"{net_type}:", bg="#f0fdf4", fg="#475569",
                 font=("Microsoft YaHei", 9), width=8, anchor="w").pack(side="left")
        tk.Label(row, text=net_desc, bg="#f0fdf4", fg="#1e293b",
                 font=("Microsoft YaHei", 9), anchor="w",
                 wraplength=380, justify="left").pack(side="left", fill="x", expand=True)

    def _copy_all_info(self):
        """一键复制所有信息：含访问链接（仅 IP，排除 localhost）+ 网络信息 + 登录提示"""
        try:
            lan_ip = self.service.get_local_ip()
            port = self.service.port
            lines = ["【工资查询系统 - 访问信息】", ""]
            # 访问链接（仅 IP 链接，不含 localhost）
            lines.append("▎访问链接")
            lines.append(f"  HTTP:  http://{lan_ip}:{port}")
            if self.service.https_enabled:
                https_port = self.service.https_port
                lines.append(f"  HTTPS: https://{lan_ip}:{https_port}")
            lines.append("")
            # 网络信息
            lines.append("▎支持访问的网络")
            lines.append("  ✓ 有线: 办公区域网络")
            lines.append("  ✓ WiFi: JTOFFICEWIFI、JTOFFICEWIFI_2.4G")
            lines.append("  ✗ 宿舍网络: 暂不支持访问")
            lines.append("")
            # 登录提示
            lines.append("▎登录提示")
            lines.append("  账号 = 工号")
            lines.append("  密码 = 紧急联系人号码")
            text = "\n".join(lines)
            # 复制到剪贴板
            self.root.clipboard_clear()
            self.root.clipboard_append(text)
            self.root.update()  # 确保剪贴板内容写入
            # 按钮反馈
            if hasattr(self, "copy_all_btn") and self.copy_all_btn is not None:
                try:
                    if self.copy_all_btn.winfo_exists():
                        self.copy_all_btn.config(
                            text="✓ 已复制到剪贴板！",
                            bg="#10b981", activebackground="#059669"
                        )
                        self.root.after(
                            1500,
                            lambda: self.copy_all_btn.config(
                                text="一键复制所有信息（IP链接+网络信息）",
                                bg="#3b82f6", activebackground="#2563eb"
                            ) if self.copy_all_btn.winfo_exists() else None
                        )
                except tk.TclError:
                    pass
        except Exception as e:
            messagebox.showerror("复制失败", f"复制信息时出错: {e}")

    def _copy_url(self, url):
        """复制链接到剪贴板"""
        self.info_container.clipboard_clear()
        self.info_container.clipboard_append(url)
        # 按钮反馈：遍历所有 Label，匹配 URL 后找到同行的复制按钮
        for widget in self.info_container.winfo_children():
            if isinstance(widget, tk.Frame):
                btn = None
                for child in widget.winfo_children():
                    if isinstance(child, tk.Label) and child.cget("text") == url:
                        for sub in widget.winfo_children():
                            if isinstance(sub, tk.Button) and sub.cget("text") == "复制":
                                btn = sub
                                break
                        break
                if btn is not None:
                    btn.config(text="已复制!", bg="#10b981", fg="white")
                    self.root.after(1200, lambda b=btn: b.config(
                        text="复制", bg="#e5e7eb", fg="#374151"))
                    break

    def _start_status_check(self):
        # #18 异常保护：防止定时器因为控件销毁等异常中断
        try:
            self._update_status()
        except tk.TclError:
            return
        # #23 轮询间隔 3s → 5s（IP/状态极少变化，减小 CPU 占用）
        self.root.after(5000, self._start_status_check)


def main():
    # 检测是否已有实例在运行
    if not _acquire_lock():
        messagebox.showwarning("提示", "程序已运行，请不要重复运行！")
        return

    root = tk.Tk()
    app = ControlPanel(root)
    root.mainloop()
