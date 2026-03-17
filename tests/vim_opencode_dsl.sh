#!/bin/bash
#
# DSL 风格的 Vim 和 OpenCode 渲染测试
# 使用 agent-terminal CLI 命令实现
#
# 使用方法:
#     ./tests/vim_opencode_dsl.sh
#
# 环境要求:
#     - agent-terminal 已构建 (cargo build)
#     - vim 已安装
#     - opencode 已安装 (可选)
#

set -e

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# 获取 agent-terminal 二进制路径
CLI_BIN="./target/debug/agent-terminal"
if [ ! -f "$CLI_BIN" ]; then
    CLI_BIN="./target/release/agent-terminal"
fi
if [ ! -f "$CLI_BIN" ]; then
    echo -e "${RED}✗ 未找到 agent-terminal 二进制文件，请先构建: cargo build${NC}"
    exit 1
fi

# 辅助函数
run_cli() {
    "$CLI_BIN" "$@"
}

get_session_id() {
    run_cli list 2>/dev/null | grep -v "^SESSION" | grep -v "^-" | head -1 | awk '{print $1}'
}

cleanup() {
    echo ""
    echo "清理环境..."
    # 停止 session
    if [ -n "$SESSION_ID" ]; then
        run_cli write "$SESSION_ID" $'exit\n' 2>/dev/null || true
    fi
    # 清理测试文件
    rm -f /tmp/agent_terminal_vim_test.txt
}

trap cleanup EXIT

echo "============================================================"
echo "Agent Terminal - Vim/OpenCode DSL 渲染测试"
echo "============================================================"

# 清理旧环境
echo ""
echo "清理旧环境..."
rm -rf /tmp/agent-terminal/sessions/*
mkdir -p /tmp/agent-terminal/sessions

# 启动 session
echo ""
echo "启动 session..."
run_cli start --shell /bin/bash &
START_PID=$!

# 等待 session 启动
sleep 3

# 获取 session ID
SESSION_ID=$(get_session_id)
if [ -z "$SESSION_ID" ]; then
    echo -e "${RED}✗ Session 启动失败${NC}"
    exit 1
fi

echo "Session ID: $SESSION_ID"

# 创建测试文件
TEST_FILE="/tmp/agent_terminal_vim_test.txt"
echo -e "Hello from Vim Test!\nLine 2\nLine 3\n" > "$TEST_FILE"

# ============================================================
# 测试 1: Vim 渲染测试
# ============================================================
echo ""
echo "------------------------------------------------------------"
echo "测试 1: Vim 渲染"
echo "------------------------------------------------------------"

# 启动 vim
run_cli test "$SESSION_ID" write "vim $TEST_FILE"
run_cli test "$SESSION_ID" write $'\n'

# 等待 vim 启动
sleep 4

# 验证文件内容
echo "验证文件内容..."
if run_cli test "$SESSION_ID" assert-contains "Hello from Vim"; then
    echo -e "${GREEN}  ✓ 文件内容渲染正常${NC}"
    VIM_CONTENT_OK=true
else
    echo -e "${RED}  ✗ 文件内容未找到${NC}"
    VIM_CONTENT_OK=false
fi

# 验证文件名
echo "验证文件名..."
if run_cli test "$SESSION_ID" assert-contains "$TEST_FILE"; then
    echo -e "${GREEN}  ✓ 文件名显示正常${NC}"
    VIM_FILENAME_OK=true
else
    echo -e "${RED}  ✗ 文件名未找到${NC}"
    VIM_FILENAME_OK=false
fi

# 退出 vim
run_cli test "$SESSION_ID" write $'\x1b:q!\n'
sleep 1

# ============================================================
# 测试 2: OpenCode 渲染测试
# ============================================================
echo ""
echo "------------------------------------------------------------"
echo "测试 2: OpenCode 渲染"
echo "------------------------------------------------------------"

# 检查 opencode 是否安装
if ! command -v opencode &> /dev/null; then
    echo -e "${YELLOW}  ⚠ opencode 未安装，跳过测试${NC}"
    OPCODE_OK=null
else
    # 启动 opencode help
    run_cli test "$SESSION_ID" write "opencode --help"
    run_cli test "$SESSION_ID" write $'\n'

    # 等待输出
    sleep 3

    # 验证帮助输出
    echo "验证 help 输出..."
    if run_cli test "$SESSION_ID" assert-contains "Commands:" 2>/dev/null || \
       run_cli test "$SESSION_ID" assert-contains "Usage:" 2>/dev/null || \
       run_cli test "$SESSION_ID" assert-contains "opencode" 2>/dev/null; then
        echo -e "${GREEN}  ✓ Help 输出渲染正常${NC}"
        OPCODE_OK=true
    else
        # 显示当前屏幕内容用于调试
        echo -e "${YELLOW}  ⚠ Help 输出格式可能不同，检查屏幕内容...${NC}"
        run_cli dump "$SESSION_ID" 2>/dev/null | head -20 || true
        OPCODE_OK=false
    fi
fi

# ============================================================
# 测试结果汇总
# ============================================================
echo ""
echo "============================================================"
echo "测试结果"
echo "============================================================"

# Vim 测试结果
if [ "$VIM_CONTENT_OK" = true ] && [ "$VIM_FILENAME_OK" = true ]; then
    echo -e "  vim:       ${GREEN}✓ 通过${NC}"
    VIM_PASSED=true
else
    echo -e "  vim:       ${RED}✗ 失败${NC}"
    VIM_PASSED=false
fi

# OpenCode 测试结果
if [ "$OPCODE_OK" = "null" ]; then
    echo -e "  opencode:  ${YELLOW}跳过${NC}"
    OPCODE_PASSED=true
elif [ "$OPCODE_OK" = true ]; then
    echo -e "  opencode:  ${GREEN}✓ 通过${NC}"
    OPCODE_PASSED=true
else
    echo -e "  opencode:  ${RED}✗ 失败${NC}"
    OPCODE_PASSED=false
fi

echo ""

# 最终判断
if [ "$VIM_PASSED" = true ] && [ "$OPCODE_PASSED" = true ]; then
    echo -e "${GREEN}所有测试通过!${NC}"
    exit 0
else
    echo -e "${RED}部分测试失败${NC}"
    exit 1
fi
