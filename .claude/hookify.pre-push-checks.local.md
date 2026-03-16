---
name: pre-push-checks
enabled: true
event: bash
pattern: git\s+push
action: block
---

🚫 **Push 被阻止 - 请完成以下检查后再执行 push**

根据项目规范，在 `git push` 之前必须完成以下步骤：

1. **运行 lint** - 检查代码规范
   ```bash
   cargo clippy  # 或项目对应的 lint 命令
   ```

2. **运行 fmt** - 格式化代码
   ```bash
   cargo fmt     # 或项目对应的 fmt 命令
   ```

3. **运行 test** - 确保测试通过
   ```bash
   cargo test    # 或项目对应的 test 命令
   ```

4. **检查是否有变更** - 如果有文件变更，请先 commit
   ```bash
   git status    # 查看状态
   git add .
   git commit -m "chore: pre-push fixes"  # 或使用更具体的提交信息
   ```

**如果确定要跳过这些检查，请确认后再执行 push。**
