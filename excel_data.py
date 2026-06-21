import os
import re
import sys
from pathlib import Path
import openpyxl


def _resolve_data_dir():
    """获取数据目录：开发时用源码同级的 data，打包后用 exe 同级的 data（唯一位置）"""
    if getattr(sys, 'frozen', False):
        return os.path.join(os.path.dirname(os.path.abspath(sys.executable)), "data")
    return os.path.join(os.path.dirname(os.path.abspath(__file__)), "data")


BASE_DIR = os.path.dirname(os.path.abspath(__file__))
DATA_DIR = _resolve_data_dir()
FIELDS_CONFIG = Path(DATA_DIR) / "fields.txt"

# 三态常量（与 control_panel.py 保持一致）
STATE_AUTO = "auto"   # 跟随全局规则：空/0 不显示
STATE_SHOW = "show"   # 强制显示
STATE_HIDE = "hide"   # 强制隐藏

# === 字段缓存：避免每次 get_all_salary_fields 都打开所有 xlsx ===
_DYNAMIC_FIELDS_CACHE = {}


def _fields_config_path():
    """返回 fields.txt 路径（支持 frozen 模式）"""
    return FIELDS_CONFIG


def _load_field_config():
    """
    读取 fields.txt 配置文件，返回 {字段名: 状态} 字典。
    状态为 'auto' / 'show' / 'hide' 三态之一。
    兼容旧版 true/false 配置。
    """
    config = {}
    if not os.path.exists(FIELDS_CONFIG):
        return config
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
                        config[key] = STATE_SHOW
                    elif value in ("false", "0", "no", "n", "否", "hide", "隐藏"):
                        config[key] = STATE_HIDE
                    elif value in ("auto", "跟随", "全局", "默认"):
                        config[key] = STATE_AUTO
                    else:
                        config[key] = STATE_AUTO
    except Exception:
        pass
    return config


def _find_employee_file():
    for name in ["员工列表.xlsx", "员工列表.xls"]:
        path = os.path.join(DATA_DIR, name)
        if os.path.exists(path):
            return path
    return None


def _find_salary_file(year, month):
    """
    查找工资表文件，支持多种命名格式：
    - 2026年05月员工工资.xlsx
    - 2026年5月员工工资.xlsx
    - 也兼容其他包含年月和"工资"的Excel文件
    """
    # 用正则一次匹配多种格式（"年"和"月"之间允许有空格/无空格）
    year_str = str(year)
    month_str = str(int(month))
    month_padded = f"{int(month):02d}"
    name_patterns = [
        re.compile(rf"{year_str}\s*年\s*{int(month)}\s*月"),  # 2026年5月
        re.compile(rf"{year_str}\s*年\s*{month_padded}\s*月"),  # 2026年05月
        re.compile(rf"{year_str}[-_/]{month_padded}\b"),  # 2026-05 / 2026_05 / 2026/05
        re.compile(rf"{year_str}[-_/]{int(month)}\b"),  # 2026-5
        re.compile(rf"{year_str}{month_padded}"),  # 202605
    ]
    # 兜底正则：年份和月份都在文件名中
    fallback_pattern = re.compile(rf"{year_str}.*{int(month)}")
    search_dirs = []
    salary_dir = os.path.join(DATA_DIR, "salary")
    if os.path.exists(salary_dir):
        search_dirs.append(salary_dir)
    if os.path.exists(DATA_DIR):
        search_dirs.append(DATA_DIR)

    matches = []
    for d in search_dirs:
        try:
            for fname in os.listdir(d):
                if not (fname.endswith(".xlsx") or fname.endswith(".xls")):
                    continue
                # 优先精确匹配
                for pat in name_patterns:
                    if pat.search(fname):
                        return os.path.join(d, fname)
                # 兜底：要求含"工资"且年月都在
                if "工资" in fname and fallback_pattern.search(fname):
                    matches.append(os.path.join(d, fname))
        except Exception:
            pass
    if matches:
        return matches[0]
    return None


