#!/usr/bin/env bash
#
# 构建脚本（Git Bash / Linux / macOS 通用）。
#
# 在 Git Bash 下会自动把 MSVC 的链接器目录插到 PATH 最前面，
# 避免 MSYS 自带的 /usr/bin/link.exe 抢占 MSVC link.exe。
#
# 用法：
#   scripts/build.sh              # 构建 debug 版
#   scripts/build.sh --release    # 构建 release 版
#   scripts/build.sh --test       # 运行测试
#   scripts/build.sh --frontend   # 只构建前端
#   scripts/build.sh --tray       # 只构建托盘控制面板
#   scripts/build.sh --all        # 前端 + 后端 + 托盘

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TAURI_DIR="$ROOT_DIR/src-tauri"

MODE="build"

for arg in "$@"; do
    case "$arg" in
        --release) MODE="release" ;;
        --test) MODE="test" ;;
        --frontend) MODE="frontend" ;;
        --tray) MODE="tray" ;;
        --all) MODE="all" ;;
        -h|--help)
            sed -n '2,17p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "未知参数: $arg" >&2
            exit 1
            ;;
    esac
done

# ---------------------------------------------------------------------------
# MSVC 链接器检测（仅 Windows + Git Bash 需要）
# ---------------------------------------------------------------------------

# 在常见的 Visual Studio 安装位置里查找 MSVC 链接器目录。
find_msvc_bin() {
    local roots=(
        "/c/Program Files/Microsoft Visual Studio/2022"
        "/c/Program Files (x86)/Microsoft Visual Studio/2022"
        "/d/Microsoft Visual Studio/2022"
        "/e/Microsoft Visual Studio/2022"
        "/f/Microsoft Visual Studio/2022"
    )
    local root dir

    for root in "${roots[@]}"; do
        [ -d "$root" ] || continue
        for dir in "$root"/*/VC/Tools/MSVC/*/bin/HostX64/x64; do
            if [ -x "$dir/link.exe" ]; then
                printf '%s\n' "$dir"
                return 0
            fi
        done
    done

    return 1
}

ensure_msvc_linker() {
    # 让 Windows 控制台以 UTF-8 显示中文
    if command -v chcp.com >/dev/null 2>&1; then
        chcp.com 65001 >/dev/null 2>&1 || true
    fi

    # 非 Windows 环境不需要处理
    case "$(uname -s)" in
        MINGW*|MSYS*|CYGWIN*) ;;
        *) return 0 ;;
    esac

    # 已经是 MSVC 的 link.exe 就无需处理
    if command -v link.exe >/dev/null 2>&1; then
        local resolved
        resolved="$(command -v link.exe)"
        case "$resolved" in
            */Visual\ Studio/*) return 0 ;;
        esac
    fi

    local msvc_bin
    if msvc_bin="$(find_msvc_bin)"; then
        export PATH="$msvc_bin:$PATH"
        echo "==> 使用 MSVC 链接器: $msvc_bin"
    else
        echo "警告：未找到 MSVC 链接器，若链接失败请在 Visual Studio 开发者命令行中运行。" >&2
    fi
}

# ---------------------------------------------------------------------------
# 任务
# ---------------------------------------------------------------------------

build_frontend() {
    echo "==> 构建前端"
    cd "$TAURI_DIR"

    if [ ! -d node_modules ]; then
        npm install --no-audit --no-fund
    fi

    npm run build
}

build_backend() {
    echo "==> 构建后端"
    cd "$ROOT_DIR"

    if [ "$MODE" = "release" ]; then
        cargo build --release --bin salary-app-backend
    else
        cargo build --bin salary-app-backend
    fi
}

build_tray() {
    echo "==> 构建托盘控制面板"
    cd "$ROOT_DIR"

    if [ "$MODE" = "release" ]; then
        cargo build --release -p salary-tray
    else
        cargo build -p salary-tray
    fi
}

run_tests() {
    echo "==> 运行测试"
    cd "$ROOT_DIR"
    cargo test
}

ensure_msvc_linker

case "$MODE" in
    frontend)
        build_frontend
        ;;
    test)
        run_tests
        ;;
    tray)
        build_tray
        ;;
    all)
        build_frontend
        build_backend
        build_tray
        ;;
    release)
        build_backend
        ;;
    *)
        build_backend
        ;;
esac

echo "==> 完成"
