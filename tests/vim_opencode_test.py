#!/usr/bin/env python3
"""
自动化测试 vim 和 opencode 在 agent-terminal 中的渲染

使用方法:
    python3 tests/vim_opencode_test.py

环境要求:
    - agent-terminal 已构建 (cargo build)
    - vim 已安装
    - opencode 已安装 (可选)
"""

import subprocess
import time
import os
import sys


def run(cmd, capture=True):
    """运行 shell 命令"""
    if capture:
        result = subprocess.run(cmd, shell=True, capture_output=True, text=True)
        return result.stdout + result.stderr
    else:
        return subprocess.run(cmd, shell=True)


def get_session_id():
    """获取当前活跃的 session ID"""
    output = run("cargo run --quiet -- list 2>/dev/null")
    lines = [l.strip() for l in output.strip().split('\n')
             if l.strip() and not l.startswith('-')]
    if len(lines) >= 2:
        parts = lines[1].split()
        if len(parts) >= 3:
            return parts[0], parts[2]
    return None, None


def test_vim():
    """测试 vim 在 alternate screen 中的渲染"""
    print("\n测试 Vim...")

    test_file = "/tmp/agent_terminal_vim_test.txt"
    with open(test_file, 'w') as f:
        f.write("Hello from Vim Test!\nLine 2\nLine 3\n")

    sid, _ = get_session_id()
    if not sid:
        print("  ✗ 未找到 session")
        return False

    # 启动 vim
    run(f"cargo run --quiet -- write {sid} $'vim {test_file}\\n'")
    time.sleep(4)

    # 获取输出
    output = run(f"cargo run --quiet -- dump {sid} 2>/dev/null")

    # 验证
    success = True
    checks = [
        ("Hello from Vim", "文件内容"),
        ('"' + test_file + '"', "文件名"),
    ]

    for text, desc in checks:
        if text in output:
            print(f"  ✓ {desc}")
        else:
            print(f"  ✗ {desc}: 未找到 '{text}'")
            success = False

    # 退出 vim
    run(f"cargo run --quiet -- write {sid} $':q!\\n'")
    time.sleep(1)

    os.remove(test_file)
    return success


def test_opencode():
    """测试 opencode 渲染"""
    print("\n测试 OpenCode...")

    # 检查 opencode 是否安装
    if not run("which opencode 2>/dev/null").strip():
        print("  ⚠ 跳过 (未安装)")
        return None

    sid, _ = get_session_id()
    if not sid:
        print("  ✗ 未找到 session")
        return False

    # 启动 opencode help
    run(f"cargo run --quiet -- write {sid} $'opencode --help\\n'")
    time.sleep(3)

    output = run(f"cargo run --quiet -- dump {sid} 2>/dev/null")

    if "opencode" in output and "Commands:" in output:
        print("  ✓ Help 输出渲染正常")
        return True
    else:
        print("  ✗ Help 输出异常")
        return False


def main():
    print("="*60)
    print("Agent Terminal - Vim/OpenCode 渲染测试")
    print("="*60)

    # 清理环境
    run("rm -rf /tmp/agent-terminal/sessions/*; pkill -f 'agent-terminal start' 2>/dev/null")
    time.sleep(1)
    os.makedirs("/tmp/agent-terminal/sessions", exist_ok=True)

    # 启动 session
    print("\n启动 session...")
    proc = subprocess.Popen(
        ["cargo", "run", "--quiet", "--", "start", "--shell", "/bin/bash"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        preexec_fn=os.setpgrp
    )
    time.sleep(4)

    sid, _ = get_session_id()
    if not sid:
        print("✗ Session 启动失败")
        proc.terminate()
        return 1

    print(f"Session: {sid}")

    # 运行测试
    results = {}
    results["vim"] = test_vim()
    results["opencode"] = test_opencode()

    # 清理
    proc.terminate()

    # 结果汇总
    print("\n" + "="*60)
    print("结果:")
    print("="*60)
    for name, result in results.items():
        if result is None:
            print(f"  {name}: 跳过")
        elif result:
            print(f"  {name}: ✓ 通过")
        else:
            print(f"  {name}: ✗ 失败")

    all_passed = all(r is None or r for r in results.values())
    return 0 if all_passed else 1


if __name__ == "__main__":
    sys.exit(main())