def _find_latest_salary_file():
    """
    兜底：找不到指定年月的工资表时，按文件修改时间返回最新的工资表文件。
    """
    candidates = []
    for d in [os.path.join(DATA_DIR, "salary"), DATA_DIR]:
        if not os.path.exists(d):
            continue
        try:
            for fname in os.listdir(d):
                fpath = os.path.join(d, fname)
                if not (fname.endswith(".xlsx") or fname.endswith(".xls")):
                    continue
                if not os.path.isfile(fpath):
                    continue
                try:
                    mtime = os.path.getmtime(fpath)
                    candidates.append((mtime, fpath, fname))
                except OSError:
                    pass
        except OSError:
            pass
    if not candidates:
        return None
    # 按 mtime 倒序排，取最新的
    candidates.sort(reverse=True)
    return candidates[0][1]


def get_all_salary_fields():
    """
    动态读取所有工资表文件的字段（表头第一行），返回去重后的字段列表
    带缓存：依据 fields.txt mtime + 工资表 mtime 决定是否刷新
    """
    all_fields = []
    seen = set()
    # === #5 缓存机制：根据 mtime 决定是否重新扫描 ===
    cache_key = _compute_fields_cache_key()
    cached = _DYNAMIC_FIELDS_CACHE.get("__cache_key__")
    if cached == cache_key and _DYNAMIC_FIELDS_CACHE.get("__fields__"):
        return _DYNAMIC_FIELDS_CACHE["__fields__"]
    search_dirs = []
    salary_dir = os.path.join(DATA_DIR, "salary")
    if os.path.exists(salary_dir):
        search_dirs.append(salary_dir)
    if os.path.exists(DATA_DIR):
        search_dirs.append(DATA_DIR)

    for d in search_dirs:
        try:
            for fname in os.listdir(d):
                if fname.endswith((".xlsx", ".xls")) and "工资" in fname:
                    fpath = os.path.join(d, fname)
                    # === 使用 DataCache 复用已加载的工资表数据 ===
                    from cache import DataCache
                    headers, _, _ = DataCache.instance().get_salary(fpath)
                    for h in headers:
                        if h and h not in seen:
                            seen.add(h)
                            all_fields.append(h)
        except OSError:
            pass
    # 缓存结果
    _DYNAMIC_FIELDS_CACHE["__cache_key__"] = cache_key
    _DYNAMIC_FIELDS_CACHE["__fields__"] = all_fields
    return all_fields


def _compute_fields_cache_key():
    """计算字段缓存的 key：fields.txt mtime + 所有工资表最大 mtime"""
    parts = []
    try:
        fields_path = _fields_config_path()
        if fields_path.exists():
            parts.append(str(fields_path.stat().st_mtime))
        else:
            parts.append("0")
    except OSError:
        parts.append("0")
    # 工资表 mtime
    latest_mtime = 0.0
    for d in [DATA_DIR, os.path.join(DATA_DIR, "salary")]:
        try:
            if not os.path.exists(d):
                continue
            for fname in os.listdir(d):
                if fname.endswith((".xlsx", ".xls")) and "工资" in fname:
                    fpath = os.path.join(d, fname)
                    try:
                        mt = os.path.getmtime(fpath)
                        if mt > latest_mtime:
                            latest_mtime = mt
                    except OSError:
                        pass
        except OSError:
            pass
    parts.append(str(latest_mtime))
    return "|".join(parts)


