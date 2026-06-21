"""
将 使用说明书.md 转换为 Word (.docx) 格式
用法: python md2docx.py
"""
import re
import docx
from docx import Document
from docx.shared import Pt, Inches, RGBColor, Cm
from docx.enum.text import WD_ALIGN_PARAGRAPH
from docx.enum.table import WD_TABLE_ALIGNMENT
from docx.oxml.ns import qn
from pathlib import Path

MD_FILE = Path("使用说明书.md")
DOCX_FILE = Path("使用说明书.docx")


def parse_markdown(text):
    """将 Markdown 解析为结构化行列表"""
    lines = text.split("\n")
    blocks = []
    i = 0
    while i < len(lines):
        line = lines[i]

        # 分隔线 ---
        if line.strip() == "---":
            blocks.append({"type": "hr"})
            i += 1
            continue

        # 标题
        hm = re.match(r"^(#{1,3})\s+(.+)$", line)
        if hm:
            level = len(hm.group(1))
            blocks.append({"type": "heading", "level": level, "text": hm.group(2).strip()})
            i += 1
            continue

        # 块引用 >
        if line.startswith(">"):
            texts = []
            while i < len(lines) and lines[i].startswith(">"):
                t = re.sub(r"^>\s?", "", lines[i])
                texts.append(t)
                i += 1
            blocks.append({"type": "blockquote", "text": "\n".join(texts)})
            continue

        # 代码块
        if line.strip().startswith("```"):
            i += 1
            texts = []
            while i < len(lines) and not lines[i].strip().startswith("```"):
                texts.append(lines[i])
                i += 1
            i += 1  # 跳过结束 ```
            code = "\n".join(texts)
            if code.strip():
                blocks.append({"type": "code", "text": code})
            continue

        # 无序列表
        ulm = re.match(r"^[-*]\s+(.+)$", line)
        if ulm:
            items = []
            while i < len(lines):
                m = re.match(r"^[-*]\s+(.+)$", lines[i])
                if not m:
                    break
                items.append(m.group(1))
                i += 1
            blocks.append({"type": "ulist", "items": items})
            continue

        # 有序列表
        olm = re.match(r"^\d+\.\s+(.+)$", line)
        if olm:
            items = []
            while i < len(lines):
                m = re.match(r"^\d+\.\s+(.+)$", lines[i])
                if not m:
                    break
                items.append(m.group(1))
                i += 1
            blocks.append({"type": "olist", "items": items})
            continue

        # 表格
        if "|" in line and line.strip().startswith("|"):
            table_lines = []
            while i < len(lines) and "|" in lines[i]:
                table_lines.append(lines[i].strip())
                i += 1
            if len(table_lines) >= 3:
                blocks.append({"type": "table", "lines": table_lines})
            continue

        # 普通段落（空行分隔）
        if line.strip():
            para_lines = [line.strip()]
            i += 1
            while i < len(lines) and lines[i].strip() and not lines[i].startswith("#"):
                para_lines.append(lines[i].strip())
                i += 1
            blocks.append({"type": "paragraph", "text": "\n".join(para_lines)})
            continue

        i += 1
    return blocks


def add_run(para, text, bold=False, italic=False, color=None, size=None):
    """添加 run 到段落"""
    run = para.add_run(text)
    run.bold = bold
    run.italic = italic
    if color:
        run.font.color.rgb = RGBColor(*color)
    if size:
        run.font.size = Pt(size)
    run.font.name = "微软雅黑"
    run._element.rPr.rFonts.set(qn("w:eastAsia"), "微软雅黑")
    return run


def add_formatted_text(para, text):
    """处理段落内格式：**粗体**、``代码``、链接 [text](url)"""
    # 按 ** 分割
    parts = re.split(r"(\*\*.*?\*\*)", text)
    for part in parts:
        if part.startswith("**") and part.endswith("**"):
            add_run(para, part[2:-2], bold=True)
        else:
            # 行内代码 `code`
            subparts = re.split(r"(`[^`]+`)", part)
            for sp in subparts:
                if sp.startswith("`") and sp.endswith("`"):
                    run = para.add_run(sp[1:-1])
                    run.font.name = "Consolas"
                    run.font.size = Pt(9)
                else:
                    add_run(para, sp)


