# Agent Terminal 调试指南

## 概述

本文档介绍如何在 agent-terminal 中调试 vim、opencode 和其他终端应用程序的渲染问题。

## 调试工具

### 1. debug 命令

`agent-terminal debug` 提供深入的终端状态分析：

```bash
# 基本用法 - 显示当前屏幕内容
agent-terminal debug <session-id>

# 显示原始 ANSI 字节（用于分析转义序列）
agent-terminal debug <session-id> --raw

# 实时监控模式（每 500ms 更新）
agent-terminal debug <session-id> --watch

# 分析 ANSI 序列和屏幕状态
agent-terminal debug <session-id> --analyze

# 组合使用
agent-terminal debug <session-id> --raw --analyze
```

### 2. 自动化诊断工具

```bash
# 运行完整的渲染测试
./tests/diagnose_rendering.sh

# 进入实时监控模式
./tests/diagnose_rendering.sh watch
```

### 3. DSL 测试脚本

```bash
# 运行 DSL 风格的 vim/opencode 测试
./tests/vim_opencode_dsl.sh
```

## 常见问题诊断

### Vim 渲染问题

#### 症状：Vim 显示空白或乱码

**检查步骤：**

1. 检查是否正确进入 alternate screen
   ```bash
   agent-terminal debug <sid> --analyze | grep "1049"
   ```
   应该看到 `Set Mode ?1049`。

2. 检查 vim 是否发送了内容
   ```bash
   agent-terminal debug <sid> --raw | head -20
   ```
   查看是否有文件内容在原始输出中。

3. 检查 VT100 parser 状态
   ```bash
   agent-terminal dump <sid>
   ```

**解决方案：**
- 确保 `TERM=xterm-256color`
- 增加等待时间（vim 启动较慢）
- 检查 vim 是否支持当前终端类型

#### 症状：颜色显示不正确

**检查：**
```bash
agent-terminal debug <sid> --analyze | grep "SGR"
```

应该看到多个 `SGR (Color/Style)` 条目。

### OpenCode 渲染问题

#### 症状：Help 命令输出异常

**检查：**
```bash
agent-terminal test <sid> write "opencode --help"
sleep 3
agent-terminal test <sid> assert-contains "Commands:"
```

**调试：**
```bash
agent-terminal debug <sid> --analyze
```

## 技术细节

### ANSI 序列处理

agent-terminal 使用以下组件处理 ANSI 序列：

1. **wezterm-term** - VT100 终端模拟器
2. **自定义 VT100 Parser** - 在 `crates/vt-100/src/lib.rs`
3. **OutputBuffer** - 在 `crates/core/src/buffer.rs`

### Alternate Screen Buffer

Vim 和其他全屏应用使用 alternate screen buffer：

- **进入**: `ESC[?1049h`
- **退出**: `ESC[?1049l`

当 vim 进入时：
1. 保存当前屏幕状态
2. 切换到 alternate buffer
3. 绘制 vim UI

当 vim 退出时：
1. 切换回主 buffer
2. 恢复之前的状态

### 关键 ANSI 序列

| 序列 | 描述 |
|------|------|
| `ESC[?1049h` | 进入 alternate screen |
| `ESC[?1049l` | 退出 alternate screen |
| `ESC[H` | 移动光标到 home (0,0) |
| `ESC[2J` | 清屏 |
| `ESC[K` | 清除从光标到行尾 |
| `ESC[n;m` | 设置颜色/样式 (SGR) |
| `ESC[n;mH` | 设置光标位置 |

## 调试示例

### 示例 1: 验证 Vim 渲染

```bash
# 1. 启动 session
agent-terminal start --shell /bin/bash &

# 2. 获取 session ID
SID=$(agent-terminal list | tail -1 | awk '{print $1}')

# 3. 创建测试文件
echo -e "Line 1\nLine 2" > /tmp/test.txt

# 4. 启动 vim
agent-terminal test $SID write "vim /tmp/test.txt"
agent-terminal test $SID write $'\n'
sleep 3

# 5. 检查屏幕状态
agent-terminal debug $SID --analyze

# 6. 验证内容
agent-terminal test $SID assert-contains "Line 1"

# 7. 退出 vim
agent-terminal test $SID write $'\x1b:q!\n'

# 8. 清理
agent-terminal write $SID "exit"
```

### 示例 2: 实时监控

```bash
# 在另一个终端窗口中运行
agent-terminal debug <session-id> --watch

# 然后在主终端中运行 vim
vim some-file.txt

# 观察 debug 窗口中的实时更新
```

### 示例 3: 原始输出分析

```bash
# 获取原始 ANSI 序列
agent-terminal debug <sid> --raw

# 解码 base64 部分查看原始字节
# 分析关键序列如 1049h, 1049l, SGR 颜色等
```

## 测试

### 运行单元测试

```bash
# VT100 parser 测试
cargo test -p vt-100

# Buffer 测试
cargo test -p agent-terminal-core buffer

# IPC 测试
cargo test -p agent-terminal-core ipc
```

### 运行集成测试

```bash
# 完整的 vim/opencode 测试
./tests/vim_opencode_dsl.sh

# 诊断测试
./tests/diagnose_rendering.sh
```

## 故障排除清单

- [ ] Session 是否正常运行？`agent-terminal list`
- [ ] PTY 是否正确捕获输出？`agent-terminal dump`
- [ ] ANSI 序列是否正确解析？`agent-terminal debug --analyze`
- [ ] Alternate screen 是否正确处理？检查 1049h/l
- [ ] 等待时间是否足够？vim 启动需要 3-4 秒
- [ ] TERM 环境变量设置正确？`xterm-256color`

## 相关文件

- `crates/vt-100/src/lib.rs` - VT100 parser
- `crates/core/src/buffer.rs` - Output buffer
- `crates/core/src/dsl.rs` - Test DSL
- `crates/cli/src/commands/debug.rs` - Debug command
- `tests/vim_opencode_dsl.sh` - DSL test
- `tests/diagnose_rendering.sh` - Diagnostic tool