def authenticate(username, password):
    """
    登录验证 - 支持两种模式：
    1. 标准模式：第1行是字段名（员工号/姓名/部门/手机号/密码/管理员），第2行起是员工数据
    2. 原始模式：第1行是账号列表，第2行是密码列表，第3行是姓名列表...
    """
    from cache import DataCache

    path = _find_employee_file()
    if not path:
        return {"ok": False, "error": "员工列表文件不存在"}

    rows, headers, id_idx, pwd_idx, name_idx, dept_idx, status_idx = \
        DataCache.instance().get_employees(path)
    if not rows:
        return {"ok": False, "error": "员工列表为空或读取失败"}

    # 模式1：标准结构
    standard_fields = ["员工号", "工号", "姓名", "部门", "手机号", "密码", "管理员",
                       "联系电话", "紧急联系人", "紧急联系人号码", "离职", "离职否"]
    has_standard_headers = any(any(f in str(h) for f in standard_fields) for h in headers)

    if has_standard_headers:
        # 列索引
        def col_idx(keywords, default=None):
            for kw in keywords:
                for i, h in enumerate(headers):
                    if h is not None and kw in str(h):
                        return i
            return default

        admin_idx = col_idx(["管理员", "是否管理员"])
        resigned_idx = col_idx(["离职", "离职否", "是否离职"])

        for d in rows:
            emp_id = str(d.get(headers[id_idx], "")).strip() if id_idx is not None and id_idx < len(headers) else ""
            pwd = str(d.get(headers[pwd_idx], "")).strip() if pwd_idx is not None and pwd_idx < len(headers) else ""
            if not emp_id:
                continue
            if emp_id == username and pwd == password:
                if resigned_idx is not None and resigned_idx < len(headers):
                    resigned_val = str(d.get(headers[resigned_idx], "")).strip()
                    if resigned_val in ("是", "已离职", "Y", "y", "1", "True", "true",
                                        "离职", "yes"):
                        return {
                            "ok": False,
                            "error": "该员工已离职，账号不可用",
                        }
                name = str(d.get(headers[name_idx], "")).strip() if name_idx is not None and name_idx < len(headers) else ""
                dept = str(d.get(headers[dept_idx], "")).strip() if dept_idx is not None and dept_idx < len(headers) else ""
                admin_val = str(d.get(headers[admin_idx], "")).strip() if admin_idx is not None and admin_idx < len(headers) else ""
                is_admin = admin_val in ("是", "管理员", "Y", "y", "1", "True", "true")
                return {
                    "ok": True,
                    "employee": {
                        "employee_id": emp_id,
                        "name": name,
                        "dept": dept,
                        "is_admin": is_admin,
                    },
                }
        return {"ok": False, "error": "工号或密码错误"}

    # 模式2：原始结构（每列一个员工）
    # 从 rows 重建列视图：第一行是账号，第二行是密码...
    # rows 是 list[dict]，需要按位置索引
    if len(rows) < 2:
        return {"ok": False, "error": "员工列表数据不足"}

    # 重新打开用列视图（模式2特殊，缓存按行存的，重新读一下）
    wb = None
    try:
        wb = openpyxl.load_workbook(path, data_only=True, read_only=True)
        ws = wb.active
        max_col = ws.max_column
        accounts = [str(ws.cell(1, c).value or "").strip() for c in range(1, max_col + 1)]
        passwords = [str(ws.cell(2, c).value or "").strip() for c in range(1, max_col + 1)]
        names = [str(ws.cell(3, c).value or "").strip() for c in range(1, max_col + 1)]
        depts = [str(ws.cell(4, c).value or "").strip() for c in range(1, max_col + 1)]
        admins = [str(ws.cell(5, c).value or "").strip() for c in range(1, max_col + 1)]
    except Exception:
        return {"ok": False, "error": "员工列表读取失败"}
    finally:
        if wb is not None:
            try:
                wb.close()
            except Exception:
                pass

    for i in range(len(accounts)):
        if accounts[i] == username and passwords[i] == password:
            is_admin = admins[i] in ("是", "管理员", "Y", "y", "1", "True", "true")
            return {
                "ok": True,
                "employee": {
                    "employee_id": accounts[i],
                    "name": names[i] if i < len(names) else "",
                    "dept": depts[i] if i < len(depts) else "",
                    "is_admin": is_admin,
                },
            }
    return {"ok": False, "error": "工号或密码错误"}


def _find_col(headers, keywords):
    for idx, h in enumerate(headers):
        for kw in keywords:
            if kw in h:
                return idx
    return None


def _is_empty_or_zero(value):
    """
    判断字段值是否为空或0：
    - None、空字符串、只有空格 -> True
    - 数字 0、0.0、字符串 "0"、"0.0"、"0.00" -> True
    - 其他 -> False
    """
    if value is None:
        return True
    s = str(value).strip()
    if not s:
        return True
    try:
        f = float(s.replace(",", "").replace("¥", "").replace("￥", "").strip())
        if f == 0:
            return True
    except (ValueError, TypeError):
        pass
    return False


def _fmt_val(value):
    """格式化字段值：datetime/日期字符串 转 'YYYY-MM-DD'"""
    if hasattr(value, 'strftime'):
        return value.strftime('%Y-%m-%d')
    if isinstance(value, str) and len(value) >= 10 and value[4] == '-' and value[7] == '-':
        return value[:10]
    return value


