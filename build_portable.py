"""
便携版打包脚本（一键完成）
- 自动运行 PyInstaller 打包成 onedir 模式
- 把 data/ 复制到 EXE 同级目录（用户可编辑）
- 生成简洁的 VBS 启动器（不会显示命令行窗口）
- 自动生成 ZIP 压缩包

用法：
    python build_portable.py
    python build_portable.py --no-zip     # 不生成 zip
    python build_portable.py --keep-build  # 保留 PyInstaller 中间文件
"""
import os
import sys
import shutil
import zipfile
import argparse
import subprocess
from pathlib import Path

ROOT = Path(__file__).parent
DIST = ROOT / "dist"
BUILD = ROOT / "build"
OUTPUT = ROOT / "便携版"
ZIP_PATH = ROOT / "便携版.zip"
APP_DIR = "工资查询系统"
EXE_NAME = "工资查询系统.exe"


# 需要打包进 EXE 的数据（运行时只读）
EMBEDDED_DATA = [
    ("templates", "templates"),
    ("static", "static"),
    ("app.py", "."),
    ("excel_data.py", "."),
    ("cache.py", "."),
    ("colors.py", "."),
    ("control_panel_pkg", "control_panel_pkg"),
]


# 复制到 EXE 同级目录（用户可编辑）
EDITABLE_DATA = [
    "data",
]

# 这些文件不应该被打包到便携版（开发期的日志/密钥）
EXCLUDE_PATTERNS = [
    "access.log",
    "secret.key",
    "startup.log",
    "__pycache__",
    "*.pyc",
    ".pytest_cache",
    "*.log",
]


def parse_args():
    parser = argparse.ArgumentParser(description="便携版打包脚本")
    parser.add_argument("--no-zip", action="store_true",
                        help="不生成 .zip 压缩包")
    parser.add_argument("--keep-build", action="store_true",
                        help="保留 PyInstaller 中间文件（build/, *.spec）")
    parser.add_argument("--no-build", action="store_true",
                        help="跳过 PyInstaller 打包，只整理便携版目录")
    return parser.parse_args()


def run_pyinstaller():
    """运行 PyInstaller 打包（用 SPEC 文件）"""
    print("\n" + "=" * 60)
    print("[1/4] 运行 PyInstaller 打包")
    print("=" * 60)

    spec_file = ROOT / "工资查询系统.spec"
    if not spec_file.exists():
        print(f"[错误] 未找到 SPEC 文件: {spec_file}")
        sys.exit(1)

    cmd = [
        sys.executable, "-m", "PyInstaller",
        "--noconfirm",
        "--clean",
        str(spec_file),
    ]
    print("执行命令:")
    print(" ".join(cmd))
    print()

    result = subprocess.run(cmd, cwd=ROOT)
    if result.returncode != 0:
        print(f"\n[错误] PyInstaller 打包失败，退出码 {result.returncode}")
        sys.exit(1)

    src_app = DIST / APP_DIR
    if not src_app.exists():
        print(f"[错误] 预期产物不存在: {src_app}")
        sys.exit(1)
    print(f"\n[OK] 打包完成: {src_app}")


def make_vbs_launcher():
    """
    生成简洁的 VBS 启动器。
    关键：用 GBK 编码保存并直接用中文（Windows 中文系统的 VBS 默认 ANSI 编码 = GBK）
    """
    # VBS 用 GBK 编码，EXE 名用纯中文
    # 不用 ChrW/Unicode 转义，简单直接
    vbs = f'''On Error Resume Next
Set WshShell = CreateObject("WScript.Shell")
Set fso = CreateObject("Scripting.FileSystemObject")

' 找到本脚本所在目录
scriptDir = fso.GetParentFolderName(WScript.ScriptFullName)

' 在同目录或子目录查找 EXE
exePath = scriptDir & "\\{EXE_NAME}"
If Not fso.FileExists(exePath) Then
    exePath = scriptDir & "\\{APP_DIR}\\{EXE_NAME}"
End If
If Not fso.FileExists(exePath) Then
    MsgBox "未找到 {EXE_NAME}！请确认 {APP_DIR} 文件夹与本脚本在同一目录", 48, "启动失败"
    WScript.Quit
End If

' 以隐藏窗口方式启动 EXE（0=隐藏, False=不等结束）
WshShell.Run """" & exePath & """", 0, False

Set fso = Nothing
Set WshShell = Nothing
'''
    return vbs


def make_bat_launcher():
    """生成 BAT 启动器（备用）"""
    bat = f'''@echo off
chcp 65001 >nul
title 员工工资查询系统
cd /d "%~dp0"
if exist "{APP_DIR}\\{EXE_NAME}" (
    start "" "%~dp0{APP_DIR}\\{EXE_NAME}"
) else if exist "{EXE_NAME}" (
    start "" "%~dp0{EXE_NAME}"
) else (
    echo 错误：未找到 {EXE_NAME}
    pause
)
'''
    return bat


