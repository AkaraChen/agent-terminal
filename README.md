# agent-terminal

> This project is under heavy construction.

**See what your CLI tests can't see.**

[中文版本](./README.CN.md)

---

## Why agent-terminal?

Testing CLI tools is hard. You can check exit codes, capture stdout/stderr, but you can't see what actually happened inside the terminal. Did the prompt render correctly? Was the loading spinner animated? Did the user's input echo properly?

**agent-terminal is built on a simple idea: the terminal is a black box that shouldn't be black.**

By transparently proxying a zsh PTY (pseudo-terminal), we capture every byte of interaction—every keypress, every escape sequence, every screen refresh—and let you observe, inject, and assert against the complete terminal state from outside.

## The Philosophy

### Observable by Design

Testing should not require modifying the code under test. agent-terminal sits between your shell and the OS, capturing everything without the target application knowing it exists. No mocks. No hooks. Just transparent observation.

### Programmable Interactions

Manual testing doesn't scale. With agent-terminal, you describe interactions as code: start a session, wait for a prompt, inject commands, assert on screen content. The terminal becomes a programmable interface.

### Snapshot the Invisible

Terminal state is ephemeral—until you capture it. `dump` gives you a point-in-time view of the exact screen your user would see, complete with cursor position, colors, and formatting. Debug rendering issues without guessing.

## What You Can Do

**Session Management**
- Start persistent, instrumented zsh sessions
- List and manage multiple concurrent sessions
- Automatic cleanup on session termination

**Interaction Control**
- Inject keystrokes and commands into running sessions
- Capture raw PTY output and parsed screen state
- Query session health via heartbeat mechanism

**Testing & Automation**
- Verify prompt rendering and UI behavior
- Automate interactive CLI workflows
- Assert on terminal screen contents

## Who It's For

agent-terminal is for developers who:
- Build interactive CLI tools (TUI apps, prompts, spinners)
- Need to test terminal rendering behavior
- Want to automate end-to-end shell workflows
- Are tired of "it works on my machine" for terminal apps

## The Vision

We believe terminal applications deserve the same testing rigor as web or mobile apps. The future of agent-terminal includes declarative test scripts, real-time output streaming, and distributed test runners connecting to remote sessions—turning the terminal from a testing blind spot into a fully observable, testable surface.

---

*Currently macOS only. Linux support planned for v0.3.*