def get_salary(employee_id, year, month):
    """
    从工资表读取指定员工的工资数据（带缓存）。
    显示规则（优先级从高到低）：
      1) 字段配置为 show -> 强制显示（即使值为 0 或空）
      2) 字段配置为 hide -> 强制隐藏
      3) 字段配置为 auto / 未配置 -> 跟随全局规则（值为 0 或空时隐藏）

    所有字段（包括员工号、序号、姓名）统一遵守字段配置，不再强制保留。
    """
    from cache import DataCache

    path = _find_salary_file(year, month)
    if not path:
        # 兜底：找一个最新的工资表文件
        fallback = _find_latest_salary_file()
        if fallback:
            path = fallback
        else:
            return {"ok": False, "error": f"{year}年{month}月工资文件不存在"}

    field_config = _load_field_config()
    headers, rows, idx_map = DataCache.instance().get_salary(path)
    if not headers:
        return {"ok": False, "error": f"工资表为空或读取失败: {year}年{month}月"}

    # 找工号列
    emp_id_col = None
    for i, h in enumerate(headers):
        if h in ("员工号", "工号"):
            emp_id_col = h
            break
    if emp_id_col is None:
        return {"ok": False, "error": "工资表中找不到员工号列"}

    # 从缓存中查找员工行（O(1) 字典查找）
    target = idx_map.get(str(employee_id).strip())
    if target is None:
        return {"ok": False, "error": f"员工 {employee_id} 在 {year}年{month}月 没有工资数据"}

    # 应用字段配置
    result_headers = []
    result_values = []
    result_pairs = []
    row_data = {}
    for h in headers:
        if not h:
            continue
        clean_h = h.strip()
        raw_val = target.get(h)
        display_val = str(raw_val) if raw_val is not None else ""

        is_emp_id = clean_h in ("员工号",)
        default_state = STATE_SHOW if is_emp_id else STATE_AUTO
        state = field_config.get(clean_h, default_state)

        if state == STATE_SHOW:
            row_data[clean_h] = display_val
            result_pairs.append((clean_h, display_val))
        elif state == STATE_HIDE:
            continue
        else:
            if _is_empty_or_zero(raw_val):
                continue
            row_data[clean_h] = display_val
            result_pairs.append((clean_h, display_val))

        if is_emp_id and clean_h not in row_data:
            row_data[clean_h] = display_val

    # 序号：优先使用"序号"列，否则用缓存中的索引位置
    row_index = row_data.get("序号", "")
    if not row_index:
        for i, r in enumerate(rows, start=1):
            if r is target:
                row_index = str(i)
                break

    filtered_data = {h: v for h, v in result_pairs}
    filtered_data["row_index"] = row_index

    for h, v in result_pairs:
        result_headers.append(h)
        result_values.append(v)

    return {"ok": True, "headers": result_headers, "values": result_values, "data": filtered_data}


