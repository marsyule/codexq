# CodexQ 文档系统

本文档是 CodexQ 项目的知识地图，明确了文档角色、权威入口（Single Source of Truth）及生命周期管理规则。

---

## 1. 文档架构与三种时间语义

我们遵循《Agent 项目文档治理》原则，将知识严格区分为三种时间语义：

| 类型 | 回答的问题 | 典型载体 | Agent 默认读取 |
| :--- | :--- | :--- | :---: |
| **Current Truth (当前事实)** | 系统现在是什么？协议是什么？ | `docs/PROJECT.md`<br>`docs/architecture.md`<br>`docs/work/current.md` | **是** |
| **Historical Knowledge (历史知识)** | 为什么做成这样？踩过哪些坑？ | `docs/work/history.md`<br>`docs/notes/` | **按需** |
| **Version History (版本历史)** | 文件过去长什么样？改了哪些行？ | Git log, diff, commit | **否** (交由 Git 处理) |

---

## 2. 目录职责与权威来源 (Canonical Sources)

```text
docs/
├── index.md                 # 【索引】本文档：文档治理说明与阅读引导
├── README_zh.md             # 【用户文档】CodexQ 简体中文说明与使用手册
├── PROJECT.md               # 【Current Truth】项目定位、产品规格、边界与非目标
├── architecture.md          # 【Current Truth】系统架构、数据模型、Profile 隔离与 RPC 通信协议
├── work/
│   ├── current.md           # 【Current Truth】当前进行中的迭代任务、技术债跟踪
│   └── history.md           # 【Historical Knowledge】已完成的重大设计决策与踩坑复盘
└── notes/                   # 【按需探索】调研资料、逆向分析草稿与基准测试结果
```

### 权威入口对照
- 关于“项目支持哪些命令与特性”：以 [PROJECT.md](file:///d:/Code/codexq/docs/PROJECT.md) 为准。
- 关于“SQLite 表结构、RPC 消息格式、Profile 沙箱隔离原理”：以 [architecture.md](file:///d:/Code/codexq/docs/architecture.md) 为准。
- 关于“当前在做什么、待办是什么”：以 [work/current.md](file:///d:/Code/codexq/docs/work/current.md) 为准。
- 关于“为什么采用纯标准库 / 为什么采用 stdio RPC”：以 [work/history.md](file:///d:/Code/codexq/docs/work/history.md) 为准。

---

## 3. 文档更新协议

1. **原位更新**：系统架构或业务逻辑变更时，必须直接原位更新对应的 Current Truth 文档，严禁创建诸如 `architecture_v2.md` 或 `new_plan.md`。
2. **任务归档**：在 `work/current.md` 中完成的任务，若包含具有长期参考价值的架构决策或避坑经验，精简提炼后迁移至 `work/history.md`，随后从 `current.md` 中移除以保持上下文轻量。
3. **机械规则升级**：任何可以通过自动化验证测试的规则，应沉淀至 `tests/` 目录，而不是仅仅在文档中进行文字告诫。
