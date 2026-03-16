# agent-terminal: Project Overview

## 项目目的

`agent-terminal` 是一个用于 CLI **集成测试**的框架。

核心想法是：通过透传一层 zsh PTY（伪终端），使得测试框架程序能捕捉到用户与 shell 的**所有交互**，并在任何时刻从外部注入命令或查询当前屏幕状态。

典型使用场景：

- 自动化测试某个 CLI 工具的交互行为（如验证提示符、输出内容、错误处理等）
- 远程"回放"一段命令序列到已打开的终端会话
- 从外部实时抓取 PTY session 的当前屏幕快照

## 当前实现阶段

v0.1：基础 PTY session 管理 + IPC 框架。支持：

1. `start` — 开启一个受管控的 zsh PTY session
2. `list` — 列出所有活跃 session
3. `write <id> <data>` — 向指定 session 注入输入（相当于模拟键盘输入）
4. `dump <id>` — 拉取 session 当前屏幕的文字快照

## 平台

- **macOS**: 使用 `/bin/zsh` 作为默认 shell
- **Linux**: 使用 `/bin/bash` 作为默认 shell（实验性支持）

PTY 系统基于 `portable-pty` crate，支持通过 `--shell` 参数指定自定义 shell 路径。

---

有关架构和设计决策，见 [architecture.md](architecture.md)。
有关快速上手，见 [quickstart.md](quickstart.md)。
有关未来计划，见 [roadmap.md](roadmap.md)。