def _get_field_category(field_name):
    """
    将字段名归类到传统工资条的列结构中。
    返回值: ('basic'|'earn'|'deduct'|'withhold'|'special'|'info', group_label)
    """
    name = field_name or ""

    # 员工基础信息
    basic_keywords = ["员工号", "工号", "姓名", "部门", "岗位", "身份证", "入职日期", "离职日期", "联系电话", "手机号码", "性别"]
    for kw in basic_keywords:
        if kw in name:
            # 岗位津贴/职务津贴/应发岗位工资 是工资项
            if kw == "岗位" and ("津贴" in name or "工资" in name):
                break
            if kw == "职务" and ("津贴" in name or "工资" in name):
                break
            return ("info", "")

    # 应发工资类 - 先检查"应发"前缀的字段
    if name.startswith("应发") or name.startswith("应(发)"):
        rest = name
        if rest.startswith("应发"):
            rest = rest[2:]
        if rest.endswith("合计"):
            return ("earn_total", "应发工资")
        if "基本" in rest:
            return ("earn_basic", "应发工资")
        if "岗位" in rest or "职务" in rest:
            return ("earn_post", "应发工资")
        if "加班" in rest or "加班" in name:
            return ("earn_overtime", "应发工资")
        if "绩效" in rest or "奖金" in rest:
            return ("earn_perf", "应发工资")
        if "津贴" in rest or "补贴" in rest:
            return ("earn_allow", "应发工资")
        if "夜班" in rest:
            return ("earn_allow", "应发工资")
        if "病假" in rest or "婚假" in rest or "产假" in rest:
            return ("earn_other", "应发工资")
        return ("earn_other", "应发工资")

    # 项目类 - 基本工资项目/岗位津贴项目等
    if "项目" in name:
        if "基本" in name:
            return ("earn_basic", "应发工资")
        if "岗位津贴" in name or "职务津贴" in name:
            return ("earn_allow", "应发工资")
        if "岗位" in name or "职务" in name:
            return ("earn_post", "应发工资")
        if "加班" in name:
            return ("earn_overtime", "应发工资")
        if "绩效" in name or "奖金" in name:
            return ("earn_perf", "应发工资")
        if "津贴" in name or "补贴" in name:
            return ("earn_allow", "应发工资")
        if "工资项目合计" in name or name.endswith("合计"):
            return ("earn_total", "应发工资")
        return ("earn_other", "应发工资")

    # 合计类
    if "合计" in name:
        if "应发" in name or "税前" in name or "工资" in name:
            return ("earn_total", "应发工资")
        if "税后" in name or "应扣" in name:
            return ("deduct_other", "应扣工资")

    # 应发工资 - 基本工资
    if "基本工资" in name:
        return ("earn_basic", "应发工资")

    # 应发工资 - 岗位津贴/职务津贴
    if "岗位津贴" in name or "职务津贴" in name:
        return ("earn_allow", "应发工资")

    # 应发工资 - 岗位/职务
    if "岗位" in name or "职务" in name:
        return ("earn_post", "应发工资")

    # 应发工资 - 加班
    if "加班" in name or "加班费" in name:
        return ("earn_overtime", "应发工资")

    # 应发工资 - 绩效/奖金
    if "绩效" in name or "奖金" in name or "绩效工资" in name or "奖金工资" in name:
        return ("earn_perf", "应发工资")

    # 应发工资 - 津贴/补贴
    if "津贴" in name or "补贴" in name:
        return ("earn_allow", "应发工资")

    # 应发工资 - 天数/数量类
    if ("天数" in name or "小时" in name or "数量" in name or "次数" in name) and "假" not in name:
        return ("earn_days", "应发工资")

    # 应发工资 - 其他
    if "应发" in name:
        return ("earn_other", "应发工资")

    # 实发 - 优先级高于 "工资"
    if "实发" in name:
        return ("net", "实发工资")

    if "工资" in name:
        return ("earn_other", "应发工资")

    # 应扣工资 - 社保/公积金
    if "社保" in name or "公积金" in name or "保险" in name:
        return ("deduct_ins", "应扣工资")

    # 应扣工资 - 个税
    if "个税" in name or "所得税" in name:
        return ("deduct_tax", "应扣工资")

    # 应扣工资 - 税后应扣
    if "税后应扣" in name:
        return ("deduct_other", "应扣工资")

    # 应扣工资 - 其他
    if "应扣" in name:
        return ("deduct_other", "应扣工资")

    # 代扣款项
    if "代扣" in name or "代发" in name:
        return ("withhold", "应(代)扣金额")

    # 实发
    if "实发" in name:
        return ("net", "实发工资")

    # 考勤/假期
    if "出勤" in name or "缺勤" in name or "旷工" in name or "事假" in name or "病假" in name or "年假" in name or "假" in name or "迟到" in name or "早退" in name or "打卡" in name:
        return ("attendance", "考勤")

    # 备注
    if "备注" in name or "说明" in name:
        return ("remark", "")

    # 其他
    return ("other", "")


def _days_from_hours(hours):
    try:
        h = float(hours)
        if h > 0:
            d = h / 8.0
            if d == int(d):
                return str(int(d))
            return f"{d:.2f}".rstrip("0").rstrip(".")
    except (ValueError, TypeError):
        pass
    return ""


def _val(data, *keys, default=""):
    for k in keys:
        if k in data and data[k] is not None:
            v = data[k]
            try:
                f = float(str(v).replace(",", "").replace("¥", "").replace("￥", "").strip())
                if f == 0 and ("0" == str(v).strip() or "" == str(v).strip()):
                    continue
                return str(v)
            except (ValueError, TypeError):
                if str(v).strip() != "":
                    return str(v)
    return default