def make_readme(port: int = 5001):
    """生成使用说明"""
    readme = f"""员工工资查询系统 - 便携版
============================

【快速开始】
1. 双击 "启动.vbs"（推荐，窗口隐藏）
   或双击 "启动.bat"（会显示命令提示符窗口）
2. 等待几秒，弹出控制面板窗口
3. 点击 "启动服务" 按钮
4. 浏览器自动打开工资查询页（如未自动打开，手动访问下方链接）

【访问地址】
本机访问:    http://localhost:{port}
局域网访问:  http://本机IP:{port}
HTTPS:       https://localhost:{port + 1}（首次会有证书警告，点"继续访问"即可）

【默认登录】
账号 = 工号（如 E0001）
密码 = 紧急联系人手机号

【目录结构】
便携版/
├── 启动.vbs          <-- 双击这个启动（推荐）
├── 启动.bat          <-- 备选启动方式
├── 使用说明.txt      <-- 本文件
├── {APP_DIR}/        <-- 主程序
│   ├── {EXE_NAME}
│   ├── _internal/    <-- Python 运行时（不要删）
│   ├── data/         <-- 数据文件夹
│   │   ├── 员工列表.xlsx
│   │   ├── port.txt
│   │   ├── fields.txt
│   │   ├── salary/   <-- 工资表
│   │   └── certs/    <-- HTTPS 证书
│   └── ...
└── ...

【修改端口】
1. 打开 {APP_DIR}\\data\\port.txt
2. 修改数字（如 5001 → 8080）
3. 保存后在控制面板点击"保存端口"，再"启动服务"

【更换数据】
- 员工账号:    编辑 data\\员工列表.xlsx
- 工资数据:    把新工资表（如 "2026年06月员工工资.xlsx"）放到 data\\salary\\
- 字段配置:    编辑 data\\fields.txt

【常见问题】
Q: 双击 VBS 没反应？
A: 1) 尝试右键 → 以管理员身份运行
   2) 看一眼 {APP_DIR}\\startup.log 是否有错误
   3) 改用 启动.bat 看看命令提示符报什么错

Q: 提示端口被占用？
A: 1) 改端口：编辑 data\\port.txt
   2) 或者：netstat -ano | findstr :5001  找到占用进程，taskkill /PID <pid> /F

Q: 局域网其他电脑访问不到？
A: 1) 确认 Windows 防火墙允许了 {EXE_NAME}
   2) 控制面板 → 系统和安全 → Windows Defender 防火墙 → 允许应用通过防火墙
   3) 在控制面板中可以查看本机 IP

Q: 杀毒软件误报？
A: 1) 将整个 {APP_DIR} 文件夹加入信任/白名单
   2) PyInstaller 打包的 EXE 是正常现象，非病毒

【复制到其他电脑】
将整个 "便携版" 文件夹复制到目标电脑，双击 启动.vbs 即可。
无需安装 Python，无需任何依赖。数据都在文件夹里。

【技术支持】
请保留 README.md 和源代码以备参考。
"""
    return readme


