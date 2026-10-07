# Knowl Pad 产品需求文档（PRD）

| 项目 | 内容 |
| --- | --- |
| 文档版本 | v2.3 |
| 编写日期 | 2026-09-17（v2.0），2026-09-22（v2.1 修正），2026-09-25（v2.2 开发可用性修订），2026-09-29（v2.3 M0 收尾修订） |
| 文档状态 | 评审修正版（v2.3：M0 收尾修订，新增 IPC 契约门禁 17 与 tauri-specta 决策，见技术方案 §8.3/§12.2 与 `docs/history/M0收尾修订说明.md`；历史修订见技术方案 §12.1；M0 核实结果见技术方案附录 D） |
| 产品版本目标 | V1.0（首个稳定版） |
| 项目状态 | M0 / M1 / M2 已完成，当前进入 M3（解析与索引引擎） |
| 依赖基线 | 《Knowl_Pad_完整技术方案_v5》依赖锁定矩阵 |
| 架构形态 | 模块化单体（Modular Monolith），**无插件系统** |
| 唯一真相源 | 本文档。与任何其他文档冲突时以本文档为准 |

> **本版说明**：本 PRD 为完全重写版本，不继承任何历史 PRD 的章节结构与内容。产品范围明确**不包含插件系统、插件市场、插件沙箱、第三方代码执行**等任何扩展机制。所有功能均为内置能力。

---

## 0. 阅读指引

| 章节 | 面向读者 | 用途 |
| --- | --- | --- |
| §1 产品概述 | 全员 | 明确做什么、不做什么 |
| §2 架构约束 | 架构 / 研发 | 不可协商的技术边界 |
| §3 数据模型 | 后端研发 | 存储契约，DDL 可直接落地 |
| §4 功能需求 | 全员 | 逐模块需求 + 验收标准 |
| §5 IPC 接口契约 | 前后端研发 | 前后端协作的唯一接口约定 |
| §6 非功能需求 | 全员 | 性能、安全、可靠性量化指标 |
| §7 页面与路由 | 前端研发 | 视图清单 |
| §8 开发规约 | 研发 | 编码红线与测试要求 |
| §9 实施路线 | 项目管理 | 里程碑与阶段验收 |
| §10 风险 | 全员 | 已知风险与未决问题 |

**需求编号规则**

| 前缀 | 含义 | 示例 |
| --- | --- | --- |
| `FR-<模块>-<序号>` | 功能需求（Functional Requirement） | `FR-VAULT-03` |
| `NFR-<类别>-<序号>` | 非功能需求（Non-Functional Requirement） | `NFR-PERF-01` |
| `SEC-<序号>` | 安全需求 | `SEC-04` |

**优先级定义**

| 标记 | 含义 | 交付要求 |
| --- | --- | --- |
| `P0` | 必须实现 | V1.0 不可裁剪，缺失即不可发布 |
| `P1` | 应该实现 | V1.0 目标交付，特殊情况可延至 V1.1 并记录 |
| `P2` | 可以实现 | 资源允许时交付，可延后 |

---

## 0.1 术语表

| 术语 | 定义 |
| --- | --- |
| **Vault（知识库）** | 用户指定的一个本地文件夹，作为一批笔记的根容器。Vault 内的 Markdown 文件构成知识网络。一个用户可拥有多个 Vault，同一时刻仅一个处于打开状态。 |
| **Note（笔记）** | Vault 内的 `.md` 文件。是链接、标签、搜索、图谱的基本单位。 |
| **Attachment（附件）** | Vault 内的非 Markdown 文件（图片、PDF、音视频等），由笔记引用。 |
| **真相源（Source of Truth）** | 指数据的权威存储位置。本项目中 **Markdown 文件是唯一真相源**，SQLite 数据库仅为其派生索引。 |
| **索引库** | 存放于 `<Vault>/.knowlpad/index.db` 的 SQLite 数据库，包含文件树、链接、标签、标题、全文索引。**可随时删除并完整重建**，不承载任何独占数据。 |
| **全局库** | 存放于用户配置目录的 SQLite 数据库，记录 Vault 注册表、界面偏好、窗口状态。跨 Vault 共享。 |
| **Wikilink** | 以双方括号书写的双向链接语法 `[[目标笔记]]`，是 Markdown 生态中广泛使用的扩展惯例。支持 `[[目标\|别名]]`、`[[目标#标题]]`、`[[目标#^块ID]]`，解析规则见 §3.1.2。 |
| **悬空链接（Dangling Link）** | 指向尚不存在笔记的链接。属于合法状态，不视为错误，可点击创建目标笔记。 |
| **反向链接（Backlink）** | 指向当前笔记的所有链接集合。 |
| **Frontmatter** | Markdown 文件头部以 `---` 包裹的 YAML 元数据块。 |
| **Block ID** | 形如 `^abc123` 的块级锚点标识，写在段落末尾，用于精确链接到某个块。 |
| **索引签名（Index Signature）** | 记录索引库是由哪个版本的解析器、分词器、schema 生成的指纹。签名不匹配即触发全量重建。 |
| **IPC** | 前端 Webview 与 Rust 主进程之间的进程间通信，通过 Tauri 的 `invoke`（命令）与 `event`（事件）实现。 |

---

## 0.2 命名规范（强制统一）

历史文档中存在多种写法并存的问题。本表为唯一规范，**所有文档、代码、配置、路径必须严格遵守**。

| 使用场景 | 规范写法 | 示例 |
| --- | --- | --- |
| 产品名称（面向用户的自然语言） | `Knowl Pad`（中间一个半角空格） | 欢迎使用 Knowl Pad |
| npm 包名 / Cargo crate 名 | `knowl-pad`（kebab-case） | `"name": "knowl-pad"` |
| 数据库目录名（Vault 内隐藏目录） | `.knowlpad`（全小写，无分隔符） | `<Vault>/.knowlpad/index.db` |
| 数据库文件名 | `index.db`（Vault 级索引库）、`global.db`（应用级全局库） | 分别位于 `.knowlpad/` 与配置目录内 |
| 应用 Bundle 标识符 | `com.knowlpad.desktop`（反向域名，全小写） | `tauri.conf.json` 的 `identifier` |
| 全局配置目录名 | `knowl-pad`（遵循各平台配置目录惯例） | `~/.config/knowl-pad/` |
| Git 仓库名 | `knowl-pad` | `gitee.com/<owner>/knowl-pad` |
| 交付文档文件名前缀 | `Knowl-Pad-` | `Knowl-Pad-PRD.md` |
| TypeScript 类型名前缀 | `Kp`（仅用于需要消歧的导出类型） | `KpSearchResult` |
| Rust 模块名 | `snake_case` | `knowl_pad::index_engine` |
| IPC Command 名 | `snake_case`，`<域>_<动作>` | `note_read`、`search_fulltext` |
| IPC Event 名 | `kp://<域>/<事件>`（kebab-case） | `kp://fs/created`、`kp://note/updated` |

**禁止**：`KnowlPad`（驼峰无空格，仅允许出现在安装包产物文件名中，因其需符合各平台安装包命名约束）、`knowl_pad` 作为产品名、`Knowl-Pad` 作为产品名出现在正文散文中。

> **Rust 模块名的例外**：Rust 语言规范要求模块名用 `snake_case`，故源码模块路径为 `knowl_pad::*`（如 `knowl_pad::domain::md_parse`）。这是语言强制约定，**不视为违反**上表——表中 `knowl-pad` 指的是包名/crate 名字符串，而非 Rust 模块路径。

**一致性门禁**：CI 增加一项命名检查（grep 扫描仓库中是否出现上述禁止写法，排除本规范说明段落自身），命中即失败。实现见技术方案 §11.4 门禁清单。

---

## 1. 产品概述

### 1.1 产品定位

Knowl Pad 是一款**跨平台、本地优先**的笔记与知识管理软件，以双向链接与知识图谱为核心组织能力。

**一句话定位**：把用户的一个本地文件夹，变成一张可检索、可导航、可可视化的个人知识网络，且用户对这些数据拥有完全的、不依赖本软件的所有权。

> **表述约定（法务）**：本文档描述产品能力时，一律以 Knowl Pad 自身的设计原则与功能规格为依据，**不以任何第三方软件作为产品定位或体验的参照系**。文中出现的第三方软件配置目录名（如 `.obsidian/`）仅为「用户磁盘上客观存在的、本软件不得干涉的目录」这一技术事实，属兼容性约束，不构成任何形式的关联、对标或比较主张。

| 维度 | 定位 |
| --- | --- |
| 部署形态 | 桌面客户端（Windows / macOS / Linux），无服务端，无账号体系 |
| 数据归属 | 全部数据保存在用户本地磁盘，软件不上传任何用户内容 |
| 数据格式 | 纯文本 Markdown（`.md`）+ 标准文件夹结构，无任何私有格式锁定 |
| 联网需求 | 除「检查软件更新」外，全部功能离线可用 |
| 商业模式 | 开源（**MIT OR Apache-2.0** 双许可）/ 免费。构建产物发布至 GitHub Release，并同步至 Gitee Release 作为国内分发渠道（2026-09-30 更新：两个仓库均为公开仓库） |
| 扩展性 | **V1.0 不提供插件机制**，全部能力内置 |

### 1.2 核心设计原则

以下五条原则具有最高优先级。任何功能设计、技术选型、需求变更若与之冲突，必须服从原则或走正式评审推翻原则。

| # | 原则 | 含义 | 违反后果 |
| --- | --- | --- | --- |
| **P-1** | **Markdown 文件是唯一真相源** | 笔记内容、链接、标签、frontmatter 全部可从 `.md` 文件完整还原。索引库是纯派生数据，删除后必须能 100% 重建。 | 用户数据被私有格式锁定，违背产品立身之本 |
| **P-2** | **永不静默破坏用户数据** | 任何写文件操作必须原子化；任何批量改写（如重命名时更新链接）必须先备份、可预览、可回滚、失败即中止并保留原状。 | 不可恢复的数据损坏，用户信任彻底崩塌 |
| **P-3** | **UI 永不阻塞在 IO 上** | 文件读写、索引、搜索全部在 Rust 侧异步执行；前端通过事件接收进度。主线程输入响应必须 < 50ms（编辑器输入延迟目标 < 16ms，见 NFR-PERF-07）。 | 大库场景下软件"卡死"，产品不可用，违背 NFR-PERF-07 等全部性能承诺 |
| **P-4** | **不执行任何第三方代码** | 无插件系统即无第三方代码执行面。渲染用户 Markdown 时，HTML 输出必须经 DOMPurify 净化。威胁模型因此大幅简化。 | 引入 RCE / XSS 高危攻击面 |
| **P-5** | **遵循既有的 Markdown 扩展语法惯例，不自创方言** | Wikilink、标签、frontmatter、Block ID 的解析规则**以本文档 §3.1 定义的规则条款为唯一权威依据**，这些条款采纳了 Markdown 生态中已被广泛使用的书写惯例。不自创互不兼容的私有语法。 | 用户已有的笔记库无法被正确解析，违背 P-1（数据可读性）与 §1.3 中"存量笔记库用户"的核心诉求 |

### 1.3 目标用户与核心场景

| 用户画像 | 特征 | 核心诉求 |
| --- | --- | --- |
| **知识工作者 / 研究者** | 长期积累数千至上万条笔记，重视资料间的关联 | 双链、图谱、全文搜索；数据必须永久可读 |
| **开发者 / 技术人员** | 熟悉 Markdown 与 Git，可能把 Vault 纳入版本控制 | 纯文本存储、无数据库锁定、`.knowlpad/` 可加入 `.gitignore` |
| **存量笔记库用户** | 已用其他工具积累 Markdown 笔记库，使用 wikilink 等扩展语法，书写习惯已固化 | 直接打开现有文件夹即可用，链接与标签解析不出错，不要求转换或重建笔记 |
| **隐私敏感用户** | 拒绝云同步、拒绝账号登录 | 完全离线、零遥测、无数据外发 |

**核心使用场景**

| 场景 ID | 场景描述 | 涉及模块 |
| --- | --- | --- |
| SC-01 | 用户首次启动，选择本地一个已有文件夹作为 Vault，软件扫描并建立索引，随后可立即搜索与浏览 | Vault、索引引擎、文件树 |
| SC-02 | 用户新建笔记，在正文中键入 `[[` 触发补全，选择一篇已有笔记建立链接 | 编辑器、链接解析 |
| SC-03 | 用户打开一篇笔记，在右侧面板查看"谁引用了我"，点击跳转到来源上下文 | 反向链接 |
| SC-04 | 用户打开知识图谱，看到整个 Vault 的关联网络，点击节点跳转，悬停高亮邻居 | 图谱 |
| SC-05 | 用户搜索一个中文短语，在 1 万篇笔记中 200ms 内得到带高亮片段的结果 | 全文搜索 |
| SC-06 | 用户重命名一篇被 300 处引用的笔记，软件自动更新全部引用，不产生死链 | 文件操作、链接改写 |
| SC-07 | 用户误删笔记，从回收站恢复，内容与链接关系完整还原 | 回收站 |
| SC-08 | 用户在 Vault 外（如系统文件管理器）新增/修改/删除文件，软件自动感知并更新索引与界面 | 文件监听、索引引擎 |
| SC-09 | 用户通过 `Ctrl+P` 唤起命令面板，模糊搜索并执行"新建今日笔记"之外的任意内置命令 | 命令面板 |
| SC-10 | 新版本发布，用户收到更新提示，验证签名后一键更新 | 自动更新 |

### 1.4 V1.0 功能范围

#### In Scope（V1.0 交付）

| # | 模块 | 模块代号 | 优先级 | 详见 |
| --- | --- | --- | --- | --- |
| 1 | Vault 知识库管理 | `VAULT` | P0 | §4.1 |
| 2 | 文件树与文件操作 | `FILE` | P0 | §4.2 |
| 3 | Markdown 编辑器 | `EDITOR` | P0 | §4.3 |
| 4 | 双向链接与反向链接 | `LINK` | P0 | §4.4 |
| 5 | 全文搜索 | `SEARCH` | P0 | §4.5 |
| 6 | 知识图谱 | `GRAPH` | P1 | §4.6 |
| 7 | 标签系统 | `TAG` | P0 | §4.7 |
| 8 | 附件管理 | `ATTACH` | P1 | §4.8 |
| 9 | 命令面板 | `PALETTE` | P0 | §4.9 |
| 10 | 回收站 | `TRASH` | P0 | §4.10 |
| 11 | 设置与主题 | `SETTINGS` | P0 | §4.11 |
| 12 | 自动更新 | `UPDATE` | P1 | §4.12 |

#### Out of Scope（V1.0 明确不做）

以下项目**不是"待补充"，而是经过决策的明确排除**。若需纳入，必须走正式需求变更流程并重新评估架构影响。

| # | 排除项 | 排除理由 |
| --- | --- | --- |
| X-01 | **插件系统 / 插件市场 / 插件 API / 第三方扩展** | 本版本核心决策：不提供任何第三方代码执行能力。此排除使威胁模型从"五层防护"简化为"四层防护"，大幅降低安全设计与测试成本。 |
| X-02 | 云同步 / 多端同步 / 账号体系 | 违背本地优先原则；同步冲突解决是独立的大型子系统 |
| X-03 | 移动端（iOS / Android） | Tauri 2 虽支持，但移动端交互范式（文件树、图谱、快捷键）需完全重新设计 |
| X-04 | 协同编辑 / 实时多人 | 与本地优先架构根本冲突 |
| X-05 | 每日笔记 / 日历视图 | 属增强能力，列入 V1.1 候选 |
| X-06 | 文档大纲面板（Outline） | 属增强能力，列入 V1.1 候选。注：标题数据在 V1.0 已入库（用于锚点解析），仅缺 UI |
| X-07 | 导出 PDF / HTML / 打印 | 属增强能力，列入 V1.1 候选 |
| X-08 | 第三方笔记库一键迁移向导 | V1.0 通过"直接打开现有文件夹"天然支持（见 §4.1）：标准 Markdown 库无需迁移即可使用，故无需独立迁移功能 |
| X-09 | 内置 AI 能力（摘要 / 问答 / 续写） | 需联网与 API Key 管理，与离线原则冲突；且引入提示词注入等新威胁面 |
| X-10 | 数据库笔记 / 表格视图（在 Markdown 中嵌入查询语法以生成动态表格） | 独立的大型子系统，且需在 Markdown 中引入非标准语法，违背 P-5 |
| X-11 | 版本历史 / 快照对比 | V1.0 仅保留回收站软删除；文件级历史建议用户借助 Git |
| X-12 | 自定义 CSS 主题 / 主题商店 | 主题商店等同插件分发渠道，与 X-01 冲突。V1.0 仅提供内置深色/浅色主题与有限的字体、字号、行宽调节 |
| X-13 | 网页剪藏 / 浏览器扩展 | 独立产品线 |
| X-14 | 加密 Vault / 密码保护 | 需完整的密钥管理与找回策略，V1.0 不承诺；错误实现比不实现更危险 |
| X-15 | 遥测 / 崩溃上报 | 违背隐私定位。**明确不采集任何用户数据**，包括匿名统计 |

### 1.5 成功判据

V1.0 视为成功交付，需同时满足：

| # | 判据 | 验证方式 |
| --- | --- | --- |
| SJ-01 | §4 中全部 P0 需求的验收标准 100% 通过 | 逐条执行 §4 各模块验收标准 |
| SJ-02 | §6.1 全部性能指标在规定的基准环境下达标 | 自动化性能基准测试（见 §8.4） |
| SJ-03 | 删除 `.knowlpad/` 目录后重新打开 Vault，全部链接、标签、搜索结果与删除前一致（**前提：回收站为空**） | 索引重建一致性测试（NFR-REL-03）。回收站非空时其内容随 `.knowlpad/` 一并删除，属预期行为，UI 必须事先明确警示（FR-SIG-04） |
| SJ-04 | 直接打开一个**已存在的第三方 Markdown 笔记库**（含 wikilink、嵌套标签、frontmatter、Block ID 等扩展语法），链接解析准确率 ≥ 99% | 扩展语法解析测试集（附录 B），期望结果以 §3.1 规则条款为准。**样本数须先扩充至 ≥ 100 条再按 ≥ 99% 判定**；附录 B 当前 25 条基线要求 25/25 全通过（24/25 = 96% < 99%） |
| SJ-05 | §6.3 威胁模型中所有已识别威胁均有对应实现的控制措施，并通过安全测试用例 | 安全测试用例执行 |
| SJ-06 | 三平台（Windows / macOS / Linux）安装包均可完成安装、启动、打开 Vault、编辑保存、搜索、查看图谱、更新 | 跨平台验收清单（§6.4） |

---

## 2. 架构约束（不可协商）

### 2.1 运行架构

Knowl Pad 采用 Tauri 2.0 的**前后端分离混合架构**：

| 层 | 技术 | 职责 | 不负责 |
| --- | --- | --- | --- |
| **前端（Webview 进程）** | Vue 3 + Vite + TypeScript | UI 渲染、用户交互、视图状态管理、Markdown 渲染为 HTML、命令面板模糊匹配 | **不做任何文件 IO**、**不直连数据库**、**不做 Markdown 结构化解析建索引** |
| **IPC 边界** | Tauri `invoke` / `event` + Capabilities 权限系统 | 前后端唯一通道，受权限白名单约束 | — |
| **后端（Rust 主进程）** | Tauri 2 + rusqlite + 本地文件系统 | 文件 IO、数据库访问、Markdown 解析与索引构建、全文搜索、文件监听、原生系统调用、自动更新 | **不做任何 UI 渲染**、**不持有视图状态** |

**强制约束**

| 约束 ID | 内容 |
| --- | --- |
| `AC-01` | 前端**禁止**使用 `@tauri-apps/plugin-fs` 直接读写文件。所有文件访问必须通过本项目自定义的 Rust Command，以便统一施加路径校验、原子写入与审计。 |
| `AC-02` | 前端**禁止**引入任何 SQLite 的 WASM 实现。数据库只存在于 Rust 侧。 |
| `AC-03` | Rust 侧**禁止**依赖任何 WebView 前端状态。Command 必须是无状态的纯函数式调用（状态只来源于 `AppState` 与磁盘）。 |
| `AC-04` | 所有可能耗时 > 16ms 的 Rust 操作**必须**在异步任务或独立线程执行，**禁止**阻塞 Tauri 的 IPC 处理线程（16ms 为 60Hz 单帧预算，任何 > 50ms 的阻塞即红线 R-08 违规；二者是同一约束的「异步化触发线」与「绝对上限」）。 |
| `AC-05` | 前端**禁止**通过 `v-html` 渲染未经 DOMPurify 净化的 HTML。 |
| `AC-06` | 索引库中**禁止**存储任何无法从 Markdown 文件重建的数据。若某数据必须持久化且无法重建（如用户对某笔记的自定义排序），必须存入 Markdown frontmatter 或全局库，不得存入索引库。 |
| `AC-07` | 全部 IPC 传输的路径**必须**是相对 Vault 根的相对路径（统一使用 `/` 分隔）。绝对路径仅允许出现在 Vault 注册与切换接口中。 |

### 2.2 技术栈基线

技术栈版本以《Knowl-Pad-完整技术方案》§3 的锁定矩阵为权威（本 PRD 仅列摘要，避免形成第二处维护点）。上游更新跟进记录与「刻意不跟进」项的理由见技术方案 §3.2.1、§3.6.1。

**基线摘要**（2026-09-21 跟进上游后）

| 类别 | 关键版本 |
| --- | --- |
| 运行时 | Node.js v24.19.0 LTS |
| 包管理 | pnpm 12.4.1 |
| 语言 | TypeScript 6.0.3（**刻意停留 6.x**，上游 latest 已是 7.0.2，理由见技术方案 §3.2.1） |
| 构建 | Vite `^8.3.0`（Rolldown + Oxc + Lightning CSS）。**已决策不回退**，生产回归风险改为必须规避（技术方案 §3.4.3） |
| 前端框架 | Vue 3.5.43 / Pinia 4.0.3 / vue-router 5.3.1 |
| 桌面框架 | Tauri 2.12.0（crate，2026-09-30 随 MSRV 1.90 跟进）/ @tauri-apps/cli 2.11.5 / @tauri-apps/api 2.11.1 |
| UI | Tailwind CSS 4.3.3 / shadcn-vue 2.8.2（基于 Reka UI）/ lucide-vue-next 1.0.0 |
| 编辑器 | md-editor-v3 6.5.6（MVP 阶段，**刻意不升 7.0.0**）→ CodeMirror 6（M8 正式版） |
| 代码高亮 | MVP 用 md-editor-v3 内置高亮；M8 起**统一用 CodeMirror 6 / Lezer 原生方案**，不引入第三方高亮库（技术方案 §3.2.4） |
| 渲染 | markdown-it 15.0.2 + DOMPurify 3.4.15 |
| 可视化 | cytoscape 3.34.3 + cytoscape-fcose 2.2.0 |
| 模糊搜索 | fuse.js 7.5.0 |
| 虚拟滚动 | @tanstack/vue-virtual 3.13.39 |
| 拖拽 | vue-draggable-next 2.3.0 |
| 数据库 | rusqlite 0.40.2（`bundled` feature） |
| Rust MSRV | **1.90**（由 tauri 2.12 家族要求；`time` / `image` 的 1.88 已不再是约束，2026-09-30 更新） |

### 2.3 数据所有权与真相源

这是本产品最核心的架构决策，直接对应原则 P-1。

```
┌─────────────────────────────────────────────────────────┐
│  Markdown 文件（.md）+ 文件夹结构  ← 唯一真相源          │
│  · 笔记正文、Wikilink、标签、frontmatter、Block ID       │
│  · 用户可用任意编辑器直接修改，可纳入 Git                │
│  · 丢失索引不影响任何数据                                │
└──────────────────────┬──────────────────────────────────┘
                       │ 解析 / 索引（单向派生）
                       ▼
┌─────────────────────────────────────────────────────────┐
│  索引库 index.db  ← 纯派生数据，可丢弃可重建             │
│  · 文件树、链接表、标签表、标题表、FTS5 全文索引         │
│  · 删除后重新打开 Vault 即可完整重建                     │
└─────────────────────────────────────────────────────────┘
```

**派生数据的判定标准**：一项数据若属于索引库，必须满足「仅通过读取 Vault 内文件即可完整重建」。

**例外处理**：确实需要持久化且无法从 Markdown 重建的数据，按以下规则存放：

| 数据类型 | 存放位置 | 理由 |
| --- | --- | --- |
| Vault 注册表、界面偏好、窗口位置、最近打开 | 全局库 `global.db` | 属于软件配置，不属于用户知识内容 |
| 用户对某笔记的自定义元数据（如别名、归档状态） | Markdown frontmatter | 属于笔记自身属性，应随文件走，且用户可见可编辑 |
| 回收站状态 | `.knowlpad/trash/manifest.json`（原始路径映射）+ 索引库 `file.deleted` 字段 + 实体文件位于 `.knowlpad/trash/` | 回收站**不是纯派生视图**：删除 `.knowlpad/` 会连带销毁实体文件。原始路径必须记在与索引库解耦的 manifest 中，否则 `index.db` 重建后将无法恢复（见 FR-TRASH-02/12） |

### 2.4 存储布局

#### 2.4.1 Vault 内布局

```
<用户选择的 Vault 根目录>/
├── 任意用户文件夹与 .md 文件          ← 用户数据，软件不干涉其组织结构
├── <用户定义的附件目录>/               ← 默认与笔记同级，可在设置中指定
└── .knowlpad/                          ← 软件私有目录（自动创建；⚠️ 内含回收站实体与备份，删除会一并丢失，见 §4.10）
    ├── index.db                        ← 索引库
    ├── index.db-shm / index.db-wal     ← SQLite WAL 模式副产物
    ├── trash/                          ← 回收站实体目录，按删除时间戳组织
    │   ├── manifest.json               ← 原始路径映射（与索引库解耦；索引重建后仍可恢复，见 FR-TRASH-02）
    │   └── <yyyy-MM>/
    │       └── <原相对路径>__<时间戳>_<随机串>.md
    └── backup/                         ← 批量改写前的自动备份（保留最近 N 次）
        └── <操作时间戳>/
            └── <被改写文件的原内容>
```

