---
name: post-commit-summary
enabled: true
event: bash
pattern: git\s+commit\s+(-m\s+['"]([^'"]+)['"]|.*)
action: warn
---

## 📝 Commit 后总结流程

刚刚执行了 git commit。请完成以下步骤：

### 步骤 1: 查看 docs 文件夹结构
首先查看 `./docs/` 文件夹下有哪些文件，找到合适记录以下内容的地方：
- `architecture.md` - 架构相关的认知
- `design-decisions.md` - 设计决策和 tradeoff
- `roadmap.md` - 项目进展和发现
- 或其他合适的文件

### 步骤 2: 总结并记录
根据 commit 的内容，将以下信息记录到合适的文件中：

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

### 步骤 3: 提交更新
- 将总结更新到 docs 文件夹中合适的文件
- 以原子 commit 提交文档更新
- commit message 示例: `docs: add learnings from [commit subject]`

**提示**：确保按照 CLAUDE.md 中的约定，一个 commit 只做一件事。
