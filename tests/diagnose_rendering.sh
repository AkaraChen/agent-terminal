#!/bin/bash
#
# Terminal 渲染问题自动诊断工具
# 自动检测 vim/opencode 渲染问题并给出修复建议
#

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
MAGENTA='\033[0;35m'
NC='\033[0m'

CLI_BIN="./target/debug/agent-terminal"
DIAGNOSE_LOG="/tmp/agent_terminal_diagnose.log"

# 初始化日志
echo "诊断日志 - $(date)" > "$DIAGNOSE_LOG"

log() {
    # 输出到终端（带颜色）
    echo -e "$1"
    # 记录到日志（不带颜色）
    echo "$1" | sed 's/\x1b\[[0-9;]*m//g' >> "$DIAGNOSE_LOG"
}

run_cli() {
    "$CLI_BIN" "$@"
}

get_session_id() {
    run_cli list 2>/dev/null | grep -v "^SESSION" | grep -v "^-" | head -1 | awk '{print $1}'
}

cleanup() {
    log "${CYAN}清理环境...${NC}"
    if [ -n "$SESSION_ID" ]; then
        run_cli write "$SESSION_ID" $'exit\n' 2>/dev/null || true
    fi
    rm -f /tmp/agent_terminal_vim_test.txt
}

trap cleanup EXIT

print_header() {
    log "${CYAN}============================================================${NC}"
    log "${CYAN}$1${NC}"
    log "${CYAN}============================================================${NC}"
}

# 检测 ANSI 序列支持
check_ansi_support() {
    print_header "检查 1: ANSI 序列支持"

    # 使用 analyze 选项检查序列
    local analysis
    analysis=$(run_cli debug "$SESSION_ID" --analyze 2>&1)

    # 检查关键的 alternate screen 序列 (从分析输出中检查)
    if echo "$analysis" | grep -q "1049h"; then
        log "${GREEN}✓ Alternate screen 进入序列支持正常 (1049h)${NC}"
    else
        log "${YELLOW}⚠ 未检测到 alternate screen 进入序列${NC}"
    fi

    if echo "$analysis" | grep -q "1049l"; then
        log "${GREEN}✓ Alternate screen 退出序列支持正常 (1049l)${NC}"
    fi

    # 检查其他重要序列
    if echo "$analysis" | grep -q "Set Mode"; then
        log "${GREEN}✓ 终端模式设置序列正常${NC}"
    fi

    if echo "$analysis" | grep -q "SGR (Color/Style)"; then
        log "${GREEN}✓ 颜色/样式序列正常${NC}"
    fi
}

# 检测屏幕内容
check_screen_content() {
    local expected="$1"
    local description="$2"

    log "${CYAN}检查: $description${NC}"

    local screen
    screen=$(run_cli dump "$SESSION_ID" 2>/dev/null | grep -v "^=== Session" | grep -v "^$")

    if echo "$screen" | grep -q "$expected"; then
        log "${GREEN}✓ 找到 '$expected'${NC}"
        return 0
    else
        log "${RED}✗ 未找到 '$expected'${NC}"
        log "${YELLOW}当前屏幕内容:${NC}"
        log "$screen" | head -10
        return 1
    fi
}