def convert_md_to_docx():
    with open(MD_FILE, encoding="utf-8") as f:
        md_text = f.read()

    doc = Document()

    # 页面设置：A4
    section = doc.sections[0]
    section.page_width = Cm(21)
    section.page_height = Cm(29.7)
    section.top_margin = Cm(2.5)
    section.bottom_margin = Cm(2.5)
    section.left_margin = Cm(2.5)
    section.right_margin = Cm(2.5)

    # 设置默认字体
    style = doc.styles["Normal"]
    font = style.font
    font.name = "微软雅黑"
    style.element.rPr.rFonts.set(qn("w:eastAsia"), "微软雅黑")
    font.size = Pt(11)

    blocks = parse_markdown(md_text)

    for block in blocks:
        if block["type"] == "heading":
            h = doc.add_heading(block["text"], level=block["level"])
            for run in h.runs:
                run.font.name = "微软雅黑"
                run._element.rPr.rFonts.set(qn("w:eastAsia"), "微软雅黑")

        elif block["type"] == "paragraph":
            # 检测是否为纯 emoji 段落或短标题行
            para = doc.add_paragraph()
            para.space_after = Pt(4)
            add_formatted_text(para, block["text"])

        elif block["type"] == "blockquote":
            para = doc.add_paragraph()
            para.paragraph_format.left_indent = Cm(1)
            add_formatted_text(para, block["text"])
            # 灰色底纹效果
            from docx.oxml import OxmlElement
            shd = OxmlElement("w:shd")
            shd.set(qn("w:val"), "clear")
            shd.set(qn("w:color"), "auto")
            shd.set(qn("w:fill"), "F2F2F2")
            para.paragraph_format.element.get_or_add_pPr().append(shd)

        elif block["type"] == "code":
            para = doc.add_paragraph()
            para.paragraph_format.left_indent = Cm(0.5)
            run = para.add_run(block["text"])
            run.font.name = "Consolas"
            run.font.size = Pt(9)

        elif block["type"] == "ulist":
            for item in block["items"]:
                para = doc.add_paragraph(style="List Bullet")
                para.paragraph_format.space_after = Pt(2)
                add_formatted_text(para, item)

        elif block["type"] == "olist":
            for item in block["items"]:
                para = doc.add_paragraph(style="List Number")
                para.paragraph_format.space_after = Pt(2)
                add_formatted_text(para, item)

        elif block["type"] == "table":
            rows = []
            for line in block["lines"]:
                if line.startswith("|") and not line.startswith("|-"):
                    cells = [c.strip() for c in line.strip("|").split("|")]
                    if cells and "---" not in cells[0]:
                        rows.append(cells)
            if rows:
                table = doc.add_table(rows=len(rows), cols=len(rows[0]))
                table.style = "Light Grid Accent 1"
                table.alignment = WD_TABLE_ALIGNMENT.CENTER
                for ri, row_cells in enumerate(rows):
                    for ci, cell_text in enumerate(row_cells):
                        cell = table.cell(ri, ci)
                        cell.text = ""
                        para = cell.paragraphs[0]
                        # 表头加粗
                        is_header = ri == 0
                        add_run(para, cell_text, bold=is_header, size=10)

        elif block["type"] == "hr":
            para = doc.add_paragraph()
            run = para.add_run("─" * 60)
            run.font.color.rgb = RGBColor(0xCC, 0xCC, 0xCC)
            run.font.size = Pt(6)

    doc.save(DOCX_FILE)
    size = DOCX_FILE.stat().st_size
    print(f"[OK] 转换完成: {DOCX_FILE} ({size / 1024:.1f} KB)")


if __name__ == "__main__":
    convert_md_to_docx()
