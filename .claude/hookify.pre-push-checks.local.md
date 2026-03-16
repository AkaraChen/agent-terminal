---
name: pre-push-checks
enabled: true
event: bash
pattern: git\s+push
action: warn
---

⚠️ **Pre-push 检查流程**

检测到 `git push` 命令。请按以下步骤执行：

### 步骤 1: 运行检查与修复

1. **运行 fmt** - 自动格式化代码
   ```bash
   cargo fmt --all
   ```

2. **运行 lint** - 检查代码规范
   ```bash
   cargo clippy --all-targets --all-features
   ```

3. **运行 test** - 确保测试通过
   ```bash
   cargo test
   ```

### 步骤 2: 处理可能的文件变更

检查是否有因格式化产生的变更：

```bash
git status --porcelain
```

**如果有变更**：
1. 暂存变更：`git add .`
2. 创建 commit：`git commit -m "style: fmt and clippy fixes"`