# 分析 vim 特定问题
analyze_vim_issues() {
    print_header "检查 2: Vim 渲染分析"

    local issues=()

    # 检查 1: 是否有 ~ 行（vim 空行标记）
    local screen
    screen=$(run_cli dump "$SESSION_ID" 2>/dev/null)

    if echo "$screen" | grep -q "~"; then
        log "${GREEN}✓ 检测到 vim 空行标记 (~)${NC}"
    else
        issues+=("未检测到 vim 空行标记 - vim 可能未正确启动")
    fi

    # 检查 2: 是否有状态行
    if echo "$screen" | grep -q '".*".*L.*B'; then
        log "${GREEN}✓ 检测到 vim 状态行${NC}"
    else
        issues+=("未检测到 vim 状态行")
    fi

    # 检查 3: 检查颜色序列
    local raw
    raw=$(run_cli debug "$SESSION_ID" --raw 2>&1)
    if echo "$raw" | grep -q "SGR (Color/Style)"; then
        log "${GREEN}✓ 颜色序列正常${NC}"
    else
        issues+=("颜色序列可能有问题")
    fi

    if [ ${#issues[@]} -eq 0 ]; then
        log "${GREEN}Vim 渲染看起来正常${NC}"
        return 0
    else
        log "${YELLOW}发现 ${#issues[@]} 个问题:${NC}"
        for issue in "${issues[@]}"; do
            log "  - $issue"
        done
        return 1
    fi
}

# 提供修复建议
provide_fixes() {
    print_header "修复建议"

    log "${MAGENTA}常见问题及解决方案:${NC}"
    log ""
    log "1. Vim 显示为空白或乱码"
    log "   - 尝试设置 TERM=xterm-256color"
    log "   - 检查 vim 是否支持该终端类型"
    log ""
    log "2. 颜色显示不正确"
    log "   - 确保终端支持 256 色"
    log "   - 尝试使用: set t_Co=256"
    log ""
    log "3. Alternate screen 不工作"
    log "   - 检查 vt-100 crate 是否正确解析 1049h/l"
    log "   - 验证 wezterm-term 库版本"
    log ""
    log "4. 屏幕内容不更新"
    log "   - 增加等待时间（vim 启动较慢）"
    log "   - 使用 debug --watch 实时观察"
    log ""
}

# 运行自动化测试
run_automated_test() {
    print_header "自动化渲染测试"

    # 启动 session
    log "${CYAN}启动测试 session...${NC}"
    rm -rf /tmp/agent-terminal/sessions/*
    mkdir -p /tmp/agent-terminal/sessions

    run_cli start --shell /bin/bash &
    sleep 3

    SESSION_ID=$(get_session_id)
    if [ -z "$SESSION_ID" ]; then
        log "${RED}✗ Session 启动失败${NC}"
        exit 1
    fi

    log "Session ID: $SESSION_ID"

    # 创建测试文件
    TEST_FILE="/tmp/agent_terminal_vim_test.txt"
    echo -e "Hello from Vim Test!\nLine 2\nLine 3" > "$TEST_FILE"

    # 测试 1: 基础 shell
    log "${CYAN}测试 1: Shell 提示符${NC}"
    if check_screen_content "bash" "Shell prompt"; then
        log "${GREEN}✓ Shell 运行正常${NC}"
    else
        log "${YELLOW}⚠ 可能不是 bash shell${NC}"
    fi

    # 测试 2: Vim
    log "${CYAN}测试 2: 启动 Vim...${NC}"
    run_cli test "$SESSION_ID" write "vim $TEST_FILE"
    run_cli test "$SESSION_ID" write $'\n'
    sleep 4

    # 运行诊断
    check_ansi_support
    analyze_vim_issues

    # 验证内容
    log "${CYAN}验证 vim 内容渲染...${NC}"
    if run_cli test "$SESSION_ID" assert-contains "Hello from Vim"; then
        log "${GREEN}✓ Vim 内容渲染成功${NC}"
    else
        log "${RED}✗ Vim 内容渲染失败${NC}"
        provide_fixes
        return 1
    fi

    # 退出 vim
    run_cli test "$SESSION_ID" write $'\x1b:q!\n'
    sleep 2

    # 测试 3: OpenCode
    if command -v opencode &>/dev/null; then
        log "${CYAN}测试 3: OpenCode${NC}"
        run_cli test "$SESSION_ID" write "opencode --help"
        run_cli test "$SESSION_ID" write $'\n'
        sleep 3

        if run_cli test "$SESSION_ID" assert-contains "Commands:" 2>/dev/null || \
           run_cli test "$SESSION_ID" assert-contains "Usage:" 2>/dev/null; then
            log "${GREEN}✓ OpenCode 渲染正常${NC}"
        else
            log "${YELLOW}⚠ OpenCode 输出格式可能不同，检查屏幕内容...${NC}"
            run_cli dump "$SESSION_ID" | head -5
        fi
    else
        log "${YELLOW}⚠ opencode 未安装，跳过测试${NC}"
    fi

    log ""
    log "${GREEN}============================================================${NC}"
    log "${GREEN}诊断完成! 日志保存到: $DIAGNOSE_LOG${NC}"
    log "${GREEN}============================================================${NC}"

    return 0
}

# 主入口
case "${1:-run}" in
    run)
        run_automated_test
        ;;
    watch)
        # 启动 session 并进入 watch 模式
        rm -rf /tmp/agent-terminal/sessions/*
        mkdir -p /tmp/agent-terminal/sessions
        run_cli start --shell /bin/bash &
        sleep 3
        SESSION_ID=$(get_session_id)
        if [ -n "$SESSION_ID" ]; then
            log "进入 watch 模式，按 Ctrl+C 退出"
            run_cli debug "$SESSION_ID" --watch
        fi
        ;;
    help|--help|-h)
        echo "Terminal 渲染问题自动诊断工具"
        echo ""
        echo "用法:"
        echo "  $0 run    - 运行自动化测试 (默认)"
        echo "  $0 watch  - 启动 session 并进入 watch 模式"
        echo "  $0 help   - 显示帮助"
        echo ""
        echo "环境要求:"
        echo "  - agent-terminal 已构建"
        echo "  - vim 已安装"
        echo "  - opencode 已安装 (可选)"
        ;;
    *)
        echo "未知命令: $1"
        echo "使用 '$0 help' 查看用法"
        exit 1
        ;;
esac
