#!/usr/bin/env python3
"""把员工表里的明文口令批量换成 PBKDF2 哈希。

只依赖 Python 标准库：读 XLSX 用 zipfile + xml，写 XLSX 复用
generate_sample_data.py 里的 write_xlsx。

哈希格式与 Rust 后端完全一致：
    $pbkdf2-sha256$<迭代次数>$<盐(base64)>$<派生密钥(base64)>
盐与派生密钥都用标准 base64（去掉结尾的 =），与 Rust 侧的
STANDARD_NO_PAD 一致。Rust 的 password 模块里有跨实现的测试向量，
保证两边算出来的结果相同。

安全约束：
- 只处理表头里带「密码」二字的列。表里没有「密码」列时直接报错退出，
  绝不碰手机号 / 紧急联系人号码那几列——把它们哈希掉就等于毁掉了联系方式。
- 已经是哈希值的单元格原样保留，可以放心重复执行。

用法：
    python scripts/hash_passwords.py                       # 生成 data/员工列表-哈希版.xlsx
    python scripts/hash_passwords.py --in-place            # 原地替换，原文件备份为 .bak
    python scripts/hash_passwords.py --dry-run              # 只报告将要改什么
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import os
import sys
import zipfile
from pathlib import Path
from xml.etree import ElementTree

sys.path.insert(0, str(Path(__file__).resolve().parent))

from generate_sample_data import write_xlsx  # noqa: E402

SPREADSHEET_NS = "{http://schemas.openxmlformats.org/spreadsheetml/2006/main}"
RELATIONSHIP_NS = "{http://schemas.openxmlformats.org/package/2006/relationships}"

#: 与 Rust 侧 password.rs 的 ITERATIONS 保持一致
DEFAULT_ITERATIONS = 210_000

#: 盐长度（字节），与 Rust 侧的 SALT_LEN 一致
SALT_LENGTH = 16

#: 哈希串前缀，用来识别「已经处理过」的单元格
HASH_PREFIX = "$pbkdf2-sha256$"

#: 识别口令列的表头关键字
PASSWORD_HEADER_KEYWORD = "密码"


def _column_index(cell_ref: str) -> int:
    """把单元格引用（如 "B7"）里的列部分转成 0 起始的列号。"""
    index = 0
    for char in cell_ref:
        if not char.isalpha():
            break
        index = index * 26 + (ord(char.upper()) - ord("A") + 1)
    return index - 1


def _read_shared_strings(archive: zipfile.ZipFile) -> list[str]:
    """读取 sharedStrings.xml。"""
    if "xl/sharedStrings.xml" not in archive.namelist():
        return []

    root = ElementTree.fromstring(archive.read("xl/sharedStrings.xml"))
    return [
        "".join(node.text or "" for node in item.iter(f"{SPREADSHEET_NS}t"))
        for item in root.findall(f"{SPREADSHEET_NS}si")
    ]


def _first_sheet_path(archive: zipfile.ZipFile) -> str:
    """按 workbook.xml 的顺序找到第一个工作表文件。"""
    if "xl/workbook.xml" in archive.namelist():
        root = ElementTree.fromstring(archive.read("xl/workbook.xml"))
        rels: dict[str, str] = {}

        if "xl/_rels/workbook.xml.rels" in archive.namelist():
            rels_root = ElementTree.fromstring(archive.read("xl/_rels/workbook.xml.rels"))
            for relation in rels_root.findall(f"{RELATIONSHIP_NS}Relationship"):
                rels[relation.get("Id", "")] = relation.get("Target", "")

        sheets = root.find(f"{SPREADSHEET_NS}sheets")
        if sheets is not None:
            for sheet in sheets.findall(f"{SPREADSHEET_NS}sheet"):
                rid = sheet.get(
                    "{http://schemas.openxmlformats.org/officeDocument/2006/relationships}id", ""
                )
                target = rels.get(rid, "")
                if target:
                    candidate = target.lstrip("/")
                    if not candidate.startswith("xl/"):
                        candidate = f"xl/{candidate}"
                    if candidate in archive.namelist():
                        return candidate

    # 兜底：按文件名排序取第一个工作表
    worksheets = sorted(name for name in archive.namelist() if name.startswith("xl/worksheets/"))
    if not worksheets:
        raise ValueError("工作簿里没有工作表")
    return worksheets[0]


def _read_sheet(archive: zipfile.ZipFile, sheet_path: str, shared: list[str]) -> list[list[str]]:
    """把工作表读成二维文本数组。"""
    root = ElementTree.fromstring(archive.read(sheet_path))
    rows: list[list[str]] = []

    for row_element in root.iter(f"{SPREADSHEET_NS}row"):
        cells: dict[int, str] = {}

        for cell in row_element.findall(f"{SPREADSHEET_NS}c"):
            column = _column_index(cell.get("r", ""))
            cell_type = cell.get("t", "")

            if cell_type == "s":
                value = cell.find(f"{SPREADSHEET_NS}v")
                text = shared[int(value.text)] if value is not None and value.text else ""
            elif cell_type == "inlineStr":
                node = cell.find(f"{SPREADSHEET_NS}is/{SPREADSHEET_NS}t")
                text = node.text if node is not None and node.text else ""
            else:
                value = cell.find(f"{SPREADSHEET_NS}v")
                text = value.text if value is not None and value.text else ""

            cells[column] = text

        width = max(cells) + 1 if cells else 0
        rows.append([cells.get(index, "") for index in range(width)])

    return rows


def read_xlsx(path: Path) -> list[list[str]]:
    """读取 XLSX 第一个工作表为二维文本数组。"""
    with zipfile.ZipFile(path) as archive:
        shared = _read_shared_strings(archive)
        sheet_path = _first_sheet_path(archive)
        return _read_sheet(archive, sheet_path, shared)


def hash_password(password: str, iterations: int) -> str:
    """生成与 Rust 后端一致的 PBKDF2-HMAC-SHA256 哈希串。"""
    salt = os.urandom(SALT_LENGTH)
    derived = hashlib.pbkdf2_hmac("sha256", password.encode("utf-8"), salt, iterations, 32)

    encode = lambda raw: base64.b64encode(raw).decode("ascii").rstrip("=")  # noqa: E731
    return f"{HASH_PREFIX}{iterations}${encode(salt)}${encode(derived)}"


def find_password_column(headers: list[str]) -> int:
    """定位口令列。

    只认表头里带「密码」二字的列；找不到就抛错，
    绝不能退回到手机号 / 紧急联系人号码那几列。
    """
    for index, header in enumerate(headers):
        if PASSWORD_HEADER_KEYWORD in header.strip():
            return index

    raise ValueError(
        "员工表里没有「密码」列，为避免把手机号等联系方式误当口令处理，"
        "请先在员工表中增加一列「密码」再执行本脚本"
    )


def main() -> int:
    parser = argparse.ArgumentParser(description="把员工表里的明文口令批量换成 PBKDF2 哈希")
    parser.add_argument("--file", default="data/员工列表.xlsx", help="员工表路径")
    parser.add_argument("--output", help="输出路径，默认在原文件名后加「-哈希版」")
    parser.add_argument(
        "--iterations", type=int, default=DEFAULT_ITERATIONS, help="PBKDF2 迭代次数"
    )
    parser.add_argument(
        "--in-place", action="store_true", help="原地替换，原文件备份为 <文件名>.bak"
    )
    parser.add_argument("--dry-run", action="store_true", help="只报告将要改动什么，不写文件")
    args = parser.parse_args()

    source = Path(args.file)
    if not source.exists():
        print(f"找不到员工表：{source}", file=sys.stderr)
        return 1

    rows = read_xlsx(source)
    if not rows:
        print("员工表是空的", file=sys.stderr)
        return 1

    headers = rows[0]
    try:
        column = find_password_column(headers)
    except ValueError as error:
        print(str(error), file=sys.stderr)
        return 1

    print(f"员工表：{source}")
    print(f"口令列：第 {column + 1} 列「{headers[column].strip()}」")

    changed = 0
    skipped = 0

    for row in rows[1:]:
        if column >= len(row):
            continue

        password = row[column]
        if not password.strip():
            continue

        if password.startswith(HASH_PREFIX):
            skipped += 1
            continue

        row[column] = hash_password(password, args.iterations)
        changed += 1

    print(f"已哈希 {changed} 个口令，跳过 {skipped} 个（本来就已是哈希值）")

    if args.dry_run:
        print("dry-run 模式，没有写入任何文件")
        return 0

    if args.in_place:
        target = source
        backup = source.with_suffix(source.suffix + ".bak")
        backup.write_bytes(source.read_bytes())
        print(f"原文件已备份到 {backup}")
    else:
        target = Path(args.output) if args.output else source.with_name(
            f"{source.stem}-哈希版{source.suffix}"
        )

    write_xlsx(target, rows, "员工列表")
    print(f"已写出 {target}")

    if not args.in_place:
        print("确认无误后，把新文件替换掉原员工表即可；服务会自动重新加载。")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
