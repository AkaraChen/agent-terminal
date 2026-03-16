# Architecture

## Workspace 结构

```
agent-terminal/               ← Cargo workspace root
  Cargo.toml
  src/main.rs                 ← 入口，调用 cli::run()
  crates/
    core/                     ← 纯业务逻辑，不含 CLI 胶水代码
      src/
        lib.rs
        protocol.rs           ← IPC 消息类型（serde）
        lock.rs               ← LockFile 读写 & 活跃性扫描
        buffer.rs             ← OutputBuffer（vt100 屏幕状态 + raw bytes）
        ipc.rs                ← IpcClient（Unix socket 客户端 + 帧编解码）
        session.rs            ← PTY session 主逻辑
    cli/                      ← 命令行交互层，依赖 core
      src/
        lib.rs                ← clap App 定义 + run() 入口
        commands/
          start.rs            ← start 命令
          list.rs             ← list 命令
          write.rs            ← write 命令
          dump.rs             ← dump 命令
          remote.rs           ← remote 命令 (TCP mode)
```

**分层原则**：`core` 不知道 clap、不打印东西、不持有 CLI 状态；`cli` 只做"解析参数 → 调 core → 格式化输出"。未来可以在 `core` 上方套 HTTP API 层，或在测试里直接调 core 函数，不受 CLI 层影响。

---

## 数据流

```
用户键盘
  │
  ▼ (raw stdin)
stdin_relay_task ──────────────────────► PTY master writer
                                                │
                                                │ (OS 内核 PTY)
                                                ▼
                                         zsh (slave PTY)
                                                │
                                                ▼
pty_reader_task ◄─────────────────────── PTY master reader
  │                   read()
  ├──► write stdout (用户看到输出)
  └──► OutputBuffer.push(data)
              │
              └──► vt100::Parser.process(data)   ← 维护屏幕状态


外部 IPC 客户端 (另一个终端)
  │  UnixStream
  │  [u32 LE len][JSON Request]
  ▼
socket_server_task
  ├── WriteInput{data} ──► PTY master writer
  └── GetOutput        ──► OutputBuffer → [u32 LE len][JSON Response]
```

---

## Session 生命周期

```
run_session(shell)
  │
  ├─ openpty() + spawn shell (default: /bin/zsh on macOS, /bin/bash on Linux)
  ├─ 写 lock 文件 (LockFile)
  ├─ bind UnixListener
  ├─ enable_raw_mode(stdin)
  │
  ├─ spawn heartbeat_task      ← 每 2s 更新 lock.tick
  ├─ spawn socket_server_task  ← 接受 IPC 连接
  ├─ spawn pty_reader_task     ← (spawn_blocking) 读 PTY → buffer + stdout
  ├─ spawn stdin_relay_task    ← (spawn_blocking) 读 stdin → PTY
  │
  ├─ await child.wait()        ← zsh 退出时解除阻塞
  │
  ├─ cancel_tx.send(true)      ← 通知所有 task 退出
  ├─ disable_raw_mode()
  ├─ lock.remove()
  └─ remove socket file
```

---

## IPC 协议

**Transport**：
- **Unix socket**：本地进程间通信，路径存放在对应 lock 文件的 `socket_path` 字段，套接字权限为 `0600`。
- **TCP socket**：跨机器远程访问，需启用 `tcp` feature，支持 TLS 加密和 token 认证。

**Framing**：每个消息由 `[4 字节 u32 LE 长度][JSON 字节流]` 组成。

**Messages**：

```
Request (client → session):
  { "type": "write_input",  "data": "ls\n" }
  { "type": "get_output"  }
  { "type": "subscribe" }          # Start streaming output
  { "type": "unsubscribe" }        # Stop streaming output
  { "type": "authenticate", "token": "..." }  # TCP mode auth

Response (session → client):
  { "type": "ok" }
  { "type": "output", "raw_b64": "<base64>", "screen": "..." }
  { "type": "output_chunk", "raw_b64": "<base64>" }  # Streaming response
  { "type": "error",  "message": "..." }
```

