"""#25 颜色常量集中管理：避免散落在 control_panel.py 各处的颜色硬编码"""


class Color:
    """UI 颜色常量（按用途分组）"""

    # === 品牌主色 ===
    PRIMARY = "#3b82f6"         # 主蓝色（启动按钮、强调）
    PRIMARY_DARK = "#2563eb"    # 主蓝色 hover
    PRIMARY_DISABLED = "#9ca3af"  # 灰色（禁用按钮）
    PRIMARY_BG = "#eff6ff"      # 浅蓝背景
    PRIMARY_BORDER = "#93c5fd"  # 蓝色边框
    PRIMARY_FG = "#1e40af"      # 蓝色文字

    # === 状态色 ===
    SUCCESS = "#10b981"         # 成功绿（运行状态）
    SUCCESS_DARK = "#059669"    # 成功绿 hover
    SUCCESS_BG = "#f0fdf4"      # 浅绿背景
    SUCCESS_BORDER = "#86efac"  # 绿色边框
    SUCCESS_FG = "#166534"      # 绿色文字
    SUCCESS_WARN = "#a16207"    # 警告文字（黄绿）

    DANGER = "#ef4444"          # 危险红（停止服务）
    DANGER_DARK = "#dc2626"     # 危险红 hover

    WARN_BG = "#fef3c7"         # 浅黄背景（登录提示）
    WARN_BORDER = "#fcd34d"     # 黄色边框
    WARN_FG = "#92400e"         # 黄色文字
    WARN_FG_DARK = "#78350f"    # 深黄文字

    # === 中性色 ===
    BG = "#f5f7fa"              # 面板背景
    BG_CARD = "white"           # 卡片背景
    BG_DISABLED = "#f3f4f6"     # 浅灰背景（未启用 HTTPS）
    BORDER = "#e8ecf1"          # 默认边框
    BORDER_DARK = "#d1d5db"     # 深灰边框
    TEXT = "#1a2332"            # 主文字
    TEXT_SECONDARY = "#374151"  # 次要文字
    TEXT_MUTED = "#6b7280"      # 弱化文字
    TEXT_DISABLED = "#9ca3af"   # 禁用文字

    # === 标题栏 ===
    TITLE_BG = "#1a2332"        # 深色标题栏
    TITLE_FG = "white"


class StateColor:
    """字段配置三态颜色（auto/show/hide）"""
    auto = {"bg": "#f3f4f6", "fg": "#6b7280", "active_bg": "#e5e7eb"}
    show = {"bg": "#d1fae5", "fg": "#065f46", "active_bg": "#a7f3d0"}
    hide = {"bg": "#fee2e2", "fg": "#991b1b", "active_bg": "#fecaca"}
