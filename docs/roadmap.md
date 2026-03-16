# Roadmap & Known Issues

## Known Issues

### SIGWINCH / Terminal Resize

`vt100::Parser` 在 session 启动时以当前终端尺寸初始化（fallback: 220×50）。如果用户在 session 运行期间 resize 终端，vt100 parser 的屏幕尺寸不会跟着变，导致 `dump` 命令的屏幕快照可能列数错误。

**修复方向**：监听 `SIGWINCH`（`tokio::signal::unix::signal(SignalKind::window_change())`），收到信号后：
1. 重新读取终端尺寸
2. 调 `pty_pair.master.resize(new_size)`
3. 重建 `vt100::Parser`（用新尺寸，并 replay 当前 raw buffer）

> 注意：replay raw buffer 会损失很多 ANSI 状态（比如已经发过的清屏命令），不是完美方案。vt100 库本身不提供 resize API。

### spawn_blocking cancel 不干净

`stdin_relay_task` 和 `pty_reader_task` 在 `spawn_blocking` 线程里通过轮询 `cancel_rx.borrow()` 检测取消。但这两个任务都阻塞在 `read()` 调用上，如果 read 没有返回，cancel 检查不到。

实际上 zsh 退出后 PTY master 会关闭，`pty_reader` 的 `read()` 会返回 0/Error，任务自然退出。`stdin_relay` 在 zsh 退出后也会因为后续写入 PTY writer 失败而退出。所以这在实践中问题不大，只是不够优雅。

**修复方向**：对 stdin 读取可以用 `crossterm::event::poll` 带超时，周期性检查 cancel 信号。

---

## Near-term Work

### v0.2：测试用例 DSL

支持写脚本描述交互序列，例如：

```
start session
wait_for "$ "          # 等待 prompt 出现
write "echo hello\n"
wait_for "hello"
assert_screen contains "hello"
stop session
```

这需要：
- `wait_for(pattern, timeout)` — 在 OutputBuffer 上阻塞轮询（或 condvar 通知）
- `assert_screen` — 对 `screen_contents()` 做 substring/regex 断言

### v0.2：多 session 并发控制

当前 `write` / `dump` 是一次性连接。测试脚本会需要保持 IpcClient 长连接（或者 session 提供 subscribe 流式输出）。

### v0.3：输出订阅 / streaming

新增 IPC 消息类型：

```json
{ "type": "subscribe" }
```

session 对订阅者持续推送新写入的 bytes（pty_reader_task 写 buffer 时同时 fan-out 给所有订阅者）。这样测试代码不用轮询，可以用 `recv().await` 等待。

### v0.3：Linux 支持

`portable-pty` 支持 Linux（openpty + exec），理论上只需改 shell 路径（`/bin/bash` 或 `/bin/sh`），以及 lock TTL 相关逻辑不依赖平台。需要在 Linux 上实测。

### v1.0：跨机器 session（TCP mode）

在服务器端运行 `agent-terminal start`，允许测试 runner 从不同机器通过 TCP 连接。需要：
- TLS（使用 `rustls`）
- 认证（token）
- TCP framing（与 Unix socket framing 相同协议）

---

## Won't Fix / Out of Scope

- **Windows 支持**：Windows 的 ConPTY API 与 Unix PTY 差异太大，不在计划内。
- **多 shell 支持**（bash/fish/etc.）：当前硬编码 `/bin/zsh`。未来可以通过 `--shell` 参数支持，但这只是配置变更，不需要架构变化。
- **PTY session 持久化（tmux-like）**：detach/attach 语义。这会大幅增加复杂度，且有更成熟的工具（tmux/screen）可以替代。`agent-terminal` 的定位是测试框架而非终端复用器。