# 工资条表格列映射：列名 -> 对应的工资表字段（用于判断是否为空/是否配置显示）
_PAYSLIP_COLUMN_MAP = {
    "base_salary": ["应发基本工资"],
    "work_days": ["应出勤", "本月应出勤"],
    "overtime_days": ["加班费合计", "加班", "加班小时", "加班工时", "普加小时", "节加小时", "国加小时"],
    "monthly_bonus": ["应发绩效工资"],
    "post_allowance": ["应发岗位工资"],
    "overtime_pay": ["应发周六加班工资"],
    "other_earn": ["应发质量奖金", "应发全勤奖", "应发卫生安全奖", "应发工龄奖"],
    "earn_total": ["税前工资绩效合计", "应发工资合计"],
    "leave_days": ["事假", "病假"],
    "social_ins": ["本月社会保险"],
    "housing_fund": ["本月住房公积金"],
    "individual_tax": ["本月个人所得税"],
    "other_deduct": ["税后应扣-电"],
    "net_salary": ["本月实发工资", "实发工资"],
}

# 固定表格中已使用的字段集合（用于识别"额外字段"，避免重复显示）
# 包括：传统工资条表格直接引用的字段，以及其对应的原始项目字段
_FIXED_TABLE_FIELDS = {
    # 基本信息
    "员工号", "工号", "姓名",
    # 岗位/部门（用于头部显示）
    "部门", "岗位",
    # 原始项目字段（生成"应发*"的来源）
    "基本工资项目", "岗位津贴项目", "周六加班工资项目", "绩效（奖金）工资项目",
    "质量奖", "全勤奖", "卫生安全奖", "工龄奖",
    # 应发类
    "应发基本工资", "应发岗位工资", "应发周六加班工资", "应发绩效工资",
    "应发质量奖金", "应发全勤奖", "应发卫生安全奖", "应发工龄奖",
    # 加班类
    "加班费合计", "加班", "加班小时", "加班工时",
    "普加小时", "节加小时", "国加小时",
    # 出勤/缺勤类
    "应出勤", "本月应出勤", "实出勤", "夜班天数", "不开餐宵夜天数",
    "缺勤", "旷工", "事假", "年假", "法定有薪假", "病假",
    "婚、产（丧）假", "病假工资", "婚假、产假、丧假工资",
    "迟到早退次数", "迟到早退小时", "忘记打卡次数",
    # 应发合计
    "工资项目合计", "税前工资绩效合计", "应发工资合计",
    "夜班补贴（满30人取消餐补）", "健康证/连体防尘服", "其他应补",
    # 代扣类
    "本月社会保险", "本月住房公积金", "本月个人所得税",
    # 其他扣款
    "税后应扣-电", "税后应扣-水", "税后应扣-劳保",
    "税后应扣-健康证", "税后应扣-其它", "税后应扣合计",
    "税后补上月个税",
    # 实发
    "本月实发工资", "实发工资", "实发特殊核定",
    # 计件
    "计件数量", "计件工资",
    # 备注
    "备注",
}


def _col_should_show(col_name, data, field_config):
    """
    判断工资条表格的某一列是否应该显示（支持 show / hide / auto 三态）：
      - 任一源字段配置为 show -> 显示
      - 所有源字段配置为 hide -> 隐藏
      - 其他情况（auto 或未配置）-> 有任一源字段非空/非0时显示
    """
    source_fields = _PAYSLIP_COLUMN_MAP.get(col_name, [])
    if not source_fields:
        return True

    has_show = False
    has_auto = False
    for f in source_fields:
        state = field_config.get(f, STATE_AUTO)
        if state == STATE_SHOW:
            has_show = True
        elif state == STATE_HIDE:
            pass
        else:  # auto / 未配置
            has_auto = True

    # 任一源字段强制显示
    if has_show:
        return True

    # 所有源字段都强制隐藏
    if not has_auto and not has_show:
        return False

    # auto 状态：有值才显示
    for f in source_fields:
        if f in data and not _is_empty_or_zero(data.get(f)):
            return True

    return False


def _filtered_field(col_name, raw_value, data, field_config):
    """
    统一字段过滤逻辑（消除 build_payslip_table 内的 15+ 处重复）：
    - 配置为显示：直接返回原值
    - 配置为隐藏/auto：值为空或 0 时返回 None（隐藏），否则保留原值
    """
    if _col_should_show(col_name, data, field_config):
        return raw_value
    if not _is_empty_or_zero(raw_value):
        return raw_value
    return None


