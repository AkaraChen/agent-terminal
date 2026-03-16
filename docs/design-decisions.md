# Design Decisions & Tradeoffs

## 为什么用 lock 文件 + Unix socket，而不是……

### 为什么不用 TCP socket？

Unix socket 路径从 lock 文件读取，整个通信链路完全在本地文件系统上，不占用网络端口，权限控制更细（`0600`），也不存在被远程访问的风险。

对于集成测试框架来说，session 和测试代码通常在同一台机器上，Unix socket 足够且更安全。

### 为什么不用命名管道（FIFO）？

FIFO 是单向的，双向通信需要两个 FIFO 管道且没有"连接"概念，无法支持 request-response 模式。Unix socket 原生支持双向通信和多客户端连接。

### 为什么不直接用共享内存？

共享内存没有自然的同步边界，需要额外的信号量或者 mutex。对于这个使用场景（IPC 频率低、数据量中等），Unix socket 的开销完全可以接受，且调试更简单。

---

## IPC 协议选型

### 为什么 JSON + 4-byte length prefix？

- JSON 对人类可读，可以用 `nc -U <socket>` 手动调试
- 4-byte length prefix 确保帧边界清晰，不需要 delimiter 转义
- 相比 MessagePack：JSON 在这个场景不是瓶颈，可读性价值更高

**未来优化**：如果需要传输大量 raw bytes（如 raw_b64 很大），可以考虑切换 MessagePack，但当前 1MB cap 使得单帧最大约 1.4MB，JSON 序列化基本无感。

---

## OutputBuffer 策略

### 为什么用 vt100::Parser 维护屏幕状态，而不是只保留 raw bytes？

`dump` 命令的目的是让外部知道"用户现在看到的终端屏幕是什么"，而不是拿到整个历史输出流。vt100 parser 解析 ANSI escape code 后维护真实的行列状态，可以准确反映当前屏幕（包括覆写、清屏、光标移动等操作后的结果）。

如果只保留 raw bytes，外部需要自己实现 vt100 解析，增加了使用者的负担。

### 为什么保留 raw bytes？

raw bytes 是给二次处理的——比如外部想做自己的 vt100 解析、想 grep 历史输出，或者把完整输出 replay 给另一个终端。两者并存，各有用途。

### 为什么 1MB 上限从头部 drain 而不是 ring buffer？

`Vec<u8>` 的 `drain(..n)` 会移动内存（O(n)），但对于 1MB 数据偶尔发生一次 drain 来说开销可以接受，且实现极简单。真正的 ring buffer 实现会复杂很多。

> **如果将来单 session 的输出流量非常大**（比如 `tail -f` 大日志文件），需要考虑用 `VecDeque<u8>` 或 `bytes::BytesMut`，避免频繁 drain 的 O(n) 拷贝。

---

## Session 目录选在 /tmp

选 `/tmp/agent-terminal/sessions/` 的原因：

1. 机器重启后自动清理——不会有 stale lock 文件堆积
2. 不需要写 `~/.local/share` 或 `~`，不污染 home 目录
3. 对于集成测试来说，session 本来就是短暂的，不需要持久化

**副作用**：如果需要在不同 macOS 用户账号之间通信，`/tmp` 目录权限可能有问题。但当前用例不涉及跨用户通信。

---

## Lock TTL = 5s，Heartbeat = 2s

系数 2.5x 的余量：即使某次心跳写文件有些延迟（比如磁盘 IO 抖动），也不会误判为死亡。

如果 session 进程被强杀（kill -9），最多等 5s 后 lock 就会在 `scan_active` 中被过滤掉，不会永久占用 session 列表。

---

## 用 watch::channel 广播取消而不是 CancellationToken

tokio 的 `watch::channel` 足够轻量，一个 `bool` 值，所有 task clone 一份 receiver 即可。tokio_util 的 `CancellationToken` 功能更完整（可以树状嵌套取消），但对于这里 4 个 task 的简单场景过度工程化。

---

## 为什么 stdin_relay 和 pty_reader 用 spawn_blocking？

`portable-pty` 提供的 reader/writer 是同步阻塞 IO（实现在 `Read`/`Write` trait 上），没有 async 版本。把它们扔到 `spawn_blocking` 线程池中，避免阻塞 tokio 的 current-thread 调度器。

---

## session_id_prefix 匹配

`LockFile::find_active(prefix)` 允许用户只输入 session ID 的前几个字符（比如 `550e84`），节省打字。这是个 UX 取舍：如果两个 session 的 session_id 前缀相同（UUID v4 碰撞概率极低），只返回第一个匹配项，不报错。未来可以改为报歧义错误。

---

## Crate 命名：从 `core` 到 `agent-terminal-core`

### 问题

在集成测试中使用 `use core::ipc::IpcClient` 会与 Rust 标准库的 `core` 模块冲突，导致编译错误：

```rust
error: could not find `prelude` in `core`
```

### 决策

将 crate 名称从 `core` 改为 `agent-terminal-core`，并在代码中使用下划线导入：

```rust
use agent_terminal_core::ipc::IpcClient;
```

### 取舍

- **收益**：消除命名冲突，集成测试可以正常编写；语义更清晰
- **成本**：需要更新所有依赖处的导入语句（CLI crate 的 4 个文件）
- **未考虑的选项**：使用 `extern crate core as core_crate` — 过于 hacky，不符合 Rust 2018+ 的 idiomatic 用法

---

## 集成测试中的 Mock 服务器

### 问题

`session.rs` 依赖真实 PTY（`portable-pty` + `/bin/zsh`），无法在测试环境中直接测试。

### 决策

在集成测试中实现 `MockSession` 结构，模拟 session 服务器的行为：

```rust
struct MockSession {
    buffer: Arc<Mutex<OutputBuffer>>,
    socket_path: String,
    session_id: String,
    shutdown_tx: Option<watch::Sender<bool>>,
}
```

Mock 服务器接受 IPC 连接，维护一个内存中的 buffer，支持 `WriteInput` 和 `GetOutput` 请求。

### 取舍

- **收益**：session 逻辑可测试；无需真实 zsh；测试运行快速稳定
- **成本**：Mock 行为与真实 session 可能有差异；需要单独维护 Mock 实现
- **关键洞察**：Mock 模式验证 IPC 协议和客户端行为，但无法捕获 PTY 相关的 bug（如 ANSI 序列处理）。这类问题需要端到端测试补充。

---

## Lock 文件并发访问的同步策略

### 问题

并发心跳测试中，多个线程同时读取-修改-写入 lock 文件会导致 race condition：

```rust
// 线程 A 读取
let mut lock = LockFile::read(&id)?;
// 线程 B 读取（相同内容）
let mut lock = LockFile::read(&id)?;
// 线程 A 更新并写入
lock.heartbeat()?;
// 线程 B 基于过期数据写入，覆盖 A 的更新
lock.heartbeat()?;
```

### 决策

在测试中使用 `Arc<Mutex<()>>` 同步 lock 文件访问：

```rust
let sync = Arc::new(Mutex::new(()));
// ...
let _guard = sync.lock().unwrap();
let mut lock = LockFile::read(&id).unwrap();
lock.heartbeat().unwrap();
```

### 取舍

- **收益**：测试稳定通过；揭示真实使用场景中的潜在问题
- **成本**：测试代码增加同步开销；生产代码未加同步（依赖单进程写，多进程读）
- **生产环境假设**：正常使用时，一个 session 只有一个 heartbeat task 写入自己的 lock 文件，不存在并发写。集成测试的并发写是人为构造的边界情况。