| 需求 ID | 需求 | 优先级 |
| --- | --- | --- |
| `FR-STORAGE-01` | `.knowlpad/` 目录在 Vault 首次打开时自动创建，权限为仅当前用户可读写 | P0 |
| `FR-STORAGE-02` | 打开 Vault 时若检测到用户已有 `.obsidian/` 目录，**不得**修改、移动或删除其中任何内容 | P0 |
| `FR-STORAGE-03` | 软件应提示用户将 `.knowlpad/` 加入 `.gitignore`（若检测到 Vault 是 Git 仓库），但**不得**自动修改用户的 `.gitignore` | P1 |
| `FR-STORAGE-04` | `.knowlpad/` 内的文件**不得**出现在文件树、搜索结果、图谱、标签统计中 | P0 |

#### 2.4.2 全局配置布局

| 平台 | 配置目录 | 依据 |
| --- | --- | --- |
| Windows | `%APPDATA%\knowl-pad\` | Tauri `path.app_config_dir()` |
| macOS | `~/Library/Application Support/knowl-pad/` | 同上 |
| Linux | `~/.config/knowl-pad/` | 遵循 XDG，同上 |

```
<配置目录>/
├── global.db            ← 全局库：Vault 注册表、偏好、窗口状态
├── logs/
│   └── knowl-pad-<yyyy-MM-dd>.log    ← 按日滚动，保留最近 14 天
└── update-cache/        ← 更新包临时下载目录，安装后清理
```

> **实现约束**：配置目录路径**必须**通过 Tauri 的 `path` API 获取，**禁止**硬编码或手工拼接平台路径。

### 2.5 模块化单体的边界纪律

无插件系统不等于无边界。为防止代码随迭代退化为"大泥球"，强制约定模块边界。

#### 2.5.1 前端 Feature 模块划分

```
src/
├── app/                 # 应用装配层：路由、Pinia 装配、全局布局、启动流程
├── core/                # 跨模块共享内核，不含业务逻辑
│   ├── ipc/             #   IPC 客户端封装：类型安全的 invoke/event 包装
│   ├── markdown/        #   markdown-it 实例、渲染管线、DOMPurify 净化
│   ├── theme/           #   主题系统、CSS 变量
│   ├── shortcut/        #   快捷键注册中心
│   └── utils/           #   纯函数工具（无副作用、无状态）
├── features/            # 业务模块，按 §4 的模块代号一一对应
│   ├── vault/
│   ├── file-tree/
│   ├── editor/
│   ├── link/            #   双链、反向链接面板
│   ├── search/
│   ├── graph/
│   ├── tag/
│   ├── attachment/
│   ├── palette/
│   ├── trash/
│   ├── settings/
│   └── update/
└── shared/              # 跨 feature 共享的 UI 组件（shadcn-vue 封装）与类型
```

| 约束 ID | 内容 |
| --- | --- |
| `FE-01` | 每个 `features/<模块>/` 内部结构固定为 `components/`、`composables/`、`stores/`、`types.ts`、`index.ts` |
| `FE-02` | **feature 之间禁止直接互相 import**。跨 feature 通信只能通过：① Pinia store（在 `app/` 层注册）② 事件总线 ③ 通过 `index.ts` 显式导出的公共 API |
| `FE-03` | `core/` 禁止 import 任何 `features/` 内容（依赖方向单向向下） |
| `FE-04` | `features/` 禁止直接调用 `@tauri-apps/api` 的 `invoke`，必须经由 `core/ipc/` 的类型安全封装 |
| `FE-05` | 每个 feature 的 `index.ts` 是该模块的唯一对外出口，未导出的内部实现视为私有 |

#### 2.5.2 Rust 领域模块划分

```
src-tauri/src/
├── main.rs              # 入口：Tauri Builder 装配、Command 注册
├── lib.rs               # crate 根
├── state.rs             # AppState 定义与生命周期
├── error.rs             # 统一错误类型 AppError + 错误码映射
├── commands/            # IPC Command 层（薄壳，不含业务逻辑）
│   ├── mod.rs
│   ├── vault.rs  note.rs  file.rs  search.rs
│   ├── graph.rs  tag.rs   attach.rs trash.rs
│   └── settings.rs  update.rs  index.rs
├── domain/              # 领域服务层（业务逻辑，可独立单元测试）
│   ├── vault.rs         #   Vault 生命周期、路径解析与安全校验
│   ├── note_io.rs       #   笔记读写、原子写入、备份
│   ├── fs_ops.rs        #   创建/重命名/移动/删除，含链接改写编排
│   ├── index_engine.rs  #   索引引擎：全量/增量、队列、一致性
│   ├── md_parse.rs      #   Markdown 解析：链接、标签、标题、frontmatter、Block ID
│   ├── tokenize.rs      #   中文分词（jieba-rs 封装）
│   ├── search.rs        #   FTS5 查询构造、结果排序与摘要高亮
│   ├── graph.rs         #   图谱数据构建、范围裁剪、降级策略
│   ├── tag.rs           #   标签规范化、层级解析、统计
│   ├── trash.rs         #   软删除、恢复、彻底删除、过期清理
│   └── watcher.rs       #   文件系统监听、去抖动、事件归并
├── storage/             # 存储层
│   ├── db.rs            #   连接管理、WAL、事务辅助
│   ├── global_db.rs     #   全局库 schema 与 DAO
│   ├── index_db.rs      #   索引库 schema、迁移、DAO
│   └── signature.rs     #   索引签名计算与校验
└── platform/            # 平台差异适配（路径、回收站、打包相关）
```

| 约束 ID | 内容 |
| --- | --- |
| `RS-01` | `commands/` 层**只做**参数反序列化、调用 `domain/`、序列化结果与错误映射。**禁止**出现 SQL、文件 IO、业务分支判断。单个 Command 函数体建议 ≤ 30 行。 |
| `RS-02` | `domain/` **禁止**直接引用 `tauri::State` 之外的 Tauri 类型，以保证可脱离 Tauri 进行单元测试 |
| `RS-03` | SQL 语句**只允许**出现在 `storage/` 层。`domain/` 通过 DAO 接口访问数据 |
| `RS-04` | 全部对外错误必须收敛为 `error.rs` 中的 `AppError` 枚举，并携带 §5.2 定义的错误码。**禁止**向前端抛出未分类的 `String` 错误 |
| `RS-05` | 依赖方向严格单向：`commands → domain → storage`，`platform` 可被 `domain` 引用。禁止反向依赖与循环依赖 |
| `RS-06` | 每个 `domain/` 模块必须有对应的单元测试文件，覆盖率要求见 §8.4 |

---

## 3. 数据模型

本章定义全部持久化结构与 Markdown 解析规则。**这是前后端协作的存储契约**，变更需走 schema 迁移流程（§3.6）。

### 3.1 Markdown 解析规则（真相源层）

**本节（§3.1.1 ~ §3.1.5）的全部规则条款是解析行为的唯一权威依据**（对应原则 P-5）。下列条款采纳了 Markdown 生态中已被广泛使用的扩展书写惯例（wikilink、层级标签、YAML frontmatter、Block ID），以便存量笔记库无需转换即可被正确解析；条款之间若有歧义，以本节文字为准，不以任何外部软件的实际行为作为裁决依据。

解析器实现于 Rust `domain/md_parse.rs`，**前端不做结构化解析建索引**（仅做渲染）。

#### 3.1.1 Frontmatter

文件首部以 `---` 独占一行开始、以 `---` 独占一行结束的 YAML 块。

| 规则 ID | 规则 |
| --- | --- |
| `MD-FM-01` | 仅当 `---` 位于**文件第 1 行第 1 列**时才识别为 frontmatter 起始；正文中间的 `---` 视为水平分割线 |
| `MD-FM-02` | 缺失闭合 `---` 时，整个 frontmatter 视为无效，按普通正文处理，并在解析结果中标记 `frontmatter_error` |
| `MD-FM-03` | YAML 解析失败**不得**导致文件被跳过或报错中断。应降级为「无 frontmatter」并记录警告，正文照常索引 |
| `MD-FM-04` | 保留字段：`tags`（字符串或字符串数组，用于补充正文标签）、`aliases`（字符串数组，用于链接补全的别名匹配）、`created`、`modified`（ISO 8601，仅展示用，不作为索引依据） |
| `MD-FM-05` | 未知字段**必须**原样保留。软件写回 frontmatter 时不得删除、重排或格式化用户未涉及的字段 |
| `MD-FM-06` | frontmatter 内容**不参与**全文搜索索引 |

#### 3.1.2 Wikilink

语法：`[[目标]]`、`[[目标|别名]]`、`[[目标#标题]]`、`[[目标#^块ID]]`、`[[目标#标题|别名]]`

| 规则 ID | 规则 |
| --- | --- |
| `MD-WL-01` | 链接文本中 `#` 前部分为**目标笔记引用**，`#` 后为锚点（标题或 `^块ID`），`\|` 后为显示别名 |
| `MD-WL-02` | 目标引用支持：完整相对路径 `folder/note`、仅文件名 `note`（不含 `.md` 扩展名） |
| `MD-WL-03` | **解析阶段一律不判定链接是否有效**。是否存在由索引阶段的链接解析器统一裁决，解析结果记为 `resolved` 或 `dangling` |
| `MD-WL-04` | 目标匹配采用**大小写不敏感**的文件名匹配。同名歧义（不同文件夹下存在同名笔记）时：优先匹配同目录 → 其次匹配最短路径 → 仍歧义则标记 `ambiguous`，并按候选列表全部记录，UI 需给出歧义提示 |
| `MD-WL-05` | 位于代码块（``` fenced 或缩进 4 空格）与行内代码（`` ` ``）中的 wikilink **不解析** |
| `MD-WL-06` | 位于 frontmatter 中的 wikilink **不解析**为链接（但 `aliases` 值参与别名匹配） |
| `MD-WL-07` | 嵌入语法 `![[附件.png]]`、`![[笔记#标题]]` 解析为嵌入引用，`link_kind = 'embed'`，与普通链接分别统计 |
| `MD-WL-08` | 转义的 `\[[` **不解析**为链接 |
| `MD-WL-09` | 链接目标中的首尾空白在匹配前**必须**去除 |

#### 3.1.3 标签

语法：正文中的 `#标签名`，支持层级 `#父/子/孙`。

| 规则 ID | 规则 |
| --- | --- |
| `MD-TAG-01` | 标签字符集：中日韩文字、字母、数字、下划线、连字符、斜杠（层级分隔）。**不得**以数字开头（避免与 `#123` 类普通文本、标题序号冲突） |
| `MD-TAG-02` | 标签终止于：空白、行尾、或标点符号（`.,;:!?'"()[]{}` 等） |
| `MD-TAG-03` | 层级标签**同时**登记自身与全部祖先。`#a/b/c` 登记为 `a/b/c`、`a/b`、`a` 三条，各自建立关联，但 `is_leaf` 仅 `a/b/c` 为真 |
| `MD-TAG-04` | 位于代码块、行内代码、frontmatter 之外的 `#` 若紧跟标题语法（`# ` 后有空格）则为**标题**，不是标签 |
| `MD-TAG-05` | 标签大小写：存储时保留原文大小写，但**匹配与去重时大小写不敏感**。规范化键（`tag_norm`）为小写形式 |
| `MD-TAG-06` | frontmatter `tags:` 字段中的值同样登记为标签，但**不记录** `line`/`col`（`line = -1` 表示来源于 frontmatter） |

#### 3.1.4 标题与 Block ID

| 规则 ID | 规则 |
| --- | --- |
| `MD-H-01` | ATX 标题（`#`~`######` 后跟空格）与 Setext 标题（下划线 `===`/`---`）均需识别。代码块内的 `#` 不是标题 |
| `MD-H-02` | 标题文本去除首尾空白与行内 Markdown 标记（`**`、`*`、`` ` ``、`~~`）后入库，用于锚点匹配 |
| `MD-H-03` | 同一笔记内标题重复时，锚点匹配取**第一个**匹配项，并标记 `heading_ambiguous` |
| `MD-BID-01` | Block ID 语法：段落/列表项/表格行末尾的 `^标识符`（标识符为 `[a-zA-Z0-9-]+`） |
| `MD-BID-02` | Block ID 在其所属块内唯一；跨笔记可重复（锚点解析始终限定在单笔记内） |
| `MD-BID-03` | Block ID **必须**入库，供 `[[笔记#^块ID]]` 锚点跳转 |

#### 3.1.5 解析器输出契约

`domain/md_parse.rs` 对单个文件解析后，输出一个 `ParsedNote` 结构，包含：

| 字段 | 内容 |
| --- | --- |
| `frontmatter` | 解析后的键值对（`Option`，失败时为 `None` + 错误标记） |
| `links` | 全部 wikilink（含别名、锚点、嵌入标记、行号列号、是否代码块内） |
| `tags` | 全部标签（含层级展开、规范化键、行号列号） |
| `headings` | 全部标题（级别、规范化文本、行号） |
| `block_ids` | 全部块 ID（标识符、所属块行范围） |
| `plain_text` | 去除 Markdown 标记与代码块后的纯文本，用于全文索引 |
| `warnings` | 解析过程中的非致命警告（frontmatter 错误、歧义等），**不中断**索引 |

> **关键约束**：解析器**必须**是纯函数——输入文件字节，输出 `ParsedNote`，无副作用、无 IO、无全局状态。这是可测试性与增量索引正确性的基础。

### 3.2 索引库 Schema（index.db）

派生数据库，可完整重建（AC-06）。启用 WAL 模式与外键约束。

#### 3.2.1 DDL

```sql
-- 索引库元信息与签名
CREATE TABLE meta (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL
);
-- 固定写入：schema_version、index_signature、vault_root、built_at、parser_version

-- 文件表：Vault 内每个被索引文件一行
CREATE TABLE file (
    id           INTEGER PRIMARY KEY,
    rel_path     TEXT    NOT NULL UNIQUE,      -- 相对 Vault 根，统一 '/' 分隔
    name         TEXT    NOT NULL,             -- 文件名（含扩展名）
    stem         TEXT    NOT NULL,             -- 不含扩展名的文件名，用于 wikilink 匹配
    ext          TEXT    NOT NULL,             -- 小写扩展名，如 'md'、'png'
    kind         TEXT    NOT NULL,             -- 'note' | 'attachment' | 'other'
    size_bytes   INTEGER NOT NULL,
    mtime_ms     INTEGER NOT NULL,             -- 文件修改时间（毫秒），增量索引依据
    content_hash TEXT,                         -- 仅 note 计算，用于变更检测（可选优化）
    deleted      INTEGER NOT NULL DEFAULT 0,   -- 0=正常 1=在回收站
    indexed_at   INTEGER NOT NULL              -- 入库时间戳（毫秒）
);
CREATE INDEX idx_file_stem   ON file(stem);
CREATE INDEX idx_file_stem_lower ON file(lower(stem));   -- 链接裁决（技术方案 §5.3.3）与大小写不敏感匹配
CREATE INDEX idx_file_kind   ON file(kind);
CREATE INDEX idx_file_deleted ON file(deleted);

-- 标题表
CREATE TABLE heading (
    id           INTEGER PRIMARY KEY,
    file_id      INTEGER NOT NULL REFERENCES file(id) ON DELETE CASCADE,
    level        INTEGER NOT NULL,             -- 1~6
    text         TEXT    NOT NULL,             -- 规范化后的标题文本
    anchor       TEXT    NOT NULL,             -- 用于 [[笔记#anchor]] 匹配
    line         INTEGER NOT NULL,
    sort_order   INTEGER NOT NULL              -- 文档内顺序，供大纲面板（V1.1）
);
CREATE INDEX idx_heading_file ON heading(file_id);

-- Block ID 表
CREATE TABLE block_id (
    id           INTEGER PRIMARY KEY,
    file_id      INTEGER NOT NULL REFERENCES file(id) ON DELETE CASCADE,
    bid          TEXT    NOT NULL,             -- 块标识符（不含 '^'）
    line_start   INTEGER NOT NULL,
    line_end     INTEGER NOT NULL,
    UNIQUE(file_id, bid)
);
CREATE INDEX idx_block_file ON block_id(file_id);

-- 链接表：每条出链一行（含未解析的悬空链接）
CREATE TABLE link (
    id             INTEGER PRIMARY KEY,
    src_file_id    INTEGER NOT NULL REFERENCES file(id) ON DELETE CASCADE,
    dst_file_id    INTEGER,                    -- NULL 表示悬空链接
    target_ref     TEXT    NOT NULL,           -- 原始目标引用文本（'#' 前部分）
    anchor         TEXT,                       -- '#' 后部分，可空
    alias          TEXT,                       -- '|' 后别名，可空
    link_kind      TEXT    NOT NULL,           -- 'link' | 'embed'
    status         TEXT    NOT NULL,           -- 'resolved' | 'dangling' | 'ambiguous'
    line           INTEGER NOT NULL,
    col            INTEGER NOT NULL
);
CREATE INDEX idx_link_src    ON link(src_file_id);
CREATE INDEX idx_link_dst    ON link(dst_file_id);   -- 反向链接查询核心索引
CREATE INDEX idx_link_status ON link(status);

-- 标签表
CREATE TABLE tag (
    id           INTEGER PRIMARY KEY,
    norm         TEXT    NOT NULL UNIQUE,      -- 小写规范化全路径，如 'a/b/c'
    display      TEXT    NOT NULL,             -- 保留原文大小写的显示名
    depth        INTEGER NOT NULL,             -- 层级深度，'a/b/c' 为 3
    is_leaf      INTEGER NOT NULL DEFAULT 1,
    ref_count    INTEGER NOT NULL DEFAULT 0    -- 冗余计数，加速标签面板；由触发器或重建维护
);

-- 标签-文件关联
CREATE TABLE file_tag (
    id           INTEGER PRIMARY KEY,          -- 代理主键：同一行/同一 frontmatter 可重复出现同一标签
    file_id      INTEGER NOT NULL REFERENCES file(id) ON DELETE CASCADE,
    tag_id       INTEGER NOT NULL REFERENCES tag(id)  ON DELETE CASCADE,
    line         INTEGER NOT NULL,             -- -1 表示来自 frontmatter
    col          INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_filetag_file ON file_tag(file_id);
CREATE INDEX idx_filetag_tag ON file_tag(tag_id);

-- 笔记别名（来自 frontmatter aliases，用于链接补全与目标匹配）
-- 支撑 MD-FM-04、FR-LINK-02 的第 ③ 级匹配、FR-EDITOR-20
CREATE TABLE file_alias (
    file_id      INTEGER NOT NULL REFERENCES file(id) ON DELETE CASCADE,
    alias        TEXT    NOT NULL,             -- 保留原文大小写；匹配时统一 lower() 比较
    PRIMARY KEY (file_id, alias)
);
CREATE INDEX idx_alias_lower ON file_alias(lower(alias));   -- 别名匹配核心索引

-- 全文搜索（FTS5）
-- 注意：使用【普通（自带内容）FTS5 表】，而非 content='' 的 contentless 表。
-- contentless 表默认不支持 DELETE/UPDATE，而增量索引需要 `DELETE FROM note_fts WHERE rowid=?`
-- （见技术方案 §5.3.4）。代价是分词后文本重复存储一份；索引库可重建，该代价可接受。
CREATE VIRTUAL TABLE note_fts USING fts5(
    plain_text,
    tokenize='unicode61'                     -- 中文分词方案见 §3.4
);
-- note_fts 的 rowid 与 file.id 对齐
```

> **说明**：`ref_count` 是唯一的冗余字段，属性能优化。它可由 `file_tag` 完整重算，因此不违反 AC-06（可重建）。重建逻辑见 §3.6。

#### 3.2.2 关键查询模式与索引对应

| 查询场景 | SQL 模式 | 依赖索引 |
| --- | --- | --- |
| 反向链接（谁引用了我） | `SELECT ... FROM link WHERE dst_file_id = ?` | `idx_link_dst` |
| 出链（我引用了谁） | `SELECT ... FROM link WHERE src_file_id = ?` | `idx_link_src` |
| 链接补全（输入 `[[` 时） | `SELECT ... FROM file WHERE stem LIKE ? OR id IN (SELECT file_id FROM file_alias WHERE lower(alias) = ?)` | `idx_file_stem` + `idx_alias_lower` |
| 别名匹配（链接裁决第 ③ 级） | `SELECT file_id FROM file_alias WHERE lower(alias) = lower(?)` | `idx_alias_lower` |
| 标签面板（列出全部标签+计数） | `SELECT norm, display, ref_count FROM tag ORDER BY ref_count DESC` | `tag.norm` UNIQUE |
| 某标签下的笔记 | `SELECT file_id FROM file_tag WHERE tag_id = ?` | `idx_filetag_tag` |
| 全文搜索 | `SELECT ... FROM note_fts WHERE note_fts MATCH ? ` | FTS5 倒排索引 |
| 悬空链接面板 | `SELECT ... FROM link WHERE status = 'dangling'` | `idx_link_status` |
| 文件树构建 | `SELECT rel_path, name, kind FROM file WHERE deleted = 0` | `idx_file_deleted` |

### 3.3 全局库 Schema（global.db）

软件配置，跨 Vault 共享，**不属于**用户知识数据。

```sql
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Vault 注册表
CREATE TABLE vault (
    id           INTEGER PRIMARY KEY,
    abs_path     TEXT    NOT NULL UNIQUE,      -- 绝对路径
    display_name TEXT    NOT NULL,             -- 默认取文件夹名，用户可改
    last_opened  INTEGER,                      -- 毫秒时间戳
    pinned       INTEGER NOT NULL DEFAULT 0,
    trust_level  TEXT    NOT NULL DEFAULT 'trusted'  -- 'trusted' | 'restricted'
);

-- 界面偏好（键值对，JSON 值）
CREATE TABLE preference (
    key   TEXT PRIMARY KEY,                    -- 如 'theme.mode'、'editor.fontSize'
    value TEXT NOT NULL                        -- JSON 编码
);

-- 每个 Vault 的窗口/布局状态（分 Vault 存储）
CREATE TABLE vault_state (
    vault_id     INTEGER NOT NULL REFERENCES vault(id) ON DELETE CASCADE,
    key          TEXT    NOT NULL,             -- 如 'layout'、'lastOpenNotes'、'scrollPositions'
    value        TEXT    NOT NULL,
    PRIMARY KEY (vault_id, key)
);
```

| 需求 ID | 需求 | 优先级 |
| --- | --- | --- |
| `FR-GLOBAL-01` | 全局库损坏或丢失时，软件必须能以默认配置启动，并重新扫描/让用户重新选择 Vault，**不得**崩溃或丢失用户笔记数据（笔记数据在 Vault 内，与全局库无关） | P0 |
| `FR-GLOBAL-02` | 偏好写入采用防抖批量提交（见 §6.1），避免高频写放大 | P1 |

### 3.4 中文分词方案

FTS5 默认 `unicode61` 分词器按 Unicode 词边界切分，**对中文不分词**（整段连续汉字被视为一个 token），导致中文全文搜索几乎不可用。

**方案**：在 Rust 侧使用 `jieba-rs` 对笔记纯文本预分词，将**空格分隔的分词结果**写入 `note_fts.plain_text`，`tokenize` 仍用 `unicode61`。查询时对用户查询串同样先经 jieba 分词再构造 FTS5 `MATCH` 表达式。

| 规则 ID | 规则 |
| --- | --- |
| `FTS-01` | 索引时：`plain_text` = jieba 分词后以单空格连接的文本 |
| `FTS-02` | 查询时：用户输入经 jieba 分词 → 各词以 `AND`（默认）或 `OR`（可选）连接 → 构造 FTS5 MATCH |
| `FTS-03` | 英文、数字保持原样（jieba 对连续英文数字不切分）；混合中英文文本正常处理 |
| `FTS-04` | 搜索结果的高亮片段（snippet）需**还原**为原文——通过 FTS5 的 `snippet()` 配合原始文本偏移，或在 Rust 侧对命中的原文重新截取上下文窗口 |
| `FTS-05` | 分词器版本纳入索引签名（§3.5）。jieba 词典升级导致分词结果变化时，签名变更触发全量重建 |

> **新增 Rust 依赖**：`jieba-rs`，版本已锁定为 **0.11.0**（2026-09-19 经 crates.io 核实，见 §10.1 RISK-03 与附录 A.1），并关闭 HMM 新词发现以保证分词确定性（FTS-05、AC-REL-03）。本 PRD 不在此重复维护版本矩阵，权威出处为技术方案 §3.5.2。

### 3.5 索引签名与一致性

索引签名用于判断"现有索引库是否仍可信"。签名不匹配 → 触发全量重建。

**签名输入**（任一变化即使签名失效）：

| 输入 | 理由 |
| --- | --- |
| schema 版本号 | 表结构变更 |
| 解析器版本号 | wikilink/tag/heading 解析规则变更 |
| 分词器版本 + 词典哈希 | 分词结果变更影响 FTS |
| Vault 根绝对路径 | 索引库与特定 Vault 绑定，防止用户复制 `.knowlpad/` 到别处误用 |

签名 = 上述输入规范化拼接后的 SHA-256。

| 需求 ID | 需求 | 优先级 |
| --- | --- | --- |
| `FR-SIG-01` | 打开 Vault 时计算当前签名并与 `meta.index_signature` 比对，不一致则全量重建 | P0 |
| `FR-SIG-02` | 全量重建期间，UI 必须显示进度且**可继续使用旧索引只读浏览**，重建完成后原子切换 | P1 |
| `FR-SIG-03` | 重建必须可中断（用户关闭软件），下次打开时检测到不完整重建标记则重新开始 | P0 |
| `FR-SIG-04` | 删除整个 `.knowlpad/` 目录后重新打开 Vault，重建结果与删除前**逐表一致**（对应成功判据 SJ-03）。⚠️ 回收站实体与备份**不是**可丢弃的派生数据：删除 `.knowlpad/` 会一并删除它们，UI 必须在执行前明确警示（含待删除条目数） | P0 |

### 3.6 Schema 迁移策略

| 规则 ID | 规则 |
| --- | --- |
| `MIG-01` | 索引库（index.db）采用**丢弃重建**策略，不做在线 ALTER 迁移。schema 版本号变更即全量重建。理由：索引库是派生数据，重建成本低且无数据丢失风险，避免维护复杂的迁移脚本 |
| `MIG-02` | 全局库（global.db）采用**版本化迁移**策略，因其含用户配置不可丢弃。使用 `meta.schema_version` + 顺序迁移脚本（V1→V2→V3...），每个迁移在事务内执行 |
| `MIG-03` | 全局库迁移**必须**向前兼容：新版本能读旧库；旧版本遇到更高 schema_version 时**拒绝写入并提示升级软件**，不得静默损坏 |
| `MIG-04` | 任何 schema 变更必须同步更新：DDL、迁移脚本、索引签名版本号、本 PRD §3 文档 |
| `MIG-05` | 迁移脚本必须有对应的自动化测试（构造旧版本库 → 执行迁移 → 断言新结构与数据正确） |

---

## 4. 功能需求

每个模块包含：功能描述 → 需求条目（含优先级）→ 验收标准。验收标准采用 Given-When-Then 形式，**可直接转化为自动化测试用例**。

### 4.1 Vault 知识库管理（`VAULT`）

**功能描述**：Vault 是用户指定的本地文件夹，作为笔记的根容器。软件围绕 Vault 组织全部数据。

#### 4.1.1 需求条目

| 需求 ID | 需求描述 | 优先级 |
| --- | --- | --- |
| `FR-VAULT-01` | 首次启动时展示欢迎页，提供「打开已有文件夹」与「新建 Vault」两个入口 | P0 |
| `FR-VAULT-02` | 「新建 Vault」= 让用户选择一个空目录或新建目录，软件在其中创建 `.knowlpad/` 并初始化为空库 | P0 |
| `FR-VAULT-03` | 「打开已有文件夹」= 用户选择任意非空目录，软件识别其中的 `.md` 文件并建立索引。**已存在的第三方 Markdown 笔记库可直接打开使用**（无需迁移向导） | P0 |
| `FR-VAULT-04` | 支持注册多个 Vault，在欢迎页与命令面板中快速切换。同一时刻仅一个 Vault 处于打开状态 | P0 |
| `FR-VAULT-05` | 切换 Vault 时必须先完成当前 Vault 的未保存写入与索引刷盘，再关闭数据库连接 | P0 |
| `FR-VAULT-06` | 记住上次打开的 Vault，启动时自动恢复（可在设置中关闭此行为） | P1 |
| `FR-VAULT-07` | Vault 列表显示名称、绝对路径、最近打开时间；支持置顶、重命名显示名、从列表移除（**移除仅删除注册记录，绝不删除磁盘文件**） | P0 |
| `FR-VAULT-08` | 若 Vault 路径已不存在（如移动硬盘被拔出），启动时给出明确提示并引导用户重新选择，**不得**崩溃或静默创建空目录 | P0 |
| `FR-VAULT-09` | 首次打开大 Vault（> 5000 文件）时显示索引进度（已处理数 / 总数），进度可取消 | P1 |
| `FR-VAULT-10` | Vault 打开后，全部文件访问被限定在该 Vault 根目录内（见 SEC-02 路径穿越防护） | P0 |
| `FR-VAULT-11` | 状态栏常驻显示当前 Vault 名称与索引状态（就绪 / 索引中 / 索引失败） | P1 |
| `FR-VAULT-12` | 若 Vault 根目录被检测为 Git 仓库，提示用户 `.knowlpad/` 建议加入忽略列表，但**不自动修改** `.gitignore` | P2 |

#### 4.1.2 验收标准

**AC-VAULT-01｜打开已存在的第三方笔记库**
- Given 一个含 300 篇笔记、使用 wikilink 与嵌套标签、并存在第三方软件配置目录（如 `.obsidian/`）的 Vault
- When 用户通过「打开已有文件夹」选中该目录
- Then 索引完成且不报错；该配置目录内文件数量与内容与打开前**完全一致**（逐字节比对，对应 FR-STORAGE-02）；文件树显示全部 300 篇笔记；随机抽查 20 篇，其链接与标签解析结果**符合 §3.1 对应规则条款的期望值**（期望值按附录 B 的推导方式预先固化为断言，不以外部软件运行时行为为基准）

**AC-VAULT-02｜Vault 路径失效**
- Given 已注册的 Vault 位于可移动磁盘，该磁盘未连接
- When 启动软件并尝试恢复该 Vault
- Then 显示「无法访问知识库 <名称>，路径 <路径> 不存在或不可访问」，提供「重新定位」与「从列表移除」两个操作；软件不崩溃、不创建任何目录

**AC-VAULT-03｜切换 Vault 的数据完整性**
- Given Vault A 中有一篇笔记处于已编辑未保存状态，且索引队列中有 5 个待处理文件
- When 用户切换到 Vault B
- Then Vault A 的编辑内容被写入磁盘；索引队列被刷盘或安全丢弃（下次打开可重建）；数据库连接正常关闭；切换耗时 < 2s；随后切回 Vault A 时内容与索引均正确

**AC-VAULT-04｜从列表移除不删文件**
- Given Vault A 已注册且磁盘上存在文件
- When 用户在 Vault 列表中执行「移除」
- Then 注册记录被删除；**磁盘上 Vault A 目录及其全部内容保持不变**（文件数与总字节数与移除前一致）

**AC-VAULT-05｜大 Vault 索引进度**
- Given 一个含 20000 个 `.md` 文件的 Vault
- When 首次打开
- Then 显示进度指示（含已处理文件数）；索引期间 UI 可响应（可取消、可关闭）；索引完成后文件树、搜索、图谱均可用；全过程主线程无冻结（输入响应 < 50ms）

---

### 4.2 文件树与文件操作（`FILE`）

**功能描述**：以树形结构展示 Vault 内的文件夹与文件，并提供完整的文件生命周期操作。

#### 4.2.1 需求条目

**浏览**

| 需求 ID | 需求描述 | 优先级 |
| --- | --- | --- |
| `FR-FILE-01` | 树形展示文件夹与文件，支持展开/折叠，记住折叠状态（存于 `vault_state`） | P0 |
| `FR-FILE-02` | 按名称排序（文件夹优先）；提供按修改时间、按类型的排序选项 | P0 |
| `FR-FILE-03` | 文件图标按类型区分（笔记 / 图片 / PDF / 音视频 / 其他） | P1 |
| `FR-FILE-04` | 支持在树内过滤（输入即筛选可见节点） | P1 |
| `FR-FILE-05` | `.knowlpad/`、以 `.` 开头的隐藏文件默认不显示（可在设置中开启显示） | P0 |
| `FR-FILE-06` | 超大 Vault（> 10 万节点）下采用虚拟滚动，滚动流畅不卡顿 | P0 |
| `FR-FILE-07` | 右键上下文菜单：新建笔记、新建文件夹、重命名、删除、在系统文件管理器中显示、复制相对路径、复制 Wikilink | P0 |

**创建与写入**

| 需求 ID | 需求描述 | 优先级 |
| --- | --- | --- |
| `FR-FILE-10` | 新建笔记：在指定目录创建 `.md` 文件，自动进入编辑态并聚焦标题行 | P0 |
| `FR-FILE-11` | 新建文件夹：支持多级路径一次创建（如 `a/b/c`） | P0 |
| `FR-FILE-12` | 文件名合法性校验：拒绝 `\/:*?"<>\|`、控制字符、首尾空格与点号、Windows 保留名（`CON`、`PRN`、`AUX`、`NUL`、`COM1-9`、`LPT1-9`） | P0 |
| `FR-FILE-13` | 同目录重名时**不得**静默覆盖：提示冲突并提供「覆盖」「重命名新建」「取消」三选项 | P0 |
| `FR-FILE-14` | 点击悬空链接 `[[不存在的笔记]]` 时，按链接文本创建新笔记（遵循默认新建位置设置） | P0 |
| `FR-FILE-15` | 全部写入操作必须**原子化**：先写临时文件 → `fsync` → 原子重命名替换（详见 §6.2） | P0 |
| `FR-FILE-16` | 默认新建位置可配置：Vault 根目录 / 当前笔记所在目录 / 指定固定目录 | P1 |

**重命名与移动**

| 需求 ID | 需求描述 | 优先级 |
| --- | --- | --- |
| `FR-FILE-20` | 重命名/移动笔记时，**自动更新 Vault 内所有指向它的 wikilink**（对应场景 SC-06） | P0 |
| `FR-FILE-21` | 链接改写前必须：① 在 `.knowlpad/backup/` 备份全部将被修改的文件 ② 向用户展示"将修改 N 个文件中的 M 处链接"预览 ③ 用户确认后才执行（可在设置中改为自动执行） | P0 |
| `FR-FILE-22` | 链接改写必须**全有或全无**：任一文件写入失败则回滚全部已修改文件至备份内容，并向用户报告。若操作包含重命名/移动，**重命名与改写同属一次原子操作**：改写失败必须撤销重命名，重命名失败则不得改写任何链接；**严禁**出现「链接已指向新名、文件仍旧名」的中间态 | P0 |
| `FR-FILE-23` | 链接改写只修改 `[[...]]` 内部的目标引用部分，**不得**改动别名、锚点、周边正文、行尾空白或换行风格 | P0 |
| `FR-FILE-24` | 位于代码块与行内代码中的 wikilink **不得**被改写 | P0 |
| `FR-FILE-25` | 移动文件夹时递归处理其下全部笔记的链接更新 | P0 |
| `FR-FILE-26` | 拖拽文件树节点实现移动；拖拽到文件夹内为其子项，拖到空白处为移动到 Vault 根 | P1 |
| `FR-FILE-27` | 重命名/移动的进度必须可见，大批量操作（> 100 处改写）可取消，取消后回滚至操作前状态 | P1 |
| `FR-FILE-28` | 若目标笔记正在编辑器中打开，重命名后编辑器**必须**跟随指向新路径，不丢失未保存内容、不出现"文件已不存在"错误 | P0 |

**删除**

| 需求 ID | 需求描述 | 优先级 |
| --- | --- | --- |
| `FR-FILE-30` | 删除文件默认进入回收站（软删除，见 §4.10），**不做**永久删除 | P0 |
| `FR-FILE-31` | 删除文件夹时递归处理其下全部内容 | P0 |
| `FR-FILE-32` | 删除被引用笔记时，提示"该笔记被 N 处引用，删除后这些链接将变为悬空链接"，需用户确认 | P0 |
| `FR-FILE-33` | 删除操作**不得**修改引用方文件内容（悬空链接是合法状态，见术语表） | P0 |

#### 4.2.2 验收标准

**AC-FILE-01｜重命名批量更新链接（核心场景）**
- Given Vault 中笔记 `A.md` 被 300 篇其他笔记通过 `[[A]]`、`[[A|别名]]`、`[[folder/A#标题]]` 三种形式引用，另有 2 处在代码块内引用
- When 用户将 `A.md` 重命名为 `B.md` 并确认链接改写
- Then 300 篇中的 298 处非代码块引用被更新为对应 `B` 形式（别名与锚点原样保留）；2 处代码块内引用**保持 `A` 不变**；反向链接面板中 `B.md` 的反链数量与重命名前 `A.md` 一致；无死链产生

**AC-FILE-02｜链接改写的原子性与回滚**
- Given 一次重命名需改写 50 个文件，其中第 30 个文件被外部程序锁定导致写入失败
- When 执行链接改写
- Then 前 29 个已修改文件全部从备份恢复至原始内容（逐字节一致）；目标文件的重命名被撤销；用户收到明确的失败报告（含失败文件路径与原因）；Vault 处于操作前的完整一致状态

**AC-FILE-03｜非法文件名拒绝**
- Given 用户在 Windows 平台新建笔记
- When 分别输入 `CON`、`a/b`、`名称.`、`na*me`、`  空格开头`
- Then 全部被拒绝并给出具体的非法原因提示；磁盘上不产生任何文件

**AC-FILE-04｜重名不静默覆盖**
- Given 目录中已存在 `note.md`（含 100 字内容）
- When 用户在同一目录新建 `note.md`
- Then 弹出冲突提示（覆盖 / 重命名新建 / 取消）；选择「取消」后原文件内容**完全不变**；选择「覆盖」前必须二次确认且原文件已备份

**AC-FILE-05｜编辑器跟随重命名**
- Given 笔记 `A.md` 在编辑器中打开且有未保存的编辑内容
- When 用户从文件树将 `A.md` 重命名为 `B.md`
- Then 编辑器标签页标题更新为 `B.md`；未保存的编辑内容**完整保留**；后续保存写入 `B.md`；不出现任何"文件不存在"错误

**AC-FILE-06｜大目录虚拟滚动性能**
- Given 一个含 100000 个文件节点的 Vault
- When 用户在文件树中连续快速滚动
- Then 帧率不低于 50 FPS；内存占用不随滚动持续增长（无节点泄漏）；滚动停止后 200ms 内可视区域渲染完成


> **度量口径（2026-10-05 登记，兑现 M2 计划 §5② 的承诺）**：自动化以**合成的 10 万节点**在组件层验证
> 「渲染行数有界、滚动窗口跟随、反复滚动不增长」；**真实帧率、内存与 200ms 渲染**需要真实 WebView 与
> 真实规模夹具，属 **M3/M9**（届时以真机度量补足本 AC）。索引引擎（M3）落地后，文件树数据源由直接读盘切到索引，
> 本 AC 的度量点随之固定。
**AC-FILE-07｜外部变更感知**
- Given Vault 已打开且索引就绪
- When 用户通过系统文件管理器在 Vault 内新增 `new.md`、修改 `exist.md` 内容、删除 `old.md`
- Then 3 秒内文件树反映全部三项变更；`new.md` 可被搜索到；`exist.md` 的新内容进入搜索索引；`old.md` 的链接变为悬空状态

**AC-FILE-08｜树形展示与折叠状态**
- Given 一个含 3 级嵌套目录的 Vault，用户展开了第 2 级目录后关闭软件
- When 重新打开同一 Vault
- Then 文件树显示全部 3 级目录结构；第 2 级目录保持展开状态（从 `vault_state` 恢复）；第 3 级目录状态与关闭前一致

**AC-FILE-09｜隐藏文件控制**
- Given Vault 中存在 `.obsidian/` 目录、`.git/` 目录和 `.knowlpad/` 目录
- When 默认模式下查看文件树
- Then 三个目录均不显示；在设置中开启「显示隐藏文件」后，`.obsidian/` 和 `.git/` 显示但 `.knowlpad/` 仍不显示（内部数据目录始终隐藏）

**AC-FILE-10｜多级目录创建**
- Given 用户在文件树根目录右键选择「新建文件夹」
- When 输入 `projects/2026/notes` 并确认
- Then 3 级嵌套目录 `projects/2026/notes` 一次性创建成功；文件树立即反映新目录结构；中间不产生任何错误提示

**AC-FILE-11｜删除操作与引用提示**
- Given 笔记 `A.md` 被 5 篇其他笔记通过 `[[A]]` 引用
- When 用户从文件树删除 `A.md`
- Then 弹出确认提示「该笔记被 5 处引用，删除后这些链接将变为悬空链接」；确认后 `A.md` 进入回收站（软删除）；5 篇引用文件的链接变为悬空状态；引用方文件内容**不被修改**

---

### 4.3 Markdown 编辑器（`EDITOR`）

**功能描述**：笔记的编辑与阅读界面，是用户使用频率最高的模块。

**分阶段实现策略**（对应技术方案 §5.1）

| 阶段 | 编辑器内核 | 说明 |
| --- | --- | --- |
| MVP 阶段 | `md-editor-v3` 6.5.6 | 快速可用，工具栏 + 分屏预览 |
| 正式版阶段 | CodeMirror 6 | 自研所见即所得（Live Preview）编辑体验：光标所在行显示 Markdown 源码，其余行显示渲染效果 |

两阶段的**外部行为契约一致**（保存时机、快捷键、链接交互），切换内核不影响其他模块。

#### 4.3.1 需求条目

**编辑能力**

| 需求 ID | 需求描述 | 优先级 |
| --- | --- | --- |
| `FR-EDITOR-01` | 完整支持 CommonMark 语法：标题、粗斜体、删除线、有序/无序列表、任务列表、引用、代码块（含语言标注）、表格、水平线、链接、图片 | P0 |
| `FR-EDITOR-02` | 支持本项目扩展语法：wikilink、嵌套标签、Block ID、frontmatter | P0 |
| `FR-EDITOR-03` | Markdown 语法高亮：标题、粗斜体、代码、链接、标签在编辑态即有视觉区分 | P0 |
| `FR-EDITOR-04` | 编辑/预览模式：纯编辑、分屏（左编辑右预览）、纯阅读三种模式切换 | P0 |
| `FR-EDITOR-05` | 正式版阶段实现 Live Preview（所见即所得）：光标所在行显示 Markdown 源码，其他行显示渲染效果 | P1 |
| `FR-EDITOR-06` | 快捷键：`Ctrl/Cmd+B` 粗体、`I` 斜体、`K` 插入链接、`Tab`/`Shift+Tab` 列表缩进、`Ctrl/Cmd+Enter` 切换任务列表状态 | P0 |
| `FR-EDITOR-07` | 列表自动续行：在列表项末尾回车自动生成下一项序号/符号；空列表项回车则退出列表 | P1 |
| `FR-EDITOR-08` | 撤销/重做栈深度 ≥ 500 步，跨保存边界保持 | P0 |
| `FR-EDITOR-09` | 查找与替换：支持区分大小写、全字匹配、正则；替换前可逐条确认或全部替换 | P0 |
| `FR-EDITOR-10` | 大文件（≥ 1MB / ≥ 5 万行）打开与编辑不卡顿，输入延迟 < 50ms | P0 |
| `FR-EDITOR-11` | 显示行号（可关闭）、当前行列位置、字数与字符数统计 | P1 |

**链接交互**

| 需求 ID | 需求描述 | 优先级 |
| --- | --- | --- |
| `FR-EDITOR-20` | 键入 `[[` 时弹出笔记补全列表，实时模糊匹配笔记名与 `aliases` | P0 |
| `FR-EDITOR-21` | 补全列表显示笔记名与相对路径；不存在的目标以「创建并链接」样式提示（灰色/斜体） | P0 |
| `FR-EDITOR-22` | 补全支持键盘上下选择、`Enter` 确认、`Esc` 关闭；鼠标点击亦可选择 | P0 |
| `FR-EDITOR-23` | 键入 `[[笔记#` 时二次补全该笔记的标题列表 | P1 |
| `FR-EDITOR-24` | 键入 `#` 时弹出标签补全（含层级），数据来自标签索引 | P0 |
| `FR-EDITOR-25` | `Ctrl/Cmd+点击` wikilink 跳转到目标笔记；`Ctrl/Cmd+Alt+点击` 在新标签页打开 | P0 |
| `FR-EDITOR-26` | 悬停 wikilink 显示目标笔记的预览浮窗（前 N 行渲染结果），悬停延迟可配置 | P1 |
| `FR-EDITOR-27` | 悬空链接在视觉上与正常链接区分（如虚线下划线/降低饱和度），点击触发创建 | P0 |
| `FR-EDITOR-28` | 歧义链接（`ambiguous`）需有视觉警示，悬停显示候选列表 | P1 |
| `FR-EDITOR-29` | 嵌入 `![[图片.png]]` 在阅读态直接渲染图片；`![[笔记#标题]]` 渲染该章节内容 | P1 |

**保存与状态**

| 需求 ID | 需求描述 | 优先级 |
| --- | --- | --- |
| `FR-EDITOR-30` | 自动保存：编辑停止后防抖 1 秒写入磁盘（防抖时长可配置 0.5–5s） | P0 |
| `FR-EDITOR-31` | `Ctrl/Cmd+S` 立即保存并清空防抖队列 | P0 |
| `FR-EDITOR-32` | 标题栏/标签页显示未保存标记（如圆点）；关闭有未保存内容的标签页前必须提示 | P0 |
| `FR-EDITOR-33` | 软件异常退出（崩溃、强杀进程）后重启，应能恢复未保存内容（通过编辑缓冲区快照，存于 `.knowlpad/`，见 §6.2） | P1 |
| `FR-EDITOR-34` | **外部修改冲突处理**：若磁盘文件在编辑器打开期间被外部程序修改，必须提示用户「文件已被外部修改」，提供「加载外部版本」「保留我的版本并覆盖」「查看差异」三选项。**禁止**静默覆盖任一方的内容 | P0 |
| `FR-EDITOR-35` | 多标签页：支持同时打开多篇笔记，标签可拖拽排序、中键关闭、右键菜单（关闭其他/关闭右侧/复制路径） | P0 |
| `FR-EDITOR-36` | 分栏：支持左右/上下分栏并排查看两篇笔记 | P2 |
| `FR-EDITOR-37` | 记住每个 Vault 的上次打开标签页与各笔记滚动位置（存于 `vault_state`） | P1 |

**渲染与安全**

| 需求 ID | 需求描述 | 优先级 |
| --- | --- | --- |
| `FR-EDITOR-40` | Markdown → HTML 由 `markdown-it` 15.0.2 完成，**`html: true` 配置下必须**经 `DOMPurify` 3.4.15 净化后再插入 DOM | P0 |
| `FR-EDITOR-41` | 净化必须移除 `<script>`、事件属性（`onerror`、`onclick` 等）、`javascript:` 协议 URL、`<iframe>`、`<object>`、`<embed>` | P0 |
| `FR-EDITOR-42` | **禁止**使用 `DOMPurify.setConfig()` 持久化配置 API（该 API 在 3.4.11 之前存在绕过 clone-guard 的 XSS 漏洞）。配置必须在每次 `sanitize()` 调用时以参数传入 | P0 |
| `FR-EDITOR-43` | 渲染外部图片时使用 `referrerPolicy="no-referrer"`，**禁止**自动加载远程资源（默认仅渲染 Vault 内本地附件；远程 URL 图片需用户显式确认，见 SEC-08） | P0 |
| `FR-EDITOR-44` | 代码块语法高亮不得引入执行代码的路径（高亮器必须是纯文本 → 带 class 的 span 转换） | P0 |

#### 4.3.2 验收标准

**AC-EDITOR-01｜XSS 净化（安全关键）**
- Given 一篇笔记正文包含：`<script>alert(1)</script>`、`<img src=x onerror=alert(2)>`、`[点击](javascript:alert(3))`、`<iframe src="http://evil.com">`
- When 该笔记以阅读态渲染
- Then 无任何脚本执行（无弹窗、无控制台脚本执行记录）；净化后的 DOM 中不含 `script`、`iframe` 标签，不含任何 `on*` 事件属性，不含 `javascript:` 协议；文本内容以安全形式可见（如链接降级为纯文本或安全 URL）

**AC-EDITOR-02｜自动保存与防抖**
- Given 一篇笔记处于编辑态，自动保存防抖设置为 1 秒
- When 用户连续输入 5 秒（每 100ms 一次按键）后停止输入
- Then 输入期间磁盘文件**未被写入**（避免高频写放大）；停止输入 1 秒后磁盘文件内容与编辑器内容一致；全过程输入延迟 < 50ms，无可感知卡顿

**AC-EDITOR-03｜外部修改冲突**
- Given 笔记 `A.md` 在编辑器中打开，用户已输入未保存内容
- When 外部程序（如 VS Code）修改并保存了 `A.md`
- Then 编辑器弹出冲突提示，明确列出三种处理方式；选择「加载外部版本」时用户未保存内容被显式警告将丢失并需二次确认；选择「保留我的版本」时写入前原磁盘内容已被备份；**任何路径下都不发生静默覆盖**

**AC-EDITOR-04｜链接补全**
- Given Vault 中存在笔记 `Rust 学习笔记.md`（frontmatter `aliases: [rust, Rustlang]`）与 `Rust 进阶.md`
- When 用户在编辑器键入 `[[ru`
- Then 补全列表按相关度显示两个候选，`Rust 学习笔记` 排在前列（因别名 `rust` 精确前缀匹配）；继续键入 `[[rust进` 时候选收敛；按 `Enter` 插入 `[[Rust 进阶]]`；补全响应延迟 < 100ms

**AC-EDITOR-05｜大文件性能**
- Given 一篇 2MB、约 60000 行的 Markdown 文件
- When 打开该文件并在文件中部连续输入
- Then 打开耗时 < 2s；输入延迟 < 50ms；滚动帧率 ≥ 50 FPS；内存占用 < 500MB

**AC-EDITOR-06｜撤销栈跨保存**
- Given 用户进行了 30 次编辑操作，期间触发了 3 次自动保存
- When 用户连续执行 30 次撤销
- Then 内容逐步回退至初始状态，**不因保存动作清空撤销栈**；重做同样可用

**AC-EDITOR-07｜崩溃恢复**
- Given 用户在笔记中输入了 500 字未触发保存（防抖窗口内）
- When 软件进程被强制终止（模拟崩溃），随后重新启动并打开同一 Vault
- Then 提示存在未保存的编辑缓冲区，用户可选择恢复；恢复后内容与崩溃前一致

---

### 4.4 双向链接与反向链接（`LINK`）

**功能描述**：链接是知识网络的核心。本模块负责链接的解析、有效性裁决、反向链接展示与链接维护。

#### 4.4.1 需求条目

**链接解析与裁决**

| 需求 ID | 需求描述 | 优先级 |
| --- | --- | --- |
| `FR-LINK-01` | 索引阶段统一裁决全部链接的有效性，状态分为 `resolved`（目标存在）、`dangling`（目标不存在）、`ambiguous`（多个候选） | P0 |
| `FR-LINK-02` | 目标匹配顺序：① 完整相对路径精确匹配 ② 文件名（stem）大小写不敏感匹配 ③ frontmatter `aliases` 匹配 | P0 |
| `FR-LINK-03` | 锚点校验：`[[笔记#标题]]` 需校验目标笔记是否存在该标题锚点；不存在则标记 `anchor_missing`（链接本身仍 `resolved`，仅锚点失效） | P1 |
| `FR-LINK-04` | Block ID 锚点 `[[笔记#^bid]]` 需校验目标笔记内该 Block ID 存在 | P1 |
| `FR-LINK-05` | 嵌入链接 `![[附件]]` 需校验附件文件存在；缺失时 UI 显示占位提示而非破图 | P1 |
| `FR-LINK-06` | 新建/重命名/删除文件后，受影响的链接状态必须增量重算（不得全量重建，见 §4.4.3 影响范围算法） | P0 |
| `FR-LINK-07` | 自链接（笔记链接到自己）需被识别并单独统计，不计入"孤立笔记"判定 | P2 |

**反向链接面板**

| 需求 ID | 需求描述 | 优先级 |
| --- | --- | --- |
| `FR-LINK-10` | 每篇笔记提供反向链接面板，列出全部指向它的链接（对应场景 SC-03） | P0 |
| `FR-LINK-11` | 每条反链显示：来源笔记名、来源相对路径、**链接所在的上下文片段**（前后各 N 行，N 可配置，默认 2）、行号 | P0 |
| `FR-LINK-12` | 上下文片段中的链接位置需高亮 | P1 |
| `FR-LINK-13` | 点击反链条目跳转到来源笔记并**滚动定位到该行**，同时短暂高亮该行 | P0 |
| `FR-LINK-14` | 反链按来源分组折叠；显示总计数 | P0 |
| `FR-LINK-15` | 区分展示：普通链接反链 / 嵌入引用反链（`embed`）分开计数与分组 | P1 |
| `FR-LINK-16` | 「未链接提及」（Unlinked Mentions）：扫描 Vault 中出现当前笔记文件名/别名纯文本但未构成 wikilink 的位置，提示用户可转为链接 | P2 |
| `FR-LINK-17` | 反链面板数据必须来自索引库实时查询，笔记变更后 1 秒内刷新 | P0 |
| `FR-LINK-18` | 反链数量大（> 500）时采用虚拟滚动或分页，不阻塞渲染 | P1 |

**链接维护工具**

| 需求 ID | 需求描述 | 优先级 |
| --- | --- | --- |
| `FR-LINK-20` | 悬空链接面板：集中列出 Vault 内全部悬空链接，按目标名分组，显示引用次数；支持点击直接创建目标笔记 | P1 |
| `FR-LINK-21` | 孤立笔记检测：列出没有任何出链且没有任何入链的笔记 | P1 |
| `FR-LINK-22` | 歧义链接面板：列出全部 `ambiguous` 链接及其候选，支持用户逐一指定正确目标（改写为完整相对路径） | P1 |
| `FR-LINK-23` | 链接格式转换：支持在「最短路径形式 `[[note]]`」与「完整相对路径形式 `[[folder/note]]`」之间批量转换，转换前预览与备份（复用 FR-FILE-21 机制） | P2 |

#### 4.4.2 验收标准

**AC-LINK-01｜反链准确性**
- Given 笔记 `B.md` 被 50 篇笔记以不同形式引用（含别名、锚点、嵌入）
- When 打开 `B.md` 的反链面板
- Then 面板显示 50 条反链，与 `SELECT COUNT(*) FROM link WHERE dst_file_id = <B的id> AND status='resolved'` 结果一致；每条均含正确的上下文片段与行号；点击任意一条能准确跳转到对应行

**AC-LINK-02｜增量重算的正确性**
- Given Vault 含 10000 篇笔记，`[[X]]` 是悬空链接，被 20 篇笔记引用
- When 用户新建笔记 `X.md`
- Then 2 秒内这 20 处链接状态从 `dangling` 变为 `resolved`；**期间未触发全量重建**（通过日志断言仅重算了受影响的链接）；反链面板与悬空链接面板同步更新

**AC-LINK-03｜重命名后的反链保持**
- Given `A.md` 有 300 条反链
- When 重命名为 `B.md` 并完成链接改写
- Then `B.md` 的反链数量为 300（与重命名前一致）；每条反链的来源与行号仍准确；不产生悬空链接

**AC-LINK-04｜歧义链接处理**
- Given Vault 中 `folder1/note.md` 与 `folder2/note.md` 同名，另有 10 篇笔记写 `[[note]]`
- When 索引完成
- Then 这 10 条链接标记为 `ambiguous`；歧义链接面板列出全部 10 条及 2 个候选；用户为其中一条指定 `folder2/note` 后，该文件中的链接被改写为 `[[folder2/note]]`（改写前已备份），且状态变为 `resolved`

**AC-LINK-05｜锚点失效检测**
- Given 笔记 `A.md` 含 `[[B#某个标题]]`，随后用户修改 `B.md` 删除了该标题
- When 索引增量更新完成
- Then 该链接状态为 `resolved` 但带 `anchor_missing` 标记；编辑器中该链接有视觉警示；点击跳转至 `B.md` 顶部而非报错

---

### 4.5 全文搜索（`SEARCH`）

**功能描述**：在 Vault 全部笔记中快速检索内容，是本地优先知识库的核心竞争力（对应场景 SC-05）。

#### 4.5.1 需求条目

**基础搜索**

| 需求 ID | 需求描述 | 优先级 |
| --- | --- | --- |
| `FR-SEARCH-01` | 全局搜索入口：侧边栏搜索面板 + `Ctrl/Cmd+Shift+F` 快捷键 | P0 |
| `FR-SEARCH-02` | 搜索范围：全部笔记正文（frontmatter 不参与，见 MD-FM-06） | P0 |
| `FR-SEARCH-03` | 中文搜索可用：基于 §3.4 的 jieba 分词方案，中文短语能正确命中 | P0 |
| `FR-SEARCH-04` | 搜索结果每条显示：笔记名、相对路径、**命中片段**（含前后上下文，命中词高亮）、修改时间 | P0 |
| `FR-SEARCH-05` | 同一笔记多处命中时合并为一条结果，显示命中次数与首个片段（可展开查看全部片段） | P1 |
| `FR-SEARCH-06` | 结果排序：默认按相关度（FTS5 `bm25`）；可切换为按修改时间、按文件名、按路径 | P0 |
| `FR-SEARCH-07` | 点击结果跳转到笔记并**滚动定位到命中位置**，高亮该处 | P0 |
| `FR-SEARCH-08` | 搜索为增量式：用户输入停顿 200ms 后触发（防抖），支持输入过程中取消上一次未完成的查询 | P0 |
| `FR-SEARCH-09` | 10000 篇笔记规模下，常规搜索响应 < 200ms（见 NFR-PERF-04） | P0 |
| `FR-SEARCH-10` | 搜索历史：保留最近 20 条查询（存于全局库），可一键复用、可清空 | P1 |
| `FR-SEARCH-11` | 空结果时给出建议（检查拼写、放宽条件、确认索引状态） | P1 |

**高级搜索语法**

| 需求 ID | 需求描述 | 优先级 |
| --- | --- | --- |
| `FR-SEARCH-20` | `path:folder/` 限定搜索路径前缀 | P1 |
| `FR-SEARCH-21` | `file:名称` 按文件名匹配（不参与全文） | P1 |
| `FR-SEARCH-22` | `tag:#标签` 限定含某标签的笔记（支持层级前缀匹配） | P1 |
| `FR-SEARCH-23` | `line:(...)` 限定在同一行内共现 | P2 |
| `FR-SEARCH-24` | `"精确短语"` 双引号包裹时按短语匹配（不拆词） | P1 |
| `FR-SEARCH-25` | `-词` 排除含该词的结果 | P1 |
| `FR-SEARCH-26` | 高级语法可与全文关键词混用，如 `tag:#工作 "季度报告" -草稿` | P1 |
| `FR-SEARCH-27` | 语法错误（如未闭合引号）时给出明确提示，**不得**静默返回空结果 | P1 |

**文件内搜索**

| 需求 ID | 需求描述 | 优先级 |
| --- | --- | --- |
| `FR-SEARCH-30` | 编辑器内 `Ctrl/Cmd+F` 在当前笔记内查找，显示"第 X / 共 Y 处"，支持上下跳转 | P0 |
| `FR-SEARCH-31` | 编辑器内 `Ctrl/Cmd+H` 替换，支持逐条确认与全部替换（复用 FR-EDITOR-09） | P0 |

**快速切换**

| 需求 ID | 需求描述 | 优先级 |
| --- | --- | --- |
| `FR-SEARCH-40` | `Ctrl/Cmd+O` 快速打开：按文件名模糊搜索笔记（基于 `fuse.js` 7.5.0），不搜正文，响应 < 50ms | P0 |
| `FR-SEARCH-41` | 快速打开支持路径片段匹配（如输入 `work/rep` 命中 `work/2026/report.md`） | P1 |

#### 4.5.2 验收标准

**AC-SEARCH-01｜中文搜索命中**
- Given Vault 中 10000 篇笔记，其中 3 篇正文含短语「知识图谱的力导向布局」
- When 用户搜索「力导向布局」
- Then 3 篇全部命中且排在前列；结果片段中「力导向布局」被高亮；响应时间 < 200ms

**AC-SEARCH-02｜高级语法组合**
- Given Vault 中含标签 `#工作/2026` 的笔记 50 篇，其中 8 篇含短语「季度报告」，2 篇同时含「草稿」
- When 用户搜索 `tag:#工作/2026 "季度报告" -草稿`
- Then 返回 6 条结果（8 - 2）；每条均满足全部三个条件；结果可解释（UI 显示已应用的过滤条件）

**AC-SEARCH-03｜增量输入与查询取消**
- Given 10000 篇笔记的 Vault
- When 用户在搜索框中快速连续输入 10 个字符（每字符间隔 50ms）
- Then 输入过程不卡顿（每键响应 < 50ms）；仅在停顿 200ms 后发起查询；前序未完成的查询被取消，最终仅展示与完整输入匹配的结果；无中间态结果闪烁残留

**AC-SEARCH-04｜搜索与索引一致性**
- Given 笔记 `A.md` 已被索引且可搜到关键词 `foo`
- When 用户将 `A.md` 中的 `foo` 改为 `bar` 并保存
- Then 2 秒内搜索 `foo` 不再命中 `A.md`；搜索 `bar` 命中 `A.md`；期间无需手动触发重建

**AC-SEARCH-05｜语法错误提示**
- Given 搜索框
- When 用户输入 `"未闭合的短语`
- Then 显示明确的语法错误提示（指出引号未闭合），而非静默返回空结果或抛错崩溃

**AC-SEARCH-06｜快速打开性能**
- Given 含 50000 个文件的 Vault
- When 用户按 `Ctrl+O` 并输入 3 个字符
- Then 候选列表在 50ms 内呈现；随输入持续收敛；键盘可完整操作（选择、打开）

---

### 4.6 知识图谱（`GRAPH`）

**功能描述**：将 Vault 的链接关系可视化为力导向图（对应场景 SC-04），基于 `cytoscape` 3.34.3。

#### 4.6.1 需求条目

| 需求 ID | 需求描述 | 优先级 |
| --- | --- | --- |
| `FR-GRAPH-01` | 全局图谱视图：展示 Vault 内全部笔记为节点、全部 `resolved` 链接为边 | P1 |
| `FR-GRAPH-02` | 局部图谱视图：以当前笔记为中心，展示 N 跳（N 可配置，默认 2）内的邻居子图 | P1 |
| `FR-GRAPH-03` | 力导向布局：节点自动排布，链接多的节点趋向中心；布局稳定后停止迭代以节省 CPU | P1 |
| `FR-GRAPH-04` | 节点视觉编码：大小按链接度（入链+出链数）；颜色可按文件夹、按标签、按是否孤立分组（可切换） | P1 |
| `FR-GRAPH-05` | 交互：拖拽节点、滚轮缩放、平移画布、点击节点打开对应笔记、悬停高亮该节点及其直接邻居与边（其余降低不透明度） | P1 |
| `FR-GRAPH-06` | 过滤器：按文件夹、按标签、按路径通配符筛选可见节点；可隐藏孤立节点；可显示/隐藏悬空链接目标（以虚化幽灵节点表示） | P1 |
| `FR-GRAPH-07` | 悬空链接在图谱中以**幽灵节点**（半透明/虚线边框）呈现，点击可创建该笔记 | P2 |
| `FR-GRAPH-08` | 搜索图谱内节点（按名称定位并居中） | P2 |
| `FR-GRAPH-09` | **大规模降级策略**：节点数超过阈值（默认 3000，可配置）时，默认不渲染全局图谱，而是提示用户缩小范围（使用局部图谱或过滤器）；提供「强制渲染」选项并明确警告可能的性能影响 | P1 |
| `FR-GRAPH-10` | 图谱数据来自索引库查询，笔记或链接变更后需可手动刷新（提供刷新按钮）；不要求实时自动更新 | P1 |
| `FR-GRAPH-11` | 图谱视图关闭时必须释放 cytoscape 实例与 WebGL/Canvas 资源，不得造成内存泄漏 | P1 |
| `FR-GRAPH-12` | 嵌入链接（`embed`）在图谱中以不同样式（如虚线边）与普通链接区分，可选择是否显示 | P2 |

#### 4.6.2 验收标准

**AC-GRAPH-01｜图谱与索引一致**
- Given 一个含 500 篇笔记、1200 条 `resolved` 链接的 Vault
- When 打开全局图谱
- Then 渲染节点数 = 500（或 500 + 幽灵节点数，若开启悬空显示）；边数 = 1200；随机抽取 10 个节点，其度数与 `SELECT COUNT(*) FROM link WHERE src_file_id=? OR dst_file_id=?` 一致

**AC-GRAPH-02｜点击跳转与悬停高亮**
- Given 全局图谱已渲染
- When 用户悬停某节点
- Then 该节点及其直接邻居与相连边保持高亮，其余元素明显降低不透明度；移出后恢复
- When 用户点击某节点
- Then 对应笔记在编辑器中打开；若为幽灵节点则触发创建流程

**AC-GRAPH-03｜大规模降级**
- Given 一个含 20000 篇笔记的 Vault，图谱节点阈值设为 3000
- When 用户打开全局图谱
- Then 不直接开始渲染，而是显示提示（含当前节点数、阈值、以及「使用局部图谱」「应用过滤器」「强制渲染」三个操作）；选择「强制渲染」前给出性能警告并需确认；选择局部图谱则正常渲染当前笔记的 2 跳子图且流畅可交互

**AC-GRAPH-04｜资源释放**
- Given 用户反复打开与关闭图谱视图 20 次（每次含 1000 节点）
- When 全部关闭后检查
- Then 内存占用回落至接近初始水平（无持续增长）；无残留的 cytoscape 实例或动画帧回调

---

### 4.7 标签系统（`TAG`）

**功能描述**：标签是链接之外的第二种组织维度，支持无限层级。

#### 4.7.1 需求条目

| 需求 ID | 需求描述 | 优先级 |
| --- | --- | --- |
| `FR-TAG-01` | 标签面板：树形展示全部标签及其层级，每个标签显示引用计数 | P0 |
| `FR-TAG-02` | 层级标签按 `/` 展开为树（`#a/b/c` 显示为 a → b → c 三级嵌套） | P0 |
| `FR-TAG-03` | 点击标签展示该标签（含全部子标签，可切换是否含子级）下的笔记列表 | P0 |
| `FR-TAG-04` | 标签列表支持按引用计数、按名称排序；支持在标签面板内过滤 | P1 |
| `FR-TAG-05` | 笔记内的标签可点击，点击后跳转到该标签的笔记列表视图 | P0 |
| `FR-TAG-06` | 编辑器内 `#` 触发标签自动补全（复用 FR-EDITOR-24），补全含层级路径 | P0 |
| `FR-TAG-07` | 标签重命名：将 Vault 内某标签的全部出现改写为新名称（含层级子标签的前缀更新），**必须**走预览 + 备份 + 原子改写流程（复用 FR-FILE-21/22） | P1 |
| `FR-TAG-08` | 标签重命名需处理层级冲突：新名称不得与现有标签形成非法嵌套或循环 | P1 |
| `FR-TAG-09` | 大小写不敏感去重：`#Work` 与 `#work` 视为同一标签，显示名取首次出现的原文形式（可在设置中改为统一小写显示） | P0 |
| `FR-TAG-10` | frontmatter `tags:` 中的标签同样纳入标签面板，与正文标签统一计数；来源可在笔记的标签列表中区分标识 | P0 |
| `FR-TAG-11` | 删除标签：从全部笔记中移除该标签文本（改写文件），走备份与预览流程 | P2 |
| `FR-TAG-12` | 标签统计的 `ref_count` 必须与 `file_tag` 实际计数一致；发现不一致时可触发重算（见 §3.6 重建逻辑） | P0 |
| `FR-TAG-13` | 无标签的笔记数量在标签面板中可见（「未分类」条目） | P2 |

#### 4.7.2 验收标准

**AC-TAG-01｜层级标签树**
- Given Vault 中存在 `#项目/alpha/前端`、`#项目/alpha/后端`、`#项目/beta`、`#随笔`
- When 打开标签面板
- Then 树形结构正确显示为「项目 → alpha → {前端, 后端}、项目 → beta、随笔」；各级计数为其自身及全部子孙的引用总和；`is_leaf` 仅最末级为真

**AC-TAG-02｜标签重命名的原子改写**
- Given 标签 `#old/name` 出现在 80 篇笔记中（含正文与 frontmatter 两种来源）
- When 用户将其重命名为 `#new/name` 并确认
- Then 80 篇笔记中全部 `#old/name` 被改写为 `#new/name`；frontmatter 中的 `tags:` 值同步更新；改写前已备份全部 80 个文件；标签面板树更新为 `new → name`；`#old` 若无其他子孙则从标签表中消失
- And 若改写过程中任一文件失败，则全部 80 个文件回滚至备份内容，标签表保持原状

**AC-TAG-03｜大小写去重**
- Given 3 篇笔记分别使用 `#Work`、`#work`、`#WORK`
- When 索引完成并打开标签面板
- Then 仅显示 1 个标签条目，`ref_count = 3`；`norm` 字段为 `work`；`display` 为首次出现的形式；点击该标签列出全部 3 篇笔记

**AC-TAG-04｜计数一致性**
- Given 索引完成后的任意时刻
- When 对全部标签执行 `SELECT COUNT(*) FROM file_tag WHERE tag_id = ?` 并与 `tag.ref_count` 比对
- Then 全部标签的两个数值完全一致；若人为制造不一致后触发重算，则恢复一致

---

### 4.8 附件管理（`ATTACH`）

**功能描述**：管理笔记引用的非 Markdown 文件（图片、PDF、音视频等）。

#### 4.8.1 需求条目

| 需求 ID | 需求描述 | 优先级 |
| --- | --- | --- |
| `FR-ATTACH-01` | 附件类型识别：图片（png/jpg/jpeg/gif/webp/svg/bmp）、PDF、音视频（mp4/mp3/wav 等）、其他文件 | P0 |
| `FR-ATTACH-02` | 附件存储位置可配置：① 与笔记同目录 ② Vault 根下统一目录（如 `attachments/`）③ 与笔记同级的子目录（如 `<笔记名>.assets/`） | P1 |
| `FR-ATTACH-03` | 拖拽文件到编辑器窗口即插入为附件：复制文件到 Vault 内（按 FR-ATTACH-02 规则）并插入 `![[文件名]]` | P1 |
| `FR-ATTACH-04` | 剪贴板粘贴图片：将剪贴板图片写入 Vault（按命名规则如 `Pasted image <时间戳>.png`）并插入引用 | P1 |
| `FR-ATTACH-05` | 附件同名冲突时自动追加序号（`name 1.png`），**不得**覆盖已有附件 | P0 |
| `FR-ATTACH-06` | 阅读态直接渲染图片附件；支持点击放大查看、缩放 | P1 |
| `FR-ATTACH-07` | PDF 附件在阅读态以内嵌查看器打开或调用系统默认程序（可配置） | P2 |
| `FR-ATTACH-08` | 音视频附件在阅读态提供播放控件 | P2 |
| `FR-ATTACH-09` | 未引用附件检测：列出 Vault 内未被任何笔记引用的附件，支持批量删除（进回收站） | P2 |
| `FR-ATTACH-10` | 附件在文件树中与笔记视觉区分（图标不同） | P0 |
| `FR-ATTACH-11` | **远程资源限制**：Markdown 中的远程 URL 图片（`http(s)://...`）**应用内始终不加载**，显示占位符与来源域名；用户点击占位符时**交由系统默认浏览器打开该 URL**（复用 shell:allow-open 的 https 白名单），应用进程自身不发起任何请求（保 AC-SEC-03 的零网络请求） | P0 |
| `FR-ATTACH-12` | 附件文件不进入全文搜索索引（仅文件名可被搜索到） | P0 |

#### 4.8.2 验收标准

**AC-ATTACH-01｜粘贴图片**
- Given 剪贴板中有一张 2MB 的 PNG 图片，附件目录设置为 `attachments/`
- When 用户在编辑器中执行粘贴
- Then `attachments/` 下生成命名规范的 PNG 文件；编辑器光标处插入 `![[Pasted image <时间戳>.png]]`；阅读态能正确显示该图片

**AC-ATTACH-02｜同名不覆盖**
- Given `attachments/` 下已存在 `diagram.png`（内容 A）
- When 用户拖入另一个同名但内容不同的 `diagram.png`（内容 B）
- Then 生成 `diagram 1.png`（内容 B）；原 `diagram.png` 内容**保持 A 不变**（逐字节校验）；插入的引用指向新文件

**AC-ATTACH-03｜远程图片不自动加载**
- Given 一篇笔记含 `![图](https://tracker.example.com/pixel.png)`
- When 以阅读态渲染
- Then 该图片**不发起网络请求**（通过网络监控断言零请求）；显示占位符与来源域名
- When 用户点击该占位符
- Then 由系统默认浏览器打开该 URL；**应用进程自身仍不发起任何网络请求**（网络监控中该请求的发起方是浏览器，不是应用）

---

### 4.9 命令面板（`PALETTE`）

**功能描述**：通过 `Ctrl/Cmd+P` 唤起的全局命令入口，模糊匹配全部内置命令与笔记（对应场景 SC-09）。

#### 4.9.1 需求条目

| 需求 ID | 需求描述 | 优先级 |
| --- | --- | --- |
| `FR-PALETTE-01` | `Ctrl/Cmd+P` 唤起命令面板，`Esc` 或失焦关闭 | P0 |
| `FR-PALETTE-02` | 基于 `fuse.js` 7.5.0 的模糊匹配。命令名支持拼音首字母匹配（如输入 `xj` 命中「新建笔记」）：由**内置静态映射表**提供（命令集合固定、约 40 条），**不引入第三方拼音库**（红线 R-16）。仅命令名参与拼音匹配，笔记标题不参与 | P0 |
| `FR-PALETTE-03` | 命令来源：全部内置功能命令（新建笔记、切换主题、打开图谱、打开设置、重新索引、切换 Vault 等），不含任何插件命令 | P0 |
| `FR-PALETTE-04` | 结果分组显示：命令 / 笔记（若开启混合模式）；每组最多显示 N 条，可展开 | P1 |
| `FR-PALETTE-05` | 每条命令显示：名称、图标、快捷键（若有） | P1 |
| `FR-PALETTE-06` | 键盘完整可操作：上下选择、`Enter` 执行、`Esc` 关闭、输入即过滤 | P0 |
| `FR-PALETTE-07` | 命令执行后自动关闭面板；执行失败的命令给出错误提示 | P0 |
| `FR-PALETTE-08` | 上下文相关命令：面板打开时依据当前焦点（编辑器 / 文件树 / 图谱）过滤出可用命令，不可用命令置灰并说明原因 | P1 |
| `FR-PALETTE-09` | 最近使用命令排序加权（存于全局库） | P2 |
| `FR-PALETTE-10` | 命令面板响应时间 < 50ms（本地模糊匹配，不走 IPC） | P0 |

#### 4.9.2 验收标准

**AC-PALETTE-01｜模糊匹配与执行**
- Given 命令面板已注册 40 个内置命令
- When 用户按 `Ctrl+P` 输入 `graph` 或 `图谱`
- Then 候选列表顶部显示「打开知识图谱」；按 `Enter` 后图谱视图打开且面板关闭；全过程 < 50ms

**AC-PALETTE-02｜上下文过滤**
- Given 当前焦点在图谱视图（无笔记打开）
- When 用户唤起命令面板
- Then 「保存笔记」等编辑器专属命令置灰并提示「当前无打开的笔记」；「导出图谱」等图谱相关命令可用

---

### 4.10 回收站（`TRASH`）

**功能描述**：软删除机制，防止误删导致数据丢失（对应场景 SC-07）。

#### 4.10.1 需求条目

| 需求 ID | 需求描述 | 优先级 |
| --- | --- | --- |
| `FR-TRASH-01` | 删除文件/文件夹时移入回收站，**不做**永久删除（对应 FR-FILE-30） | P0 |
| `FR-TRASH-02` | 回收站实体存储于 `.knowlpad/trash/<yyyy-MM>/`，文件名附加时间戳与随机串避免冲突；**原始相对路径必须记录在 `.knowlpad/trash/manifest.json`**（与可丢弃的 `index.db` 解耦，保证索引重建甚至 `index.db` 丢失后仍可恢复），采用追加写 + fsync，损坏时逐条降级解析 | P0 |
| `FR-TRASH-03` | 回收站视图：列出全部已删除项，显示原路径、删除时间、大小 | P0 |
| `FR-TRASH-04` | 恢复：将项目移回原始路径；若原路径已被占用则提示冲突并提供「恢复为新名称」「恢复到其他位置」「取消」 | P0 |
| `FR-TRASH-05` | 恢复后重新纳入索引，其原有链接关系重新解析 | P0 |
| `FR-TRASH-06` | 彻底删除：从回收站永久移除，需二次确认并明确告知不可恢复 | P0 |
| `FR-TRASH-07` | 自动过期清理：超过 N 天（默认 30，可配置）的回收站项自动彻底删除；可关闭此行为 | P1 |
| `FR-TRASH-08` | 清空回收站：一次性彻底删除全部项，需二次确认 | P1 |
| `FR-TRASH-09` | 回收站中的项**不得**出现在文件树、搜索结果、图谱、标签统计中（对应 FR-STORAGE-04） | P0 |
| `FR-TRASH-10` | 删除大量文件（如含 1000 文件的文件夹）时显示进度，可取消（取消则已移入的项目可回滚） | P1 |
| `FR-TRASH-11` | 被删除笔记的入链自动变为 `dangling` 状态（引用方文件不被修改，对应 FR-FILE-33） | P0 |
| `FR-TRASH-12` | 回收站数据跨软件重启保持：**权威来源为实体文件 + `.knowlpad/trash/manifest.json`**，索引库 `file.deleted` 仅作可重建的加速字段。索引库重建后，回收站列表与恢复能力**不得丢失** | P0 |

#### 4.10.2 验收标准

**AC-TRASH-01｜删除与恢复完整性**
- Given 笔记 `folder/A.md`（含 200 字内容与 5 条出链，被 10 篇笔记引用）
- When 用户删除该笔记，随后从回收站恢复
- Then 文件回到 `folder/A.md` 且内容逐字节一致；5 条出链重新解析为原状态；10 条入链从 `dangling` 恢复为 `resolved`；删除期间该笔记不出现在文件树与搜索结果中

**AC-TRASH-02｜恢复路径冲突**
- Given 笔记 `A.md` 已删除进回收站，随后用户在原位置新建了同名 `A.md`
- When 用户从回收站恢复旧的 `A.md`
- Then 提示路径冲突并给出三个选项；**不覆盖**新建的 `A.md`；选择「恢复为新名称」后生成 `A 1.md` 且两个文件内容各自正确

**AC-TRASH-03｜彻底删除确认**
- Given 回收站中有 1 个项目
- When 用户执行「彻底删除」
- Then 首次点击仅弹出二次确认对话框，**文件仍存在**；确认后文件从磁盘移除；取消则文件保留

**AC-TRASH-04｜自动过期**
- Given 回收站中有一个删除时间为 31 天前的项目，过期天数设为 30
- When 软件启动并执行过期清理
- Then 该项目被彻底删除；删除时间为 29 天前的项目**保留**；清理动作在日志中记录

---

### 4.11 设置与主题（`SETTINGS`）

**功能描述**：软件配置中心与视觉主题。

#### 4.11.1 需求条目

| 需求 ID | 需求描述 | 优先级 |
| --- | --- | --- |
| `FR-SET-01` | 设置面板按分类组织：编辑器、外观、文件与链接、搜索、图谱、回收站、更新、关于 | P0 |
| `FR-SET-02` | 设置项分类清单见 §4.11.3 | P0 |
| `FR-SET-03` | 全部设置持久化到全局库 `preference` 表，写入采用防抖批量提交（500ms） | P0 |
| `FR-SET-04` | 设置分作用域：**全局设置**（跨 Vault，如主题）与 **Vault 设置**（仅当前 Vault，如附件目录、默认新建位置），后者存于 `vault_state` | P1 |
| `FR-SET-05` | 设置搜索：在设置面板内按关键词过滤设置项 | P1 |
| `FR-SET-06` | 恢复默认：单项恢复与全部恢复（全部恢复需二次确认） | P1 |
| `FR-SET-07` | 设置变更后即时生效，无需重启（除明确标注需重启的项） | P0 |
| `FR-SET-08` | 导出/导入设置（JSON 文件），便于跨设备迁移配置 | P2 |

**主题**

| 需求 ID | 需求描述 | 优先级 |
| --- | --- | --- |
| `FR-SET-20` | 内置深色与浅色两套主题，支持「跟随系统」 | P0 |
| `FR-SET-21` | 主题通过 CSS 变量实现，切换无需重载页面 | P0 |
| `FR-SET-22` | 可调节：编辑器字体族（提供等宽字体候选）、字号（12–24px）、行高、编辑器最大行宽、界面缩放（80%–150%） | P1 |
| `FR-SET-23` | 界面缩放通过 CSS `zoom` 或根字号实现，须保证布局不破版 | P1 |
| `FR-SET-24` | **不提供**自定义 CSS 注入与第三方主题安装（对应 Out of Scope X-12） | P0 |
| `FR-SET-25` | 全部颜色对比度满足 WCAG 2.1 AA 级（正文 ≥ 4.5:1，大字 ≥ 3:1） | P1 |

#### 4.11.2 验收标准

**AC-SET-01｜设置持久化**
- Given 用户将编辑器字号改为 18px、主题改为深色
- When 关闭软件并重新启动
- Then 两项设置均保持；全局库 `preference` 表中存在对应键值

**AC-SET-02｜Vault 级设置隔离**
- Given Vault A 的附件目录设为 `assets/`，Vault B 使用默认设置
- When 用户从 A 切换到 B
- Then B 的附件目录为默认值而非 `assets/`；切回 A 时仍为 `assets/`

**AC-SET-03｜即时生效**
- Given 设置面板与编辑器同时可见
- When 用户调整界面缩放至 120%
- Then 界面立即缩放且无破版（无元素溢出、无重叠、无滚动条异常）；无需重启

---

#### 4.11.3 设置项清单

| 分类 | 设置项 | 默认值 | 作用域 |
| --- | --- | --- | --- |
| 编辑器 | 自动保存防抖时长 | 1000ms | 全局 |
| 编辑器 | 默认编辑模式（编辑/分屏/阅读） | 分屏 | 全局 |
| 编辑器 | 显示行号 | 关 | 全局 |
| 编辑器 | 拼写检查语言 | 跟随系统 | 全局 |
| 编辑器 | 悬停预览延迟 | 300ms | 全局 |
| 编辑器 | 撤销栈深度 | 500 | 全局 |
| 外观 | 主题（深/浅/跟随系统） | 跟随系统 | 全局 |
| 外观 | 编辑器字体族 | 系统等宽 | 全局 |
| 外观 | 编辑器字号 | 16px | 全局 |
| 外观 | 行高 | 1.6 | 全局 |
| 外观 | 编辑器最大行宽 | 720px | 全局 |
| 外观 | 界面缩放 | 100% | 全局 |
| 文件与链接 | 默认新建位置 | Vault 根目录 | Vault |
| 文件与链接 | 附件存储策略 | 与笔记同目录 | Vault |
| 文件与链接 | 链接改写模式（预览确认/自动） | 预览确认 | Vault |
| 文件与链接 | 新建链接默认格式（最短路径/完整路径） | 最短路径 | Vault |
| 文件与链接 | 显示隐藏文件 | 关 | Vault |
| 搜索 | 搜索防抖 | 200ms | 全局 |
| 搜索 | 默认匹配模式（AND/OR） | AND | 全局 |
| 搜索 | 结果默认排序 | 相关度 | 全局 |
| 搜索 | 片段上下文行数 | 2 | 全局 |
| 图谱 | 局部图谱跳数 | 2 | 全局 |
| 图谱 | 节点数降级阈值 | 3000 | 全局 |
| 图谱 | 节点着色依据 | 文件夹 | 全局 |
| 图谱 | 显示孤立节点 | 开 | 全局 |
| 图谱 | 显示悬空链接幽灵节点 | 关 | 全局 |
| 回收站 | 自动过期天数 | 30 | 全局 |
| 回收站 | 启用自动过期清理 | 开 | 全局 |
| 更新 | 自动检查更新 | 开 | 全局 |
| 更新 | 检查频率 | 每天 | 全局 |
| 更新 | 更新通道 | stable | 全局 |
| 索引 | 索引线程数 | CPU 核数 - 1 | 全局 |
| 索引 | 单文件大小上限（超出跳过并警告） | 5MB | 全局 |
| 隐私 | 远程图片自动加载 | 关 | 全局 |

---

### 4.12 自动更新（`UPDATE`）

**功能描述**：基于 `@tauri-apps/plugin-updater` 的客户端自动更新。更新元数据为 `latest.json`；分发载体为 GitHub Release（构建产出）与 Gitee Release（国内分发镜像），二者均为**公开仓库**。

> ⚠️ **分发端点硬约束（2026-09-30 登记，与 `updater分发端点决策-2026-09-30.md` 对应）**：
> 分发仓库**必须保持公开**。updater 是**匿名** GET 请求，私有仓库的 Release 资产需要认证才能下载，
> 一旦改为私有，FR-UPDATE-01~14 会**全部静默失效**（实测：匿名访问私有仓库 API 返回 404）。
> 若将来确需恢复私有，**必须同时**提供公开的替代分发端点（自建静态端点或专用公开发布仓库），
> 并同步更新 `tauri.conf.json` 的 `plugins.updater.endpoints`。

#### 4.12.1 需求条目

| 需求 ID | 需求描述 | 优先级 |
| --- | --- | --- |
| `FR-UPDATE-01` | 启动时按检查频率自动检查更新（可关闭）；命令面板与设置页提供手动「检查更新」 | P1 |
| `FR-UPDATE-02` | 更新流程：请求 `latest.json` → SemVer 比对本地版本 → 发现新版本则提示（显示版本号与 Release Notes） | P1 |
| `FR-UPDATE-03` | 用户确认后下载安装包，显示下载进度，可取消 | P1 |
| `FR-UPDATE-04` | **签名验证**：使用 `tauri.conf.json` 中配置的公钥验证更新包签名，验签失败**必须**中止并提示，不得安装 | P0 |
| `FR-UPDATE-05` | **完整性校验**：验证下载文件的 SHA-256 与 `latest.json` 声明一致，不一致则中止并删除下载文件 | P0 |
| `FR-UPDATE-06` | 传输强制 TLS 1.2+；更新源 URL 必须为 HTTPS | P0 |
| `FR-UPDATE-07` | 安装需用户确认，重启后完成更新；更新前提示用户保存工作 | P1 |
| `FR-UPDATE-08` | 更新失败时保留当前版本可继续使用，给出失败原因与手动下载链接 | P1 |
| `FR-UPDATE-09` | 强制更新：`latest.json` 中标记 `force: true` 的版本，提示不可跳过（用于严重安全漏洞修复） | P2 |
| `FR-UPDATE-10` | 「跳过此版本」选项：记录被跳过的版本号，不再提示，但更高版本仍提示 | P1 |
| `FR-UPDATE-11` | 更新元数据请求失败（离线）时静默降级，**不得**弹窗打扰用户或阻塞启动 | P0 |
| `FR-UPDATE-12` | 设置页显示当前版本号、更新通道、最近一次检查结果 | P1 |
| `FR-UPDATE-13` | 更新**不得**触碰用户 Vault 数据；更新过程与 Vault 完全隔离 | P0 |
| `FR-UPDATE-14` | 更新包下载至配置目录 `update-cache/`，安装完成或失败后清理 | P1 |

> **关于 v5 中的「增量更新」与「自动回滚」**：v5 §11.7 提到 bsdiff 增量更新与"更新失败自动回滚到上一版本"。经评估，Tauri 官方 updater 插件**不支持**二进制差分增量更新，也**不支持**应用二进制的自动回滚（安装包替换由操作系统完成，失败即保持原版本）。因此本 PRD 将 V1.0 范围限定为**全量更新**，"回滚"的实际语义为「更新失败时保持当前版本不变」（FR-UPDATE-08）。此差异已记录于 §10 风险与技术方案的修正说明中。

#### 4.12.2 验收标准

**AC-UPDATE-01｜正常更新流程**
- Given 本地版本 1.0.0，更新源 `latest.json` 声明 1.1.0 且签名有效
- When 用户执行检查更新并确认安装
- Then 显示 Release Notes；下载显示进度；签名与 SHA-256 校验均通过；提示重启安装；重启后版本为 1.1.0；Vault 数据完整无变化

**AC-UPDATE-02｜签名验证失败（安全关键）**
- Given 更新包被篡改导致签名不匹配
- When 下载完成并校验
- Then 更新被**中止**；显示明确的安全警告（不得安装）；下载文件被删除；当前版本继续正常使用；事件记入日志

**AC-UPDATE-03｜SHA-256 不匹配**
- Given `latest.json` 声明的哈希与实际下载文件不符
- When 校验执行
- Then 更新中止并提示"文件可能已损坏或被篡改"；下载文件被清理；不留下半成品状态

**AC-UPDATE-04｜离线静默降级**
- Given 设备无网络连接
- When 软件启动并触发自动检查更新
- Then 启动流程**不被阻塞**（启动耗时不因检查更新增加 > 500ms）；无任何弹窗；日志记录检查失败原因；其余功能全部正常

**AC-UPDATE-05｜跳过版本**
- Given 用户对 1.1.0 选择「跳过此版本」
- When 后续再次检查更新
- Then 不再提示 1.1.0；当更新源发布 1.2.0 时正常提示

---

## 5. IPC 接口契约

本章是前后端协作的**唯一接口约定**。完整类型定义（TypeScript interface + Rust struct）在技术方案 §6 给出，此处定义清单、错误码与事件协议。

### 5.1 通信机制

| 机制 | 方向 | 用途 |
| --- | --- | --- |
| `invoke(command, payload)` | 前端 → Rust | 请求-响应式调用，全部数据操作走此通道 |
| `emit` / `listen(event)` | Rust → 前端 | 单向事件推送：文件变更、索引进度、长任务进度 |

**类型安全约定**

| 约束 ID | 内容 |
| --- | --- |
| `IPC-01` | 前端**必须**通过 `core/ipc/` 封装调用，封装层为每个 Command 提供强类型的入参与返回值定义。**禁止**在 feature 中裸写 `invoke('字符串', {...})` |
| `IPC-02` | Rust 侧 Command 的入参结构体与返回结构体**必须**使用 `serde` 派生 `Deserialize`/`Serialize`，并通过 `#[serde(rename_all = "camelCase")]` 使 IPC 传输的 JSON 键统一 **camelCase**；前端 TS 类型字段名与之严格对应。Rust 结构体内部字段名保持 `snake_case`（语言惯例），仅传输层映射为 camelCase。决策理由见技术方案 §8.4 |
| `IPC-03` | 全部 Command **必须**返回 `Result<T, AppError>`。成功时 `T` 为业务数据，失败时为 §5.2 定义的错误结构 |
| `IPC-04` | 路径参数一律使用**相对 Vault 根**的路径（`/` 分隔）。Rust 侧**必须**对每个路径参数执行安全校验（SEC-02） |
| `IPC-05` | 单个 Command 的响应体**不得**超过 4MB。大批量数据必须分页或流式（通过事件推送） |
| `IPC-06` | 全部 Command 在 Tauri Capabilities 中显式声明。**禁止**使用 `core:default` 之外的通配权限；**禁止**授予 `fs:allow-*` 全量文件权限（对应 AC-01） |

### 5.2 统一错误结构

```typescript
interface KpError {
  code: KpErrorCode;        // 机器可读错误码，见下表
  message: string;          // 面向用户的中文提示
  detail?: string;          // 技术细节，仅写入日志，不展示给用户
  context?: Record<string, unknown>;  // 结构化上下文，如冲突路径、失败文件列表
}
```

| 错误码 | 含义 | HTTP 类比 | 前端处理建议 |
| --- | --- | --- | --- |
| `E_VAULT_NOT_OPEN` | 当前无打开的 Vault | 409 | 引导至欢迎页 |
| `E_VAULT_PATH_INVALID` | Vault 路径不存在或不可访问 | 404 | 提示重新定位 Vault |
| `E_PATH_OUTSIDE_VAULT` | 路径越界（穿越攻击或程序错误） | 403 | 记录日志，提示操作非法 |
| `E_PATH_ESCAPE_DENY` | 路径含非法字符或被拒绝的符号链接 | 403 | 同上 |
| `E_FILE_NOT_FOUND` | 文件不存在 | 404 | 提示并刷新文件树 |
| `E_FILE_EXISTS` | 目标已存在（重名冲突） | 409 | 弹出冲突处理对话框 |
| `E_FILE_LOCKED` | 文件被外部程序占用 | 423 | 提示关闭占用程序后重试 |
| `E_FILE_TOO_LARGE` | 文件超过大小上限 | 413 | 提示并给出上限值 |
| `E_PERMISSION_DENIED` | 操作系统拒绝访问 | 403 | 提示检查文件权限 |
| `E_INVALID_FILENAME` | 文件名非法 | 400 | 提示具体的非法原因 |
| `E_IO_FAILURE` | 底层 IO 失败（磁盘满等） | 500 | 提示检查磁盘空间 |
| `E_PARSE_FRONTMATTER` | frontmatter YAML 解析失败（非致命） | 200+warning | 降级处理，不阻塞 |
| `E_DB_ERROR` | 数据库操作失败 | 500 | 提示重建索引 |
| `E_INDEX_SIGNATURE_MISMATCH` | 索引签名不匹配，需重建 | 409 | 触发重建流程 |
| `E_INDEX_BUSY` | 索引正在进行，操作需排队 | 429 | 提示稍后重试 |
| `E_PREVIEW_EXPIRED` | 改写预览已过期或已被消费 | 410 | 提示重新执行预览 |
| `E_WRITE_CONFLICT` | 保存时检测到文件已被外部修改（`base_mtime` 不一致） | 409 | 弹出三选项（FR-EDITOR-34），**禁止静默覆盖** |
| `E_SEARCH_SYNTAX` | 搜索语法错误 | 400 | 显示语法提示 |
| `E_LINK_AMBIGUOUS` | 链接目标歧义 | 409 | 展示候选列表 |
| `E_REWRITE_FAILED` | 批量链接改写失败（已回滚） | 500 | 显示失败文件清单 |
| `E_BACKUP_FAILED` | 备份创建失败，操作已中止 | 500 | 提示磁盘空间或权限 |
| `E_TRASH_RESTORE_CONFLICT` | 恢复时原路径被占用 | 409 | 提供三种恢复选项 |
| `E_UPDATE_SIGNATURE` | 更新包签名验证失败 | 403 | **安全警告**，禁止安装 |
| `E_UPDATE_CHECKSUM` | 更新包哈希不匹配 | 403 | 同上 |
| `E_UPDATE_NETWORK` | 更新检查网络失败 | 502 | 静默降级（FR-UPDATE-11） |
| `E_CANCELLED` | 用户取消操作 | 499 | 静默处理 |
| `E_INTERNAL` | 未分类内部错误 | 500 | 提示并引导查看日志 |

**错误处理原则**

| 约束 ID | 内容 |
| --- | --- |
| `ERR-01` | 前端**必须**处理每个 Command 的错误分支。**禁止**使用空的 `catch {}` 吞掉错误 |
| `ERR-02` | 用户可见的 `message` 必须是中文、非技术性、含可操作建议。技术细节仅入日志 |
| `ERR-03` | 数据破坏类错误（`E_REWRITE_FAILED`、`E_BACKUP_FAILED`）**必须**在 UI 中显著提示，并说明当前数据状态（已回滚 / 部分完成） |
| `ERR-04` | 全部错误必须写入日志（含错误码、上下文、时间戳），日志不含笔记正文内容（隐私） |

### 5.3 Command 清单

共 **60** 个 Command，按域分组。标注 ⚠️ 的为**写操作**，需施加额外的确认与审计（见 SEC-05）。

#### 5.3.1 Vault 域（9）

| Command | 入参 | 返回 | 说明 |
| --- | --- | --- | --- |
| `vault_list` | — | `VaultSummary[]` | 已注册 Vault 列表 |
| `vault_open` | `{ abs_path }` | `VaultInfo` | 打开并初始化 Vault |
| `vault_create` | `{ abs_path, name? }` | `VaultInfo` | 新建 Vault |
| `vault_close` | — | `null` | 关闭当前 Vault（先刷盘） |
| `vault_current` | — | `VaultInfo \| null` | 当前 Vault 信息 |
| `vault_register_remove` ⚠️ | `{ vault_id }` | `null` | 从列表移除（不删文件） |
| `vault_rename` ⚠️ | `{ vault_id, display_name }` | `null` | 修改显示名 |
| `vault_relocate` ⚠️ | `{ vault_id, new_abs_path }` | `VaultInfo` | 路径失效后重新定位 |
| `vault_pin` ⚠️ | `{ vault_id, pinned }` | `null` | 置顶 / 取消置顶（FR-VAULT-07；**勘误 D-10 补齐**，M2 落地） |

#### 5.3.2 文件域（14）

| Command | 入参 | 返回 | 说明 |
| --- | --- | --- | --- |
| `file_tree` | `{ parent_rel_path?, include_hidden? }` | `FileNode[]` | **单层**子节点（虚拟滚动数据源，逐层展开）。**禁止**一次返回全量树，避免单响应超过 IPC-05 的 4MB 上限 |
| `file_list_dir` | `{ rel_path }` | `FileNode[]` | 单层目录列表（懒加载用） |
| `note_read` | `{ rel_path }` | `NoteContent` | 读取笔记全文 |
| `note_write` ⚠️ | `{ rel_path, content, base_mtime? }` | `WriteResult` | 原子写入（`base_mtime` 用于冲突检测） |
| `note_create` ⚠️ | `{ rel_path, content?, on_conflict }` | `WriteResult` | 新建笔记 |
| `file_rename` ⚠️ | `{ from, to, on_conflict }` | `RenameResult` | 重命名（不含链接改写） |
| `file_move` ⚠️ | `{ from, to, on_conflict }` | `RenameResult` | 移动 |
| `file_delete` ⚠️ | `{ rel_path, recursive }` | `DeleteResult` | 移入回收站 |
| `folder_create` ⚠️ | `{ rel_path }` | `null` | 创建文件夹（支持多级） |
| `file_stat` | `{ rel_path }` | `FileStat` | 文件元信息 |
| `file_reveal` | `{ rel_path }` | `null` | 在系统文件管理器中显示 |
| `file_validate_name` | `{ name }` | `ValidationResult` | 文件名合法性校验 |
| `attachment_import` ⚠️ | `{ source, target_dir?, on_conflict }` | `ImportResult` | 导入外部文件为附件 |
| `attachment_paste_image` ⚠️ | `{ bytes_b64, ext, target_dir? }` | `ImportResult` | 剪贴板图片落盘 |


#### 5.3.2.1 文件域返回结构（权威定义）

> **来源**：原 §5.3.2 只给出返回类型**名**而未定义字段（2026-10-02 复核发现，见勘误 **D-11**）。
> 本节为权威定义；IPC 传输键为 **camelCase**（技术方案 §8.4），Rust 结构体内部字段保持 snake_case。

| 结构 | 字段（IPC 键） | 说明 |
| --- | --- | --- |
| `FileNode` | `relPath`、`name`、`isDir`、`kind`、`sizeBytes?`、`mtimeMs?`、`hasChildren?` | 树节点**单层**返回；`kind ∈ {note, attachment, other}`；`hasChildren` 供展开箭头判断 |
| `NoteContent` | `relPath`、`content`、`mtimeMs`、`sizeBytes` | `mtimeMs` 作为 `note_write` 的 `baseMtime`，供 FR-EDITOR-34 冲突检测 |
| `FileStat` | `relPath`、`isDir`、`kind`、`sizeBytes`、`mtimeMs` | 元信息查询 |
| `ValidationResult` | `valid`、`reason?` | `reason` 为**中文、非技术性**提示（ERR-02） |
| `RenameResult` | `from`、`to`、`newMtimeMs` | 重命名/移动（**不含**链接改写，改写属 M4） |
| `DeleteResult` | `relPath`、`trashedCount` | 软删除（FR-FILE-30）；`trashedCount` 为递归删除的条目数 |
| `ConflictPolicy` | `"overwrite" \| "renameNew" \| "cancel"` | 重名冲突三选项（FR-FILE-13）；选 `overwrite` 前必须二次确认并备份原文件（AC-FILE-04） |

> **实现状态**：`note_read` 的返回形状**已于 M2 落地**为本节的 `NoteContent`（含 `mtimeMs`，编辑器据此做冲突检测基线）；M0 期的裸字符串实现已被替换。
#### 5.3.3 链接改写域（4）

| Command | 入参 | 返回 | 说明 |
| --- | --- | --- | --- |
| `link_rewrite_preview` | `{ from_ref, to_ref, rename? }` | `RewritePreview` | 预览将改写的文件与处数（FR-FILE-21）。`rename: { from, to }` 时冻结改名规格，供执行阶段与改写合并为同一次可回滚操作 |
| `link_rewrite_apply` ⚠️ | `{ preview_id, rename? }` | `RewriteResult` | 执行改写（全有或全无）。传入 `rename: { from, to }` 时，**重命名与链接改写属于同一次可回滚操作**（FR-FILE-22、NFR-REL-04） |
| `link_rewrite_rollback` ⚠️ | `{ operation_id }` | `null` | 从备份回滚指定操作 |
| `tag_rename_apply` ⚠️ | `{ from_tag, to_tag, preview_id }` | `RewriteResult` | 标签批量重命名（FR-TAG-07） |

#### 5.3.4 搜索域（6）

| Command | 入参 | 返回 | 说明 |
| --- | --- | --- | --- |
| `search_fulltext` | `{ query, options, page }` | `SearchResultPage` | 全文搜索 |
| `search_quick_open` | `{ query, limit }` | `QuickOpenItem[]` | 文件名模糊匹配 |
| `search_in_file` | `{ rel_path, query, options }` | `InFileMatch[]` | 文件内查找 |
| `replace_in_file` ⚠️ | `{ rel_path, matches, replacement }` | `ReplaceResult` | 文件内替换 |
| `search_history_get` | — | `string[]` | 搜索历史 |
| `search_history_clear` ⚠️ | — | `null` | 清空历史 |

#### 5.3.5 链接与图谱域（9）

| Command | 入参 | 返回 | 说明 |
| --- | --- | --- | --- |
| `link_backlinks` | `{ rel_path, include_embeds? }` | `BacklinkGroup[]` | 反向链接（含上下文片段） |
| `link_outgoing` | `{ rel_path }` | `OutgoingLink[]` | 出链列表 |
| `link_dangling_list` | `{ page }` | `DanglingPage` | 悬空链接清单 |
| `link_ambiguous_list` | `{ page }` | `AmbiguousPage` | 歧义链接清单 |
| `link_orphan_list` | `{ page }` | `OrphanPage` | 孤立笔记清单 |
| `link_resolve_ambiguous` ⚠️ | `{ link_id, target_rel_path }` | `RewriteResult` | 指定歧义链接目标 |
| `link_headings` | `{ rel_path }` | `Heading[]` | 查询指定笔记的标题列表（供 FR-EDITOR-23 标题补全用） |
| `graph_full` | `{ filters, limit }` | `GraphData \| GraphTooLarge` | 全局图谱（含降级返回） |
| `graph_local` | `{ rel_path, depth, filters }` | `GraphData` | 局部图谱 |

#### 5.3.6 标签域（5）

| Command | 入参 | 返回 | 说明 |
| --- | --- | --- | --- |
| `tag_tree` | — | `TagNode[]` | 标签树（含计数） |
| `tag_notes` | `{ tag_norm, include_children }` | `TaggedNote[]` | 某标签下的笔记 |
| `tag_suggest` | `{ prefix, limit }` | `TagSuggestion[]` | 标签补全 |
| `tag_rename_preview` | `{ from_tag, to_tag }` | `RewritePreview` | 重命名预览 |
| `tag_delete_apply` ⚠️ | `{ tag_norm, preview_id }` | `RewriteResult` | 删除标签（FR-TAG-11） |

#### 5.3.7 索引域（5）

| Command | 入参 | 返回 | 说明 |
| --- | --- | --- | --- |
| `index_status` | — | `IndexStatus` | 索引状态与统计 |
| `index_rebuild` ⚠️ | `{ force }` | `null`（进度走事件） | 触发全量重建 |
| `index_cancel` | — | `null` | 取消进行中的索引任务 |
| `index_signature_get` | — | `SignatureInfo` | 当前签名信息（诊断用） |
| `index_stats` | — | `IndexStats` | 各类实体计数（设置页展示） |

#### 5.3.8 回收站域（5）

| Command | 入参 | 返回 | 说明 |
| --- | --- | --- | --- |

**索引域的返回结构**（勘误 **D-18** 的索引域部分，2026-10-05 补齐；文件域见 §5.3.2.1）：

| 类型 | 字段 | 说明 |
| --- | --- | --- |
| `IndexStatus` | `indexDir: string`、`indexDb: string`、`ready: boolean`、`schemaVersion?: number`、`signaturePrefix?: string`、`builtAt?: number` | 索引状态与统计（`ready=false` 时表示尚未建好，UI 显示「索引中」） |
| `IndexStats` | `files`、`deletedFiles`、`links`、`tags`、`headings`、`blockIds`、`ftsRows`（均 `number`） | 各表实体计数；`files` 不含软删除项 |
| `SignatureInfo` | `expected: SignatureParts`、`stored: SignatureParts \| null`、`matched: boolean`、`reason: string \| null` | 诊断用：`stored` 为 null 表示索引库未记录签名（首次打开或未完成），一律按「不可信即重建」处理 |
| `SignatureParts` | `schemaVersion`、`parserVersion`、`tokenizerVersion`、`tokenizerDictHash`、`vaultRoot`、`digest` | 签名六要素；`digest` 为规范化拼接后的 SHA-256 |

| `trash_list` | `{ page }` | `TrashPage` | 回收站列表 |
| `trash_restore` ⚠️ | `{ trash_id, on_conflict }` | `RestoreResult` | 恢复 |
| `trash_delete_permanent` ⚠️ | `{ trash_id }` | `null` | 彻底删除 |
| `trash_empty` ⚠️ | — | `EmptyResult` | 清空回收站 |
| `trash_cleanup_expired` ⚠️ | — | `CleanupResult` | 过期清理 |

#### 5.3.9 设置与系统域（5）

| Command | 入参 | 返回 | 说明 |
| --- | --- | --- | --- |
| `preference_get` | `{ keys? }` | `Record<string, unknown>` | 读取偏好 |
| `preference_set` ⚠️ | `{ entries }` | `null` | 批量写入偏好 |
| `vault_state_get` | `{ keys? }` | `Record<string, unknown>` | 读取**当前 Vault** 的界面状态（文件树展开、滚动位置等；**勘误 D-13 补齐**） |
| `vault_state_set` ⚠️ | `{ entries }` | `null` | 批量写入当前 Vault 的界面状态（界面防抖后调用） |
| `system_info` | — | `SystemInfo` | 平台、版本、路径信息（关于页） |

> **统计核对**：9 + 14 + 4 + 6 + 9 + 5 + 5 + 5 + 5 = **62** 个 Command（`vault_pin` 为勘误 D-10 补齐项，`vault_state_get/set` 为勘误 D-13 补齐项）。其中标注 ⚠️ 的写操作共 **26** 个，分域为：Vault 域 4、文件域 8、链接改写域 3、搜索域 2、链接与图谱域 1、标签域 1、索引域 1、回收站域 4、设置与系统域 2（4+8+3+2+1+1+1+4+2 = 26）。

### 5.4 事件清单

共 **13** 个事件，命名规范 `kp://<域>/<事件>`（见 §0.2）。

| 事件 | 载荷 | 触发时机 | 前端响应 |
| --- | --- | --- | --- |
| `kp://fs/created` | `{ rel_path, kind }` | 外部新增文件 | 文件树插入节点，入索引队列 |
| `kp://fs/modified` | `{ rel_path, mtime_ms }` | 外部修改文件 | 若为当前打开笔记则触发冲突检测（FR-EDITOR-34），入索引队列 |
| `kp://fs/removed` | `{ rel_path }` | 外部删除文件 | 文件树移除节点，链接重算 |
| `kp://fs/renamed` | `{ from, to }` | 外部重命名/移动 | 文件树更新，链接重算 |
| `kp://index/progress` | `{ phase, done, total, current_file? }` | 索引进行中（节流 ≥ 100ms） | 更新进度条 |
| `kp://index/completed` | `{ stats, duration_ms }` | 索引完成 | 隐藏进度，刷新各面板 |
| `kp://index/failed` | `{ code, message, failed_files }` | 索引失败 | 显示错误与重试入口 |
| `kp://index/rebuild-required` | `{ reason, old_sig, new_sig }` | 签名不匹配 | 提示需重建（FR-SIG-01） |
| `kp://note/updated` | `{ rel_path }` | 内部写入笔记完成 | 刷新反链、标签、图谱缓存 |
| `kp://link/changed` | `{ affected_files }` | 链接改写完成 | 刷新反链面板与悬空/歧义清单 |
| `kp://vault/changed` | `{ vault_id, change_type }` | Vault 注册表变更 | 刷新 Vault 列表 |
| `kp://update/available` | `{ version, notes, force }` | 发现新版本 | 弹出更新提示 |
| `kp://update/progress` | `{ phase, percent }` | 更新下载中 | 更新进度条 |

**事件设计约束**

| 约束 ID | 内容 |
| --- | --- |
| `EVT-01` | 文件监听事件必须经**去抖动与归并**（见技术方案 §5.4）：200ms 窗口内同一文件的多次变更合并为一次；短时间内 create+modify 合并为 create |
| `EVT-02` | `kp://index/progress` 必须节流，发射间隔 ≥ 100ms，避免高频事件淹没前端 |
| `EVT-03` | 事件载荷**不得**包含笔记正文内容（仅路径与元信息），降低隐私暴露面与序列化开销 |
| `EVT-04` | 前端对全部事件**必须**做幂等处理：重复收到同一事件不得产生错误状态 |
| `EVT-05` | 事件监听器在组件卸载时**必须**注销，防止内存泄漏（对应 AC-GRAPH-04 类问题） |

### 5.5 IPC 性能预算

| 指标 | 预算 | 说明 |
| --- | --- | --- |
| 单次 `invoke` 往返开销 | < 2ms | Tauri IPC 基线开销（不含业务逻辑） |
| `note_read`（100KB 文件） | < 20ms | 磁盘读 + UTF-8 校验 |
| `search_fulltext`（1 万篇） | < 200ms | 见 NFR-PERF-04 |
| `link_backlinks`（500 条反链） | < 50ms | 含上下文片段读取 |
| `file_tree`（单层，10 万节点库） | < 50ms | 单层子节点，仅元数据；全库树按需逐层加载（IPC-05 限 4MB，禁止整树返回） |
| `graph_full`（3000 节点） | < 500ms | 超阈值走降级返回 |

---

## 6. 非功能需求

### 6.1 性能指标

#### 6.1.1 基准环境

性能指标必须在以下**最低基准环境**中达标。低于此环境的机器不作为验收依据，但仍需保证不崩溃。

| 项 | 规格 |
| --- | --- |
| CPU | 4 核 @ 2.5GHz（x86_64） |
| 内存 | 8GB |
| 磁盘 | SATA SSD（非机械硬盘、非网络盘） |
| 操作系统 | Windows 10 21H2+ / macOS 12+ / Ubuntu 22.04+ |
| 基准数据集 | **标准库**：3000 篇笔记，平均 2KB，含 15% 附件；**大库**：20000 篇笔记，平均 4KB；**压力库**：100000 篇笔记 |

#### 6.1.2 指标清单

| 指标 ID | 场景 | 标准库目标 | 大库目标 | 压力库目标 |
| --- | --- | --- | --- | --- |
| `NFR-PERF-01` | 冷启动到欢迎页可交互 | < 1.5s | < 1.5s | < 1.5s |
| `NFR-PERF-02` | 打开 Vault 到文件树可用（索引已存在） | < 0.8s | < 1.5s | < 4s |
| `NFR-PERF-03` | 首次全量索引（含分词与 FTS 构建） | < 10s | < 60s | < 8min |
| `NFR-PERF-04` | 全文搜索响应（输入停顿后到结果呈现） | < 100ms | < 200ms | < 500ms |
| `NFR-PERF-05` | 快速打开（`Ctrl+O`）候选呈现 | < 30ms | < 50ms | < 100ms |
| `NFR-PERF-06` | 打开单篇笔记（100KB）到可编辑 | < 100ms | < 100ms | < 100ms |
| `NFR-PERF-07` | 编辑器输入延迟（按键到屏幕呈现） | < 16ms | < 16ms | < 50ms |
| `NFR-PERF-08` | 反向链接面板加载（≤ 500 条） | < 50ms | < 80ms | < 150ms |
| `NFR-PERF-09` | 单文件保存（含原子写入与 fsync） | < 30ms | < 30ms | < 30ms |
| `NFR-PERF-10` | 索引队列单文件增量处理 | < 50ms | < 50ms | < 50ms |
| `NFR-PERF-11` | 外部文件变更到 UI 反映 | < 1s | < 3s | < 5s |
| `NFR-PERF-12` | 全局图谱渲染（≤ 阈值节点） | < 1s | < 2s | 降级（FR-GRAPH-09） |
| `NFR-PERF-13` | 常驻内存占用（标准库，编辑器打开 5 篇） | < 300MB | < 500MB | < 900MB |
| `NFR-PERF-14` | 空闲态 CPU 占用（无操作 30 秒后） | < 1% | < 1% | < 1% |
| `NFR-PERF-15` | 文件树滚动帧率（10 万节点） | — | — | ≥ 50 FPS |
| `NFR-PERF-16` | 重命名 + 链接改写（300 处引用） | < 3s | < 5s | < 15s |

#### 6.1.3 性能约束原则

| 约束 ID | 内容 |
| --- | --- |
| `PERF-01` | **主线程零阻塞**：任何单次 IPC 处理耗时可能 > 16ms 的操作必须移至异步任务，并通过事件推送进度（对应原则 P-3、AC-04；16ms 为一帧预算，> 50ms 的阻塞即红线 R-08 违规） |
| `PERF-02` | **虚拟化强制**：文件树、搜索结果、反链列表、回收站列表、标签笔记列表在数据量 > 200 项时**必须**启用虚拟滚动 |
| `PERF-03` | **懒加载**：图谱视图、设置面板、回收站视图采用路由级懒加载，不计入冷启动关键路径 |
| `PERF-04` | **索引并发**：全量索引使用工作线程池（默认 CPU 核数 - 1，可配置），文件解析并行、DB 写入串行（避免 SQLite 写锁竞争） |
| `PERF-05` | **批量事务**：索引写入按批提交（默认每 200 文件或每 500ms），禁止逐条 `INSERT` 各自开事务 |
| `PERF-06` | **偏好写入防抖**：`preference_set` 在前端侧防抖 500ms 批量提交（FR-SET-03） |
| `PERF-07` | **禁止在渲染路径上做 Markdown 解析建索引**：前端仅做渲染用的 markdown-it 转换，结构化解析只在 Rust 侧发生 |
| `PERF-08` | 全部指标须有自动化基准测试守护；**超出绝对预算或相对基线回退 > 2× 时 CI 失败**（见 §8.4 与勘误 D-15） |

### 6.2 数据可靠性

这是本产品的**最高优先级非功能需求**，直接对应原则 P-2「永不静默破坏用户数据」。

#### 6.2.1 原子写入协议

全部文件写入**必须**遵循以下协议，无例外：

```
1. 生成临时文件：<目标目录>/.kp-tmp-<随机串>.<扩展名>
   （必须与目标同目录，保证 rename 在同一文件系统内，否则非原子）
2. 写入全部内容到临时文件
3. flush + fsync 临时文件（确保数据落盘，而非仅在 OS 缓存）
4. 若目标文件已存在且需备份 → 复制原内容至 .knowlpad/backup/<操作时间戳>/
5. rename 临时文件 → 目标路径（POSIX 下原子；Windows 下需处理目标存在的情况）
6. fsync 父目录（确保目录项本身落盘）
7. 删除残留临时文件（异常路径清理）
```

| 需求 ID | 需求 | 优先级 |
| --- | --- | --- |
| `NFR-REL-01` | 任何时刻进程被强杀或系统断电，磁盘上的笔记文件**必须**是「完整的旧内容」或「完整的新内容」，**绝不允许**出现半截文件、零字节文件或内容交错 | P0 |
| `NFR-REL-02` | Windows 平台的 rename 语义差异必须正确处理：目标存在时先备份再替换，替换失败必须回滚且不丢失原文件 | P0 |
| `NFR-REL-03` | 索引库启用 WAL 模式；进程异常终止后重新打开时 SQLite 能自动恢复，不产生 `database disk image is malformed` | P0 |
| `NFR-REL-04` | 批量改写（链接改写、标签重命名）**必须**全有或全无：开始前备份全部受影响文件，任一失败即回滚全部；**若操作包含重命名/移动，回滚必须同时撤销该重命名**（FR-FILE-22） | P0 |
| `NFR-REL-05` | 备份目录 `.knowlpad/backup/` 保留最近 N 次操作（默认 10，可配置），超出按时间淘汰；但**最近一次批量改写操作的备份不得自动淘汰**（否则 `link_rewrite_rollback` 必然失败），淘汰前须在 UI 中告知用户 | P1 |
| `NFR-REL-06` | 编辑缓冲区快照：编辑中的未保存内容定期（默认 10s）写入 `.knowlpad/` 内的快照文件，崩溃后可恢复（FR-EDITOR-33） | P1 |
| `NFR-REL-07` | 索引重建必须可中断续做：中断后下次打开检测到不完整标记则重新开始，**不得**产生半索引状态被当作有效索引使用 | P0 |
| `NFR-REL-08` | 磁盘空间不足时，写入必须失败并明确报错，**不得**产生截断文件 | P0 |
| `NFR-REL-09` | 数据库连接全部使用连接池或单连接 + 串行化访问，**禁止**多线程无保护并发写同一连接 | P0 |
| `NFR-REL-10` | 长任务（索引、批量改写）必须支持取消，取消后系统处于一致状态 | P0 |

#### 6.2.2 可靠性验收标准

**AC-REL-01｜断电/强杀一致性（核心）**
- Given 自动化测试对 100 篇笔记各执行「写入新内容」操作，并在写入过程中随机时刻强杀进程（重复 1000 次）
- Then 每次重启后检查：全部 100 个文件均为「完整旧内容」或「完整新内容」；无零字节文件；无 `.kp-tmp-*` 残留在用户目录（残留仅在 `.knowlpad/` 内可接受，且启动时清理）；无内容交错或截断

**AC-REL-02｜索引库崩溃恢复**
- Given 索引进行中（约 50% 完成）强杀进程
- When 重新启动并打开 Vault
- Then SQLite 无损坏报错；检测到不完整重建标记后自动重新开始全量索引；索引完成后结果与一次性完整索引**逐表一致**

**AC-REL-03｜索引可丢弃可重建（成功判据 SJ-03）**
- Given 一个已完成索引的 Vault，记录当前全部链接、标签、标题、搜索命中结果作为基线
- When 删除整个 `.knowlpad/` 目录，重新打开 Vault 并完成索引
- Then 重建后的全部数据与基线**逐项一致**（链接数、标签树与计数、标题数、任意 100 个查询的搜索结果集完全相同）
- And 该断言以**回收站为空**为前提；回收站内项目在删除 `.knowlpad/` 时一并消失，测试需分别覆盖两种前提

**AC-REL-04｜磁盘满处理**
- Given 磁盘剩余空间不足以完成写入（通过测试环境模拟）
- When 用户保存笔记
- Then 返回 `E_IO_FAILURE` 并提示检查磁盘空间；原文件内容**完整保留**；临时文件被清理；编辑器中的内容不丢失（用户可另存）

### 6.3 安全设计

**核心前提**：本产品**无插件系统、不执行任何第三方代码**（Out of Scope X-01，原则 P-4）。因此威胁模型相比可扩展架构**根本性简化**——不存在「恶意插件窃取数据」「插件沙箱逃逸」「插件权限提升」等一整类威胁。

安全设计为**四层防护**：

```
第 1 层：进程与权限边界    ← Tauri Capabilities，最小权限
第 2 层：路径与文件系统边界 ← Vault 根限定，路径穿越防护
第 3 层：内容渲染边界      ← DOMPurify 净化，远程资源限制
第 4 层：更新与供应链边界   ← 签名验证，哈希校验，依赖锁定
```

#### 6.3.1 威胁模型

| # | 威胁 | 攻击向量 | 影响 | 防护措施 | 需求 ID |
| --- | --- | --- | --- | --- | --- |
| T-01 | **Markdown 存储型 XSS** | 用户在笔记中写入 `<script>`、`onerror` 等，渲染时执行 | 在 Webview 内执行任意 JS，可读取已加载的笔记内容 | markdown-it 输出经 DOMPurify 3.4.15 净化；禁用 `setConfig()` | SEC-01 |
| T-02 | **DOMPurify 配置绕过** | 使用 `setConfig()` 持久化配置导致 clone-guard 被绕过（3.4.11 前的漏洞） | 净化失效，XSS 复活 | 锁定 3.4.15（≥ 3.4.11 即含修复）；禁止使用 `setConfig()`，配置逐次传参 | SEC-01 |
| T-03 | **路径穿越** | 恶意构造的相对路径（`../../etc/passwd`）通过 IPC 传入 | 读写 Vault 外任意文件 | 全部路径经规范化 + 前缀校验 + 符号链接解析后校验 | SEC-02 |
| T-04 | **符号链接逃逸** | Vault 内放置指向外部目录的符号链接，绕过路径前缀检查 | 读写 Vault 外文件 | 解析 canonical path 后重新校验是否仍在 Vault 内 | SEC-02 |
| T-05 | **markdown-it DoS / ReDoS** | 特制 Markdown（linkify、smartquotes 二次复杂度）导致 CPU 飙升 | 软件无响应 | 锁定 markdown-it 15.0.2（已修复）；单文件大小上限；解析超时保护 | SEC-03 |
| T-06 | **Vite 构建期 ReDoS** | 旧版 Vite 的已知 CVE（CVE-2026-39364、CVE-2026-53571） | 构建环境被拖垮 | 锁定 Vite 8.x（含修复） | SEC-03 |
| T-07 | **远程资源隐私泄露** | 笔记含远程图片 URL，自动加载时暴露用户 IP 与访问时间（追踪像素） | 隐私泄露 | 远程图片默认不加载，需用户显式确认；`referrerPolicy="no-referrer"` | SEC-08 |
| T-08 | **更新包篡改 / 中间人** | 攻击者替换更新源文件或劫持传输 | 用户安装恶意程序（等同 RCE） | RSA/Ed25519 签名验证 + SHA-256 校验 + 强制 HTTPS/TLS 1.2+ | SEC-04 |
| T-09 | **依赖供应链投毒** | 引入被接管的 npm/crates 包 | 构建产物含恶意代码 | lockfile（`pnpm-lock.yaml`/`Cargo.lock`）锁定精确版本与哈希并提交入库 + CI 用 `--frozen-lockfile` 安装 + 审计门禁（`pnpm audit` / `cargo audit`）+ 弃用停更包 | SEC-06 |
| T-10 | **IPC 权限过宽** | Tauri Capabilities 配置为通配全量权限，一旦有 XSS 即可调用任意原生能力 | XSS 升级为本地文件读写 | 最小权限白名单；**禁止**授予 `fs:allow-*`；全部文件操作走自定义 Command | SEC-05 |
| T-11 | **Webview 导航劫持** | 笔记中的链接诱导 Webview 导航至外部恶意站点 | 钓鱼、脱离应用上下文 | 拦截 Webview 导航，外部 URL 一律交由系统默认浏览器打开 | SEC-07 |
| T-12 | **日志泄露隐私** | 日志中记录笔记正文或完整路径 | 隐私泄露 | 日志仅记录错误码、相对路径、操作类型；**禁止**记录笔记正文 | SEC-09 |
| T-13 | **索引库被复制误用** | 用户将 `.knowlpad/` 复制到另一 Vault，索引与实际文件不匹配 | 数据展示错乱 | 索引签名包含 Vault 绝对路径，不匹配即重建 | SEC-10 |
| T-14 | **超大文件资源耗尽** | 打开/索引超大文件（如 500MB）导致 OOM | 软件崩溃 | 单文件大小上限（默认 5MB），超限跳过并警告 | SEC-11 |
| T-15 | **批量改写数据损坏** | 链接改写过程中失败导致部分文件被改、部分未改 | 知识库链接关系损坏 | 备份 + 预览 + 全有或全无 + 失败回滚 | SEC-12 |

> **威胁模型完整性说明**：以上 15 项覆盖本产品实际攻击面。由于不存在插件系统，**无需**考虑插件沙箱逃逸、插件权限提升、第三方代码执行、插件市场投毒等威胁类别。若未来版本引入插件机制，本节必须整体重新评估。

#### 6.3.2 安全需求

| 需求 ID | 需求 | 对应威胁 | 优先级 |
| --- | --- | --- | --- |
| `SEC-01` | 全部由 Markdown 生成的 HTML **必须**经 `DOMPurify.sanitize()` 净化后才能进入 DOM。净化配置**必须**在每次调用时以参数传入，**禁止**使用 `DOMPurify.setConfig()`。白名单外标签与属性一律移除 | T-01, T-02 | P0 |
| `SEC-02` | 全部接收路径参数的 Command **必须**执行路径安全校验：① 拒绝绝对路径（除 Vault 注册接口）② 规范化后校验仍在 Vault 根内 ③ 解析符号链接的 canonical path 后**再次**校验 ④ 拒绝含 `\0` 的路径。校验失败返回 `E_PATH_OUTSIDE_VAULT` 并记入日志 | T-03, T-04 | P0 |
| `SEC-03` | 锁定 `markdown-it` ≥ 15.0.2、`Vite` ≥ 8.x、`DOMPurify` ≥ 3.4.12（**此为安全下限**，即修复对应漏洞的最低版本；当前实际锁定值见技术方案 §3.2.4，为 3.4.15）。单文件解析设置大小上限（默认 5MB）与超时保护（默认 10s），超限跳过并记录警告 | T-05, T-06, T-14 | P0 |
| `SEC-04` | 更新包**必须**通过签名验证（公钥固化于 `tauri.conf.json`）与 SHA-256 校验，任一失败即中止安装并删除下载文件。更新源**必须**为 HTTPS | T-08 | P0 |
| `SEC-05` | Tauri Capabilities 采用最小权限白名单：仅授予本项目自定义 Command 与必需的 `core:` 能力（path、event、window、updater、dialog、shell-open）。**明确禁止** `fs:allow-*`、`shell:allow-execute`/`shell:allow-spawn`、`process:allow-exit` 等宽权限；`process:` 仅允许 `process:allow-restart`（更新后重启，见技术方案 §9.1.1） | T-10 | P0 |
| `SEC-06` | 依赖采用「**声明层面兼容、安装层面锁定**」策略：`package.json` 声明向后兼容范围，实际版本由 `pnpm-lock.yaml` 锁定；`Cargo.toml` 声明范围，`Cargo.lock` 锁定。**两个 lockfile 必须提交入库**，CI 与生产一律用 `pnpm install --frozen-lockfile`（禁止用会自行解析新版本的 `pnpm install` / `npm install`）。依赖升级为显式动作，lockfile 变更单独成 commit。CI 中执行 `pnpm audit --audit-level=high` 与 `cargo audit`，发现高危漏洞即构建失败（详见技术方案 §3.6.1） | T-09 | P0 |
| `SEC-07` | Webview 导航拦截：应用内**禁止**导航至非 `tauri://` / `http://tauri.localhost` 的 URL。笔记中的外部链接一律调用系统默认浏览器打开（经 `shell:allow-open` 且限定 `https?://` 协议） | T-11 | P0 |
| `SEC-08` | 远程资源在应用内**一律不加载**：Markdown 中的 `http(s)://` 图片渲染为占位符并显示完整域名；点击占位符由**系统默认浏览器**打开该 URL，应用进程不发起请求。`<img>` 标签设置 `referrerPolicy="no-referrer"` | T-07 | P0 |
| `SEC-09` | 日志隐私：日志**禁止**记录笔记正文、frontmatter 内容、搜索查询词。允许记录：错误码、相对路径、操作类型、耗时、文件大小 | T-12 | P0 |
| `SEC-10` | 索引签名包含 Vault 绝对路径（§3.5），签名不匹配即触发全量重建 | T-13 | P0 |
| `SEC-11` | 资源上限：单文件解析大小上限（默认 5MB）、单 Vault 文件数软上限（超出提示但不阻止）、图谱节点阈值（默认 3000） | T-14 | P0 |
| `SEC-12` | 批量改写安全：改写前**必须**成功创建全部受影响文件的备份，备份失败则中止操作（返回 `E_BACKUP_FAILED`）；改写失败**必须**回滚全部已改文件 | T-15 | P0 |
| `SEC-13` | CSP 配置：`tauri.conf.json` 中设置严格 CSP，`default-src 'self'`；`img-src 'self' asset: https://asset.localhost`（远程图片经用户确认后按需放行）；`script-src 'self'`；**禁止** `unsafe-inline` 与 `unsafe-eval` | T-01, T-11 | P0 |
| `SEC-14` | 生产构建**必须**禁用 devtools、禁用 source map 公开分发、Tauri `devUrl` 不出现在生产包中 | T-10 | P0 |
| `SEC-15` | `.knowlpad/` 目录权限设为仅当前用户可读写（Windows ACL / POSIX 0700） | T-13 | P1 |
| `SEC-16` | **零遥测**：软件**不得**包含任何形式的数据上报、崩溃自动上传、使用统计。唯一的对外网络请求是更新检查与更新包下载 | 隐私承诺 | P0 |

#### 6.3.3 安全验收标准

**AC-SEC-01｜路径穿越全面拦截**
- Given 一组恶意路径载荷：`../../../etc/passwd`、`..\\..\\windows\\system32\\config`、`folder/../../outside.md`、`%2e%2e%2f`、含 `\0` 的路径、指向 Vault 外的符号链接
- When 分别通过 `note_read`、`note_write`、`file_delete` 传入
- Then 全部返回 `E_PATH_OUTSIDE_VAULT` 或 `E_PATH_ESCAPE_DENY`；磁盘上无任何 Vault 外文件被读取、创建、修改或删除；事件记入日志

**AC-SEC-02｜Capabilities 最小权限审计**
- Given 生产构建产物
- When 检查 `tauri.conf.json` 与 capabilities 配置文件
- Then 不含 `fs:allow-*`、`shell:allow-execute` 等宽权限；`process:` 仅含 `process:allow-restart`；`shell:allow-open` 限定 `https://` 与 `mailto:`（**不含** `http://` 与 `file://`）；本地文件「在文件管理器中显示」由**自定义 Command 在 Rust 侧**调用 opener 实现，不额外授予前端能力（AC-01）；CSP 的 `script-src` 无 `unsafe-inline`、`unsafe-eval`

**AC-SEC-03｜零网络请求验证（隐私关键）**
- Given 软件在完全隔离的网络监控环境运行，打开含 100 篇笔记（含远程图片链接）的 Vault，执行编辑、搜索、图谱、标签等全部常规操作 10 分钟
- When 检查网络流量
- Then **除更新检查外零对外请求**；远程图片未被加载；无任何遥测、统计、崩溃上报流量

**AC-SEC-04｜依赖审计门禁**
- Given CI 流水线
- When 执行构建
- Then `pnpm audit --audit-level=high` 与 `cargo audit` 均通过；lockfile 与 manifest 一致（`pnpm install --frozen-lockfile`）；发现高危 CVE 时流水线失败并阻止发布

**AC-SEC-05｜更新签名验证**
- Given 更新源提供的包签名与固化公钥不匹配（模拟篡改）
- When 客户端执行更新
- Then 返回 `E_UPDATE_SIGNATURE`；显示安全警告；下载文件被删除；**不执行**任何安装动作；当前版本不受影响

### 6.4 跨平台适配

| 需求 ID | 需求 | 优先级 |
| --- | --- | --- |
| `NFR-PLAT-01` | 支持平台：Windows 10 21H2+（x64、arm64）、macOS 12+（Intel、Apple Silicon）、Linux（Ubuntu 22.04+ / Debian 12+ / Fedora 38+，x64） | P0 |
| `NFR-PLAT-02` | 路径分隔符：内部统一使用 `/`；调用 OS API 前转换为平台分隔符。**禁止**在业务逻辑中硬编码 `\` 或 `/` | P0 |
| `NFR-PLAT-03` | 文件名大小写：Windows/macOS 默认大小写不敏感、Linux 敏感。链接匹配统一采用大小写不敏感（MD-WL-04），避免跨平台行为差异导致链接失效 | P0 |
| `NFR-PLAT-04` | 非法文件名字符集按平台取**最严格并集**校验（FR-FILE-12），保证 Vault 可在三平台间自由拷贝 | P0 |
| `NFR-PLAT-05` | 换行符：读取时兼容 `\n` 与 `\r\n`；写入时保持文件原有换行风格（新建文件按平台默认） | P0 |
| `NFR-PLAT-06` | 文件编码：统一 UTF-8。读取时检测 BOM 并保留原样；检测到非 UTF-8 编码文件时给出警告并按 UTF-8 有损读取，**不得**静默损坏 | P0 |
| `NFR-PLAT-07` | 快捷键：`Ctrl` 在 macOS 上映射为 `Cmd`。全部快捷键定义集中管理，按平台生成显示文本 | P0 |
| `NFR-PLAT-08` | 原生菜单：macOS 提供标准应用菜单（含 About、Preferences、Services、Hide、Quit）；Windows/Linux 提供窗口菜单或省略 | P1 |
| `NFR-PLAT-09` | 文件监听：三平台使用各自原生机制（Windows ReadDirectoryChangesW / macOS FSEvents / Linux inotify），通过 `notify` crate 统一抽象。inotify watch 数量不足时给出明确提示与解决建议 | P0 |
| `NFR-PLAT-10` | 长路径：Windows 下支持超过 260 字符的路径（使用 `\\?\` 前缀或启用长路径清单） | P1 |
| `NFR-PLAT-11` | 高 DPI：三平台在 100%/150%/200% 缩放下布局正常、文字清晰 | P1 |
| `NFR-PLAT-12` | 安装包格式：Windows `.msi` + `.exe`(NSIS)、macOS `.dmg` + `.app`、Linux `.deb` + `.AppImage` | P0 |
| `NFR-PLAT-13` | macOS 签名与公证：若分发至非开发者机器，需处理 Gatekeeper。**V1.0 若无 Apple 开发者账号，必须在 README 与安装说明中明确告知用户绕过方式及其风险，不得隐瞒** | P1 |
| `NFR-PLAT-14` | Linux AppImage 需正确集成 FUSE；若不可用则给出降级说明 | P2 |
| `NFR-PLAT-15` | 全部平台差异代码集中在 `src-tauri/src/platform/`，业务层通过 trait 抽象调用（对应 RS 模块约束） | P0 |

#### 6.4.1 跨平台验收清单

每个平台发布前**必须**逐项通过：

| # | 验收项 | Win | macOS | Linux |
| --- | --- | --- | --- | --- |
| 1 | 安装包可正常安装与卸载 | ☐ | ☐ | ☐ |
| 2 | 首次启动显示欢迎页，无 CSP/白屏错误 | ☐ | ☐ | ☐ |
| 3 | 新建 Vault 并创建 `.knowlpad/` | ☐ | ☐ | ☐ |
| 4 | 打开含 3000 篇笔记的标准库，索引成功 | ☐ | ☐ | ☐ |
| 5 | 中文全文搜索命中正确 | ☐ | ☐ | ☐ |
| 6 | 编辑器输入无可感知延迟，自动保存生效 | ☐ | ☐ | ☐ |
| 7 | `[[` 链接补全可用，`Ctrl/Cmd+点击` 跳转正常 | ☐ | ☐ | ☐ |
| 8 | 重命名笔记后 300 处链接正确改写 | ☐ | ☐ | ☐ |
| 9 | 知识图谱渲染与交互正常 | ☐ | ☐ | ☐ |
| 10 | 外部文件变更（文件管理器操作）被感知 | ☐ | ☐ | ☐ |
| 11 | 回收站删除与恢复正常 | ☐ | ☐ | ☐ |
| 12 | 强杀进程后重启，文件无损坏、索引可恢复 | ☐ | ☐ | ☐ |
| 13 | 快捷键符合平台习惯（macOS 用 Cmd） | ☐ | ☐ | ☐ |
| 14 | 高 DPI（150%）下布局正常 | ☐ | ☐ | ☐ |
| 15 | 深色/浅色主题切换正常 | ☐ | ☐ | ☐ |
| 16 | 系统文件管理器「显示」功能正常 | ☐ | ☐ | ☐ |
| 17 | 自动更新流程可走通（或明确标注该平台暂不支持） | ☐ | ☐ | ☐ |
| 18 | 长路径（> 260 字符）可正常读写 | ☐ | n/a | n/a |
| 19 | Gatekeeper 提示已妥善处理或已明确告知用户 | n/a | ☐ | n/a |
| 20 | AppImage 在无 FUSE 环境下有降级说明 | n/a | n/a | ☐ |

---

## 7. 页面与路由

### 7.1 视图清单

| 视图 ID | 名称 | 路由 | 说明 | 懒加载 |
| --- | --- | --- | --- | --- |
| `V-WELCOME` | 欢迎页 / Vault 选择 | `/welcome` | 首次启动或无 Vault 打开时 | 否 |
| `V-WORKSPACE` | 主工作区（布局容器） | `/` | 侧边栏 + 编辑区 + 右侧面板的三栏容器 | 否 |
| `V-EDITOR` | 笔记编辑器 | `/note/:relPath*` | 主内容区，支持多标签 | 否 |
| `V-GRAPH` | 知识图谱 | `/graph` | 全局/局部图谱 | 是 |
| `V-SEARCH` | 搜索结果页 | `/search` | 全文搜索结果（侧边栏面板亦可） | 是 |
| `V-TAG` | 标签视图 | `/tag/:tagNorm*` | 某标签下的笔记列表 | 是 |
| `V-TRASH` | 回收站 | `/trash` | 已删除项管理 | 是 |
| `V-SETTINGS` | 设置 | `/settings/:section?` | 设置面板 | 是 |
| `V-ABOUT` | 关于 | `/settings/about` | 版本、许可、更新信息 | 是 |

### 7.2 主工作区布局

```
┌────────────────────────────────────────────────────────────┐
│  标题栏（Vault 名称 · 标签页 · 窗口控制）                    │
├──────────┬─────────────────────────────────┬───────────────┤
│          │                                 │               │
│  左侧栏   │        编辑区（多标签）          │   右侧面板     │
│          │                                 │               │
│ · 文件树  │  ┌───────────────────────────┐  │ · 反向链接     │
│ · 搜索    │  │  标签页 1 │ 标签页 2 │ ... │  │ · 出链         │
│ · 标签    │  ├───────────────────────────┤  │ · 大纲(V1.1)   │
│ · 书签    │  │                           │  │ · 标签         │
│  (V1.1)  │  │      Markdown 编辑/预览    │  │ · 属性         │
│          │  │                           │  │               │
├──────────┴─────────────────────────────────┴───────────────┤
│  状态栏（索引状态 · 字数 · 光标位置 · 缩放）                  │
└────────────────────────────────────────────────────────────┘
```

| 需求 ID | 需求 | 优先级 |
| --- | --- | --- |
| `FR-LAYOUT-01` | 左侧栏与右侧面板均可折叠，宽度可拖拽调整，状态记忆于 `vault_state` | P0 |
| `FR-LAYOUT-02` | 左侧栏图标栏（Ribbon）：文件树、搜索、标签、图谱、回收站、设置的快速入口 | P0 |
| `FR-LAYOUT-03` | 右侧面板按当前笔记上下文切换内容（反链/出链/标签/属性） | P0 |
| `FR-LAYOUT-04` | 窄窗口（< 900px）下自动折叠侧栏，优先保证编辑区可用 | P1 |
| `FR-LAYOUT-05` | 布局状态在软件重启后完整恢复（折叠态、宽度、打开的标签页、滚动位置） | P1 |

### 7.3 路由守卫

| 约束 ID | 内容 |
| --- | --- |
| `RT-01` | 除 `/welcome` 外，全部路由**必须**有打开的 Vault；否则重定向至 `/welcome` |
| `RT-02` | 路由参数中的 `relPath` **必须**经前端校验（不含 `..`、不以 `/` 开头）后再发起 IPC；Rust 侧仍独立执行 SEC-02 校验（**不依赖**前端校验作为安全边界） |
| `RT-03` | 切换笔记路由时，若当前笔记有未保存内容，先触发保存或提示 |
| `RT-04` | 懒加载视图的 chunk 加载失败时给出重试入口，不白屏 |

---

## 8. 开发规约

### 8.1 代码规范

| 约束 ID | 内容 |
| --- | --- |
| `CODE-01` | TypeScript **必须**开启 `strict`（TS 6.0 默认）。**禁止**使用 `any`，确需时用 `unknown` + 类型收窄 |
| `CODE-02` | Rust **必须**通过 `cargo clippy -- -D warnings`，无 warning |
| `CODE-03` | 前端遵循 ESLint（`eslint-plugin-vue` recommended + TS 规则）与 Prettier；提交前经 lint-staged 检查 |
| `CODE-04` | Vue 组件使用 `<script setup lang="ts">` 组合式 API。**禁止**选项式 API 与混入（mixin） |
| `CODE-05` | 状态管理使用 Pinia 4.x，store 按 feature 划分。**禁止**用全局事件总线替代应有的 store 状态 |
| `CODE-06` | 命名遵循 §0.2 规范。文件名：Vue 组件 `PascalCase.vue`，TS 模块 `kebab-case.ts`，Rust 模块 `snake_case.rs` |
| `CODE-07` | **禁止**提交 `console.log` / `dbg!` / `println!` 到生产代码（调试日志走统一 logger） |
| `CODE-08` | 每个 feature 模块与 Rust domain 模块必须有 `README.md` 说明职责边界（对应 FE/RS 约束） |
| `CODE-09` | Git 提交遵循 Conventional Commits（见技术方案 §9）；**禁止**直接推送到 `main`，须经 PR + Code Review |
| `CODE-10` | 代码注释解释「为什么」而非「做什么」；复杂算法（链接改写、分词、索引一致性）必须有设计说明注释 |
| `CODE-11` | 单文件行数限制：**警告阈值 200 行**，**硬性上限 350 行**。超出警告阈值时 linter 报 warning；超出硬性上限时 CI 门禁报 error 并拒绝合并。Rust 文件同样适用（含 `mod.rs`）。测试文件与 `schema.rs`（DDL 定义）豁免硬性上限 |

### 8.2 禁止事项（红线）

以下为**不可逾越的红线**，违反即视为缺陷：

| # | 禁止事项 | 关联约束 |
| --- | --- | --- |
| R-01 | 禁止前端直接读写文件（绕过自定义 Command） | AC-01 |
| R-02 | 禁止前端引入 WASM SQLite 或直连数据库 | AC-02 |
| R-03 | 禁止在索引库存储无法从 Markdown 重建的数据 | AC-06 |
| R-04 | 禁止用 `v-html` 渲染未净化的 HTML | AC-05, SEC-01 |
| R-05 | 禁止使用 `DOMPurify.setConfig()` | SEC-01 |
| R-06 | 禁止非原子的文件写入（直接 `fs::write` 覆盖用户文件） | NFR-REL-01 |
| R-07 | 禁止静默覆盖用户数据（重名、外部冲突、批量改写） | P-2, FR-FILE-13 |
| R-08 | 禁止在 IPC/主线程执行同步耗时 > 16ms 的操作；任何 > 50ms 的阻塞（含 IO/锁等待）即视为缺陷（一帧预算与异步化触发线见 AC-04） | AC-04, PERF-01 |
| R-09 | 禁止硬编码平台路径或分隔符 | NFR-PLAT-02 |
| R-10 | 禁止授予 Tauri 宽泛文件/shell 权限 | SEC-05 |
| R-11 | 禁止加入任何遥测、统计、崩溃自动上报 | SEC-16 |
| R-12 | 禁止在日志中记录笔记正文或搜索词 | SEC-09 |
| R-13 | 禁止自动加载远程资源 | SEC-08 |
| R-14 | 禁止引入插件系统或任何第三方代码执行机制 | X-01, P-4 |
| R-15 | 禁止 `catch {}` 空吞错误 | ERR-01 |
| R-16 | 禁止使用已停更的依赖（如 `vuedraggable`） | SEC-06 |
| R-17 | 禁止在 CI / 生产环境用会自行解析新版本的安装命令（`pnpm install`、`npm install`）；必须用 `pnpm install --frozen-lockfile`。禁止把 lockfile 变更混入功能提交 | SEC-06 |

### 8.3 测试策略

| 测试层级 | 范围 | 工具 | 覆盖率要求 |
| --- | --- | --- | --- |
| **单元测试** | Rust `domain/` 纯函数（解析器、分词、链接匹配、路径校验）；前端 `core/utils` 纯函数 | `cargo test` + `cargo llvm-cov` / `vitest` | Rust `domain/` 行覆盖率 ≥ 85%（`cargo llvm-cov`，门禁 6 强制，见技术方案 §11.1/DEBT-10）；前端 `core/utils` ≥ 90%（vitest 目录级阈值，见技术方案 §11.7.6） |
| **组件测试** | Vue 组件渲染与交互 | `@vue/test-utils` + `vitest` | 核心组件（编辑器、文件树、搜索、反链面板）必测 |
| **集成测试** | IPC Command 端到端（Rust 侧 + 真实临时 Vault） | `cargo test`（集成测试目录） | 全部 62 个 Command 至少 1 个正向 + 1 个错误用例 |
| **契约测试** | 前后端类型一致性（TS interface ↔ Rust struct） | 自动生成的类型比对脚本 | 100% Command 覆盖 |
| **E2E 测试** | 关键用户旅程（打开 Vault → 编辑 → 链接 → 搜索 → 重命名改写 → 回收站恢复） | `tauri-driver` + `WebDriverIO` | §1.3 全部 10 个核心场景 |
| **安全测试** | §6.3.3 全部 AC-SEC 用例（路径穿越、XSS、签名、零网络请求） | 自动化脚本 + 网络监控 | 100% 通过为发布门禁 |
| **可靠性测试** | §6.2.2 断电/强杀一致性、索引重建一致性 | 故障注入脚本 | AC-REL-01~04 全通过为发布门禁 |
| **性能基准** | §6.1.2 全部指标 | 自动化基准 + CI 记录 | 超预算或回退 > 2× 时 CI 失败 |
| **扩展语法解析测试** | Markdown 扩展语法用例集（附录 B） | 结构化夹具 + 断言 | 解析准确率 ≥ 99%（SJ-04） |
| **跨平台验收** | §6.4.1 清单 × 3 平台 | 人工 + 自动混合 | 发布前逐项通过 |

| 约束 ID | 内容 |
| --- | --- |
| `TEST-01` | **解析器测试优先**：`domain/md_parse.rs` 是正确性根基，必须建立包含边界情况的用例集（见附录 B），每个 wikilink/tag/frontmatter 规则（§3.1）至少一个正例一个反例 |
| `TEST-02` | 安全测试与可靠性测试为**发布门禁**，不通过不得发布 |
| `TEST-03` | 每个 bug 修复必须附带复现该 bug 的回归测试 |
| `TEST-04` | 性能基准在 CI 中每次构建记录，形成趋势；显著回退自动阻断 |
| `TEST-05` | 测试**禁止**依赖网络（除更新模块的 mock）；使用临时目录构造 Vault，测试后清理 |

### 8.4 CI 门禁

合并到 `main` 前，CI **必须**全部通过。**门禁的唯一可执行真相源是 `scripts/run-gates.mjs`**（本地与 CI 跑同一脚本，共 **17** 项；下表 1–13 为 PRD 基准门禁，14–17 为已落地的扩展门禁，执行顺序以脚本为准）：

| # | 门禁项 | 命令 | 失败后果 |
| --- | --- | --- | --- |
| 1 | TypeScript 类型检查 | `vue-tsc --noEmit` | 阻断 |
| 2 | 前端 Lint | `eslint . --max-warnings 0` | 阻断 |
| 3 | Rust Lint | `cargo clippy -- -D warnings` | 阻断 |
| 4 | Rust 格式 | `cargo fmt --check` | 阻断 |
| 5 | 前端单元/组件测试 | `vitest run --coverage` | 阻断（低于覆盖率要求） |
| 6 | Rust 单元/集成测试 | `cargo test` | 阻断（低于覆盖率要求） |
| 7 | 依赖安全审计 | `pnpm audit --audit-level=high` + `cargo audit` | 阻断（高危漏洞） |
| 8 | Lockfile 一致性 | `pnpm install --frozen-lockfile --dry-run`（校验 lockfile 与 `package.json` 一致，不一致即失败；M0 实测 pnpm 12.4.2 支持 `--dry-run`；策略见技术方案 §3.6.1） | 阻断 |
| 9 | 生产构建 | `pnpm run tauri build` | 阻断（构建失败） |
| 10 | 性能基准 | 自动化基准脚本 | 阻断（超绝对预算，或相对基线回退 > 2×；勘误 D-15） |
| 11 | 安全测试集 | AC-SEC 自动化用例 | 阻断 |
| 12 | 可靠性测试集 | AC-REL 故障注入 | 阻断 |
| 13 | 命名一致性 | grep 扫描 §0.2 禁止写法（排除规范说明段落自身） | 阻断 |
| 14 | 前端构建 + 完整性 | `pnpm build:verify`（校验 chunk 引用全部存在且无零字节产物） | 阻断 |
| 15 | Rust domain 覆盖率 ≥ 85% | `cargo llvm-cov -p kp-domain --fail-under-lines 85` | 阻断（低于阈值） |
| 16 | 路径封装检查 | `pnpm gate:path-encapsulation`（SEC PATH-02） | 阻断 |
| 17 | IPC 契约一致性（TS↔Rust） | `node scripts/verify-ipc-contract.mjs`（TS 调用的命令名必须都存在同名 `#[tauri::command]`） | 阻断 |

---

## 9. 实施路线

项目状态：**M0 / M1 / M2 已完成**（详见 `docs/AGENTS.md` §10 与 `docs/history/`）。按以下里程碑推进，每个里程碑有明确的阶段验收标准（Definition of Done）。

### 9.1 里程碑规划

| 里程碑 | 目标 | 主要交付 | 阶段验收（DoD） |
| --- | --- | --- | --- |
| **M0 工程初始化** | 搭建可运行的空壳应用 | Tauri 2 + Vue 3 + TS 6 + Vite 8 脚手架；CI 流水线；Capabilities 最小权限配置；目录结构（§2.5）；错误类型与 IPC 封装骨架 | 三平台能启动空窗口；CI 全绿；`invoke` 一个 ping Command 往返成功 |
| **M1 存储与 Vault** | 数据层地基 | 全局库/索引库 schema 与迁移；Vault 打开/创建/切换；路径安全校验（SEC-02）；原子写入协议（§6.2.1） | **AC-VAULT-02/03/04 全通过**；AC-VAULT-01 的**存储相关部分**（不修改第三方配置目录、`.knowlpad/` 正确创建、文件计数正确）；AC-REL-01 强杀一致性通过；路径穿越测试全拦截。⚠️ AC-VAULT-01 的**解析断言**与 **AC-VAULT-05** 顺延至 M3（见 D-08） |
| **M2 文件树与基础编辑** | 能看能改 | 文件树（虚拟滚动）；文件 CRUD；`md-editor-v3` 集成；自动保存；多标签；**DOMPurify 净化管线就位**（SEC-01，安全关键特性不推迟） | AC-FILE-01~05 全通过；AC-EDITOR-01/02 通过；AC-FILE-06 与 AC-EDITOR-05/06 为**近似口径**（**勘误 D-17**） |
| **M3 解析与索引引擎** | 知识网络的数据基础 | Markdown 解析器（§3.1 全部规则）；jieba 分词；FTS5；全量/增量索引；文件监听；索引签名 | AC-SEARCH-01/04；解析器测试集（附录 B）准确率 ≥ 99%；AC-REL-03 索引重建一致 |
| **M4 链接与反链** | 核心差异化能力 | 反链/悬空/歧义/孤立**面板**；链接改写（预览+备份+原子+回滚）——**链接裁决本身在 M3 的索引阶段完成**（见勘误 D-20） | AC-LINK-01~05；AC-FILE-01/02（改写与回滚）全通过 |
| **M5 搜索与标签** | 检索能力完整 | 全文搜索 UI 与高级语法；快速打开；标签树与标签视图；标签重命名 | AC-SEARCH 全通过；AC-TAG-01~04 全通过 |
| **M6 图谱与附件** | 可视化与多媒体 | cytoscape 全局/局部图谱；降级策略；附件导入/粘贴/渲染；远程资源限制 | AC-GRAPH-01~04；AC-ATTACH-01~03；性能 NFR-PERF-12 达标 |
| **M7 安全加固与回收站** | 安全闭环 | CSP 加固；导航拦截；回收站完整流程；日志隐私；安全测试集 | AC-SEC-02~05 全通过；AC-TRASH-01~04 全通过 |
| **M8 编辑器正式版** | 编辑体验达成 | CodeMirror 6 替换 md-editor-v3；Live Preview；悬停预览；外部冲突处理 | AC-EDITOR-01/03/04/07 通过；NFR-PERF-07 输入延迟达标 |
| **M9 更新与发布** | 可交付 | 自动更新（签名+校验）；semantic-release；多平台打包；跨平台验收 | AC-UPDATE-01~05；§6.4.1 三平台清单逐项通过 |
| **M10 稳定版 V1.0** | 首个稳定版 | 全部 P0 完成；成功判据 SJ-01~06 全通过；文档齐备 | 发布 v1.0.0 |

### 9.2 关键路径与并行

```
M0 ──► M1 ──► M2 ──► M3 ──► M4 ──► M5 ──► M8 ──► M10
                │              │              ▲
                └──► M6 ◄──────┘              │
                └──► M7 ──────────────────────┘
                                  M9 ─────────►┘
```

- **关键路径**：M0→M1→M2→M3→M4→M5→M8→M10（数据层与链接是根基，必须串行）
- **可并行**：M6（图谱/附件）、M7（安全/回收站）在 M3 之后可与 M4/M5 并行；M9（更新/发布）在 M2 之后即可起步
- **风险提示**：M3 解析器与 M4 链接改写是**最高技术风险**环节，建议投入最强人力并优先建立测试集

### 9.3 V1.1 候选（不承诺）

以下项目在 V1.0 明确排除（§1.4），列为 V1.1 候选，**不构成本版承诺**：每日笔记、日历视图、文档大纲面板、导出 PDF/HTML、书签、内置 AI（需重新评估隐私边界）。

---

## 10. 风险登记册与未决问题

### 10.1 技术风险

| 风险 ID | 风险 | 可能性 | 影响 | 缓解措施 | 责任阶段 |
| --- | --- | --- | --- | --- | --- |
| `RISK-01` | **Vite 8 生产回归**：v5 已标注字符串枚举别名编译错误、`writeBundle` 非确定性遗漏 chunk、DevBundle 崩溃等问题 | 高 | 高 | **已决策不回退**（2026-09-21，`OPEN-01` 关闭）。三项回归的规避手段升级为硬性门禁：禁用 `const enum`（ESLint 拦截）、禁用自定义 `writeBundle` 插件 + CI 连续 3 次构建比对产物清单 + chunk 完整性校验（门禁 9）、开发期崩溃用 `vite build --watch` 绕过。若出现无法规避的阻断缺陷，作为独立高优先级决策项上报重审，**不静默降级**（技术方案 §3.4.3） | M0 |
| `RISK-02` | **TypeScript 6 生态兼容**：TS 6 默认值变更（strict/types/module/target/rootDir）可能导致工具链或依赖类型报错 | 中 | 中 | 按技术方案 §3.3.2 tsconfig 配置；用官方 `ts5to6` 迁移工具；`ignoreDeprecations: "6.0"` 缓冲；M0 验证 `vue-tsc ^3.3.11` 与 `typescript-eslint ^8` 兼容性。**刻意不升 TS 7**（上游 latest 已是 7.0.2 的 Go 重写版），理由见技术方案 §3.2.1 | M0 |
| `RISK-03` | **jieba-rs 分词质量**：版本已锁定 `0.11.0`（2026-09-19 核实，技术方案 §3.5.2）；`OPEN-02` 已决为词典全量内置，体积不再构成约束。**剩余风险仅为分词质量**——切分粒度直接影响中文搜索的召回与排序 | 中 | 中 | M3 建立中文分词测试集实测质量；调用 `cut(text, false)` 关闭 HMM 新词发现（**M0 实测：jieba-rs 0.11.0 无 `hmm` cargo feature**，HMM 只能在调用时关闭）以保证索引可重建与两次结果一致（PRD P-1、AC-REL-03）；分词器版本纳入索引签名 | M3 |
| `RISK-04` | **中文 FTS5 相关性**：jieba + unicode61 方案的分词粒度可能影响搜索召回与排序质量 | 中 | 中 | M3 建立中文搜索测试集调优；提供 AND/OR 匹配模式；必要时评估 FTS5 自定义分词器（成本高，列为后备） | M3 |
| `RISK-05` | **链接改写的正确性**：批量改写 300+ 处引用，边界情况多（代码块、别名、锚点、嵌套链接、跨平台换行） | 高 | 高 | 改写器必须是纯函数并有穷尽测试；强制备份+预览+全有或全无+回滚（SEC-12）；M4 重点投入 | M4 |
| `RISK-06` | **大 Vault 性能**：10 万文件下文件树、索引、图谱的性能与内存 | 中 | 高 | 虚拟滚动强制（PERF-02）；索引并发（PERF-04）；图谱降级（FR-GRAPH-09）；压力库基准测试 | M2/M3/M6 |
| `RISK-07` | **CodeMirror 6 Live Preview 复杂度**：自研所见即所得体验工作量大、边界多 | 高 | 中 | MVP 先用 md-editor-v3 保底（§4.3 分阶段）；Live Preview 列为 M8 且 P1，必要时降级为「分屏预览」发布。**代码高亮不引入第三方库**：M8 起编辑态用 `syntaxHighlighting()`、阅读态用 `@lezer/highlight` 的 `highlightCode()`，统一出自 CodeMirror 6 / Lezer 体系（技术方案 §3.2.4） | M8 |
| `RISK-08` | **跨平台文件监听差异**：inotify watch 上限、FSEvents 延迟、Windows 事件风暴 | 中 | 中 | `notify` crate 统一抽象；去抖动归并（EVT-01）；inotify 不足时明确提示；三平台各自测试 | M3 |
| `RISK-09` | **原子写入的 Windows 语义**：Windows rename 目标存在时的行为与 POSIX 不同 | 中 | 高 | §6.2.1 协议明确处理 Windows 分支；AC-REL-01 在 Windows 上重点测试 | M1 |
| `RISK-10` | **自动更新平台差异**：macOS 无开发者账号时的 Gatekeeper、Linux 各发行版包管理、updater 不支持增量与自动回滚 | 中 | 中 | V1.0 限定全量更新；更新失败保持当前版本（FR-UPDATE-08）；macOS 签名问题如实告知用户（NFR-PLAT-13） | M9 |

### 10.2 未决问题（需决策）

| 编号 | 问题 | 当前倾向 | 需决策时点 |
| --- | --- | --- | --- |
| ~~`OPEN-01`~~ ✅ **已决**（2026-09-21） | ~~Vite 8 还是回退 Vite 6.4.3？~~ | **确定采用 Vite 8，不设回退路径**。理由：回退会重新引入 ReDoS 安全风险、双轨配置维护成本高、回退窗口仅在 M0。三项生产回归的规避手段升级为硬性门禁（见 RISK-01、技术方案 §3.4.3） | — |
| ~~`OPEN-02`~~ ✅ **已决**（2026-09-20） | ~~jieba-rs 词典打包方式？~~ | **全量内置**（约 5MB），保证完全离线可用与开箱即用的分词质量。当前阶段**不以打包体积为约束条件**，故不采用精简词典或按需下载；`DEBT-03` 随之关闭 | — |
| `OPEN-03` | 编辑器正式版是否必须在 V1.0 完成 Live Preview？ | 不强制；可先发分屏预览版，Live Preview 延至 V1.1 | M8 启动前 |
| `OPEN-04` | macOS 是否购买 Apple 开发者账号做签名公证？ | 视分发范围决定；无账号则明确告知用户绕过方式 | M9 启动前 |
| `OPEN-05` | 是否支持 Vault 跨平台同步（用户自行用 Git/网盘同步文件夹）的冲突处理？ | V1.0 不特殊处理，依赖文件监听感知外部变更；文档提示用户风险 | V1.1 |
| `OPEN-06` | 索引库单文件 vs 分库（按 Vault）？ | 每 Vault 独立 `index.db`（已定，§2.4.1），全局配置独立 `global.db` | 已决 |

### 10.3 与源文档（v5）的差异说明

本 PRD 在 v5 依赖与安全基线之上，做了以下**修正与补充**，均在正文对应位置标注：

| # | v5 的表述 | 本 PRD 的处理 | 依据 |
| --- | --- | --- | --- |
| D-01 | v5 §11.7 称支持 bsdiff **增量更新** | V1.0 限定**全量更新**；Tauri updater 插件不支持二进制差分 | FR-UPDATE-14 说明 |
| D-02 | v5 §11.7 称更新失败**自动回滚到上一版本** | 修正为「更新失败时保持当前版本不变」；应用二进制无法自动回滚 | FR-UPDATE-08 |
| D-03 | v5 Cargo.toml 仅锁 4 个 crate | 补充 `jieba-rs`、`notify`、`walkdir`、`sha2`、`thiserror`、`anyhow`、`tokio`、`yaml-rust`/`serde_yaml`、`regex`、`tracing` 等实现必需依赖，**版本待开发前核实**，不臆造 | §3.4, §10.1 RISK-03 |
| D-04 | v5 未定义任何功能需求 | 本 PRD 补齐 12 个功能模块、62 个 Command（含勘误 D-10 的 `vault_pin` 与 D-13 的 `vault_state_get/set`）、13 个事件、完整 DDL 与验收标准 | 全文 |
| D-05 | v5 未涉及中文分词 | 明确 jieba-rs 预分词 + FTS5 unicode61 方案 | §3.4 |
| D-06 | v5 CI 用 `github.*` 变量但声称 Gitee Go | 技术方案 §9 给出 Gitee Go 正确变量映射 | 技术方案 |
| D-07 | v5 §11.7 与本草稿使用 `update.json` | 统一改为 **`latest.json`**——Tauri 2 updater 的约定文件名（`createUpdaterArtifacts: true` 生成），FR-UPDATE-02/05/09 与验收样例已同步 | 技术方案 §11.5、§12 D-05 |
| D-08 | M1 的阶段验收写作「AC-VAULT 全通过」，但其中两条在 M1 阶段物理上不可验证 | 修正为可验证口径：AC-VAULT-01 的**解析断言**依赖 M3 解析器与 M4 链接裁决、**AC-VAULT-05** 依赖 M3 索引引擎与 M2 文件树，故 M1 只验收 AC-VAULT-01 的存储相关部分 + AC-VAULT-02/03/04，其余顺延 M3（2026-10-01 确认） | AC-VAULT-01、AC-VAULT-05 |
| D-10 | FR-VAULT-07（P0）要求 Vault 列表「支持置顶」，但 §5.3.1 的 8 个 vault 命令中**没有任何置顶命令**，全局库 `vault.pinned` 列也无写入入口（2026-10-02 复核发现） | 需求与命令契约不一致：按现状置顶无法实现 | **已于 M2 PR-1 落地**：新增 `vault_pin { vault_id, pinned }`（§5.3.1 已补行；命令总数 59 → 60）；`pinned` 列与列表排序在 M1 已就绪 | FR-VAULT-07 / §5.3.1 |
| D-11 | §5.3.2 的 10 个文件域命令只给出返回类型**名**（`FileNode`/`NoteContent`/`FileStat`/`ValidationResult`/`RenameResult`/`DeleteResult`），**从未定义字段**；且 `note_read` 的 M0 实现返回裸字符串，与表中 `NoteContent` 不符（2026-10-02 复核发现） | 契约不完整会导致 TS 与 Rust 各自臆造形状而漂移（门禁 17 只校验命令名，不校验结构） | 新增 §5.3.2.1 定义全部返回结构；`note_read` → `NoteContent` 的修正列入 M2 计划 WP2 交付行 | §5.3.2 / §5.3.2.1 |
| D-13 | §3.3 定义了 `vault_state` 表、FR-FILE-01/FR-EDITOR-37/AC-FILE-08 均要求把界面状态持久化到其中，但 §5.3.9 的命令表**没有任何读写入口**（2026-10-04 M2 复核发现，与 D-10 同类） | 需求要求持久化、契约却无通道：实现只能各自造轮子或静默丢弃状态（AC-FILE-08 无法通过） | 新增 `vault_state_get/set` 两条命令（§5.3.9 已补行，命令总数 60 → 62）并在 M2 落地 | FR-FILE-01 / FR-EDITOR-37 / AC-FILE-08 / §5.3.9 |
| D-09 | v5 §11.3 表格有错行（`feat!` 行损坏）与重复段落 | 技术方案 §9 修正为完整正确的 Conventional Commits 表 | 技术方案（**编号修正**：本条原误编为 D-07，与上一条重复，2026-10-01 改为 D-09） |
| D-14 | 应用**没有任何 Vault 打开/新建入口**：`vaultOpen/vaultCreate/vaultPin/vaultRename/vaultRelocate/vaultRegisterRemove` 在前端零调用；M1 报告写「AC-VAULT-02 的 UI 提示与引导属 M2」，而 M2 计划与报告中没有该项（2026-10-05 两轮独立复核发现） | **已于 PR #39 补齐**：新增欢迎页（打开已有文件夹 / 新建知识库 / 切换 / 置顶 / 重命名显示名 / 从列表移除，移除只删注册记录）、AC-VAULT-02 的「重新定位 / 从列表移除」引导、`/welcome` 路由与 **RT-01 守卫**；`current` 收归 vault store（同时修掉状态栏 watch 空转）。FR-VAULT-01/02/03/07/08 的归属明确为 **M1 契约 + M2 UI**（均已交付） | FR-VAULT-01/03/04/07/08、AC-VAULT-02、PRD §7.1 |
| D-15 | PERF-08 / §8.4 门禁 10 写「回退 > 20% 即阻断」，实现为「绝对预算 + 相对基线 2×」（本机 vs CI 实测差异 47–56%，20% 会产生**跨机假失败**；2026-10-05 M2 复核发现） | 口径修订为：**每项指标 = 绝对预算（硬性，机器无关）+ 相对基线回退 ≤ 2×**（仍能抓 O(n²) 级算法回退）；门禁名称同步改为「性能基准（预算 + 回退 ≤ 2×）」 | NFR-PERF-08 / PERF-08 |
| D-16 | §5.3 头部写「共 62 个 Command」，而 §5.3 末尾统计与 D-04 均为 **62**（D-10/D-13 各补命令后头部未同步） | 头部修正为 62，与统计行、D-04 一致 | §5.3、D-04 |
| D-17 | M2 的阶段验收（§9.1）写作「AC-FILE-01~06（除链接改写）；AC-EDITOR-02/05/06 通过」，但其中三条在本阶段只能做到结构性近似（2026-10-05 M2 复核发现） | 按 **D-08 先例**登记可验证口径：**M2 实际判定 = AC-FILE-01~05 + AC-EDITOR-01/02 全通过**；AC-FILE-06 = 组件级近似（真机帧率/内存归 M3/M9，见该 AC 的度量口径注）；AC-EDITOR-05 = 「我们这一侧」解析+渲染预算通过 + 内核侧归 **DEBT-13**（M8）；AC-EDITOR-06 = 保存不回灌内核的**结构保证** + 内核撤销深度归 M8 | AC-FILE-06、AC-EDITOR-05/06 |
| D-18 | §5.3.1/§5.3.7/§5.3.9 的 `VaultInfo`/`VaultSummary`/`IndexStatus`/`SystemInfo`/`CleanupResult` 等**只给类型名、从未定义字段**（与 **D-11** 同类缺陷；2026-10-05 M1 复核发现） | 登记为待补：随 **M3** 的索引域契约一起补 §5.3 的形状表（文件域已在 **§5.3.2.1** 补齐，可作模板） | §5.3.1/§5.3.7/§5.3.9、D-11 |
| D-19 | **需求域 → 里程碑覆盖缺口**：`FR-SET`（14 条 / 7 条 P0）、`FR-PALETTE`（10 / 6）、`FR-LAYOUT`（5 / 3）、`FR-GLOBAL-01`（P0）以及**渲染态代码块高亮**（`FR-EDITOR-44` P0、`FR-EDITOR-03`）在 §9.1 的里程碑交付行里**没有任何归属**——若不登记，M10 的「全部 P0 完成」不可能达成（2026-10-05 复核发现） | 新增 **§9.1.1「需求域 → 里程碑」覆盖表**并把上述域明确归属（FR-SET / FR-PALETTE / FR-LAYOUT / FR-GLOBAL / 高亮 → **M5**） | §9.1、FR-SET-*、FR-PALETTE-*、FR-LAYOUT-*、FR-GLOBAL-01、FR-EDITOR-44 |
| D-20 | §9.1 的 M4 行把「**链接解析与裁决**」列为 M4 交付，而 §3.1.2 的 **MD-WL-03** 明文「解析阶段一律不判定有效性，由**索引阶段**链接解析器统一裁决」，技术方案的全量索引流程也把裁决列为第 ⑤ 步（`index_engine/link_resolve.rs`）——两处真相源互相矛盾（2026-10-05 M3 复核发现） | 按**规则条款优先**：**裁决归 M3（索引阶段）**；M4 行改为「反链/悬空/歧义/孤立**面板** + 链接改写（预览+备份+原子+回滚）」。已同步修改 §9.1 的 M4 行措辞 | §3.1.2 MD-WL-03、§9.1 |
| D-21 | 解析器实现路径不一致：§2.5/§3.1.5/§9.3 的 TEST-01 写 `domain/md_parse.rs`（**单文件**），技术方案 §5.1 写 `domain/md_parse/`（**目录**，含 frontmatter/wikilink/tag/heading/block_id/code_fence 七个文件） | 以**技术方案（目录）**为准（单文件放不下 27 条规则且不利于 CODE-11）；本行登记口径，实现按目录落地 | §2.5、§3.1.5、§9.3 TEST-01、技术方案 §5.1 |
| D-22 | ~~事件表只有 13 个事件，技术方案会 emit `kp://note/updated`/`kp://link/changed` 而 PRD 未定义~~ **本条经复核为误判，已撤回（2026-10-05）**：这两个事件**本就在 §5.4 的事件表中**（事件表共 13 条，M3 复核时的抽取只列了其中与索引相关的 8 条，被错误推广为「PRD 未定义」） | **无需改动**；保留本行作为记录，并提示：**只列子集的抽取结果不能当作全集使用**（教训已写入 M3 计划 §8） | §5.4 事件表 |
| D-23 | **三处事件载荷口径不一致**（2026-10-05 M3 复核，逐条核对 §5.4 的 13 条事件表后发现）：① `kp://index/failed`——技术方案写有 `detail`（inotify 耗尽的 sysctl 建议），PRD 只有 `code/message/failed_files`；② `kp://note/updated`——PRD 为 `{ rel_path }`，技术方案为 `{ rel_path, mtime_ms }`；③ `kp://link/changed`——PRD 为 `{ affected_files }`，技术方案为 `{ rel_path }` | 以 **PRD 为准**（EVT-03 要求载荷只含路径与元信息，且前端契约以 PRD 定义为准）：`index/failed` 的 sysctl 建议并入 `message`、`note/updated` 不带 `mtime_ms`、`link/changed` 用 `affected_files`；**技术方案侧需同步标注**，实现时不得引入 PRD 未定义的字段 | §5.4 事件表、EVT-03、技术方案 §5.4 |

---

## 附录 A：技术栈风险与降级预案

### A.1 完整依赖矩阵

依赖版本以《Knowl-Pad-完整技术方案》§3 的锁定矩阵为**唯一权威**（已于 2026-09-21 跟进上游）。本附录仅列**带风险项**与处置手段，不重复完整清单，以免形成第二处维护点。

> **2026-09-21 更新**：Vite 与 TypeScript 的回退路径均已**正式取消**（见下方 A.2），本表相应条目改为「不回退」并列出实际的风险处置手段。其余条目版本已同步至跟进上游后的值。

| 依赖 | 当前锁定 | 风险 | 处置手段 |
| --- | --- | --- | --- |
| Vite | `^8.3.0` | 🟡 v5 自述存在生产回归（字符串枚举别名编译错误、`writeBundle` 非确定性遗漏 chunk、DevBundle 崩溃） | **不回退**。三项回归的规避手段已升级为硬性门禁：禁用 `const enum`（ESLint 拦截）、禁用自定义 `writeBundle` 插件 + CI 连续 3 次构建比对产物清单 + chunk 完整性校验（门禁 9）、开发期崩溃用 `vite build --watch` 绕过。详见技术方案 §3.4.3 |
| TypeScript | `~6.0.3` | 🟡 默认值变更（strict/types/module/target/rootDir）可能触发工具链不兼容 | **不升 7.x、也不回退 5.x**。上游 latest 已是 7.0.2（Go 重写版），刻意停留 6.x：`~` 范围天然阻断自动升级；M0 验证 `vue-tsc ^3.3.11` 与 `typescript-eslint ^8` 的兼容性；tsconfig 显式设置全部受影响项（技术方案 §3.3.2） |
| markdown-it | `^15.0.2` | 🟢 安全修复版，API 稳定 | 无需处置（项目未开发，无迁移成本） |
| DOMPurify | `^3.4.15` | 🟢 安全修复版；已跟进上游（3.4.12→3.4.15） | 安全库的 patch 常含修复，建议每次跟进；由门禁 7 `pnpm audit` 兜底 |
| pinia | `^4.0.3` | 🟡 仅支持 ESM | 项目本身即 ESM（`"type": "module"`），无影响 |
| vue-router | `^5.3.1` | 🟡 主版本升级；已跟进（5.2.0→5.3.1） | 官方声明无破坏性变更；若出现阻断问题，回退 4.5.x（API 差异小） |
| md-editor-v3 | `^6.5.6` | 🟡 上游已发布 **7.0.0**（major breaking） | **刻意不升级**：该包在 M8 即被 CodeMirror 6 整体替换，为临时方案做 breaking 升级投入产出比过低。`^6.5.6` 的范围天然阻断升到 7.x |
| jieba-rs | `0.11.0`（2026-09-19 经 crates.io 核实，不在 v5 矩阵） | 🟡 分词质量决定中文搜索可用性；词典全量内置增加约 5MB 包体（`OPEN-02` 已决接受） | 调用 `cut(text, false)` 关闭 HMM 新词发现（确定性优先；**M0 实测 jieba-rs 0.11.0 无 `hmm` cargo feature**）以保证索引可重建且两次结果一致；若分词质量不达标，备选 `tantivy`（自带 CJK 分词）或 `cang-jie`，需在 M3 前评估 |
| tauri / tauri-plugin-updater | `2.12.0` / `2.12.0` | 🟢 已跟进上游（minor）；tauri 2.12 家族要求 **rustc ≥ 1.90** | 插件版本须与 tauri 主版本配套；`3.0.0-alpha.0` 为预发布，一律禁用 |

### A.2 降级决策点（2026-09-21 更新）

| 决策点 | 触发条件 | 动作 |
| --- | --- | --- |
| ~~Vite 降级~~ **已取消** | — | ❌ **不再保留回退 6.4.3 的选项**。取消理由：① 回退会重新引入 ReDoS 安全风险（CVE-2026-39364、CVE-2026-53571 的修复状态需重新论证）② 双轨维护 `rolldownOptions`/`rollupOptions` 两套配置成本长期存在 ③ 回退窗口仅在 M0，保留常驻备选反而诱导拖延决策。**改为**：升级 Vite patch 版、调整规避手段，或将阻断性缺陷作为独立高优先级决策项上报重审 |
| ~~TS 降级~~ **已取消** | — | ❌ 不再保留回退 5.9.x 的选项。TS 6.0 与 5.9 API 兼容，回退无实际收益，反而使 `tsconfig` 需维护两套默认值假设 |
| TS 7 迁移（新增评估项） | `vue-tsc`、`typescript-eslint`、`@vue/language-core` 均明确支持 TS 7，且 M0 之后的某个稳定窗口 | 作为**独立决策项**评估，不与功能开发混排。迁移需同步更新 §3.3 的 tsconfig 与 `ignoreDeprecations` 设置 |
| 分词器替换 | M3 阶段 jieba-rs 分词**质量**不达标（体积已不构成触发条件，`OPEN-02` 已决全量内置） | 评估 tantivy（自带 CJK 分词）/ cang-jie，重新锁定并递增索引签名版本以触发重建 |

---

## 附录 B：扩展语法解析测试集（SJ-04）

为验证成功判据 SJ-04（链接解析准确率 ≥ 99%），建立以下测试样本。每个样本为一个 `.md` 文件 + 期望解析结果断言。

**期望结果的确定方式**：下表的「期望解析」列**直接由 §3.1 的规则条款推导得出**（每条用例在备注中标明其依据条款）。测试断言以本表为唯一基准，**不以任何外部软件的运行时行为作为比对基准**——这既保证了验收的可重复性（不依赖第三方软件版本），也避免了将产品合格标准建立在第三方实现之上。

| 用例 ID | 输入 | 期望解析 | 依据条款 |
| --- | --- | --- | --- |
| B-01 | `[[Note]]` | link → `Note`，resolved/dangling 视目标存在性 | MD-WL-01/03 |
| B-02 | `[[Note\|别名]]` | link → `Note`，alias = `别名` | MD-WL-01 |
| B-03 | `[[Note#标题]]` | link → `Note`，anchor = `标题` | MD-WL-01 |
| B-04 | `[[Note#^abc123]]` | link → `Note`，anchor = block `abc123` | MD-WL-01、MD-BID-01 |
| B-05 | `[[folder/sub/Note]]` | link → 完整路径匹配 | MD-WL-02 |
| B-06 | `![[image.png]]` | embed，link_kind = embed | MD-WL-07 |
| B-07 | `![[Note#标题]]` | embed 笔记章节 | MD-WL-07 |
| B-08 | `` `[[Note]]` ``（行内代码） | **不解析**为链接 | MD-WL-05 |
| B-09 | ` ```\n[[Note]]\n``` `（代码块） | **不解析**为链接 | MD-WL-05 |
| B-10 | `\[[Note]]`（转义） | **不解析**为链接 | MD-WL-08 |
| B-11 | `#标签` | tag = `标签` | MD-TAG-01/02 |
| B-12 | `#父/子/孙` | tag 层级展开为 3 条 | MD-TAG-03 |
| B-13 | `#123`（数字开头） | **不解析**为标签 | MD-TAG-01 |
| B-14 | `# 标题`（# 后有空格） | heading，**非**标签 | MD-TAG-04、MD-H-01 |
| B-15 | frontmatter `tags: [a, b]` | tag a、b，line = -1 | MD-TAG-06、MD-FM-04 |
| B-16 | frontmatter `aliases: [x, y]` | 别名 x、y 参与链接补全 | MD-FM-04、MD-WL-06 |
| B-17 | frontmatter YAML 格式错误 | 降级为无 frontmatter + 警告，正文照常索引 | MD-FM-02/03 |
| B-18 | 段落末 `^blockid` | block_id 入库 | MD-BID-01/02 |
| B-19 | 同名笔记在不同文件夹，`[[note]]` | ambiguous，记录全部候选 | MD-WL-04 |
| B-20 | `[[note]]` 大小写不同（`[[Note]]` vs `note.md`） | 大小写不敏感匹配成功 | MD-WL-04 |
| B-21 | Setext 标题（`标题\n===`） | heading level 1 | MD-H-01 |
| B-22 | 含第三方软件配置目录（如 `.obsidian/`）的完整 Vault | 该目录被忽略且不索引，其余正确解析 | FR-STORAGE-02/04 |
| B-23 | CRLF 换行的笔记 | 正确解析，写入保持 CRLF | NFR-PLAT-05 |
| B-24 | 含 BOM 的 UTF-8 笔记 | 正确解析，BOM 保留 | NFR-PLAT-06 |
| B-25 | 远程图片 `![](https://...)` | 不自动加载，占位符 | SEC-08、FR-ATTACH-11 |

> 上表「依据条款」列使每个用例都可追溯到 §3.1 或相应需求条目。若某用例在评审中无法找到对应条款，说明**规则集存在缺口**，应补充条款而非直接固化断言。

**验收方式**：构造包含以上全部用例的测试 Vault，将上表「期望解析」列固化为自动化断言，再用 Knowl Pad 的解析器逐项比对，准确率 = 通过项 / 总项 ≥ 99%。

**断言固化流程（M3 执行一次，产物入库）**：

1. 由研发依据 §3.1 规则条款，为 25 个用例逐条写出期望的 `ParsedNote` 结构（链接数、状态、标签展开、标题层级、Block ID 等）
2. 结果以结构化夹具形式提交至 `tests/fixtures/`（**不是**运行时抓取外部软件输出）
3. 评审时逐条核对夹具与 §3.1 条款的对应关系，确认无遗漏、无自相矛盾
4. 此后 CI 只比对「解析器输出 vs 夹具」，**不引入任何外部软件依赖**（同时满足 PRD TEST-05「测试禁止依赖网络」）

> **与既往做法的区别**：本附录早期版本的验收方式曾要求以某第三方笔记软件的实际解析行为作为「期望基准」。该做法存在两个问题：① 使产品合格标准依附于第三方实现，其版本变化会导致验收结果漂移；② 在文档中形成对第三方软件的比较性主张。现改为以 §3.1 规则条款为唯一权威依据，上述两个问题同时消除，且验收的可重复性更强。

---

## 附录 C：需求覆盖矩阵

| 核心场景（§1.3） | 覆盖需求 | 覆盖验收标准 |
| --- | --- | --- |
| SC-01 打开 Vault 建索引 | FR-VAULT-01~09, FR-SIG-01 | AC-VAULT-01/05 |
| SC-02 `[[` 链接补全 | FR-EDITOR-20~24 | AC-EDITOR-04 |
| SC-03 反链跳转 | FR-LINK-10~18 | AC-LINK-01 |
| SC-04 知识图谱 | FR-GRAPH-01~12 | AC-GRAPH-01~04 |
| SC-05 中文全文搜索 | FR-SEARCH-01~11, §3.4 | AC-SEARCH-01 |
| SC-06 重命名更新链接 | FR-FILE-20~28 | AC-FILE-01/02 |
| SC-07 回收站恢复 | FR-TRASH-01~12 | AC-TRASH-01/02 |
| SC-08 外部变更感知 | FR-FILE-07, §5.4 事件 | AC-FILE-07 |
| SC-09 命令面板 | FR-PALETTE-01~10 | AC-PALETTE-01/02 |
| SC-10 自动更新 | FR-UPDATE-01~14 | AC-UPDATE-01~05 |

| 成功判据（§1.5） | 验证依据 |
| --- | --- |
| SJ-01 P0 需求全通过 | §4 各模块 AC |
| SJ-02 性能达标 | §6.1.2 + §8.3 性能基准 |
| SJ-03 索引可丢弃重建 | AC-REL-03 |
| SJ-04 扩展语法解析准确率 ≥ 99% | 附录 B |
| SJ-05 威胁全覆盖 | §6.3 + AC-SEC-01~05 |
| SJ-06 三平台验收 | §6.4.1 |

---

**文档结束**

> 本 PRD 为 Knowl Pad V1.0 的唯一需求真相源。任何实现、技术方案、架构图与本 PRD 冲突时，以本 PRD 为准；本 PRD 的变更须经正式评审并更新版本号与变更记录。