`raw_b64` 是所有捕捉到的 raw PTY bytes 的 base64 编码（超 1MB 会从头部截断）。  
`screen` 是当前 vt100 屏幕状态的纯文本，每行 trailing 空格被 trim，trailing 空行被丢弃。

---

## Lock 文件

**路径**：`/tmp/agent-terminal/sessions/<session_id>.lock`

```json
{
  "session_id": "550e8400-e29b-41d4-a716-446655440000",
  "pid": 12345,
  "socket_path": "/tmp/agent-terminal/sessions/550e8400-....sock",
  "tick": 1710000042,
  "started_at": 1710000000
}
```

**活跃判断**：`now - tick ≤ 5s`（heartbeat 每 2s 更新 tick，因此正常 session 的 tick 总是新鲜的）。

`LockFile::scan_active()` 扫描整个 sessions 目录，过滤掉死亡 session（含文件损坏的）。支持用 session_id **前缀**匹配（`LockFile::find_active(prefix)`），方便命令行只输入前几个字符。

---

## OutputBuffer

- 同时维护两份数据：
  - `raw: Vec<u8>` — 原始 PTY 字节流，用于 `raw_b64` 响应
  - `vt100::Parser` — ANSI/VT100 状态机，用于 `screen_contents()` 渲染
- **容量上限**：raw 超过 1MB 时，`drain(..excess)` 截掉最老的字节（ring-buffer 语义）
- Parser 的行列维度在 `run_session()` 开始时从当前终端尺寸读取（`crossterm::terminal::size()`，fallback 220×50）

## Terminal Resize (SIGWINCH)

当终端窗口大小变化时，session 会收到 `SIGWINCH` 信号并自动调整：

```
sigwinch_task
  │
  ├─► 重新读取终端尺寸 (crossterm::terminal::size)
  ├─► pty_master.resize(new_size)     # 调整 PTY
  └─► OutputBuffer.resize(rows, cols)  # 重建 VT100 parser
```

**实现细节**：
- 使用 `tokio::signal::unix::signal(SignalKind::window_change())` 监听 SIGWINCH
- `OutputBuffer::resize()` 创建新的 `vt100::Parser` 并重播 raw buffer
- 注意：重播会损失部分 ANSI 状态（如清屏命令历史），但这是 vt100 库的限制

---

## 输出 Streaming

使用 `tokio::sync::broadcast` 实现多订阅者输出流：

```
pty_reader_task
  │
  ├─► OutputBuffer.push(data)
  └─► broadcast_tx.send(data) ─────► 订阅者1 (recv().await)
                                    订阅者2 (recv().await)
                                    订阅者N (recv().await)
```

**特点**：
- 广播容量：1024 条消息（超限时旧订阅者会收到 `broadcast::error::RecvError::Lagged`）
- 订阅者在订阅瞬间开始接收新数据（不保证历史数据）
- 每个 IPC 连接独立管理订阅状态

---

## 测试用例 DSL

`dsl` 模块提供高层测试原语，封装复杂的交互逻辑：

```
TestRunner (客户端)
  │
  ├─► subscribe() ────────────┐
  │                            │
  ├─► wait_for(pattern) ──────┤──► 接收 OutputChunk ──► 累积输出 ──► 匹配 pattern
  │                            │
  └─► assert_screen_contains()─┘
```

**核心功能**：
- `wait_for(pattern, timeout)`: 使用 streaming 订阅等待输出模式出现
- `assert_screen_contains(text)`: 断言当前屏幕包含指定文本
- 自动管理订阅/取消订阅生命周期

**CLI 集成**：
```bash
# 等待提示符出现
agent-terminal test <session> wait-for "$ " --timeout 5

# 断言屏幕内容
agent-terminal test <session> assert-contains "hello"
```

---

## 并发模型

