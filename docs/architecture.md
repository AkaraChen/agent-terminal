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
run_session()
  │
  ├─ openpty() + spawn zsh
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

**Transport**：Unix socket，路径存放在对应 lock 文件的 `socket_path` 字段，套接字权限为 `0600`。

**Framing**：每个消息由 `[4 字节 u32 LE 长度][JSON 字节流]` 组成。

**Messages**：

```
Request (client → session):
  { "type": "write_input",  "data": "ls\n" }
  { "type": "get_output"  }

Response (session → client):
  { "type": "ok" }
  { "type": "output", "raw_b64": "<base64>", "screen": "..." }
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

> **注意**：vt100::Parser 不动态 resize。如果 SIGWINCH 后终端尺寸变化，Parser 的屏幕大小不会跟着变。此问题记录在 [roadmap.md](roadmap.md)。

---

## 并发模型

- 整体运行在 `tokio` current-thread 调度器下（从 `cli::run()` 手动 `build().block_on()`）
- PTY 读/写使用 `tokio::task::spawn_blocking`，因为 `portable-pty` 的 reader/writer 是同步阻塞 IO
- 多 task 之间通过 `watch::channel(bool)` 广播取消信号
- `OutputBuffer` 和 PTY writer 通过 `Arc<Mutex<_>>` 在 task 间共享
