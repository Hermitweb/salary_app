"""
FieldConfig + FieldItem: 字段配置（持久化 + UI 元素）
"""
import os
import tkinter as tk

from .constants import (
    FIELDS_CONFIG,
    STATE_AUTO, STATE_SHOW, STATE_HIDE,
    STATE_COLORS, STATE_OPTIONS,
    get_dynamic_fields,
)


class FieldConfig:
    """三态字段配置（auto/show/hide），持久化到 fields.txt"""

    def __init__(self):
        self.config = {}
        self.fields = []
        self.load()

    def load(self):
        self.config = {}
        self.fields = get_dynamic_fields()
        if FIELDS_CONFIG.exists():
            try:
                with open(FIELDS_CONFIG, "r", encoding="utf-8") as f:
                    for line in f:
                        line = line.strip()
                        if not line or line.startswith("#"):
                            continue
                        if "=" in line:
                            key, value = line.split("=", 1)
                            key = key.strip()
                            value = value.strip().lower()
                            if value in ("true", "1", "yes", "y", "是", "show", "显示"):
                                self.config[key] = STATE_SHOW
                            elif value in ("false", "0", "no", "n", "否", "hide", "隐藏"):
                                self.config[key] = STATE_HIDE
                            elif value in ("auto", "跟随", "全局", "默认"):
                                self.config[key] = STATE_AUTO
                            else:
                                self.config[key] = STATE_AUTO
            except Exception:
                pass

    def save(self):
        """保存配置：保留用户手写注释和未识别的字段行（#4 增强）"""
        try:
            preserved_lines = []
            known_keys = set(self.config.keys())
            if os.path.exists(FIELDS_CONFIG):
                try:
                    with open(FIELDS_CONFIG, "r", encoding="utf-8") as old:
                        for line in old:
                            stripped = line.strip()
                            if not stripped or stripped.startswith("#"):
                                preserved_lines.append(line)
                                continue
                            if "=" in stripped:
                                key = stripped.split("=", 1)[0].strip()
                                if key not in known_keys:
                                    preserved_lines.append(line)
                except OSError:
                    pass

            with open(FIELDS_CONFIG, "w", encoding="utf-8") as f:
                f.write("# ================================================\n")
                f.write("# 员工工资查询系统 - 明细字段显示配置\n")
                f.write("# ================================================\n")
                f.write("# 说明：每行一个字段，格式：字段名 = 状态\n")
                f.write("#       状态 = show  显示（即使值为 0/空 也显示）\n")
                f.write("#       状态 = hide  隐藏（强制不显示）\n")
                f.write("#       状态 = auto  跟随全局规则（默认，空/0 不显示）\n")
                f.write("#       以 # 开头的行为注释，不会被读取\n")
                f.write("#       修改后保存即可生效，无需重启服务\n")
                f.write("# ================================================\n\n")
                f.write("# 字段列表（从工资表自动读取）\n\n")
                for field in self.fields:
                    val = self.config.get(field, STATE_AUTO)
                    f.write(f"{field} = {val}\n")
                if preserved_lines:
                    f.write("\n# --- 以下为自定义内容（保留） ---\n")
                    for line in preserved_lines:
                        f.write(line)
            return True
        except Exception as e:
            return False

    def get_state(self, field):
        return self.config.get(field, STATE_AUTO)

    def set_state(self, field, state):
        self.config[field] = state
        self.save()

    def cycle_state(self, field):
        """循环切换: auto -> show -> hide -> auto"""
        current = self.get_state(field)
        order = [STATE_AUTO, STATE_SHOW, STATE_HIDE]
        next_idx = (order.index(current) + 1) % len(order)
        next_state = order[next_idx]
        self.set_state(field, next_state)
        return next_state

    def get_fields_by_state(self):
        auto, show, hide = [], [], []
        for field in self.fields:
            state = self.get_state(field)
            if state == STATE_SHOW:
                show.append(field)
            elif state == STATE_HIDE:
                hide.append(field)
            else:
                auto.append(field)
        return auto, show, hide


class FieldItem:
    """统一字段配置项：一个字段标签 + 三个单选框（显示/隐藏/全局）"""

    def __init__(self, parent, field, state, on_change):
        self.field = field
        self.state_var = tk.StringVar(value=state)
        self.on_change = on_change
        self.radios = {}

        self.lbl = tk.Label(
            parent, text=field, bg="white", fg="#1a2332",
            font=("Microsoft YaHei", 9), anchor="w"
        )
        self.rb_frame = tk.Frame(parent, bg="white", bd=0, highlightthickness=0)

        for s, text in STATE_OPTIONS:
            rb = tk.Radiobutton(
                self.rb_frame, text=text, value=s,
                variable=self.state_var,
                bg="white", activebackground=STATE_COLORS[s]["active_bg"],
                activeforeground=STATE_COLORS[s]["fg"],
                fg=STATE_COLORS[s]["fg"],
                selectcolor=STATE_COLORS[s]["bg"],
                font=("Microsoft YaHei", 8),
                indicatoron=True,
                padx=1, pady=0,
                command=self._on_radio_change,
            )
            rb.pack(side="left", padx=(1, 0))
            self.radios[s] = rb

        self._update_style(state)

    def _on_radio_change(self):
        new_state = self.state_var.get()
        self._update_style(new_state)
        self.on_change(self.field, new_state)

    def _update_style(self, active_state):
        for state, rb in self.radios.items():
            if state == active_state:
                rb.config(
                    bg=STATE_COLORS[state]["bg"],
                    fg=STATE_COLORS[state]["fg"],
                    activebackground=STATE_COLORS[state]["bg"],
                    activeforeground=STATE_COLORS[state]["fg"],
                )
                rb.config(indicatoron=True)
            else:
                rb.config(
                    bg="white",
                    fg=STATE_COLORS[state]["fg"],
                    activebackground=STATE_COLORS[state]["active_bg"],
                    activeforeground=STATE_COLORS[state]["fg"],
                )

    def refresh(self, state):
        self.state_var.set(state)
        self._update_style(state)