def build_payslip_table(salary_data, employee_info, year, month):
    """
    构建传统工资条表格结构（仿照用户提供的款式）。
    返回结构化数据供模板渲染。
    结合控制面板字段配置：默认不显示为空/0的列，但配置为 true 的列强制显示。
    """
    data = salary_data or {}
    field_config = _load_field_config()

    def gv(*keys, default=""):
        return _val(data, *keys, default=default)

    # 基础信息
    emp_id = employee_info.get("employee_id", "")
    name = employee_info.get("name", "")
    dept = employee_info.get("dept", "")

    # 出勤天数 = 应出勤小时 / 8
    work_hours = gv("应出勤", "本月应出勤", default="")
    work_days = _filtered_field("work_days", _days_from_hours(work_hours), data, field_config)

    # 加班天数 = 加班小时 / 8
    overtime_hours = gv("加班费合计", "加班", "加班小时", "加班工时", "普加小时", "节加小时", "国加小时", default="")
    overtime_days = _filtered_field("overtime_days", _days_from_hours(overtime_hours), data, field_config)

    # 应发工资各列
    base_salary = _filtered_field("base_salary", gv("应发基本工资", default=""), data, field_config)
    monthly_bonus = _filtered_field("monthly_bonus", gv("应发绩效工资", default=""), data, field_config)
    post_allowance = _filtered_field("post_allowance", gv("应发岗位工资", default=""), data, field_config)
    overtime_pay = _filtered_field("overtime_pay", gv("应发周六加班工资", default=""), data, field_config)
    other_earn = _filtered_field(
        "other_earn",
        gv("应发质量奖金", "应发全勤奖", "应发卫生安全奖", "应发工龄奖", default=""),
        data, field_config,
    )
    earn_total = _filtered_field(
        "earn_total",
        gv("税前工资绩效合计", "应发工资合计", default=""),
        data, field_config,
    )

    # 事假/病假天数
    leave_days = ""
    for k in ["事假", "病假"]:
        if k in data and data[k] not in (None, "", 0, "0"):
            v = data[k]
            try:
                fv = float(str(v))
                if fv != 0:
                    leave_days = str(v)
                    break
            except (ValueError, TypeError):
                pass
    leave_days = _filtered_field("leave_days", leave_days, data, field_config)

    # 代扣
    social_ins = _filtered_field("social_ins", gv("本月社会保险", default=""), data, field_config)
    housing_fund = _filtered_field("housing_fund", gv("本月住房公积金", default=""), data, field_config)
    individual_tax = _filtered_field("individual_tax", gv("本月个人所得税", default=""), data, field_config)
    other_deduct = _filtered_field("other_deduct", gv("税后应扣-电", default=""), data, field_config)

    # 应扣合计 / 代扣金额合计
    def _sum_non_empty(values):
        total = 0.0
        found = False
        for v in values:
            if v:
                try:
                    total += float(str(v).replace(",", ""))
                    found = True
                except (ValueError, TypeError):
                    pass
        return f"{total:.2f}".rstrip("0").rstrip(".") if found else ""

    deduct_total = _sum_non_empty([social_ins, housing_fund, individual_tax, other_deduct])
    withhold_total = _sum_non_empty([social_ins, housing_fund, individual_tax, other_deduct])

    # 实发工资
    net_salary = _filtered_field("net_salary", gv("本月实发工资", "实发工资", default=""), data, field_config)

    def _first_label(keys, fallback=""):
        """从多个候选字段中返回第一个在工资表里实际存在的字段名（去除首尾空格）"""
        # 工资表字段名可能带首尾空格，先做清洗映射
        clean_data_keys = {k.strip(): k.strip() for k in data.keys()}
        for k in keys:
            if k in clean_data_keys:
                return k
        return fallback

    # 工资条字段：全部按工资表原始顺序，根据字段配置决定是否显示。
    # 所有字段（序号、姓名、员工号等）统一遵守字段配置，不再强制保留。
    fields = []
    for h, v in data.items():
        if h in ("员工号", "row_index"):
            continue
        # get_salary 已根据 show/hide/auto 过滤，data 中剩下的字段直接显示即可
        clean_h = h.strip()
        fd = {
            "key": clean_h,
            "label": clean_h,
            "value": _fmt_val(v),
        }
        if clean_h in ("本月实发工资", "实发工资"):
            fd["is_net"] = True
        fields.append(fd)

    return {
        "employee": {
            "employee_id": emp_id,
            "name": name,
            "dept": dept,
            "year": year,
            "month": month,
        },
        "fields": fields,
        "row": {
            "index": str(salary_data.get("row_index", "1")) if isinstance(salary_data, dict) else "1",
            "name": name,
            "base_salary": base_salary,
            "work_days": work_days,
            "overtime_days": overtime_days,
            "monthly_bonus": monthly_bonus,
            "post_allowance": post_allowance,
            "overtime_pay": overtime_pay,
            "other_earn": other_earn,
            "earn_total": earn_total,
            "leave_days": leave_days,
            "social_ins": social_ins,
            "housing_fund": housing_fund,
            "individual_tax": individual_tax,
            "other_deduct": other_deduct,
            "withhold_total": withhold_total,
            "net_salary": net_salary,
        },
        "attendance": [],
        "remark": "",
    }


