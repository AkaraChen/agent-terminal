# Quick Start

## 构建

```bash
cd agent-terminal
cargo build
```

产物：`target/debug/agent-terminal`

---

## 基本用法

### 1. 启动一个受管控的 shell session

在终端 A 中：

```bash
# 使用平台默认 shell（macOS: /bin/zsh, Linux: /bin/bash）
cargo run -- start

# 或指定自定义 shell
cargo run -- start --shell /bin/bash
cargo run -- start -s /bin/zsh
```

这会打开一个外观和普通 shell 完全相同的终端。背后这个进程：
- 持有一个 PTY，zsh 运行在 slave 端
- 在 `/tmp/agent-terminal/sessions/<uuid>.lock` 写入心跳
- 在 `/tmp/agent-terminal/sessions/<uuid>.sock` 监听 IPC 连接
- 所有输入输出都被捕获进 `OutputBuffer`

### 2. 查看活跃 session

在终端 B 中：

```bash
cargo run -- list
```

输出示例：

```
SESSION ID                              PID     SOCKET                           STARTED AT
-----------------------------------------------------------------------------------------------
550e8400-e29b-41d4-a716-446655440000   12345   /tmp/agent-terminal/sessions/... 2026-03-16 10:00:00
```

### 3. 向 session 注入输入

```bash
# 支持用 session_id 前缀匹配
cargo run -- write 550e84 "echo hello\n"
```

这会把 `echo hello\n` 直接写入 PTY master，相当于在终端 A 中按键。

> `\n` 需要实际传入换行符。在 shell 里可用 `$'echo hello\n'` 语法：
> ```bash
> cargo run -- write 550e84 $'echo hello\n'
> ```

### 4. 查看当前屏幕快照

```bash
cargo run -- dump 550e84
```

输出 session 当前 vt100 屏幕的纯文本渲染，相当于截图。

### 5. DSL 测试命令

使用 test 命令执行测试 DSL：

```bash
# 等待输出中出现指定模式（默认超时 5 秒）
cargo run -- test 550e84 wait-for "$ "
cargo run -- test 550e84 wait-for "hello" --timeout 10

# 断言屏幕包含指定文本
cargo run -- test 550e84 assert-contains "hello world"
```

### 6. 远程访问（TCP 模式）

在不同机器上访问 session（需要编译时启用 TCP 支持）：

```bash
# 编译启用 TCP 支持
cargo build --release --features tcp

# 在服务器端启动 TCP 服务（在 session 所在机器运行）
# 注意：当前 TCP 服务需要手动启动，会监听指定端口
cargo run --release --features tcp -- start

# 从远程机器连接
# 需要先知道服务器地址和认证 token
# agent-terminal remote <addr>:<port> --token <secret> write "ls\n"
# agent-terminal remote <addr>:<port> --token <secret> dump
```

**安全注意**：
- TCP 模式使用 TLS 加密通信
- 需要 token 认证才能访问 session
- 生产环境请使用强 token 并妥善保管

### 7. Session 结束

在终端 A 中输入 `exit` 或按 `Ctrl+D` 退出 zsh。进程会自动清理 lock 文件和 socket 文件。

---

## 文件路径

| 用途 | 路径 |
|---|---|
| Session 目录 | `/tmp/agent-terminal/sessions/` |
| Lock 文件 | `/tmp/agent-terminal/sessions/<uuid>.lock` |
| Unix socket | `/tmp/agent-terminal/sessions/<uuid>.sock` |

> `/tmp` 在 macOS 重启后清空，无需手动清理历史 session 文件。

---

## 手动调试 IPC

可以用 Python 手动向 socket 发 request：

```python
import socket, json, struct

sock_path = "/tmp/agent-terminal/sessions/<uuid>.sock"
req = json.dumps({"type": "get_output"}).encode()

with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
    s.connect(sock_path)
    s.sendall(struct.pack("<I", len(req)) + req)
    raw = s.recv(4)
    n = struct.unpack("<I", raw)[0]
    print(json.loads(s.recv(n)))
```
