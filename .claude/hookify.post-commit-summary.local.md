---
name: pre-commit-docs-check
enabled: true
event: bash
pattern: git\s+(add\s+.*|status)|^\s*commit\b
action: warn
---

## 📝 Commit 前文档检查流程

检测到准备 commit 的操作。请完成以下步骤：

### 步骤 1: 查看当前的 Changes

运行 `git diff --staged`（如果有 staged 文件）或 `git diff` 查看即将 commit 的改动。

### 步骤 2: 确定是否需要更新文档

根据改动内容，判断是否需要更新 docs：

| 改动类型 | 需要更新的文档 |
|---------|--------------|
| 新增/修改功能 | `architecture.md` - 更新架构描述 |
| 设计决策、tradeoff | `design-decisions.md` - 记录决策过程 |
| 功能完成、里程碑 | `roadmap.md` - 标记完成项 |
| bug 修复 | 相关文档中更新已知问题状态 |

### 步骤 3: 先更新 docs，再 commit

**正确顺序**：
1. 更新 docs 文件（architectrue.md / design-decisions.md / roadmap.md）
2. `git add docs/`
3. `git commit -m "docs: ..."`（文档更新单独提交）
4. `git add <代码文件>`
5. `git commit -m "feat/fix/refactor: ..."`（代码提交）

**或者一起提交**：
1. 更新 docs 文件
2. `git add docs/ <代码文件>`
3. `git commit -m "type: description"`（确保 commit message 描述代码改动）

### 步骤 4: 总结要记录的内容

在 docs 中记录以下信息：

1. **认知 (Insights)**
   - 这次改动带来的新理解
   - 对系统/架构的新认识
   - 解决问题的关键思路

2. **Tradeoff (权衡)**
   - 做出的设计选择及其取舍
   - 为什么选择当前方案而非其他
   - 牺牲了什么，获得了什么

3. **发现 (Discoveries)**
   - 遇到的意外问题或坑
   - 新发现的最佳实践
   - 值得记录的技术细节

---

**⚠️ 重要**：确保在 commit 代码之前，相关的认知和决策已经记录到 docs 中！
