#!/bin/bash
#
# Step debugger 演示脚本
# 展示交互式逐步调试功能
#

set -e

CYAN='\033[0;36m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${CYAN}============================================================${NC}"
echo -e "${CYAN}Step Debugger 演示${NC}"
echo -e "${CYAN}============================================================${NC}"
echo ""
echo "这个脚本将启动一个 session，然后你可以使用 step 调试器"
echo "来交互式地执行命令并观察终端状态变化。"
echo ""

# 清理并启动 session
rm -rf /tmp/agent-terminal/sessions/*
mkdir -p /tmp/agent-terminal/sessions

echo -e "${CYAN}启动 session...${NC}"
./target/debug/agent-terminal start --shell /bin/bash &
sleep 3

SID=$(./target/debug/agent-terminal list | grep -v "^SESSION" | grep -v "^-" | head -1 | awk '{print $1}')
echo "Session ID: $SID"
echo ""

# 创建测试文件
echo -e "Line 1\nLine 2\nLine 3" > /tmp/step_test.txt

echo -e "${GREEN}启动 step 调试器...${NC}"
echo ""
echo "在调试器中尝试以下命令:"
echo "  w vim /tmp/step_test.txt  - 启动 vim"
echo "  d 3000                     - 等待 3 秒"
echo "  s                          - 查看屏幕状态"
echo "  h 5                        - 查看历史记录"
echo "  w :q!                      - 退出 vim"
echo "  q                          - 退出调试器"
echo ""
echo -e "${YELLOW}按 Enter 启动调试器...${NC}"
read

./target/debug/agent-terminal step "$SID"

# 清理
echo ""
echo -e "${CYAN}清理...${NC}"
./target/debug/agent-terminal write "$SID" "exit" 2>/dev/null || true
rm -f /tmp/step_test.txt

echo -e "${GREEN}完成!${NC}"