def assemble_portable(skip_build: bool = False):
    """组装便携版目录"""
    print("\n" + "=" * 60)
    print("[2/4] 组装便携版目录")
    print("=" * 60)

    src_app = DIST / APP_DIR
    if not src_app.exists():
        print(f"[错误] {src_app} 不存在，请先运行 PyInstaller 打包")
        sys.exit(1)

    # 清空并重建输出目录
    if OUTPUT.exists():
        shutil.rmtree(OUTPUT)
    OUTPUT.mkdir()

    # 复制 PyInstaller 产物
    dst_app = OUTPUT / APP_DIR
    shutil.copytree(src_app, dst_app)
    print(f"[OK] 复制主程序: {dst_app}")

    # 复制可编辑数据
    for data_dir in EDITABLE_DATA:
        src = ROOT / data_dir
        if not src.exists():
            print(f"[警告] {src} 不存在，跳过")
            continue
        dst = dst_app / data_dir
        if dst.exists():
            shutil.rmtree(dst)
        # 使用 ignore 排除不需要的文件
        def _ignore(src_path, names):
            ignored = set()
            for n in names:
                for pat in EXCLUDE_PATTERNS:
                    if Path(n).match(pat):
                        ignored.add(n)
                        break
            return ignored
        shutil.copytree(src, dst, ignore=_ignore)
        print(f"[OK] 复制数据: {dst}（已排除 dev 残留: access.log, secret.key, *.log 等）")

    # 强制删除 secret.key（让生产环境首次启动重新生成独立的密钥）
    secret_key = dst_app / "data" / "secret.key"
    if secret_key.exists():
        secret_key.unlink()
        print(f"[OK] 清除 dev 密钥（运行时自动生成新密钥）")

    # 读取端口配置
    port_file = dst_app / "data" / "port.txt"
    port = 5001
    if port_file.exists():
        try:
            port = int(port_file.read_text(encoding="utf-8").strip())
        except (ValueError, OSError):
            pass

    # 生成 VBS 启动器
    vbs = make_vbs_launcher()
    vbs_path = OUTPUT / "启动.vbs"
    vbs_path.write_text(vbs, encoding="gbk")
    print(f"[OK] 生成启动器: {vbs_path} (GBK)")

    # 生成 BAT 启动器（备用）
    bat = make_bat_launcher()
    bat_path = OUTPUT / "启动.bat"
    bat_path.write_text(bat, encoding="utf-8")
    print(f"[OK] 生成备用启动器: {bat_path}")

    # 生成使用说明
    readme = make_readme(port)
    readme_path = OUTPUT / "使用说明.txt"
    readme_path.write_text(readme, encoding="utf-8")
    print(f"[OK] 生成简易使用说明: {readme_path}")

    # 复制完整使用说明书（MD 版本）
    src_manual = ROOT / "使用说明书.md"
    if src_manual.exists():
        dst_manual = OUTPUT / "使用说明书.md"
        shutil.copy2(src_manual, dst_manual)
        print(f"[OK] 复制完整说明书: {dst_manual}")

    # 复制 Word 版本说明书
    src_docx = ROOT / "使用说明书.docx"
    if src_docx.exists():
        dst_docx = OUTPUT / "使用说明书.docx"
        shutil.copy2(src_docx, dst_docx)
        print(f"[OK] 复制 Word 说明书: {dst_docx}")

    return port


def make_zip(port: int, no_zip: bool):
    """生成 ZIP 压缩包"""
    print("\n" + "=" * 60)
    print("[3/4] 生成 ZIP 压缩包")
    print("=" * 60)

    if no_zip:
        print("[跳过] --no-zip 已指定")
        return

    if ZIP_PATH.exists():
        ZIP_PATH.unlink()

    with zipfile.ZipFile(ZIP_PATH, "w", zipfile.ZIP_DEFLATED, compresslevel=6) as zf:
        file_count = 0
        total_size = 0
        for f in OUTPUT.rglob("*"):
            if f.is_file():
                arcname = f.relative_to(OUTPUT)
                zf.write(f, arcname)
                file_count += 1
                total_size += f.stat().st_size
    zip_size = ZIP_PATH.stat().st_size
    print(f"[OK] ZIP: {ZIP_PATH}")
    print(f"     文件数: {file_count}")
    print(f"     原始大小: {total_size / 1024 / 1024:.1f} MB")
    print(f"     压缩后:   {zip_size / 1024 / 1024:.1f} MB")
    print(f"     压缩率:   {zip_size / total_size * 100:.1f}%")


def cleanup(keep_build: bool):
    """清理中间文件"""
    print("\n" + "=" * 60)
    print("[4/4] 清理中间文件")
    print("=" * 60)

    if not keep_build:
        if BUILD.exists():
            shutil.rmtree(BUILD)
            print(f"[OK] 清理: {BUILD}")
        # 保留 SPEC 文件方便后续增量打包，只清理临时 spec
        for spec in ROOT.glob("*.spec"):
            if spec.name != "工资查询系统.spec":
                spec.unlink()
                print(f"[OK] 清理: {spec}")
    else:
        print("[跳过] --keep-build 已指定")


def main():
    args = parse_args()

    print("=" * 60)
    print("  员工工资查询系统 - 便携版打包工具")
    print("=" * 60)
    print(f"  Python:   {sys.version.split()[0]}")
    print(f"  平台:     {sys.platform}")
    print(f"  工作目录: {ROOT}")
    print()

    # 1. PyInstaller 打包
    if not args.no_build:
        run_pyinstaller()
    else:
        print("[跳过 PyInstaller 打包] --no-build 已指定")

    # 2. 组装便携版
    port = assemble_portable(skip_build=args.no_build)

    # 3. ZIP
    make_zip(port, args.no_zip)

    # 4. 清理
    cleanup(args.keep_build)

    # 完成
    print("\n" + "=" * 60)
    print("  ✓ 便携版打包完成")
    print("=" * 60)
    print(f"  便携版目录: {OUTPUT}")
    print(f"  ZIP 压缩包: {ZIP_PATH} (如未禁用)")
    print()
    print(f"  复制 {OUTPUT.name}/ 文件夹到目标电脑，双击 启动.vbs 即可使用")
    print()
    return 0


if __name__ == "__main__":
    sys.exit(main())
