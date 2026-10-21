#!/usr/bin/env python3
"""把 Python 原版的 Jinja 模板渲染成静态 HTML，用于与 Rust 版做视觉比对。

仅用于开发期的对照检查，不是运行时代码的一部分。

用法：
    python scripts/render_original_preview.py --api http://localhost:5001 --out /tmp/original
"""

from __future__ import annotations

import argparse
import json
import re
import urllib.request
from pathlib import Path

# Python 原版的模板目录，可用 --templates 覆盖
DEFAULT_TEMPLATE_DIR = Path(r"E:\workspace\salary_app\templates")


def fetch(url: str) -> dict:
    with urllib.request.urlopen(url, timeout=10) as response:
        return json.loads(response.read().decode("utf-8"))


def api_post(url: str, payload: dict) -> dict:
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=10) as response:
        return json.loads(response.read().decode("utf-8"))


def render_login(out_dir: Path, template_dir: Path) -> None:
    html = (template_dir / "login.html").read_text(encoding="utf-8")
    html = html.replace("{% if error %}", "").replace("{% endif %}", "")
    html = html.replace("{% if expired %}", "").replace("{{ error }}", "")
    html = html.replace("{{ csrf_token() }}", "preview-token")
    (out_dir / "login.html").write_text(html, encoding="utf-8")


def render_salary(
    out_dir: Path,
    template_dir: Path,
    months: list[dict],
    year: int,
    month: int,
    payslip: dict,
    name: str,
) -> None:
    html = (template_dir / "salary.html").read_text(encoding="utf-8")

    years = sorted({m["year"] for m in months}, reverse=True)
    months_for_year = sorted([m["month"] for m in months if m["year"] == year], reverse=True)
    fields = payslip["fields"]
    net_salary = payslip["net_salary"]

    # 年份 / 月份下拉
    year_options = "".join(
        f'<option value="{y}"{" selected" if y == year else ""}>{y}年</option>' for y in years
    )
    month_options = "".join(
        f'<option value="{m}"{" selected" if m == month else ""}>{m}月</option>'
        for m in months_for_year
    )
    html = re.sub(
        r"\{% for y in available_years %\}.*?\{% endfor %\}", year_options, html, flags=re.S
    )
    html = re.sub(
        r"\{% for m in months_for_year %\}.*?\{% endfor %\}", month_options, html, flags=re.S
    )

    # 工资条字段
    header_cells = "".join(f"<th>{f['label']}</th>" for f in fields)
    body_cells = "".join(f"<td>{f['value'] if f['value'] else ''}</td>" for f in fields)
    html = re.sub(
        r"\{% for f in payslip\.fields %\}\s*<th>\{\{ f\.label \}\}</th>\s*\{% endfor %\}",
        header_cells,
        html,
        flags=re.S,
    )
    html = re.sub(
        r"\{% for f in payslip\.fields %\}\s*<td>\s*\{\{ f\.value if f\.value else '' \}\}\s*</td>\s*\{% endfor %\}",
        body_cells,
        html,
        flags=re.S,
    )

    # 其余变量
    html = html.replace("{{ employee.name }}", name)
    html = html.replace("{{ csrf_token()|tojson }}", '"preview-token"')
    html = html.replace("{{ csrf_token() }}", "preview-token")
    html = html.replace(
        "{{ url_for('static', filename='js/html2canvas.min.js') }}",
        "https://cdn.jsdelivr.net/npm/html2canvas@1.4.1/dist/html2canvas.min.js",
    )
    html = html.replace("{{ selected_year|tojson }}", f'"{year}"')
    html = html.replace("{{ selected_month|tojson }}", f'"{month}"')
    html = html.replace(
        "{{ ((payslip.employee.name if payslip else \"\") ~ \" - \" ~ selected_year ~ \"年\" ~ selected_month ~ \"月工资条\")|tojson }}",
        f'"{name} - {year}年{month}月工资条"',
    )
    html = html.replace(
        '{{ (payslip.employee.name if payslip else "")|tojson }}', f'"{name}"'
    )
    html = html.replace(
        "{{ payslip.employee.year }}年{{ payslip.employee.month }}月 工资条",
        f"{year}年{month}月 工资条",
    )
    html = html.replace(
        "{{ payslip.row.net_salary or '0' }}", str(net_salary if net_salary else "0")
    )

    # 去掉剩余的 Jinja 条件块
    html = re.sub(r"\{%\s*if payslip\s*%\}", "", html)
    html = re.sub(r"\{%\s*endif\s*%\}", "", html)
    html = re.sub(r"\{%\s*if error\s*%\}", "", html)
    html = re.sub(r"\{%\s*elif payslip\s*%\}", "", html)
    html = re.sub(r"\{%\s*else\s*%\}", "", html)
    html = re.sub(r"\{%.*?%\}", "", html, flags=re.S)
    html = re.sub(r"\{\{.*?\}\}", "", html, flags=re.S)

    (out_dir / "salary.html").write_text(html, encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--api", default="http://localhost:5001")
    parser.add_argument("--out", default="/tmp/original")
    parser.add_argument(
        "--templates",
        default=str(DEFAULT_TEMPLATE_DIR),
        help="Python 原版的 templates 目录",
    )
    args = parser.parse_args()

    template_dir = Path(args.templates)
    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)

    login = api_post(
        f"{args.api}/api/auth/login", {"username": "001", "password": "salary2026"}
    )
    token = login["token"]

    request = urllib.request.Request(
        f"{args.api}/api/available_months", headers={"Authorization": f"Bearer {token}"}
    )
    with urllib.request.urlopen(request, timeout=10) as response:
        months = json.loads(response.read().decode("utf-8"))["months"]

    year, month = months[0]["year"], months[0]["month"]

    request = urllib.request.Request(
        f"{args.api}/api/salary/{year}/{month}", headers={"Authorization": f"Bearer {token}"}
    )
    with urllib.request.urlopen(request, timeout=10) as response:
        payload = json.loads(response.read().decode("utf-8"))

    headers = payload["headers"]
    values = payload["values"]
    fields = [
        {"label": h.strip(), "value": values[i]}
        for i, h in enumerate(headers)
        if h.strip() and h.strip() != "员工号"
    ]
    net_salary = next(
        (f["value"] for f in fields if f["label"] in ("本月实发工资", "实发工资")), ""
    )
    payslip = {"fields": fields, "net_salary": net_salary}

    render_login(out_dir, template_dir)
    render_salary(out_dir, template_dir, months, year, month, payslip, login["employee"]["name"])

    print(f"已渲染到 {out_dir}")
    print(f"  login.html")
    print(f"  salary.html  ({year}年{month}月, {len(fields)} 个字段)")


if __name__ == "__main__":
    main()
