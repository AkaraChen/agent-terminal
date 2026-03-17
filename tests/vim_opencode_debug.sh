#!/bin/bash
#
# Vim 和 OpenCode 渲染调试测试脚本
# 使用 debug 命令进行深入分析
#

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

CLI_BIN="./target/debug/agent-terminal"

run_cli() {
    "$CLI_BIN" "$@"
}

get_session_id() {
    run_cli list 2>/dev/null | grep -v "^SESSION" | grep -v "^-" | head -1 | awk '{print $1}'
}

cleanup() {
    echo -e "\n${CYAN}清理环境...${NC}"
    if [ -n "$SESSION_ID" ]; then
        run_cli write "$SESSION_ID" $'exit\n' 2>/dev/null || true
    fi
    rm -f /tmp/agent_terminal_vim_test.txt
}

trap cleanup EXIT

echo "============================================================"
echo "Vim/OpenCode 渲染调试测试"
echo "============================================================"

# 清理并启动 session
rm -rf /tmp/agent-terminal/sessions/*
mkdir -p /tmp/agent-terminal/sessions

echo -e "\n${CYAN}启动 session...${NC}"
run_cli start --shell /bin/bash &
sleep 3

SESSION_ID=$(get_session_id)
if [ -z "$SESSION_ID" ]; then
    echo -e "${RED}✗ Session 启动失败${NC}"
    exit 1
fi

echo "Session ID: $SESSION_ID"

# 创建测试文件
TEST_FILE="/tmp/agent_terminal_vim_test.txt"
echo -e "Hello from Vim Test!\nLine 2\nLine 3" > "$TEST_FILE"

# ============================================================
# 测试 1: Vim 启动前状态
# ============================================================
echo -e "\n${CYAN}============================================================${NC}"
echo -e "${CYAN}步骤 1: Vim 启动前的 shell 状态${NC}"
echo -e "${CYAN}============================================================${NC}"
run_cli debug "$SESSION_ID" --analyze

# ============================================================
# 测试 2: 启动 Vim
# ============================================================
echo -e "\n${CYAN}============================================================${NC}"
echo -e "${CYAN}步骤 2: 启动 Vim...${NC}"
echo -e "${CYAN}============================================================${NC}"

run_cli test "$SESSION_ID" write "vim $TEST_FILE"
run_cli test "$SESSION_ID" write $'\n'
sleep 4

echo -e "\n${GREEN}Vim 启动后的屏幕状态:${NC}"
run_cli debug "$SESSION_ID" --analyze

# 验证关键内容
echo -e "\n${CYAN}验证 Vim 内容...${NC}"
if run_cli test "$SESSION_ID" assert-contains "Hello from Vim"; then
    echo -e "${GREEN}✓ 文件内容正确渲染${NC}"
else
    echo -e "${RED}✗ 文件内容未找到${NC}"
fi

# ============================================================
# 测试 3: 原始 ANSI 分析
# ============================================================
echo -e "\n${CYAN}============================================================${NC}"
echo -e "${CYAN}步骤 3: 分析原始 ANSI 序列${NC}"
echo -e "${CYAN}============================================================${NC}"
run_cli debug "$SESSION_ID" --raw --analyze 2>&1 | head -100

# ============================================================
# 测试 4: 退出 Vim
# ============================================================
echo -e "\n${CYAN}============================================================${NC}"
echo -e "${CYAN}步骤 4: 退出 Vim...${NC}"
echo -e "${CYAN}============================================================${NC}"

run_cli test "$SESSION_ID" write $'\x1b:q!\n'
sleep 2

echo -e "\n${GREEN}退出 Vim 后的状态:${NC}"
run_cli debug "$SESSION_ID" --analyze

# ============================================================
# 测试 5: OpenCode (如果已安装)
# ============================================================
echo -e "\n${CYAN}============================================================${NC}"
echo -e "${CYAN}步骤 5: 测试 OpenCode${NC}"
echo -e "${CYAN}============================================================${NC}"

if command -v opencode &> /dev/null; then
    run_cli test "$SESSION_ID" write "opencode --help"
    run_cli test "$SESSION_ID" write $'\n'
    sleep 3

    echo -e "\n${GREEN}OpenCode help 输出:${NC}"
    run_cli debug "$SESSION_ID" --analyze

    if run_cli test "$SESSION_ID" assert-contains "Commands:" 2>/dev/null || \
       run_cli test "$SESSION_ID" assert-contains "Usage:" 2>/dev/null; then
        echo -e "${GREEN}✓ OpenCode 渲染正常${NC}"
    else
        echo -e "${YELLOW}⚠ OpenCode 输出格式可能不同${NC}"
        run_cli dump "$SESSION_ID" 2>/dev/null | head -20
    fi
else
    echo -e "${YELLOW}⚠ opencode 未安装，跳过测试${NC}"
fi

echo -e "\n${GREEN}============================================================${NC}"
echo -e "${GREEN}调试测试完成${NC}"
echo -e "${GREEN}============================================================${NC}"
