#!/usr/bin/env python3
"""生成示例员工表与工资表。

只依赖 Python 标准库（zipfile + xml），不需要安装 openpyxl。
生成的 XLSX 可被 Rust 后端的 calamine 正常解析。

用法：
    python scripts/generate_sample_data.py [--data-dir data] [--months 3]
"""

from __future__ import annotations

import argparse
import datetime
import zipfile
from pathlib import Path
from xml.sax.saxutils import escape

CONTENT_TYPES = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>
<Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>
</Types>"""

ROOT_RELS = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"""

WORKBOOK = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets><sheet name="{sheet}" sheetId="1" r:id="rId1"/></sheets>
</workbook>"""

WORKBOOK_RELS = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
<Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>
</Relationships>"""

STYLES = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts>
<fills count="1"><fill><patternFill patternType="none"/></fill></fills>
<borders count="1"><border/></borders>
<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>
<cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs>
</styleSheet>"""


def column_letter(index: int) -> str:
    """0 -> A, 25 -> Z, 26 -> AA"""
    letters = ""
    index += 1
    while index:
        index, remainder = divmod(index - 1, 26)
        letters = chr(65 + remainder) + letters
    return letters


def write_xlsx(path: Path, rows: list[list[object]], sheet_name: str = "Sheet1") -> None:
    """把二维数组写成 XLSX 文件。

    字符串走 sharedStrings，数字写成数值单元格。
    """
    shared: list[str] = []
    shared_index: dict[str, int] = {}

    def index_of(text: str) -> int:
        if text not in shared_index:
            shared_index[text] = len(shared)
            shared.append(text)
        return shared_index[text]

    sheet_rows: list[str] = []

    for row_number, row in enumerate(rows, start=1):
        cells: list[str] = []
        for column_number, value in enumerate(row):
            reference = f"{column_letter(column_number)}{row_number}"

            if value is None or value == "":
                continue

            if isinstance(value, (int, float)) and not isinstance(value, bool):
                cells.append(f'<c r="{reference}"><v>{value}</v></c>')
            else:
                cells.append(
                    f'<c r="{reference}" t="s"><v>{index_of(str(value))}</v></c>'
                )

        sheet_rows.append(f'<row r="{row_number}">{"".join(cells)}</row>')

    sheet_xml = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
        f"<sheetData>{''.join(sheet_rows)}</sheetData>"
        "</worksheet>"
    )

    strings_xml = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
        f'count="{len(shared)}" uniqueCount="{len(shared)}">'
        + "".join(f"<si><t>{escape(text)}</t></si>" for text in shared)
        + "</sst>"
    )

    path.parent.mkdir(parents=True, exist_ok=True)

    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as archive:
        archive.writestr("[Content_Types].xml", CONTENT_TYPES)
        archive.writestr("_rels/.rels", ROOT_RELS)
        archive.writestr("xl/workbook.xml", WORKBOOK.format(sheet=escape(sheet_name)))
        archive.writestr("xl/_rels/workbook.xml.rels", WORKBOOK_RELS)
        archive.writestr("xl/styles.xml", STYLES)
        archive.writestr("xl/sharedStrings.xml", strings_xml)
        archive.writestr("xl/worksheets/sheet1.xml", sheet_xml)


EMPLOYEE_HEADERS = ["工号", "姓名", "部门", "密码", "手机号", "角色", "在职状态"]

EMPLOYEES = [
    ["001", "张三", "工程部", "salary2026", "13800000001", "员工", "在职"],
    ["002", "李四", "财务部", "salary2026", "13800000002", "员工", "在职"],
    ["003", "王五", "人力资源部", "admin2026", "13800000003", "管理员", "在职"],
    ["004", "赵六", "市场部", "salary2026", "13800000004", "员工", "在职"],
    ["005", "钱七", "工程部", "salary2026", "13800000005", "员工", "离职"],
]

SALARY_HEADERS = [
    "员工号",
    "姓名",
    "部门",
    "基本工资",
    "岗位工资",
    "绩效工资",
    "津贴",
    "扣款",
    "社保",
    "个税",
    "本月实发工资",
]


def build_salary_rows(year: int, month: int) -> list[list[object]]:
    """按月份生成工资行，数值随月份小幅浮动。"""
    rows: list[list[object]] = [SALARY_HEADERS]

    bases = [9800, 8600, 9200, 7800, 8100]

    for index, employee in enumerate(EMPLOYEES):
        employee_id, name, dept = employee[0], employee[1], employee[2]

        # 离职员工没有当月工资
        if employee[6] == "离职":
            continue

        # 用月份做一个确定性的伪随机波动，保证每次生成结果一致
        seed = (year * 12 + month + index) % 7
        base = bases[index] + seed * 120
        position = 1500 + seed * 80
        performance = 1200 + seed * 260
        allowance = 300 + seed * 50
        deduction = 200 + seed * 40
        social_insurance = round(base * 0.105, 2)
        tax = round(max(base + position + performance - deduction - social_insurance - 5000, 0) * 0.1, 2)
        net = round(base + position + performance + allowance - deduction - social_insurance - tax, 2)

        rows.append([
            employee_id,
            name,
            dept,
            base,
            position,
            performance,
            allowance,
            deduction,
            social_insurance,
            tax,
            net,
        ])

    return rows


def write_raw_employee_file(path: Path) -> None:
    """写出「原始模式」员工表：每列一名员工，前 5 行分别是工号/密码/姓名/部门/管理员。

    这种表没有表头，与 Python 原版支持的第二种员工表结构一致。
    """
    rows = [
        ["001", "002", "003"],
        ["salary2026", "salary2026", "admin2026"],
        ["张三", "李四", "王五"],
        ["工程部", "财务部", "人力资源部"],
        ["员工", "员工", "管理员"],
    ]
    write_xlsx(path, rows, "员工列表")


def main() -> None:
    parser = argparse.ArgumentParser(description="生成示例员工表与工资表")
    parser.add_argument("--data-dir", default="data", help="数据目录，默认 data")
    parser.add_argument("--months", type=int, default=3, help="生成最近几个月的工资表")
    parser.add_argument(
        "--raw-employee-file",
        help="额外输出一份「原始模式」员工表（每列一名员工），用于测试该解析分支",
    )
    parser.add_argument(
        "--employee-fixture",
        help=(
            "额外输出一份明文口令的员工表，供集成测试当登录夹具用。"
            "data/ 下的员工表在正式使用时会被换成哈希口令，"
            "集成测试需要一份口令可读的固定样本"
        ),
    )
    args = parser.parse_args()

    data_dir = Path(args.data_dir)
    today = datetime.date.today()

    # 员工表
    employee_file = data_dir / "员工列表.xlsx"
    write_xlsx(employee_file, [EMPLOYEE_HEADERS] + EMPLOYEES, "员工列表")
    print(f"已生成 {employee_file}")

    # 原始模式员工表
    if args.raw_employee_file:
        raw_file = Path(args.raw_employee_file)
        write_raw_employee_file(raw_file)
        print(f"已生成 {raw_file}")

    # 集成测试用的明文口令样本
    if args.employee_fixture:
        fixture = Path(args.employee_fixture)
        write_xlsx(fixture, [EMPLOYEE_HEADERS] + EMPLOYEES, "员工列表")
        print(f"已生成 {fixture}")

    # 工资表
    salary_dir = data_dir / "salary"
    for offset in range(args.months):
        year = today.year
        month = today.month - offset
        while month <= 0:
            month += 12
            year -= 1

        file = salary_dir / f"{year} 年 {month:02d} 月员工工资.xlsx"
        write_xlsx(file, build_salary_rows(year, month), "工资表")
        print(f"已生成 {file}")


if __name__ == "__main__":
    main()