- 整体运行在 `tokio` current-thread 调度器下（从 `cli::run()` 手动 `build().block_on()`）
- PTY 读/写使用 `tokio::task::spawn_blocking`，因为 `portable-pty` 的 reader/writer 是同步阻塞 IO
- 多 task 之间通过 `watch::channel(bool)` 广播取消信号
- `OutputBuffer` 和 PTY writer 通过 `Arc<Mutex<_>>` 在 task 间共享

### spawn_blocking 任务取消

`stdin_relay_task` 使用 `crossterm::event::poll` 带 100ms 超时读取输入，超时后检查 `cancel` 信号，实现快速干净的取消：

```
stdin_relay_task (spawn_blocking)
  │
  ├─► event::poll(100ms timeout) ──► 有输入? 读取并转发到 PTY
  │                                  超时? 检查 cancel 信号
  └─► cancel == true ? 退出
```

`pty_reader_task` 在 zsh 退出后会因 PTY master 关闭而自然退出（`read()` 返回 0 或 Error），无需额外处理。

---

## 测试策略

### 单元测试（`crates/core`）

`core` crate 的设计目标之一就是"可直接在测试里调用，不需要 CLI 或真实 PTY"。目前四个可单测模块均有完整的 `#[cfg(test)]` 块：

| 模块 | 覆盖率 | 测试内容 |
|---|---|---|
| `buffer.rs` | 100% | push/raw_b64 roundtrip、screen_contents 渲染、1MB trim 边界、resize/replay |
| `protocol.rs` | 100% | Request/Response 所有变体的 serde 序列化与反序列化 |
| `lock.rs` | ~98% | 路径生成、write/read roundtrip、heartbeat、scan_active 含错误分支 |
| `ipc.rs` | 100% | write_frame/read_frame framing、超大帧拒绝、IpcClient 所有分支（含 mock Unix socket server）|
| `session.rs` | N/A (集成测试) | Mock PTY 生命周期、命令执行、并发客户端、SIGWINCH resize |
| `dsl.rs` | N/A (集成测试) | TestRunner wait_for、assert_screen_contains、output 缓冲管理 |

运行方式：

```bash
cargo test -p agent-terminal-core
```

覆盖率测量（排除 `session.rs`）：

```bash
cargo tarpaulin -p agent-terminal-core --out Stdout
```

### 集成测试（`crates/core/tests/`）

87 个集成测试覆盖跨组件场景：

| 测试文件 | 数量 | 覆盖场景 |
|---|---|---|
| `ipc_integration_test.rs` | 10 | IPC roundtrip、多客户端并发、重连、大 payload、畸形帧 |
| `lock_integration_test.rs` | 11 | 会话生命周期、心跳、过期清理、前缀匹配、并发心跳 |
| `buffer_integration_test.rs` | 15 | VT100 解析、1MB 边界、并发读写、二进制/Unicode 数据 |
| `protocol_integration_test.rs` | 14 | 序列化、Unicode/特殊字符、无效 JSON、边界情况 |
| `session_integration_test.rs` | 9 | Mock PTY 完整生命周期、命令执行、并发客户端 |
| `cli_integration_test.rs` | 8 | 列表、写入、获取输出的完整工作流 |
| `concurrency_test.rs` | 5 | 并发写入/读取、并发会话创建、并发心跳与清理 |
| `error_handling_test.rs` | 11 | socket 不存在、损坏的 lock 文件、权限拒绝、网络分区 |
| `stress_test.rs` | 4 | 快速写入、大量会话、高频心跳、大输出 |

运行方式：

```bash
# 所有集成测试
cargo test -p agent-terminal-core --test '*'

# 特定测试文件
cargo test -p agent-terminal-core --test ipc_integration_test
```

### session.rs — 仅集成测试

`session.rs` 直接调用 `portable-pty` openpty、spawn zsh、`crossterm::terminal::enable_raw_mode`，需要真实 TTY 和 `/bin/zsh`，无法在 CI 的无头环境中单测。

它被排除在 tarpaulin 覆盖率统计之外（见 `.tarpaulin.toml`）。对它的验证通过手动端到端测试，或集成测试中的 Mock 实现完成。