def get_available_months():
    """
    扫描 data 和 data/salary 目录下的所有工资表文件，提取年月信息。
    支持多种命名格式：
    - 2026年05月员工工资.xlsx
    - 2026-05 工资.xlsx
    - 2026_05_工资.xlsx
    - 202605工资.xlsx
    - 工资202605.xlsx
    - 工资单2026年5月.xlsx
    找不到年月时使用文件修改时间作为兜底年月。
    按 (年, 月) 倒序排，最新的在前。
    """
    months = []
    seen = set()
    search_dirs = []
    salary_dir = os.path.join(DATA_DIR, "salary")
    if os.path.exists(salary_dir):
        search_dirs.append(salary_dir)
    if os.path.exists(DATA_DIR):
        search_dirs.append(DATA_DIR)

    # 多种年月匹配模式（按优先级排序）
    patterns = [
        re.compile(r"(\d{4})\s*年\s*(\d{1,2})\s*月"),  # 2026年05月
        re.compile(r"(\d{4})[-_/](\d{1,2})(?:[-_/](\d{1,2}))?"),  # 2026-05 或 2026/05
        re.compile(r"(\d{4})(\d{2})"),  # 202605
    ]

    def _extract_year_month(fname):
        # 优先用"工资"附近的位置匹配（更准确）
        # 但简单起见，全文件名扫描
        # 跳过明显的非年月数字（如手机号、ID）
        for pat in patterns:
            for m in pat.finditer(fname):
                groups = m.groups()
                year = int(groups[0])
                if year < 2000 or year > 2100:
                    continue  # 不是合理年份
                if len(groups) >= 2 and groups[1]:
                    month = int(groups[1])
                    if 1 <= month <= 12:
                        return year, month
        return None

    candidates = []  # [(year, month, full_path), ...]
    # 优先扫 salary/ 子目录（更可靠）
    for d in search_dirs:
        try:
            for fname in os.listdir(d):
                if not (fname.endswith(".xlsx") or fname.endswith(".xls")):
                    continue
                fpath = os.path.join(d, fname)
                ym = _extract_year_month(fname)
                if ym:
                    candidates.append((ym[0], ym[1], fpath))
                # 只在 salary/ 子目录里使用 mtime 兜底（避免把员工列表误识别）
                elif d.endswith("salary") or d.endswith("salary/") or d.endswith("salary\\"):
                    if _looks_like_salary_file(fpath):
                        try:
                            mtime = os.path.getmtime(fpath)
                            import datetime
                            dt = datetime.datetime.fromtimestamp(mtime)
                            candidates.append((dt.year, dt.month, fpath))
                        except OSError:
                            pass
        except Exception:
            pass

    # 去重（按 year-month），同月只保留第一个
    for y, m, _f in candidates:
        key = (y, m)
        if key not in seen:
            seen.add(key)
            months.append({"year": y, "month": m})

    months.sort(key=lambda x: (x["year"], x["month"]), reverse=True)
    return months


def _looks_like_salary_file(fpath):
    """快速判断 xlsx 文件是否是工资表（通过读取前几行）"""
    try:
        import openpyxl
        wb = openpyxl.load_workbook(fpath, data_only=True, read_only=True)
        try:
            ws = wb.active
            # 必须有"应发"、"基本工资"、"实发"等工资表特征字段
            # 避免把员工列表（只含"工号"/"姓名"）误判为工资表
            strong_keywords = ("应发", "实发", "基本工资", "绩效工资", "岗位工资",
                                "社会保险", "公积金", "个税", "个人所得税")
            for r in range(1, 4):
                for c in range(1, 11):
                    try:
                        v = ws.cell(r, c).value
                        if v and any(k in str(v) for k in strong_keywords):
                            return True
                    except Exception:
                        pass
            return False
        finally:
            try:
                wb.close()
            except Exception:
                pass
    except Exception:
        return False
