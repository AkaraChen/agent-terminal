#!/bin/bash
#
# Vim 屏幕录制和回放调试工具
# 使用 screen history 功能捕获 vim 启动过程的每一帧
#

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
MAGENTA='\033[0;35m'
NC='\033[0m'

CLI_BIN="./target/debug/agent-terminal"

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
    rm -f /tmp/.vim_test.txt.swp /tmp/vim_test.txt
}

log() {
    echo -e "$1"
}

trap cleanup EXIT

log "${CYAN}============================================================${NC}"
log "${CYAN}Vim 屏幕录制和回放调试工具${NC}"
log "${CYAN}============================================================${NC}"

# 清理并启动 session
rm -rf /tmp/agent-terminal/sessions/*
mkdir -p /tmp/agent-terminal/sessions

log "${CYAN}启动 session...${NC}"
run_cli start --shell /bin/bash &
sleep 3

SESSION_ID=$(get_session_id)
if [ -z "$SESSION_ID" ]; then
    log "${RED}✗ Session 启动失败${NC}"
    exit 1
fi

log "Session ID: $SESSION_ID"

# 创建测试文件
echo -e "Line 1: Hello Vim!\nLine 2: Testing screen recording\nLine 3: Last line" > /tmp/vim_test.txt

# ============================================================
# 步骤 1: 记录 vim 启动过程
# ============================================================
log ""
log "${CYAN}步骤 1: 启动 vim 并记录屏幕变化...${NC}"

# 启动 vim
run_cli test "$SESSION_ID" write "vim /tmp/vim_test.txt"
run_cli test "$SESSION_ID" write $'\n'

# 等待 vim 完全启动
sleep 4

# 查看屏幕历史记录
log "${YELLOW}查看 vim 启动过程中的屏幕状态变化:${NC}"
run_cli debug "$SESSION_ID" --history 10

# ============================================================
# 步骤 2: 分析当前 vim 状态
# ============================================================
log ""
log "${CYAN}步骤 2: 详细分析当前 vim 状态...${NC}"
run_cli debug "$SESSION_ID" --analyze

# ============================================================
# 步骤 3: 在 vim 中进行编辑操作并记录
# ============================================================
log ""
log "${CYAN}步骤 3: 在 vim 中移动光标...${NC}"

# 按 j 向下移动
run_cli test "$SESSION_ID" write "j"
sleep 0.5

# 按 $ 移动到行尾
run_cli test "$SESSION_ID" write "$"
sleep 0.5

# 查看新的屏幕历史
log "${YELLOW}编辑操作后的屏幕状态:${NC}"
run_cli debug "$SESSION_ID" --history 5

# ============================================================
# 步骤 4: 退出 vim 并观察状态恢复
# ============================================================
log ""
log "${CYAN}步骤 4: 退出 vim...${NC}"

run_cli test "$SESSION_ID" write $'\x1b:q!\n'
sleep 2

log "${YELLOW}退出 vim 后的屏幕历史:${NC}"
run_cli debug "$SESSION_ID" --history 5

# ============================================================
# 步骤 5: 对比分析
# ============================================================
log ""
log "${GREEN}============================================================${NC}"
log "${GREEN}调试完成!${NC}"
log "${GREEN}============================================================${NC}"
log ""
log "使用以下命令进行更深入的分析:"
log "  ./target/debug/agent-terminal debug $SESSION_ID --history 20"
log "  ./target/debug/agent-terminal debug $SESSION_ID --analyze"
log "  ./target/debug/agent-terminal debug $SESSION_ID --watch"
log ""
