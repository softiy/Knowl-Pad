# Knowl Pad 完整技术方案

| 项目 | 内容 |
| --- | --- |
| 文档版本 | v6.3 |
| 编写日期 | 2026-09-17（v6.0），2026-09-22（v6.1 修正），2026-09-25（v6.2 开发可用性修订），2026-09-29（v6.3 M0 收尾修订） |
| 文档状态 | 评审修正版（v6.3：M0 收尾修订，含契约同步决策与门禁 17，见 §8.3/§12.2；历史修订见 §12.1；M0 本地核实见附录 D） |
| 配套文档 | 《Knowl-Pad-PRD》v2.3（需求真相源）、《Knowl-Pad-架构图》v2.4 |
| 上游基线 | 《Knowl_Pad_完整技术方案_v5》依赖锁定矩阵与安全审计结论 |
| 架构形态 | 模块化单体（Modular Monolith），**无插件系统** |
| 项目状态 | 尚未开始开发 |

> **本文档定位**：PRD 定义「做什么」与「验收标准」，本文档定义「怎么做」。凡与 PRD 冲突之处，以 PRD 为准。
>
> **相对 v5 的关系**：v5 的内容实质是「依赖版本锁定矩阵 + 安全漏洞修复清单 + 版本管理与 CI/CD 方案」，**不含任何架构设计、数据设计或核心算法设计**。本文档在完整继承 v5 依赖与安全基线的前提下，补齐从架构到实现的全部技术设计，并修正 v5 中的 7 处技术错误（见 §12）。

---

## 目录

| 章节 | 内容 |
| --- | --- |
| §1 | 架构总览：进程模型、数据流、分层 |
| §2 | 工程结构：目录布局与模块边界落地 |
| §3 | 技术选型：完整依赖矩阵、新增依赖、风险与降级 |
| §4 | 数据架构：数据库设计、连接管理、迁移 |
| §5 | 核心引擎：Markdown 解析、分词、索引、文件监听 |
| §6 | 关键算法：链接改写、路径安全、原子写入 |
| §7 | 前端架构：状态管理、渲染管线、性能 |
| §8 | IPC 实现：类型安全、契约同步 |
| §9 | 安全实现：四层防护的具体落地 |
| §10 | 跨平台实现 |
| §11 | 质量保障：测试、CI/CD、发布、自动更新 |
| §12 | v5 勘误与修正留痕 |
| §13 | 实施路线与技术债 |
| 附录 | A 配置清单、B 性能基准实现、C 风险登记 |

---

## 1. 架构总览

### 1.1 设计出发点

三条硬约束决定了整体架构形态：

| 约束 | 来源 | 架构后果 |
| --- | --- | --- |
| **Markdown 文件是唯一真相源** | PRD P-1 | 必须存在一个「文件 → 索引」的单向派生管线，且索引可随时丢弃重建。数据库不是主存储，因此不需要复杂的 ORM 或数据同步层。 |
| **UI 永不阻塞在 IO 上** | PRD P-3 | 文件 IO、解析、分词、索引、搜索全部下沉 Rust 侧异步执行；前端只发请求、收事件。这排除了「前端用 WASM SQLite 直连」的方案。 |
| **不执行任何第三方代码** | PRD P-4 | 无插件系统 → 无沙箱层、无插件 API、无权限提升防护、无插件市场。威胁模型从可扩展架构的五层收敛为四层，安全设计与测试成本显著下降。 |

### 1.2 进程模型

Tauri 2 应用由两类进程构成：

```
┌──────────────────────────────────────────────────────────────┐
│  Rust 主进程（knowl-pad.exe / Knowl Pad.app）                 │
│                                                              │
│  ┌────────────────────────────────────────────────────────┐  │
│  │ 主线程：Tauri 事件循环、窗口管理、IPC 分发               │  │
│  └────────────────────────────────────────────────────────┘  │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────────────┐  │
│  │ tokio 运行时  │ │ 索引工作线程池 │ │ 文件监听线程          │  │
│  │ (异步 IO)    │ │ (解析+分词)   │ │ (notify crate)       │  │
│  └──────────────┘ └──────────────┘ └──────────────────────┘  │
│           │                │                  │              │
│           └────────────────┴──────────────────┘              │
│                            ▼                                 │
│              ┌──────────────────────────┐                    │
│              │ SQLite (rusqlite, WAL)   │                    │
│              │ index.db / global.db     │                    │
│              │ 单写连接 + 多读连接        │                    │
│              └──────────────────────────┘                    │
└───────────────────────▲──────────────────────────────────────┘
                        │ Tauri IPC
                        │ invoke(命令) / event(事件)
                        │ 受 Capabilities 白名单约束
┌───────────────────────┴──────────────────────────────────────┐
│  Webview 进程（系统 WebView：WebView2 / WKWebView / WebKitGTK）│
│                                                              │
│  Vue 3 SPA：文件树 · 编辑器 · 搜索 · 图谱 · 标签 · 设置        │
│  markdown-it 渲染 + DOMPurify 净化                            │
│  Pinia 状态 · vue-router 路由 · cytoscape 图谱                │
└──────────────────────────────────────────────────────────────┘
```

**线程模型要点**

| 线程/池 | 职责 | 约束 |
| --- | --- | --- |
| 主线程 | Tauri 事件循环、窗口、IPC 分发 | **禁止**执行同步耗时 > 16ms 的操作（PRD AC-04；16ms 为单帧预算，> 50ms 即红线 R-08 违规） |
| tokio 异步运行时 | 文件 IO、数据库异步访问、长任务编排 | 多工作线程，IO 密集型任务在此调度 |
| 索引工作线程池 | Markdown 解析 + jieba 分词（CPU 密集） | 默认 `num_cpus - 1`，可配置；**只做纯计算，不写库** |
| DB 写线程 | 索引结果串行写入 SQLite | **单线程独占写连接**，避免 WAL 写锁竞争 |
| 文件监听线程 | `notify` 事件接收与去抖动归并 | 事件归并后经通道投递至索引队列 |

> **关键设计**：解析（CPU 密集、可并行）与写库（IO 密集、需串行）分离。工作线程池并行解析文件产出 `ParsedNote`，通过 `mpsc` 通道投递给单一 DB 写线程批量入库。这既利用了多核，又规避了 SQLite 的写并发限制。

### 1.3 端到端数据流

#### 1.3.1 打开 Vault（冷启动关键路径）

```
用户选择目录
   │
   ▼
[vault_open] ──► 路径合法性与可访问性校验
   │
   ▼
创建/打开 .knowlpad/index.db ──► PRAGMA 初始化（WAL、synchronous、foreign_keys）
   │
   ▼
计算当前索引签名（schema版本 + 解析器版本 + 分词器版本&词典哈希 + Vault绝对路径）
   │
   ├─ 签名匹配 ──► 读取 meta，进入增量模式
   │                  │
   │                  ▼
   │            启动文件监听 ──► 扫描 mtime 差异 ──► 仅处理变更文件
   │
   └─ 签名不匹配 / 无索引 ──► 全量重建
                              │
                              ▼
                        walkdir 遍历（跳过 .knowlpad/、.obsidian/、隐藏文件）
                              │
                              ▼
                        文件清单入索引队列 ──► 工作线程池并行解析+分词
                              │                        │
                              │                        ▼
                              │                  mpsc → DB写线程
                              │                        │
                              │                        ▼
                              │                  批量事务提交（200文件/批）
                              │                        │
                              ▼                        ▼
                        kp://index/progress ◄──── 节流 ≥100ms
                              │
                              ▼
                        链接解析器：裁决全部 link 的 resolved/dangling/ambiguous
                              │
                              ▼
                        写入 index_signature ──► kp://index/completed
```

**性能关键点**

| 环节 | 优化手段 |
| --- | --- |
| 目录遍历 | `walkdir` 流式遍历，不一次性载入全部路径；遍历时即过滤忽略目录 |
| 文件读取 | 并行读取（线程池），单文件超 5MB 跳过并警告（SEC-11） |
| 解析 | 纯函数、无锁、可并行；frontmatter 用零拷贝切片解析 |
| 分词 | jieba 实例**线程本地复用**（初始化开销大，禁止每文件新建） |
| 写库 | 单写线程 + 批量事务；`INSERT` 用预编译语句复用 |
| 链接裁决 | 全部文件入库后**统一**执行（需全局 stem 视图），用一次 SQL JOIN 完成而非逐链接查询 |
| 进度上报 | 节流 ≥ 100ms（EVT-02），避免事件风暴淹没前端 |

#### 1.3.2 编辑保存（热路径）

```
用户输入 ──► 编辑器内存状态更新（同步，< 16ms）
   │
   ▼ 防抖 1000ms（可配置）
[note_write] { rel_path, content, base_mtime }
   │
   ▼
Rust: base_mtime 与磁盘 mtime 比对
   ├─ 不一致 ──► 返回 E_FILE_EXISTS/冲突 ──► 前端弹出三选项（FR-EDITOR-34）
   └─ 一致 ──► 原子写入协议（§6.3）
                 │
                 ▼
              写临时文件 → fsync → rename → fsync 父目录
                 │
                 ▼
              入增量索引队列（单文件重解析）
                 │
                 ▼
              emit kp://note/updated { rel_path }
                 │
                 ▼
前端刷新反链面板、标签面板（局部失效，不全量刷新）
```

#### 1.3.3 重命名 + 链接改写（最高风险路径）

```
[file_rename] { from: "A.md", to: "B.md" }
   │
   ▼
1. 前置校验：目标合法性、重名检测、from 存在性
   │
   ▼
2. [link_rewrite_preview] { from_ref: "A", to_ref: "B" }
   │     └─► SQL: 查找全部指向 A 的 link 记录
   │     └─► 读取各源文件，定位精确的 [[...]] 文本区间（复用解析器的 line/col）
   │     └─► 排除代码块/行内代码内的匹配（MD-WL-05）
   │     └─► 返回 { preview_id, 文件数 N, 处数 M, 逐文件明细 }
   │
   ▼
3. 前端展示「将修改 N 个文件中的 M 处链接」──► 用户确认
   │
   ▼
4. [link_rewrite_apply] { preview_id, rename? }
   │     └─► a. 备份全部 N 个文件至 .knowlpad/backup/<ts>/
   │     │      备份失败 ──► 返回 E_BACKUP_FAILED，操作中止（SEC-12）
   │     └─► b. 【先】执行文件重命名（原子 rename）
   │     │      失败 ──► 未改写任何链接，直接中止（避免"链接指新名、文件仍旧名"）
   │     └─► c. 事务内逐个原子改写（仅替换 [[...]] 内的目标引用子串）
   │     │      任一失败 ──► 回滚全部已改文件 **并撤销 rename** ──► E_REWRITE_FAILED
   │     └─► d. 增量重算受影响的链接状态
   │     └─► emit kp://link/changed { affected_files }
   │
   ▼
5. 前端刷新文件树、编辑器标签（跟随新路径，FR-FILE-28）、反链面板
```

**为什么这个流程不能简化**：链接改写是本产品唯一会**批量修改用户既有文件**的操作。一旦出错，损坏的是用户多年积累的知识网络，且难以人工修复。因此 PRD 将「备份 + 预览 + 全有或全无 + 回滚」定为 P0 红线（P-2、SEC-12），本方案在实现层严格对应。

**为什么 rename 要放在改写之前**：若先改写全部链接、再改名，改名一旦失败（目标被占用、权限不足），磁盘上就留下「300 处链接全部指向 B、文件却仍叫 A」的大规模悬空链接，而失败路径并不会触发回滚——因为回滚只在改写阶段触发。把 rename 前置后，rename 失败时磁盘零改动，rename 成功后才进入可回滚的改写阶段，消除了该中间态。

### 1.4 分层与依赖方向

```
┌─────────────────────────────────────────────────┐
│ 前端 Vue 3                                       │
│  app（装配） → features（业务） → core（内核）     │
│         依赖方向：单向向下，禁止反向               │
└──────────────────┬──────────────────────────────┘
                   │ core/ipc（唯一出口）
┌──────────────────┴──────────────────────────────┐
│ Rust                                             │
│  commands（薄壳） → domain（业务） → storage（存储）│
│                        ↓                         │
│                   platform（平台适配）            │
│         依赖方向：单向，禁止循环（RS-05）          │
└─────────────────────────────────────────────────┘
```

**分层价值**：`domain/` 不依赖 Tauri 类型（RS-02），因此可脱离桌面环境做纯单元测试——这对解析器、链接改写器这类正确性关键的纯逻辑模块至关重要。`storage/` 独占 SQL（RS-03），使数据库 schema 变更的影响面收敛在单一层。

---

## 2. 工程结构

### 2.1 仓库布局

```
knowl-pad/
├── package.json                  # 前端依赖与脚本
├── pnpm-lock.yaml                # 提交入库（SEC-06）
├── .nvmrc                        # 24.19.0
├── tsconfig.json                 # TS 6.0 配置（§3.3）
├── tsconfig.node.json
├── vite.config.ts                # Vite 8 配置（§3.4）
├── tailwind.config.ts
├── components.json               # shadcn-vue 配置
├── index.html
├── .github/
│   └── workflows/
│       ├── ci.yml                # 17 项门禁（§11.4）
│       ├── platform-smoke.yml    # 三平台窗口启动冒烟（§10）
│       └── release.yml           # 多平台构建 + 发布 + Gitee 同步（§11.5）
├── .gitee/
│   └── README.md                 # Gitee Go 已迁移说明（DEBT-06）
├── src/                          # 前端源码（§2.2）
├── src-tauri/                    # Rust 后端（§2.3）
├── tests/                        # 跨端集成与 E2E
│   ├── unit/                     #   vitest 前端单测（含 setup.ts：mock __TAURI_INTERNALS__）
│   ├── e2e/                      #   tauri-driver + WebDriverIO
│   ├── security/                 #   AC-SEC 自动化用例
│   ├── reliability/              #   AC-REL 故障注入
│   ├── perf/                     #   性能基准
│   └── fixtures/
│       ├── standard-vault/       #   标准库 3000 篇（生成脚本产出）
│       ├── large-vault/          #   大库 20000 篇
│       ├── stress-vault/         #   压力库 100000 篇
│       └── syntax-compat/        #   附录 B 扩展语法解析测试集（结构化断言夹具）
├── scripts/
│   ├── gen-fixture-vault.mjs          # 基准数据集生成（三档夹具，附录 B）
│   ├── verify-ipc-contract.mjs        # 前后端类型契约校验（§8.3）
│   ├── bump-version.mjs               # 版本号四处同步（§11.3）
│   ├── verify-build-integrity.mjs     # Vite 8 chunk 完整性校验（§3.4.2，门禁 9）
│   ├── compare-build-manifests.mjs    # 三次构建产物一致性比对（门禁 9）
│   ├── check-path-encapsulation.sh    # 前端禁止直接路径操作的 lint 补充（SEC PATH-02）
│   └── check-naming.sh                # 命名一致性检查（PRD §0.2，门禁 13）
└── docs/
    ├── Knowl-Pad-PRD.md
    ├── Knowl-Pad-完整技术方案.md
    ├── Knowl-Pad-架构图.html
    └── AGENTS.md                 # AI 编程工具指令入口（§13.4）
```

> **脚本清单完整性**：以上 7 个脚本与 §11.4 CI 配置中引用的脚本**一一对应**，无遗漏。`check-path-encapsulation.sh` 与 `check-naming.sh` 为 shell 脚本（纯文本扫描，无需 Node 依赖），其余为 `.mjs`（ESM Node 脚本）。

### 2.2 前端结构（对应 PRD FE-01~05）

```
src/
├── main.ts                       # 应用入口
├── app/
│   ├── router/                   # 路由定义与守卫（PRD §7.3）
│   ├── stores/                   # 跨 feature 的全局 store 注册
│   ├── layout/                   # 三栏布局容器、标题栏、状态栏
│   └── bootstrap.ts              # 启动流程编排（Vault 恢复、偏好加载）
├── core/
│   ├── ipc/
│   │   ├── client.ts             #   invoke/event 的类型安全封装
│   │   ├── commands.ts           #   59 个 Command 的强类型定义（自动生成，§8.3）
│   │   ├── events.ts             #   13 个事件的载荷类型
│   │   └── errors.ts             #   KpError 与错误码常量
│   ├── markdown/
│   │   ├── renderer.ts           #   markdown-it 实例与插件链
│   │   ├── sanitize.ts           #   DOMPurify 封装（SEC-01）
│   │   └── highlight.ts          #   代码高亮（纯文本→span，无执行路径）
│   ├── theme/                    #   CSS 变量、深浅主题、缩放
│   ├── shortcut/                 #   快捷键注册中心（平台映射，NFR-PLAT-07）
│   ├── logger/                   #   前端日志（转发至 Rust 统一落盘）
│   └── utils/                    #   纯函数（无副作用、无状态）
├── features/
│   ├── vault/        { components/ composables/ stores/ types.ts index.ts }
│   ├── file-tree/    …
│   ├── editor/       …           #   含 md-editor-v3 与 CodeMirror 双内核适配层
│   ├── link/         …
│   ├── search/       …
│   ├── graph/        …
│   ├── tag/          …
│   ├── attachment/   …
│   ├── palette/      …
│   ├── trash/        …
│   ├── settings/     …
│   └── update/       …
└── shared/
    ├── components/               # shadcn-vue 封装的通用组件
    └── types/                    # 跨 feature 共享类型
```

**编辑器双内核适配**：为支持 PRD §4.3 的分阶段策略（MVP 用 `md-editor-v3`，正式版换 CodeMirror 6），`features/editor/` 内定义统一接口：

```typescript
// features/editor/types.ts
interface KpEditorAdapter {
  mount(el: HTMLElement, opts: EditorOptions): void;
  getValue(): string;
  setValue(content: string): void;
  on(event: 'change' | 'cursor' | 'save', handler: (...args: unknown[]) => void): void;
  insertLink(target: string, alias?: string): void;
  triggerSuggest(kind: 'link' | 'tag' | 'heading'): void;
  find(query: string, opts: FindOptions): InFileMatch[];
  replace(matches: InFileMatch[], replacement: string): ReplaceResult;
  destroy(): void;
}
```

MVP 阶段实现 `MdEditorV3Adapter`，M8 阶段实现 `CodeMirror6Adapter`。其余模块只依赖 `KpEditorAdapter` 接口，内核替换不外溢。

### 2.3 Rust 结构（对应 PRD RS-01~06）

```
src-tauri/
├── Cargo.toml                    # §3.5
├── Cargo.lock                    # 提交入库（SEC-06）
├── tauri.conf.json               # §9.5 CSP、Capabilities、updater
├── capabilities/
│   └── default.json              # 最小权限白名单（SEC-05）
├── icons/
└── src/
    ├── main.rs                   # 入口：Builder 装配、Command 注册
    ├── lib.rs
    ├── state.rs                  # AppState（Vault 句柄、DB 池、队列、取消令牌）
    ├── error.rs                  # AppError 枚举 + 错误码映射（PRD §5.2）
    ├── commands/                 # 薄壳层（RS-01：≤30 行，无业务逻辑）
    │   ├── mod.rs
    │   ├── vault.rs   note.rs    file.rs    search.rs
    │   ├── graph.rs   tag.rs     attach.rs  trash.rs
    │   └── settings.rs update.rs index.rs   rewrite.rs
    ├── domain/                   # 业务逻辑（可脱离 Tauri 单测，RS-02）
    │   ├── vault.rs              #   生命周期、路径解析
    │   ├── path_guard.rs         #   路径安全校验（SEC-02，§6.2）
    │   ├── note_io.rs            #   读写、原子写入、备份
    │   ├── fs_ops.rs             #   CRUD、重命名、移动、删除
    │   ├── md_parse/             #   Markdown 解析器（§5.1）
    │   │   ├── mod.rs            #     ParsedNote 与入口
    │   │   ├── frontmatter.rs
    │   │   ├── wikilink.rs
    │   │   ├── tag.rs
    │   │   ├── heading.rs
    │   │   ├── block_id.rs
    │   │   └── code_fence.rs     #     代码块/行内代码区间识别（供排除用）
    │   ├── tokenize.rs           #   jieba-rs 封装（§5.2）
    │   ├── index_engine/         #   索引引擎（§5.3）
    │   │   ├── mod.rs            #     编排：全量/增量
    │   │   ├── queue.rs          #     任务队列与去重
    │   │   ├── worker.rs         #     工作线程池
    │   │   ├── writer.rs         #     单写线程 + 批量事务
    │   │   ├── link_resolve.rs   #     链接裁决（全局 stem 视图）
    │   │   └── signature.rs      #     索引签名
    │   ├── search.rs             #   FTS5 查询构造、bm25 排序、snippet
    │   ├── graph.rs              #   图谱构建、N 跳裁剪、降级判定
    │   ├── tag.rs                #   规范化、层级、计数维护
    │   ├── rewrite.rs            #   链接/标签批量改写（§6.1，最高风险模块）
    │   ├── trash.rs              #   软删除、恢复、过期
    │   └── watcher.rs            #   文件监听、去抖动归并（§5.4）
    ├── storage/                  # 存储层（SQL 唯一所在地，RS-03）
    │   ├── db.rs                 #   连接管理、PRAGMA、事务辅助
    │   ├── global_db.rs          #   全局库 schema + 迁移 + DAO
    │   ├── index_db.rs           #   索引库 schema + DAO
    │   └── dao/                  #   按实体拆分的 DAO（file/link/tag/heading/...）
    └── platform/                 # 平台差异（NFR-PLAT-15）
        ├── mod.rs
        ├── path.rs               #   分隔符、长路径、大小写
        ├── fs_atomic.rs          #   rename 语义差异（§6.3）
        ├── perms.rs              #   .knowlpad 目录权限（SEC-15）
        └── menu.rs               #   macOS 原生菜单
```

### 2.4 AppState 设计

```rust
pub struct AppState {
    /// 当前打开的 Vault（None 表示未打开）。
    /// ⚠️ 必须是**异步锁**：Command 中会 `.read().await`（见 §8.1）；
    ///    且禁止在持有该锁期间执行文件 IO 或再发起 IPC（避免在 .await 中持锁跨调用）。
    pub vault: tokio::sync::RwLock<Option<VaultHandle>>,
    /// 全局库连接（单写连接 + 读连接池）
    pub global_db: DbPool,
    /// 索引任务队列发送端
    pub index_tx: mpsc::Sender<IndexTask>,
    /// 长任务取消令牌注册表（operation_id → token）
    pub cancellations: DashMap<Uuid, CancellationToken>,
    /// 改写预览缓存（preview_id → 预览明细，TTL 10 分钟）
    pub previews: DashMap<Uuid, RewritePreview>,
    /// jieba 分词器（内部 Arc，线程安全复用）
    pub jieba: Arc<Jieba>,
    /// 应用配置目录
    pub config_dir: PathBuf,
}

pub struct VaultHandle {
    pub id: i64,
    pub root: PathBuf,               // 规范化后的绝对路径（路径校验基准）
    pub canonical_root: PathBuf,     // 解析符号链接后的真实路径（SEC-02）
    pub index_db: DbPool,
    pub watcher: Option<RecommendedWatcher>,
    pub signature: IndexSignature,
}
```

**设计要点**

| 要点 | 理由 |
| --- | --- |
| `vault` 用 `RwLock<Option<..>>` | 支持运行时切换 Vault；读多写少 |
| 同时保存 `root` 与 `canonical_root` | 路径校验必须用 canonical 形式，否则符号链接可逃逸（SEC-02 / T-04） |
| `cancellations` 注册表 | 满足 NFR-REL-10（长任务可取消）；`operation_id` 由前端持有 |
| `previews` 带 TTL | 预览与执行分离（两步式改写），TTL 防止内存泄漏；执行时校验 preview 未过期 |
| `jieba` 全局单例 | 分词器初始化含词典加载（数十 MB），**禁止**每文件新建 |

---

## 3. 技术选型

### 3.1 选型原则

| 原则 | 说明 |
| --- | --- |
| **继承 v5 基线** | v5 的依赖矩阵已经过一轮安全审计与漏洞修复对比（v2→v5 的演进即为安全更新过程），本文档不重复评估已锁定项，仅继承并补充风险预案 |
| **不臆造版本号** | v5 未涵盖的新增依赖（分词、文件监听、哈希等）**不编造精确版本号**，仅给出主版本约束并标注「开发前需核实当前稳定版」。理由：编造版本号会导致 `Cargo.toml` 无法解析或引入不存在/有漏洞的版本 |
| **优先纯 Rust / 无原生依赖** | 降低三平台交叉编译复杂度（如选择 `rusqlite` 的 `bundled` feature 避免依赖系统 SQLite） |
| **停更包一律替换** | v5 已确立此规则（`vuedraggable` → `vue-draggable-next`），本文档延续 |

### 3.2 前端依赖矩阵（继承 v5）

#### 3.2.1 运行环境与构建

| 依赖 | 锁定版本 | 用途 | 风险 |
| --- | --- | --- | --- |
| Node.js | `v24.19.0` | 运行时（LTS 支持至 2028-04） | ⚪ 无 |
| pnpm | `12.4.1` | 包管理（Rust 重写版，兼容 pnpm 11 命令与 lockfile 格式） | 🟡 主版本，需验证 CI 缓存键 |
| TypeScript | `6.0.3` | 类型系统 | 🟡 **刻意停留在 6.x**：npm `latest` 已是 `7.0.2`（Go 重写版 tsgo），见下方「TS 版本决策」 |
| Vite | `^8.3.0` | 构建（Rolldown + Oxc + Lightning CSS） | 🟡 **已决策不回退**（2026-09-21）：保留 8.x，回归风险改为「必须规避」而非「可回退」，见 §3.4.3 |
| `@vitejs/plugin-vue` | `^6.0.9` | Vue SFC 编译（Vite 8 要求 6.0+） | ⚪ 已核实 |
| `vue-tsc` | `^3.3.11` | 类型检查（3.0+ 才完整支持 TS 6） | ⚪ 已核实 |

> **TS 版本决策（2026-09-21）**：`typescript` 的 npm `latest` tag 已指向 **7.0.2**（Go 重写版）。本项目**刻意不跟进**，保持 `~6.0.3`，理由：
> ① TS 7 是 Go 实现的全新编译器，与 6.x 的 API/插件生态（`vue-tsc`、`typescript-eslint`、`@vue/language-core`）兼容性需重新验证；
> ② 本项目尚未开始开发，无理由在 M0 同时承担「Vite 8 回归 + TS 7 编译器换代」两项高风险变更；
> ③ `~6.0.3` 的范围写法**天然阻断**自动升到 7.x（`~` 只允许 patch），配合 `--frozen-lockfile` 可确保 CI 不会意外拉取 TS 7。
> **重新评估时点**：TS 7 生态成熟且 `vue-tsc` 明确支持后，作为独立决策项评估，不与功能开发混排。届时需同步更新 §3.3 的 tsconfig 与 `ignoreDeprecations` 设置。

#### 3.2.2 前端核心

| 依赖 | 锁定版本 | 用途 | 备注 |
| --- | --- | --- | --- |
| `vue` | `^3.5.43` | UI 框架 | 2026-09-21 跟进上游（3.5.42→3.5.43，patch） |
| `pinia` | `^4.0.3` | 状态管理 | **仅支持 ESM**；项目本身即 ESM，无影响 |
| `vue-router` | `^5.3.1` | 路由 | 2026-09-21 跟进上游（5.2.0→5.3.1，minor）；整合文件式路由，官方声明无破坏性变更 |
| `@tauri-apps/api` | `^2.11.1` | IPC 客户端 | — |
| `@tauri-apps/cli` | `^2.11.5` | 构建工具（devDep） | 2026-09-21 跟进（2.11.4→2.11.5）；与 Rust crate 版本体系独立，无需一致 |
| `@vue/devtools-api` | `^8.2.1` | 开发调试 | 仅 dev；由 `8.x` 范围写法收敛为具体版本 |

#### 3.2.3 UI 与样式

| 依赖 | 锁定版本 | 用途 | 备注 |
| --- | --- | --- | --- |
| `tailwindcss` | `4.3.3` | 原子化 CSS | 支持 `--watch --poll` |
| `shadcn-vue` | `2.8.2` | 组件库（基于 Reka UI） | 组件**复制入仓**而非依赖，便于定制 |
| `lucide-vue-next` | `1.0.0` | 图标 | 主版本；按需 tree-shaking 引入 |
| `reka-ui` | 随 shadcn-vue | 无样式组件底座 | 由 shadcn-vue 引入 |

#### 3.2.4 编辑器与渲染

| 依赖 | 锁定版本 | 用途 | 备注 |
| --- | --- | --- | --- |
| `md-editor-v3` | `^6.5.6` | MVP 阶段编辑器 | M2 使用，M8 被替换。⚠️ **刻意不升到 7.0.0**：上游已发布 major 版（2026-09-21 核实 latest = 7.0.0），但该包在 M8 即被 CodeMirror 6 整体替换，为将废弃的临时方案做 breaking 升级投入产出比过低。`^6.5.6` 的范围天然阻断升到 7.x |
| **CodeMirror 6**（M8 引入） | 见下方子包清单 | 正式版编辑器内核 | ✅ **2026-09-21 已锁定各子包版本**（原为「各子包最新，开发前核实」） |
| `markdown-it` | `^15.0.2` | Markdown → HTML | 🔴 安全版：修复 linkify DoS、smartquotes 二次复杂度 |
| `dompurify` | `^3.4.15` | HTML 净化 | 🔴 安全版：修复 `setConfig()` 绕过 clone-guard 的 XSS；2026-09-21 跟进上游（3.4.12→3.4.15，patch，安全库建议及时跟进） |
| 代码高亮 | **两阶段，均不引入第三方高亮库** | 代码块着色 | 详见下方「代码高亮方案」 |

##### CodeMirror 6 子包清单（M8 引入，2026-09-21 核实 latest）

| 包 | 版本 | 用途 | 是否高亮必需 |
| --- | --- | --- | --- |
| `codemirror` | `^6.0.2` | meta 包，含 `basicSetup` | 可选（手动组装时不需要） |
| `@codemirror/state` | `^6.7.5` | `EditorState` | 必需（编辑器基础） |
| `@codemirror/view` | `^6.43.12` | `EditorView`；`syntaxHighlighting()` 返回的 `ViewPlugin` 需由此挂载 | ✅ **高亮必需** |
| `@codemirror/language` | `^6.12.4` | **导出 `syntaxHighlighting()` 与 `HighlightStyle.define()`** | ✅ **高亮必需** |
| `@codemirror/commands` | `^6.11.1` | 编辑命令与快捷键 | 编辑器需要，高亮不需要 |
| `@codemirror/search` | `^6.7.2` | 查找替换（FR-EDITOR-09） | 编辑器需要，高亮不需要 |
| `@codemirror/autocomplete` | `^6.20.3` | `[[` 链接补全、`#` 标签补全 | 编辑器需要，高亮不需要 |
| `@codemirror/lang-markdown` | `^6.5.2` | Markdown 语言支持（含 wikilink/tag 扩展语法的挂载点） | ✅ 必需 |
| `@codemirror/lang-javascript` | `^6.2.5` | 代码块内 JS/TS 高亮 | ✅ 阅读态需要 |
| `@lezer/highlight` | `^1.2.3` | **导出 `tags`、`classHighlighter`、`highlightCode()`、`highlightTree()`** | ✅ **高亮必需** |
| `@lezer/markdown` | `^1.7.2` | Markdown 的 Lezer 语法树 | ✅ 必需 |
| `@lezer/javascript` | `^1.5.5` | 阅读态 JS 代码块解析 | 阅读态需要。⚠️ 导出名为 `parser`（`LRParser` 实例），**不是** `javascript` |
| `@codemirror/legacy-modes` | `^6.5.4` | 约 100 种语言的移植 mode（sql/shell/ruby/toml/powershell/lua/perl/swift/clike 等） | 🟡 **待 M8 spike 验证可用性**（`DEBT-09`）：其导出为 StreamParser，能否供 `highlightCode()` 使用未验证。按 `./mode/<lang>` 子路径导出，可 tree-shake |
| `@codemirror/lang-sql` | `^6.10.0` | SQL 语言支持（`@lezer/sql` **不存在**，SQL grammar 在此包内） | 若需 SQL 高亮则必需；已核实真实存在 |
| `@lezer/python`、`@lezer/css`、`@lezer/html`、`@lezer/json`、`@lezer/xml`、`@lezer/rust`、`@lezer/go`、`@lezer/java`、`@lezer/cpp`、`@lezer/php`、`@lezer/yaml` 等 | **M8 前逐个核实** | 阅读态其他语言代码块解析 | 官方 `@lezer/*` 约 14 种（已核实 `rust` 1.0.3、`yaml` 1.0.4 活跃）；**`@lezer/sql`、`@lezer/bash` 不存在**。按支持的语言清单引入，不支持的语言不引入 |

> **易错点（已核实）**：`HighlightStyle`、`defaultHighlightStyle`、`syntaxHighlighting()` 三个符号**属于 `@codemirror/language`**，不在 `@lezer/highlight` 中；后者提供的是 `tags`、`styleTags`、`classHighlighter`、`tagHighlighter`、`highlightTree()`、`highlightCode()`、`getStyleTags()`。M8 实现时按此对应关系 import，勿混淆。
>
> 各 `@lezer/<语言>` parser 包为独立发布、版本号互不相同（如 javascript 1.5.5、python 1.1.19），**M8 启动前需逐个核实**当前 latest 后锁定；本项目不支持的语言**不引入**其 parser，以控制包体与攻击面。

##### 代码高亮方案（两阶段，统一由 CodeMirror 6 / Lezer 体系承担）

> **决策日期 2026-09-21**：M8 替换为 CodeMirror 6 后，**全部代码高亮统一使用 CodeMirror 6 原生方案**，不引入 `shiki`、`highlight.js` 等任何第三方独立高亮库。此前 §3.2.4 标注的「M8 需单独选型（倾向 shiki）」作废。

| 阶段 | 高亮路径 | 实现 |
| --- | --- | --- |
| **MVP（M2~M7）** | `md-editor-v3` **内置** highlight.js 集成 | 开箱即用，仅需 `codeTheme` 属性选主题（如 `github`）；**不在 `dependencies` 中单独声明高亮库**，随 `md-editor-v3` 一同引入、M8 一并移除 |
| **正式版（M8+）·编辑态** | CodeMirror 6 原生 | `syntaxHighlighting(HighlightStyle.define(...))` 作为 `EditorView` extension；`@lezer/highlight` 的 `tags` 提供语义标签，`@codemirror/lang-markdown` 提供 Markdown 语法树 |
| **正式版（M8+）·阅读态** | **Lezer 独立高亮 API**（关键） | `@lezer/highlight` 的 **`highlightCode()`** |

**阅读态是必须单独说明的关键点**：阅读态的渲染管线是 `markdown-it` → HTML 字符串 → DOMPurify → 插入 DOM（§7.2），**完全不经过 CodeMirror 编辑器实例**，因此 `syntaxHighlighting()` extension 在此**不生效**。解决方式是使用 `@lezer/highlight` 提供的、可脱离 `EditorView` 独立调用的函数：

```typescript
// core/markdown/highlight.ts —— 阅读态代码块高亮（M8+）
import { highlightCode, classHighlighter } from '@lezer/highlight';
import type { LRParser } from '@lezer/common';

// ⚠️ 导入名易错点（已核实 2026-09-21）：`@lezer/*` 语言包导出的是名为 `parser`
// 的 LRParser 实例，**不是**以语言名命名的变量。
// 正确：import { parser } from '@lezer/javascript'
// 错误：import { javascript } from '@lezer/javascript'  ← 该导出不存在，无法编译
import { parser as jsParser } from '@lezer/javascript';
import { parser as pyParser } from '@lezer/python';

/// 超过此行数的代码块跳过高亮（见 HL-07：highlightCode 是同步函数，会阻塞主线程）
const HIGHLIGHT_LINE_LIMIT = 2000;

/// parser 模块级单例复用（HL-04）。键为小写规范化后的 info string。
const PARSERS: ReadonlyMap<string, LRParser> = new Map([
  ['javascript', jsParser], ['js', jsParser], ['jsx', jsParser],
  // ⚠️ TS/TSX 没有官方 Lezer grammar：不得映射到 jsParser（会错误高亮类型语法），
  //    按 HL-03 降级为纯文本；是否引入 TS grammar 见 DEBT-09。
  // ['typescript', jsParser], ['ts', jsParser], ['tsx', jsParser],
  ['python', pyParser], ['py', pyParser],
  // 其余语言按需注册，见下方「语言覆盖」表
]);

/**
 * 将代码文本转为带 class 的 span 片段（纯文本 → HTML，无执行路径，满足 FR-EDITOR-44）。
 * 输出随后仍须经 sanitizeHtml() 净化（§9.3），不得跳过。
 */
export function highlightCodeBlock(code: string, lang: string): string {
  const parser = PARSERS.get(lang.trim().toLowerCase());
  // HL-03：未支持的语言降级为纯文本转义，不报错、不破版
  if (!parser) return escapeHtml(code);
  // HL-07：超大代码块跳过解析，避免同步高亮长时间阻塞主线程
  if (countLines(code) > HIGHLIGHT_LINE_LIMIT) return escapeHtml(code);

  const tree = parser.parse(code);
  const parts: string[] = [];
  // highlightCode(code, tree, highlighter, putText, putBreak, from?, to?)
  highlightCode(
    code,
    tree,
    classHighlighter,   // 输出 `tok-keyword` / `tok-comment` / `tok-string` 等可读类名（非混淆名）
    (text, classes) => {
      // HL-02：转义必须发生在包裹 span 之前
      parts.push(classes ? `<span class="${classes}">${escapeHtml(text)}</span>` : escapeHtml(text));
    },
    () => parts.push('\n'),
  );
  return parts.join('');
}
```

**与 markdown-it 的集成方式**：覆盖 `md.renderer.rules.fence`，在其中调用 `highlightCodeBlock(token.content, token.info)`。要点：

- `token.info` 是 fence 的 info string（` ```rust ` 中的 `rust`），**可能含额外参数**，须 `trim().split(/\s+/)[0]` 取首段
- **inline 代码（单反引号）不做语法高亮**：内容过短无法可靠判定语言，主流实现（含 markdown-it 生态）亦默认关闭
- 未知语言 fallback 为 `<pre><code>` 纯文本，**不做**语言自动探测（探测有误判风险且增加开销）

**DOMPurify 兼容性（已核实）**：`<span>` 标签与 `class` 属性均在 DOMPurify **默认白名单内**，无需为本方案额外放宽配置——§9.3 现有配置即可放行高亮结果，同时仍拦截 `script`/`iframe`/`on*`。⚠️ 但 §9.3 的 `FORBID_TAGS` **必须**保留 `style`：若高亮改为输出内联 `style=""` 属性或注入 `<style>` 块，会绕过 `class`-based 的主题隔离，形成 CSS 注入面（`DEBT-02`）。**本方案坚持只用 `class`，不用内联样式**。

##### 语言覆盖（2026-09-21 核实，这是本方案的主要权衡点）

CodeMirror 体系的语言支持分三层，覆盖面差异很大：

| 层 | 包 | 语言数 | 语法精度 | 说明 |
| --- | --- | --- | --- | --- |
| ① Lezer 原生 grammar | `@lezer/*` | **约 14 种** | 最高（真实语法树） | javascript、python、css、html、json、xml、rust、go、java、cpp、php、sass、markdown、yaml。**已核实 `@lezer/rust` 1.0.3、`@lezer/yaml` 1.0.4 真实存在且活跃**；**`@lezer/sql`、`@lezer/bash` 确认不存在（npm 404）** |
| ② CodeMirror 官方语言包 | `@codemirror/lang-*` | 十余种 | 高 | 部分语言只在 `@codemirror/lang-*` 中提供 grammar 而无独立 `@lezer/*` 包，例如 **`@codemirror/lang-sql` 6.10.0 已核实存在**（内置 SQL grammar）。SQL 可高亮，但需从此包取 parser |
| ③ 移植的 legacy modes | `@codemirror/legacy-modes` 6.5.4 | **约 100 种** | 中（基于旧 StreamParser，正则式） | 已核实含 `sql`、`shell`、`ruby`、`toml`、`powershell`、`lua`、`perl`、`swift`、`r`、`haskell`、`erlang`、`dockerfile`、`nginx`、`protobuf`、`diff`、`clike`（C/C++/C#/Java/Scala 等）、`vbscript`、`yaml`、`xml`… 按 `./mode/<lang>` 子路径导出，可 tree-shake 按需引入 |

> ⚠️ **②③ 层的可用性需 M8 spike 验证**（诚实标注：本次未验证）：`legacy-modes` 导出的是 **StreamParser**（旧式流式接口），需经 `StreamLanguage.define()` 包装为 `Language` 后，才能取其内部 `LRParser` 供 `highlightCode()` 使用。**该链路是否可行、以及包装后的语法树能否被 `highlightCode()` 正确遍历，未经实测**。M8 必须先做一个最小 spike（取 `legacy-modes/mode/shell` 走通一次高亮）再决定语言覆盖范围。若不可行，②③ 层语言在阅读态将无法高亮，只能走 `HL-03` 的纯文本降级。

**结论与权衡**：

| 维度 | 本方案（Lezer） | 对照：highlight.js | 对照：shiki |
| --- | --- | --- | --- |
| 语言数 | ①层 14 种可靠；③层约 100 种**待验证** | 193 种，成熟 | 200+，成熟 |
| 语法精度 | **最高**（真实语法树） | 中（正则近似） | 高（TextMate） |
| 与编辑态一致性 | **✅ 同一套语法体系，编辑/阅读高亮必然一致** | ❌ 两套体系，配色与切分可能不同 | ❌ 两套体系 |
| 运行时开销 | 同步解析，需 `HL-07` 守卫 | 正则匹配，较轻 | 构建期预渲染则为零 |
| 额外依赖 | 无（M8 本就要装 CodeMirror） | +1 个库 | +1 个库（含 grammar 数据，体积较大） |

**本方案的核心优势是「编辑态与阅读态共用一套语法体系」**——这是 `HL-05`（两态配色一致）能低成本达成的根本原因，也避免了引入第三方库。**代价是①层语言仅 14 种**，超出部分依赖③层的可行性验证。

**若 M8 spike 证明③层不可用**，则出现取舍：
- **选项 1**：接受 14 种语言上限，其余纯文本降级（保持零第三方依赖与两态一致）
- **选项 2**：阅读态改用 highlight.js/shiki，接受两套语法体系（违反 `ED-06`，需正式变更该约束）

此为 `DEBT-09`，决策点在 M8 spike 之后、M8 实现之前。

**M8 实现约束**

| 约束 | 内容 |
| --- | --- |
| `HL-01` | 阅读态高亮输出**必须**继续经 `sanitizeHtml()`（§9.3）净化后再入 DOM，**不得**因「内容是自家生成的」而跳过净化 |
| `HL-02` | `escapeHtml` 必须对 `&`、`<`、`>`、`"`、`'` 全部转义；高亮片段的拼接顺序必须保证转义发生在**包裹 span 之前** |
| `HL-03` | 未支持的语言**降级为纯文本转义**，不得报错或渲染破损 |
| `HL-04` | parser 实例**必须**复用（模块级单例），禁止每个代码块重新创建——`@lezer/*` parser 的构造有开销 |
| `HL-05` | 主题一致性：编辑态 `HighlightStyle.define()` 的配色与阅读态 `classHighlighter` 的 class 映射**必须**共用同一套 CSS 变量（`core/theme/`），避免同一笔记在编辑/阅读两态下代码配色不同 |
| `HL-06` | 移除 `md-editor-v3` 时**必须**同步移除其内置的 highlight.js 相关样式与主题配置，避免遗留死代码与重复 CSS |
| `HL-07` | **大代码块守卫（性能）**：`highlightCode()` 是**同步**函数、一次性遍历整棵语法树，无分块或增量能力。超过 `HIGHLIGHT_LINE_LIMIT`（默认 2000 行，可配置）的代码块**跳过解析**，直接输出转义后的纯文本。理由：主线程长时间阻塞会破坏 `NFR-PERF-07`（输入延迟）与滚动流畅度；阈值应经 M8 实测标定 |
| `HL-08` | **只用 `class`，禁止内联样式**：高亮输出必须走 `class` + `core/theme/` 的 CSS 变量，**禁止**输出 `style="..."` 内联属性或注入 `<style>` 块。理由：① 内联样式会绕过主题隔离，且 `<style>` 标签构成 CSS 注入面（`DEBT-02`）② §9.3 的 `FORBID_TAGS` 已含 `style`，输出内联样式将与净化配置冲突 |
| `HL-09` | **语言映射表集中管理**：info string → parser 的映射（含 `js`/`javascript`/`jsx` 等别名归一化）必须集中在单一模块，**禁止**在各渲染钩子内散落 `switch`。大小写一律归一化为小写后查表 |

#### 3.2.5 功能组件

| 依赖 | 锁定版本 | 用途 | 备注 |
| --- | --- | --- | --- |
| `cytoscape` | `3.34.3` | 知识图谱可视化 | ✅ 已核实为 npm 当前 latest |
| `cytoscape-fcose` | `2.2.0` | 大图力导向布局 | ✅ 2026-09-19 核实；peerDep 为 `cytoscape ^3.2.0`，与 `3.34.3` 兼容。fCoSE 为 Bilkent 大学针对大规模图优化，优于 `cytoscape-cola`/`cytoscape-dagre`（后者列为备选） |
| `@tanstack/vue-virtual` | `3.13.39` | 文件树/搜索结果的虚拟滚动 | ✅ **新增选型**（原缺项）：支撑 G6「1 万条列表 ≥55fps」。Headless 组合式 API、`sideEffects: false` 可完全 tree-shake；备选 `vue-virtual-scroller 3.0.5`（组件式，原生支持动态高度） |
| `dayjs` | `1.11.23` | 时间显示、相对时间、回收站按月目录命名 | ✅ **新增选型**（原缺项）：核心 gzip ≈3KB，按需加载 `timezone`/`locale` 插件；备选 `date-fns 4.4.0`（tree-shake 更彻底）、`luxon 3.7.2`（体积过重，不采用） |
| `fuse.js` | `7.5.0` | 命令面板/快速打开模糊匹配 | 纯前端，不走 IPC（PRD FR-PALETTE-10） |
| `vue-draggable-next` | `2.3.0` | 拖拽排序 | 替换已停更的 `vuedraggable`（SEC-06 / R-16） |

### 3.3 TypeScript 6.0 配置

TS 6.0 是 Go 重写版（TS 7.0）之前**最后一个 JS 实现版本**，API 与 5.9 兼容，但多项默认值发生重大变更。

#### 3.3.1 默认值变更与应对

| 配置项 | 5.x 默认 | 6.0 默认 | 影响 | 本项目应对 |
| --- | --- | --- | --- | --- |
| `strict` | `false` | `true` | 新项目直接获得严格模式 | 符合 CODE-01，无需改动 |
| `types` | 自动包含全部 `@types` | `[]` | **内置模块类型大量丢失** | 必须显式声明 `"types": ["node", "vite/client"]` |
| `module` | `commonjs` | `esnext` | 符合 ESM 项目 | 无需改动 |
| `target` | `es5` | 当前年份 ES 版本 | 输出更现代 | 显式设 `es2024`，避免随年份漂移导致产物变化 |
| `rootDir` | 自动推断 | 不推断，默认 `.` | 输出路径可能变为 `./dist/src/index.js` | 显式设 `"rootDir": "./src"` |

#### 3.3.2 tsconfig.json

```json
{
  "compilerOptions": {
    "strict": true,
    "types": ["node", "vite/client"],
    "module": "esnext",
    "target": "es2024",
    "rootDir": "./src",
    "moduleResolution": "bundler",
    "jsx": "preserve",
    "resolveJsonModule": true,
    "isolatedModules": true,
    "verbatimModuleSyntax": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true,
    "exactOptionalPropertyTypes": true,
    "skipLibCheck": true,
    "ignoreDeprecations": "6.0",
    "baseUrl": ".",
    "paths": {
      "@/*": ["./src/*"],
      "@core/*": ["./src/core/*"],
      "@features/*": ["./src/features/*"],
      "@shared/*": ["./src/shared/*"]
    }
  },
  "include": ["src/**/*.ts", "src/**/*.vue"],
  "exclude": ["node_modules", "dist", "src-tauri"]
}
```

测试文件的类型检查由独立配置承担（避免 TS6059）：

```json
{
  "extends": "./tsconfig.json",
  "compilerOptions": {
    "rootDir": ".",
    "noEmit": true,
    "types": ["node", "vitest/globals"]
  },
  "include": ["tests/**/*.ts", "src/**/*.test.ts", "src/**/*.spec.ts"]
}
```

| 配置说明 | 理由 |
| --- | --- |
| `ignoreDeprecations: "6.0"` | 暂时忽略 6.0 中标记废弃但尚未移除的选项，为 TS 7.0 迁移留缓冲 |
| `exactOptionalPropertyTypes: true` | 与 Rust `Option<T>` 语义对齐，避免 `undefined` 与「字段缺失」混淆导致的 IPC 契约错位 |
| `verbatimModuleSyntax: true` | 强制显式 `import type`，配合 `isolatedModules` 保证 Rolldown/Oxc 单文件转译正确 |
| `paths` 别名 | 支撑 §2.2 的模块边界；ESLint 规则据此强制 FE-02/03（禁止 feature 互引、禁止 core 反向依赖） |
| `skipLibCheck: true` | TS 6 新默认值变更可能导致第三方 `.d.ts` 报错，此项避免被上游类型问题阻塞 |
| `types: ["node","vite/client"]` | **前置条件**：devDependencies 必须包含 `@types/node`（TS 6 默认 `types: []`，缺失会丢内置模块类型） |
| `rootDir: "./src"` 且 `include` 不含 `tests/**` | 二者**互斥**：把测试文件放进 `include` 会触发 TS6059（文件不在 rootDir 下）。测试类型检查由独立的 `tsconfig.test.json` 承担，命令：`vue-tsc -p tsconfig.test.json --noEmit`（并入 CI 门禁 1） |

> **vue-tsc 兼容性**：Vue 语言工具团队已在 PR #6123 适配 TS 6/7 的 API 变更；`@vue/tsconfig` 已将对等依赖放宽至 `>= 5.8`（含 TS 6）。M0 阶段必须实测验证（RISK-02）——✅ **M0 已实测通过（2026-09-25，附录 D.1）**：`vue-tsc --noEmit` 在 TS 6.0.3 下通过。
>
> **迁移工具**：官方提供 `ts5to6`，可自动调整 `baseUrl` 与 `rootDir`。本项目为新建，无需迁移。**TS 版本不再保留回退选项**（2026-09-21 决策，见 PRD 附录 A.2）：TS 6.0 与 5.9 API 兼容，回退无实际收益，反而使 `tsconfig` 需维护两套默认值假设。

### 3.4 Vite 8 风险评估与配置

#### 3.4.1 破坏性变更对照

| 旧配置（Vite 6） | 新配置（Vite 8） | 处理 |
| --- | --- | --- |
| `build.rollupOptions` | `build.rolldownOptions` | 使用新名（旧名自动转换但已弃用） |
| `worker.rollupOptions` | `worker.rolldownOptions` | 同上 |
| `manualChunks`（对象形式） | `codeSplitting` | 使用新形式。⚠️ M0 实测：`advancedChunks` 在 Vite 8.3.x **已废弃**（构建时输出 `advancedChunks option is deprecated, please use codeSplitting instead`） |
| `import.meta.hot.accept(URL)` | **已移除** | 改用模块 ID 字符串形式；本项目 HMR 依赖 `@vitejs/plugin-vue` 自动注入，无需手写 |

#### 3.4.2 v5 已确认的生产回归

| # | 问题 | 对本项目的实际影响评估 | 规避手段 |
| --- | --- | --- | --- |
| 1 | **字符串枚举别名编译错误**：对带别名成员的字符串 `const enum` 生成错误代码（输出反转映射） | 🟡 **中等**。本项目在 IPC 错误码中大量使用字符串枚举（PRD §5.2 的 `KpErrorCode`）。若使用 `const enum` + 别名成员会直接命中此 bug | **规避**：错误码与全部跨边界枚举**不使用 `const enum`**，改用 `as const` 对象字面量 + 联合类型。这同时更符合 `isolatedModules`/`verbatimModuleSyntax` 约束 |
| 2 | **`writeBundle` hook 非确定性遗漏 chunk**：约每 28 次生产构建有 1 次遗漏一个已发出的 chunk | 🔴 **高**。本项目为 Tauri 桌面应用，遗漏 chunk 会导致生产包白屏或部分视图加载失败，且**间歇性**难以复现排查 | **规避**：① 本项目不使用自定义 `writeBundle` 插件 ② CI 中对构建产物执行**完整性校验**（§11.4 门禁 9 扩展：校验 `dist/assets/` 中每个被 `index.html` 与懒加载路由引用的 chunk 均存在）③ 生产构建重复执行 3 次并比对产物文件清单一致性 |
| 3 | **DevBundle 崩溃**：开发 bundle 在 TypeScript 项目上崩溃、chunk 数量增加、DevTools 构建回归 | 🟡 **中等**。仅影响开发体验，不影响生产产物 | **规避**：若开发期频繁崩溃，临时用 `vite build --watch` 替代 dev server。**不回退 Vite 版本**（§3.4.3 已决策）；崩溃情况记录后跟踪 Rolldown/Oxc 的 patch 更新 |

#### 3.4.3 决策结论：确定采用 Vite 8，**不设回退路径**

> **决策日期 2026-09-21，状态：已决（`OPEN-01` 关闭）**。此前版本将「回退 Vite 6.4.3」列为备选方案，现**正式取消该退路**。

| 决策 | 内容 |
| --- | --- |
| **采用** | Vite `^8.3.0`（2026-09-21 核实 latest 仍为 8.3.0） |
| **不回退** | 不再保留 6.4.3 作为备选。理由见下 |
| **风险处置方式** | §3.4.2 的三项回归由「可规避/可回退」升级为「**必须规避，规避失败即阻断发布**」 |

**取消回退路径的理由**

1. **回退会重新引入安全风险**。§3.4.2 之所以选 8.x，部分原因是它覆盖了 CVE-2026-39364、CVE-2026-53571 的 ReDoS 修复；回退 6.4.3 需重新论证其安全状态（v5 自身在这点上的表述互相矛盾，见 §12 的 D-11）。保留一条「可能不安全」的退路，等于把安全门禁变成可选项。
2. **双轨维护成本高**。保留回退方案意味着 `vite.config.ts` 要同时兼容 `rolldownOptions`/`rollupOptions`、`codeSplitting`/`manualChunks` 两套写法，CI 也要跑两套构建验证——这是长期负担，而收益只是心理安慰。
3. **回退窗口已过**。回退的唯一合理时机是 M0 脚手架阶段；一旦 M1 之后代码依赖 Vite 8 的产物结构，再回退就是破坏性变更。把它作为常驻备选反而诱导拖延决策。

**因此，§3.4.2 的三项规避手段从「建议」升级为「硬性门禁」**

| # | 回归问题 | 硬性要求 | 门禁 |
| --- | --- | --- | --- |
| 1 | 字符串枚举别名编译错误 | 跨边界枚举**禁止**使用 `const enum`，一律用 `as const` 对象 + 联合类型 | ESLint 规则拦截（§11.7.5）+ 门禁 2 |
| 2 | `writeBundle` 非确定性遗漏 chunk | **禁止**使用自定义 `writeBundle` 插件；CI 必须连续 3 次生产构建并比对产物清单一致；必须校验 `dist/assets/` 中每个被引用的 chunk 均存在 | 门禁 9（`verify-build-integrity.mjs` + `compare-build-manifests.mjs`，§11.4） |
| 3 | DevBundle 在 TS 项目崩溃 | 若开发期崩溃，用 `vite build --watch` 替代 dev server 作为临时手段，**不视为回退理由** | M0 实测记录 |

**M0 必须完成的验证**（`OPEN-01` 的验收内容）

1. 用 `as const` 对象替代 `const enum` 后，验证枚举在开发/生产两种模式下取值一致
2. 连续执行 30 次生产构建，比对产物文件清单，确认无间歇性遗漏
3. 校验 `dist/` 中全部懒加载 chunk 均存在且可被正确引用
4. **验证失败时的处置**：不再回退 Vite 版本，而是① 记录具体失败模式 ② 评估是否可通过 Vite 插件配置或升级 Vite patch 版解决 ③ 若 Vite 8 分支存在无法规避的阻断性缺陷，作为**独立的高优先级决策项**上报，重新评审而非静默降级

> **附录 A 的同步修正**：PRD 附录 A 与本文档 §3.2.1 中「回退 Vite 6.4.3」的降级预案已作废，改为「升级 Vite patch 版 / 调整规避手段」。PRD `RISK-01` 的缓解措施同步更新。

#### 3.4.4 vite.config.ts

```typescript
import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';
import tailwindcss from '@tailwindcss/vite';
import { resolve } from 'node:path';

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [vue(), tailwindcss()],
  resolve: {
    alias: {
      '@': resolve(__dirname, 'src'),
      '@core': resolve(__dirname, 'src/core'),
      '@features': resolve(__dirname, 'src/features'),
      '@shared': resolve(__dirname, 'src/shared'),
    },
  },
  // Tauri 需要固定端口，失败即报错而非自增
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
    watch: { ignored: ['**/src-tauri/**'] },
  },
  // 生产构建不生成 sourcemap（SEC-14：禁止公开分发 source map）
  build: {
    target: 'es2024',
    sourcemap: false,
    minify: 'oxc',
    // Vite 8：使用 rolldownOptions（非 rollupOptions）
    rolldownOptions: {
      output: {
        // Vite 8.3：使用 codeSplitting（advancedChunks 已废弃，见 M0 核实报告）
        codeSplitting: {
          groups: [
            { name: 'vendor-vue', test: /node_modules[\\/](vue|@vue|pinia|vue-router)[\\/]/ },
            { name: 'vendor-editor', test: /node_modules[\\/](md-editor-v3|markdown-it|dompurify|@codemirror|@lezer)[\\/]/ },
            { name: 'vendor-graph', test: /node_modules[\\/](cytoscape|cytoscape-fcose)[\\/]/ },
          ],
        },
      },
    },
  },
  // 环境变量前缀：仅 VITE_ 开头暴露给前端，防止误泄 Rust 侧配置
  envPrefix: 'VITE_',
});
```

> **注（M0 已核实，2026-09-25）**：`minify: 'oxc'` 是 Vite 8 的**默认值**且取值合法（官方类型为 `boolean | 'oxc' | 'terser' | 'esbuild'`）；`build.rolldownOptions.output.advancedChunks` 可用但**已废弃**，应改用 `codeSplitting.groups`（两者结构相同，实测均能正确产出 vendor chunk）。

### 3.5 Rust 依赖矩阵

#### 3.5.1 继承 v5 的已锁定项（已复核）

> **核实日期 2026-09-19，2026-09-21 跟进上游更新**，数据源为 crates.io 官方 API。下列版本经复核**真实存在且为当前稳定版**，其中 `tauri-build` 修正了 v5 的过期版本。

| Crate | 锁定版本 | 复核结论 | 用途 |
| --- | --- | --- | --- |
| `tauri` | `2.12.0` | ✅ 2026-09-30 跟进（2.11.6→2.12.0，minor）；**要求 rustc ≥ 1.90**，据此把项目 MSRV 提升至 1.90；`3.0.0-alpha.x` 为预发布，禁用 | 桌面框架 |
| `tauri-build` | `2.7.0` | ✅ 与 tauri 2.12.0 配套（随 MSRV 1.90 一并跟进） | 构建脚本 |
| `rusqlite` | `0.40.2`（`bundled`） | ✅ `bundled` feature 确认存在 | SQLite 访问 |
| `serde` | `1.0.229`（`derive`） | ✅ | 序列化 |
| `serde_json` | `1.0.151` | ✅ | JSON |

#### 3.5.2 新增依赖（版本号已于 2026-09-19 联网核实并锁定）

> ✅ **原 `TBD` 占位已全部替换为核实过的版本号**（数据源：crates.io 官方 API）。
> 下表「锁定版本」列指 **M0 首次 `cargo build` 后应写入 `Cargo.lock` 的精确值**；
> 写入 `Cargo.toml` 时按 Cargo 的 caret 语义声明为范围（见 §3.6.1）。
>
> ⚠️ **MSRV 硬约束（2026-09-30 再次修正）**：`tauri 2.12.0` 及其同族 crate（`tauri-build 2.7.0`、`tauri-utils 2.10.0`、`tauri-runtime 2.12.0`、`tauri-runtime-wry 2.12.0`、`tauri-codegen/macros/plugin 2.7.0`、`muda 0.20.0`）的 `rust-version` 均为 **1.90**，因此**本项目实际有效 MSRV 为 1.90**。

| Crate | 锁定版本 | 用途 | 选型理由 | 备选 | 引入阶段 |
| --- | --- | --- | --- | --- | --- |
| `jieba-rs` | `0.11.0` | 中文分词（FTS5 预分词） | 纯 Rust、词典内置、无原生依赖、社区活跃（2026-09-16 刚发布） | `tantivy`（自带 CJK tokenizer，但引入完整搜索引擎偏重）、`cang-jie`（绑定 jieba C++，需交叉编译） | M3 |
| `notify` | `8.2.0` | 跨平台文件监听 | Rust 生态事实标准，统一封装 inotify/FSEvents/ReadDirectoryChangesW。⚠️ `9.0.0-rc.5` 为预发布，禁用 | `ignore`（含 gitignore 语义，若需尊重 `.gitignore` 则更合适） | M1 |
| `notify-debouncer-full` | `0.7.0` | notify 去抖动封装（索引器必需，避免保存时重复触发） | 与 `notify 8.x` 配套（依赖 `notify ^8`）。⚠️ 两者版本号**不要求相同**，但 `0.8.0-rc.2` 为预发布，禁用 | 自研去抖（易出错） | M1 |
| `walkdir` | `2.5.0` | 递归目录遍历 | 流式迭代、内存友好、支持深度控制与过滤 | `jwalk`（并行遍历，全量扫描更快，但引入 rayon 依赖） | M1 |
| `sha2` | `0.11.0` | SHA-256（索引签名、更新校验、内容哈希） | RustCrypto 出品，纯 Rust。⚠️ **major bump**（0.10→0.11）：移除了 `asm`/`compress`/`force-soft`，新增 `alloc`/`oid`/`zeroize`，MSRV 1.85 | `ring`（含加密原语，但体积大且构建复杂） | M1 |
| `thiserror` | `2.0.21` | `AppError` 枚举派生 | 库层错误定义标准做法。⚠️ **major bump**（1.x→2.x），API 有 breaking change，代码示例按 2.x 语义书写 | 手写 `Display`/`Error` 实现 | M0 |
| `anyhow` | `1.0.104` | Command 层错误聚合 | 应用层错误处理便利 | 仅用 `thiserror` | M0 |
| `tokio` | `1.53.1` | 异步运行时 | Tauri 2 已内置 tokio，复用即可，**注意避免引入第二个运行时**。所需 features `rt-multi-thread`/`sync`/`fs`/`macros` 均确认存在 | Tauri 自带的 `tauri::async_runtime` | M0 |
| `yaml-rust2` | `0.13.0` | frontmatter YAML 解析 | ✅ **选型已确认**：`serde_yaml` 已于 2024-03-25 标记 deprecated 停止维护，社区接续版 `serde_yml` **也已废弃**（其最新版明确指向 `noyalib`）。`yaml-rust2` 为当前**唯一活跃维护**的纯 Rust YAML 解析器（2026-09-11 更新），MSRV 1.85 | 自研极简 YAML 子集解析器（仅需支持 frontmatter 常见结构，可控且无依赖风险）；若需 serde `Deserialize` 派生则改用 `noyalib` | M3 |
| `regex` | `1.13.1` | 搜索高级语法解析、wikilink 匹配 | Rust 官方 regex，线性时间保证（无 ReDoS） | `fancy-regex`（若需回溯特性，但引入 ReDoS 风险，**不建议**） | M3 |
| `tracing` | `0.1.44` | 结构化日志 | 生态标准，支持级别过滤与多 sink | `log` + `fern` | M0 |
| `tracing-subscriber` | `0.3.23`（`env-filter`） | 日志过滤与格式化 | 与 `tracing` 配套，`env-filter` feature 确认存在 | — | M0 |
| `tracing-appender` | `0.2.5` | 日志按日滚动（PRD §2.4.2 保留 14 天） | 与 `tracing` 配套，`rolling::daily` 原生支持 | 手写滚动 | M0 |
| `uuid` | `1.26.1`（`v4`） | `operation_id` / `preview_id` | 取消令牌与预览缓存的键。`v4` feature 确认存在（依赖 `rng`），MSRV 1.85 | 自增计数器（但跨重启不安全） | M1 |
| `dashmap` | `6.2.1` | 并发哈希表（cancellations、previews） | 无锁读、分片写。⚠️ `7.0.0-rc2` 为预发布，禁用 | `std::sync::RwLock<HashMap>` | M1 |
| `time` | `0.3.55`（`formatting`, `local-offset`） | 时间戳、回收站按月目录、过期计算 | ✅ **选型已确认**：`time` 比 `chrono` 更轻量无历史包袱。其自身要求 Rust ≥ 1.88，**低于项目 MSRV 1.90，自 2026-09-30 起不再是瓶颈** | `chrono`（生态更广，MSRV 更低） |
| `dunce` | `1.0.5` | Windows 路径规范化（去除 `\\?\` 前缀的可读形式） | 解决 Windows 长路径显示问题（NFR-PLAT-10），无 features | 手写 | M1 |
| `base64` | `0.23.1` | 剪贴板图片 IPC 传输编码（`attachment_paste_image`） | 标准实现。⚠️ **major bump**（0.22→0.23），`Engine` trait 用法与 0.21 不同，代码示例按 0.23 API 书写 | Tauri 自带的 IPC 二进制传输（若支持则更优，避免 base64 膨胀 33%） | M6 |
| `image` | `0.25.10`（`default-features = false`, `png`/`jpeg`/`webp`） | 剪贴板图片格式转换与尺寸读取 | 纯 Rust，三个 feature 均确认存在。其自身要求 Rust ≥ 1.88，**低于项目 MSRV 1.90，不再是瓶颈**；⚠️ `webp` 现委托 `image-webp` crate 实现 | 仅存原始字节不转换（更轻，但无法生成缩略图） |

#### 3.5.3 Tauri 插件（版本已核实，2026-09-19）

> 所有插件均需与 `tauri 2.11.x` 配套（当前锁定 `tauri 2.11.6`）。下列版本为 2026-09-21 crates.io 核实的当前稳定版；**每个插件都存在 `3.0.0-alpha.0` 预发布版，一律禁用**。

| 插件 | 锁定版本 | 用途 | Capabilities 权限 |
| --- | --- | --- | --- |
| `tauri-plugin-updater` | `2.12.0` | 自动更新（PRD §4.12） | `updater:default` |
| `tauri-plugin-dialog` | `2.8.0` | 原生文件/文件夹选择对话框（Vault 选择） | `dialog:allow-open` |
| `tauri-plugin-shell` | `2.4.0` | 外部链接交系统浏览器打开 | `shell:allow-open`（**仅限 `https://` 与 `mailto:`**，SEC-07；不含 `http://` / `file://`） |
| `tauri-plugin-opener` | `2.7.0` | 「在文件管理器中显示」（`file_reveal`）与远程 URL 交系统浏览器。**仅在 Rust 侧调用**（自定义 Command 内） | 无（Rust 侧调用不经过 Capabilities；见 §9.1.1 与 AC-01） |
| `tauri-plugin-process` | `2.4.0` | 更新后重启应用 | `process:allow-restart`（**仅此项，禁止 `process:allow-exit` 之外的宽权限**） |
| `tauri-plugin-log` | 不引入 | 前端日志转发 | 改用自研 `log_report` Command，统一落盘路径与轮转策略（§9.5），避免与 `tracing` 形成两套日志体系 |

> **明确不引入**：`tauri-plugin-fs`（违反 PRD AC-01，全部文件操作走自定义 Command 以统一施加路径校验与原子写入）、`tauri-plugin-sql`（违反 AC-02）、`tauri-plugin-store`（偏好存储走自定义 `global.db`，保持单一数据层）。
>
> ⚠️ M0 实测：`tauri-plugin-fs` 会作为**间接依赖**被其他插件拉入依赖树（编译日志中出现 `tauri-plugin-fs v2.5.2`）。这不违反本约束——约束针对的是**直接声明依赖与前端能力**：`capabilities/default.json` 中不得出现任何 `fs:*` 权限，前端也不得 import `@tauri-apps/plugin-fs`（由 `check-path-encapsulation.sh` 守护）。

#### 3.5.4 Cargo.toml

```toml
[package]
name = "knowl-pad"
version = "0.1.0"
edition = "2021"
rust-version = "1.90"          # ⚠️ 实际有效 MSRV：由 tauri 2.12 家族（含 muda 0.20.0）决定，2026-09-30 核实
description = "跨平台本地优先笔记与知识管理软件"
license = "MIT OR Apache-2.0"

[build-dependencies]
tauri-build = { version = "2.6.3", features = [] }   # 修正：与 tauri 2.11.x 配套（v5 的 2.1.0 已过期）

[dependencies]
# ── 继承 v5 锁定（2026-09-19 复核；2026-09-21 跟进 tauri 至 2.11.6）─────
tauri = { version = "2.11.6", features = [] }
rusqlite = { version = "0.40.2", features = ["bundled"] }
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.151"

# ── Tauri 插件（2026-09-21 跟进；全部禁用 3.0.0-alpha 预发布）──
tauri-plugin-updater     = "2.12.0"   # 2026-09-21 跟进（2.11.0→2.12.0，minor）
tauri-plugin-dialog      = "2.7.3"
tauri-plugin-shell       = "2.3.6"
tauri-plugin-opener      = "2.5.5"
tauri-plugin-process     = "2.3.1"

# ── 新增依赖（2026-09-19 联网核实并锁定，crates.io 官方 API）──
jieba-rs              = "0.11.0"          # 中文分词，M3
notify                = "8.2.0"           # 文件监听（禁用 9.0.0-rc），M1
notify-debouncer-full = "0.7.0"           # 去抖动，与 notify 8.x 配套（禁用 0.8.0-rc），M1
walkdir               = "2.5.0"           # 目录遍历，M1
sha2                  = "0.11.0"          # SHA-256（major bump 0.10→0.11），M1
thiserror             = "2.0.21"          # 错误派生（major bump 1.x→2.x）；M0 实测 2.0.21 可用
anyhow                = "1.0.104"         # Command 层错误聚合，M0
tokio                 = { version = "1.53.1", features = ["rt-multi-thread", "sync", "fs", "macros"] }  # 复用 Tauri 运行时，M0
yaml-rust2            = "0.13.0"          # frontmatter 解析（serde_yaml/serde_yml 均已废弃），M3
regex                 = "1.13.1"          # 搜索语法/wikilink，线性时间无 ReDoS，M3
tracing               = "0.1.44"          # 结构化日志，M0
tracing-subscriber    = { version = "0.3.23", features = ["env-filter"] }  # M0
tracing-appender      = "0.2.5"           # 日志按日滚动，M0
uuid                  = { version = "1.26.1", features = ["v4"] }          # 操作/预览令牌，M1
dashmap               = "6.2.1"           # 并发哈希表（禁用 7.0.0-rc2），M1
time                  = { version = "0.3.55", features = ["formatting", "local-offset"] }  # 其 MSRV 1.88 < 项目 1.90，不再是瓶颈；M1
dunce                 = "1.0.5"           # Windows 路径规范化，M1
base64                = "0.23.1"          # 剪贴板图片编码（major bump 0.22→0.23），M6
image                 = { version = "0.25.10", default-features = false, features = ["png", "jpeg", "webp"] }  # 其 MSRV 1.88 < 项目 1.90；M6
r2d2                  = "0.8.10"           # 读连接池，M1
r2d2_sqlite           = "0.35.0"           # ✅ M0 实测：crate 名为下划线 `r2d2_sqlite`（连字符名在 Cargo 中解析失败）；
                                          # 0.35.0 依赖 rusqlite ^0.40，与 0.40.2 配套；版本号**不跟随** rusqlite
tokio-util            = { version = "0.7.19", features = ["rt"] }  # 异步工具（与 tokio 1.x 配套），M0

[dev-dependencies]
# 测试专用依赖（§11.1）；M0 已复核（附录 D.1：tempfile 3.27.0 编译通过）
tempfile = "3.27.0"
tokio = { version = "1.53.1", features = ["rt-multi-thread", "macros", "test-util"] }

[profile.release]
panic = "abort"        # 发布版直接终止，避免 unwind 后的不确定状态
codegen-units = 1      # 更好的优化
lto = "fat"            # 全量链接时优化，换取最优运行时性能
opt-level = 3          # 性能优先（2026-09-20 决策：暂不考虑打包体积，索引与搜索的运行时性能优先）
strip = true           # 去除符号
debug = false          # 生产不含调试信息（SEC-14）

[profile.release-debug]  # 用于排查发布版问题的独立 profile
inherits = "release"
debug = true
strip = false
```

> **版本锁定纪律**：以上版本号已于 **2026-09-19** 通过 crates.io 官方 API（`https://crates.io/api/v1/crates/<name>`）**抽样核实**为当时稳定版，不再是 `TBD` 占位。⚠️ 该结论不可离线复核，且 `0.x` crate 的适配关系（尤其 `r2d2-sqlite` 与 `rusqlite`）历史上存在失配：**原计划 M0 逐项重新核实并以实际 `Cargo.lock` 回写** —— ✅ **M0 已复核（2026-09-25，附录 D.1）**：`cargo check` 在 rustc 1.98.1 与 **1.88.0** 下均通过，`0.x` 适配关系由真实 `Cargo.lock` 验证（`r2d2_sqlite` 实测取 `0.35.0` 配 `rusqlite ^0.40`）。 ⚠️ **2026-09-30 复核定版**：随 MSRV 提升至 1.90，tauri 家族升到 2.12.0 一档（详见 `docs/history/MSRV升至1.90-2026-09-30.md`）；此后由 `.cargo/config.toml` 的 MSRV 感知解析保证解析结果不越界。
>
> **三条后续纪律**：
> 1. **禁用预发布**：`notify 9.0.0-rc`、`notify-debouncer-full 0.8.0-rc`、`dashmap 7.0.0-rc2`、所有 tauri 插件的 `3.0.0-alpha.0` 均为预发布，**不得**误用。
> 2. **major bump 复核（M0 已实测，2026-09-25）**：`sha2`（0.10→0.11）、`thiserror`（1→2）、`base64`（0.22→0.23）三个 crate 的 API 均已通过真实编译验证。其中一条**必须注意**：`sha2 0.11` 的 `finalize()` 返回 `hybrid-array` 的 `Array`，**不再实现 `LowerHex`**，`format!("{:x}", h.finalize())` 会报 E0277；十六进制需手动编码（如 `d.iter().map(|b| format!("{b:02x}")).collect::<String>()`）。`base64 0.23` 需 `use base64::Engine` trait 才能调用 `.encode()`。
> 3. **MSRV 门禁**：`tauri 2.12.0` 家族要求 Rust ≥ 1.90（`time 0.3.55` 与 `image 0.25.10` 的 1.88 已不再是约束），故 `rust-toolchain.toml`（§9.4.2）与 CI 的 Rust 安装步骤**必须**固定 `1.90` 或更高，低于此版本构建必然失败。

### 3.6 package.json（继承 v5，补充脚本）

```json
{
  "name": "knowl-pad",
  "version": "0.1.0",
  "description": "跨平台本地优先笔记与知识管理软件",
  "private": true,
  "type": "module",
  "engines": { "node": ">=24.19.0" },
  "packageManager": "pnpm@12.4.2",
  "scripts": {
    "dev": "vite",
    "build": "vite build",
    "build:verify": "pnpm build && node scripts/verify-build-integrity.mjs",
    "preview": "vite preview",
    "tauri": "tauri",
    "tauri:dev": "tauri dev",
    "tauri:build": "tauri build",
    "typecheck": "vue-tsc --noEmit && vue-tsc -p tsconfig.test.json --noEmit",
    "lint": "eslint . --max-warnings 0",
    "lint:fix": "eslint . --fix",
    "format": "prettier --write \"src/**/*.{ts,vue,css,json}\"",
    "test": "vitest run",
    "test:watch": "vitest",
    "test:coverage": "vitest run --coverage",
    "test:e2e": "wdio run tests/e2e/wdio.conf.ts",
    "test:security": "node scripts/run-security.mjs",
    "test:reliability": "node scripts/run-reliability.mjs",
    "test:perf": "node scripts/run-perf.mjs",
    "contract:verify": "node scripts/verify-ipc-contract.mjs",
    "fixture:gen": "node scripts/gen-fixture-vault.mjs",
    "gate:clippy": "cargo clippy --workspace --all-targets -- -D warnings",
    "gate:fmt": "cargo fmt --all --check",
    "gate:rust-test": "cargo test --workspace",
    "gate:coverage": "cargo llvm-cov -p kp-domain --fail-under-lines 85",
    "gate:lockfile": "pnpm install --frozen-lockfile --dry-run",
    "gate:naming": "node scripts/run-shell-gate.mjs check-naming",
    "gate:path-encapsulation": "node scripts/run-shell-gate.mjs check-path-encapsulation",
    "gate:rust-build": "cargo build --manifest-path src-tauri/Cargo.toml",
    "gate:rust": "pnpm gate:clippy && pnpm gate:fmt && pnpm gate:rust-test && pnpm gate:coverage && cargo audit --file Cargo.lock",
    "ci": "node scripts/run-gates.mjs"
  },
  "dependencies": {
    "vue": "^3.5.43",
    "pinia": "^4.0.3",
    "vue-router": "^5.3.1",
    "@tauri-apps/api": "^2.11.1",
    "md-editor-v3": "^6.5.6",
    "markdown-it": "^15.0.2",
    "dompurify": "^3.4.15",
    "cytoscape": "^3.34.3",
    "cytoscape-fcose": "^2.2.0",
    "@tanstack/vue-virtual": "^3.13.39",
    "dayjs": "^1.11.23",
    "fuse.js": "^7.5.0",
    "vue-draggable-next": "^2.3.0",
    "lucide-vue-next": "^1.0.0"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2.11.5",
    "@vue/devtools-api": "^8.2.1",
    "@types/node": "^24.0.0",
    "husky": "^9.1.0",
    "lint-staged": "^15.2.0",
    "typescript": "~6.0.3",
    "vite": "^8.3.0",
    "@vitejs/plugin-vue": "^6.0.9",
    "vue-tsc": "^3.3.11",
    "tailwindcss": "^4.3.3",
    "@tailwindcss/vite": "^4.3.3",
    "shadcn-vue": "^2.8.2",
    "eslint": "^9.0.0",
    "@eslint/js": "^9.0.0",
    "typescript-eslint": "^8.0.0",
    "eslint-plugin-vue": "^9.0.0",
    "vue-eslint-parser": "^9.0.0",
    "@vue/eslint-config-typescript": "^14.0.0",
    "prettier": "^3.0.0",
    "prettier-plugin-tailwindcss": "^0.6.0",
    "vitest": "^3.0.0",
    "@vue/test-utils": "^2.4.0",
    "@vitest/coverage-v8": "^3.0.0",
    "jsdom": "^25.0.0",
    "@wdio/cli": "^9.0.0"
  }
}
```

> **CodeMirror 6 / Lezer 子包不在 M0 的 `dependencies` 中**：它们自 M8 才被使用（M2~M7 用 `md-editor-v3`），M8 启动时执行 `pnpm add` 引入，lockfile 变更单独成 commit（§3.6.1 规则 3）。版本清单见 §3.2.4；**禁止提前引入**，以保持包体与攻击面最小。

> **与 v5 的差异**：v5 的 `package.json` 缺少 `@tailwindcss/vite`（Tailwind 4 的 Vite 集成方式，不再用 PostCSS 插件）、测试工具链、Lint 工具链与 CI 脚本。这些是实现 PRD §8.3/§8.4 的必要条件，故补齐。`shadcn-vue` 从 `dependencies` 移至 `devDependencies`——它是 CLI 工具（组件复制入仓），运行时不依赖。

> **CodeMirror 6 子包（`@codemirror/*`、`@lezer/*`）的引入时机**：这批依赖**在 M8 才真正使用**（MVP 阶段 M2~M7 用 `md-editor-v3`）。可在 M8 启动时再 `pnpm add`，届时 lockfile 变更单独成 commit（§3.6.1 规则 3）。**代码高亮不引入任何第三方库**（`shiki` / `highlight.js` 等）：MVP 用 `md-editor-v3` 内置高亮；M8 起编辑态用 `syntaxHighlighting()`、阅读态用 `@lezer/highlight` 的 `highlightCode()`，全部出自 CodeMirror 6 / Lezer 体系（详见 §3.2.4「代码高亮方案」）。`@lezer/javascript` 等语言 parser 按实际支持的语言清单引入，M8 前逐个核实 latest 后锁定。

> **`md-editor-v3` 保持 `^6.5.6`、不升 7.0.0**：上游已发布 major 版（2026-09-21 核实 latest = 7.0.0），但该包在 M8 即被整体替换，为临时方案做 breaking 升级得不偿失；`^6.5.6` 的范围写法天然阻断升到 7.x。

#### 3.6.1 依赖管理策略：**声明层面兼容，安装层面锁定**

> **决策日期：2026-09-20**（取代本文档此前「生产依赖用精确版本」的写法，见 §11.4 的「精确版本锁定」条目已同步修正）

核心思想：**`package.json` 表达「哪些版本是可接受的」，`pnpm-lock.yaml` 决定「实际装的是哪个」**。两者职责分离，使更新流程受控而非随机。

| 层面 | 载体 | 写法 | 作用 |
| --- | --- | --- | --- |
| **声明** | `package.json` | 向后兼容范围（`^` / `~`） | 表达兼容性意图，允许维护者在范围内做安全补丁升级 |
| **锁定** | `pnpm-lock.yaml`（**提交入库**） | 精确版本 + 完整性哈希 | 保证所有人、所有环境、所有构建装到**逐字节相同**的依赖树 |
| **安装** | CI / 生产 | `pnpm install --frozen-lockfile` | 严格按 lockfile 安装；**lockfile 与 manifest 不一致即失败**，绝不自行解析新版本 |

**三条硬性规则**：

1. **CI 与生产环境一律用 `pnpm install --frozen-lockfile`**（npm 生态的等价物是 `npm ci`）。
   **禁止**在 CI 中用 `pnpm install` / `npm install`——那会在 lockfile 落后于 manifest 时静默解析新版本，
   使构建结果不可复现，且可能引入未经审计的代码（违反 `SEC-06` / 威胁 T-09）。
2. **`pnpm-lock.yaml` 与 `Cargo.lock` 必须提交入库**（`.gitignore` 中不得排除，见 §11.7.2）。
   门禁 8 用 `pnpm install --frozen-lockfile --dry-run` 校验一致性（PRD §8.4；M0 实测 pnpm 12.4.2 支持 `--dry-run`）。
3. **升级依赖是显式动作，不是自动行为**：
   `pnpm update <包名>` → 本地跑全部门禁（`pnpm ci`）→ lockfile 变更**单独成 commit**，
   commit message 写明升级原因（安全修复 / 功能需要 / 例行）。**禁止**把 lockfile 变更混在功能提交里，
   否则 review 无法看出依赖树发生了什么变化。

**Rust 侧的等价规则（2026-09-30 补充，起因见 §12 勘误 R-11）**：

| 层面 | 载体 | 写法 | 作用 |
| --- | --- | --- | --- |
| 声明 | `src-tauri/Cargo.toml`、`crates/*/Cargo.toml` | 向后兼容范围（`^` / `~`）+ `rust-version = "1.90"` | 表达兼容性意图 |
| 锁定 | `Cargo.lock`（**提交入库**，工作区根唯一一份） | 精确版本 | 保证所有人构建同一依赖树 |
| 解析 | `.cargo/config.toml` 的 `[resolver] incompatible-rust-versions = "fallback"` + 工作区 `resolver = "3"` | **MSRV 感知解析** | 自动挑选与 1.90 兼容的版本，不会解析到超出已固定 toolchain 能力的依赖 |
| 构建 | CI 与门禁 | `cargo ... --locked` | lockfile 与 manifest 不一致即失败，**绝不自行解析新版本**（Rust 版的 R-17） |

> ⚠️ **不得删除 `.cargo/config.toml`**：缺少它时 cargo 按「最新可用」解析依赖，一旦依赖抬高 MSRV，CI（固定 rustc 1.90）会在 `cargo check` 计划阶段直接失败。
> 也**不能只靠 `--locked`**：它保证「不再重新解析」，防不住**首次解析 / 删锁重建**时选中过高版本——两者互补，缺一不可。

**范围符的选择依据**：

| 包 | 声明 | 为什么用这个范围符 |
| --- | --- | --- |
| 绝大多数依赖 | `^X.Y.Z` | SemVer 承诺 minor/patch 向后兼容，允许自动获得修复 |
| `typescript` | **`~6.0.3`** | ⚠️ **刻意收紧**：TypeScript 的 **minor 版本历来包含破坏性变更**（新增的类型检查会揭示既有代码的错误），`^6.0.3` 会放行 6.1/6.2。TS 官方也不遵循严格 SemVer，故只允许 patch 升级 |
| `dompurify` | `^3.4.15` | 安全关键依赖，但净化库的 patch 常含安全修复，需要能及时获得；配合门禁 7 的 `pnpm audit` 兜底 |
| `vite` | `^8.3.0` | 🟡 已知存在生产回归（§3.4.2），但**已决策不回退 6.x**（§3.4.3）。`^` 范围允许自动获得 8.x 内的 patch 修复——这正是保留 `^` 而非锁死的理由：回归修复通常以 patch 形式发布 |

**lockfile 当前锁定值**（2026-09-19 经 npm 官方 API 核实，即 M0 首次 `pnpm install` 后应产出的锁定结果）：

> **决策日期 2026-09-21：跟进上游更新**。下表为跟进后的锁定值。`typescript` 与 `md-editor-v3` 为**刻意不跟进**项，理由见备注。

| 包 | 声明范围 | lockfile 锁定 | 上游 latest | 说明 |
| --- | --- | --- | --- | --- |
| `vue` | `^3.5.43` | `3.5.43` | `3.5.43` | ✅ 已跟进（3.5.42→3.5.43，patch） |
| `vue-router` | `^5.3.1` | `5.3.1` | `5.3.1` | ✅ 已跟进（5.2.0→5.3.1，minor） |
| `dompurify` | `^3.4.15` | `3.4.15` | `3.4.15` | ✅ 已跟进（安全库 patch） |
| `@tauri-apps/cli` | `^2.11.5` | `2.11.5` | `2.11.5` | ✅ 已跟进 |
| `@vue/devtools-api` | `^8.2.1` | `8.2.1` | `8.2.1` | ✅ 已跟进（原 `8.x` 范围收敛为具体值） |
| `@vitejs/plugin-vue` | `^6.0.9` | `6.0.9` | `6.0.9` | ✅ 已跟进 |
| `vue-tsc` | `^3.3.11` | `3.3.11` | `3.3.11` | ✅ 已跟进 |
| `vite` | `^8.3.0` | `8.3.0` | `8.3.0` | — 无更新 |
| `pinia` | `^4.0.3` | `4.0.3` | `4.0.3` | — 无更新 |
| `@tauri-apps/api` | `^2.11.1` | `2.11.1` | `2.11.1` | — 无更新 |
| `cytoscape` / `cytoscape-fcose` | `^3.34.3` / `^2.2.0` | `3.34.3` / `2.2.0` | 同 | — 无更新 |
| `@tanstack/vue-virtual` | `^3.13.39` | `3.13.39` | `3.13.39` | — 无更新 |
| `dayjs` | `^1.11.23` | `1.11.23` | `1.11.23` | — 无更新 |
| `markdown-it` | `^15.0.2` | `15.0.2` | `15.0.2` | — 无更新（安全版） |
| `tailwindcss` / `@tailwindcss/vite` | `^4.3.3` | `4.3.3` | `4.3.3` | — 无更新 |
| `shadcn-vue` | `^2.8.2` | `2.8.2` | `2.8.2` | — 无更新 |
| **`typescript`** | **`~6.0.3`** | `6.0.3` | `7.0.2`（latest） | ⛔ **刻意不跟进**：`~` 阻断 7.x。理由见 §3.2.1「TS 版本决策」。6.x 分支当前最新即 6.0.3 |
| **`md-editor-v3`** | **`^6.5.6`** | `6.5.6` | `7.0.0` | ⛔ **刻意不跟进**：major breaking，且 M8 即被替换（§3.2.4） |

> **Rust 侧无需改动**：`Cargo.toml` 的 `foo = "0.11.0"` 在 Cargo 语义下**本就是** `^0.11.0`
> （Cargo 默认 caret 匹配，只有 `0.x` 版本例外——`0.11.0` 视为 `^0.11.0`，即允许 `>=0.11.0, <0.12.0`），
> 而 `Cargo.lock` 负责锁定精确版本与 checksum。因此 Rust 侧**已天然符合**本策略，
> 文档中的 crate 版本号写法保持不变，只需确保 `Cargo.lock` 入库（§11.7.2）。
> ⚠️ **`0.x` 版本的语义差异**：Cargo 的 caret 对主版本为 0 的依赖更保守。
> `sha2 = "0.11.0"` 解析为 `>=0.11.0, <0.12.0`（**只允许同 minor 内升级**），
> 而 `tauri = "2.11.6"` 解析为 `>=2.11.6, <3.0.0`（允许整个 major 内升级）。
> 这恰好符合 `sha2`、`base64`、`notify-debouncer-full`、`yaml-rust2`、`dashmap` 等 `0.x` 库
> API 尚未稳定、minor 即可能破坏兼容的现实，**无需额外收紧**。
> 注意 `Cargo.lock` 仍锁定精确版本，因此 `cargo build` 不会自行升级；
> 升级需显式执行 `cargo update -p <crate>` 并跑通门禁后单独提交。

**pnpm 隔离式 node_modules 的作用**（`.npmrc`，§11.7.3）：pnpm 默认的 `node-linker=isolated`
可防止**幽灵依赖**（未在 `package.json` 声明、却因扁平化提升而可被 import 的包）。
幽灵依赖会绕过本策略——它不受 lockfile 的范围约束，升级时可能被静默改变。
⚠️ 注意：本项目的 `.npmrc` **显式设置了 `strict-peer-dependencies=false`**（避免 peer 冲突阻塞安装），
它**不承担**防幽灵依赖职责；该职责由隔离式 node_modules 承担。两者不可混为一谈。

---

## 4. 数据架构

Schema 定义见 PRD §3.2/§3.3（本节不重复 DDL），此处定义**连接管理、PRAGMA 策略、事务模式与迁移实现**。

### 4.1 连接管理策略

SQLite 的并发模型是「单写多读」。本项目的读写比极不均衡（索引期写密集，浏览期读密集），因此采用**分离式连接池**：

```rust
pub struct DbPool {
    /// 唯一的写连接，由专用线程持有，通过 channel 接收写请求
    writer: mpsc::Sender<WriteJob>,
    /// 读连接池（r2d2 或手写 Vec<Connection> + 空闲队列）
    readers: Arc<Pool<SqliteConnectionManager>>,
}

enum WriteJob {
    /// 批量索引写入（索引引擎专用）
    IndexBatch { items: Vec<ParsedNote>, done: oneshot::Sender<Result<usize>> },
    /// 单条业务写入（偏好、Vault 注册、回收站元数据）
    Single { sql: SqlOp, done: oneshot::Sender<Result<()>> },
    /// 事务组（批量改写等需原子性的场景）
    Transaction { ops: Vec<SqlOp>, done: oneshot::Sender<Result<()>> },
}
```

| 设计决策 | 理由 |
| --- | --- |
| **写连接单线程独占** | 彻底规避 `SQLITE_BUSY` 与写锁竞争。全部写操作经 channel 串行化，天然满足 PRD NFR-REL-09 |
| **读连接池化** | 读操作（搜索、反链、图谱查询、文件树）可并发，池大小默认 4，可按平台调整 |
| **索引批量走专用通道** | `IndexBatch` 携带完整批次，写线程内部开单一事务批量提交（PERF-05），避免逐条 `INSERT` 的事务开销 |
| **读写不共用连接** | WAL 模式下读者不阻塞写者，但同一连接上混用会导致事务语义混乱 |

### 4.2 PRAGMA 配置

```sql
-- 索引库与全局库均适用
PRAGMA journal_mode = WAL;          -- 写前日志：读写并发、崩溃可恢复（NFR-REL-03）
PRAGMA synchronous = NORMAL;        -- WAL 模式下 NORMAL 已保证崩溃一致性，FULL 会显著拖慢索引
PRAGMA foreign_keys = ON;           -- 外键约束（rusqlite 默认关闭，必须显式开启）
PRAGMA busy_timeout = 5000;         -- 写锁等待上限，超时报错而非死等
PRAGMA cache_size = -64000;         -- 64MB 页缓存（负数表示 KB）
PRAGMA mmap_size = 268435456;       -- 256MB mmap，加速大库读取
PRAGMA temp_store = MEMORY;         -- 临时表在内存
PRAGMA wal_autocheckpoint = 1000;   -- 每 1000 页自动 checkpoint，防 WAL 文件无限增长
```

| PRAGMA | 权衡说明 |
| --- | --- |
| `synchronous = NORMAL` | 在 WAL 模式下，NORMAL 保证**进程崩溃**不丢数据；**系统断电**时可能丢失最后一批未 checkpoint 的事务。对索引库（可重建）完全可接受。**全局库**因含用户配置，同样用 NORMAL——因为配置丢失的后果远轻于笔记丢失，且笔记数据的安全性由 §6.3 的 fsync 保证，不依赖 SQLite |
| `cache_size = -64000` | 64MB 对 8GB 内存的基准环境合理。压力库场景下若内存吃紧可降至 32MB，通过配置项暴露 |
| `mmap_size` | 显著加速大库随机读（FTS5 查询）。Windows 上 mmap 行为需 M1 实测验证 |
| `foreign_keys = ON` | **必须**。`ON DELETE CASCADE` 依赖它，否则删除 `file` 行会留下孤立的 `link`/`tag` 关联，破坏一致性 |

### 4.3 事务模式

| 场景 | 事务类型 | 说明 |
| --- | --- | --- |
| 索引批量写入 | `BEGIN IMMEDIATE` + 批量 `INSERT` + `COMMIT` | `IMMEDIATE` 立即获取写锁，避免 deferred 模式下的升级死锁 |
| 单文件增量索引 | 单一事务内：删旧记录（`file_id` 级联）→ 插新记录 → 更新 FTS | **必须**原子，否则中途失败会留下「有 file 无 link」的半索引状态 |
| 偏好写入 | 前端防抖 500ms 后批量 `INSERT OR REPLACE` 单事务 | PERF-06 |
| 回收站恢复 | 单一事务：更新 `deleted` 标记 + 移动文件 + 重算链接 | 文件移动在事务外先执行（IO 无法回滚），失败则不进事务 |
| 全量重建 | **不用单一大事务**。分批提交（每 200 文件），配合 `meta` 中的重建进度标记 | 单一大事务在 10 万文件下会撑爆 WAL 且不可中断（违反 NFR-REL-07/10） |

### 4.4 索引库迁移：丢弃重建

PRD MIG-01 已定策略。实现细节：

```rust
pub fn ensure_index_db(vault_root: &Path, expected: &IndexSignature) -> Result<DbPool> {
    let db_path = vault_root.join(".knowlpad/index.db");
    let fresh = !db_path.exists();

    let pool = open_pool(&db_path)?;

    if fresh {
        create_schema(&pool)?;                 // 执行完整 DDL
        write_meta(&pool, expected)?;
        return Ok(pool);
    }

    let stored = read_signature(&pool)?;
    if stored == *expected {
        // 检查是否有未完成的重建（NFR-REL-07）
        if read_meta(&pool, "rebuild_in_progress")?.is_some() {
            return rebuild_from_scratch(pool, expected);
        }
        return Ok(pool);
    }

    // 签名不匹配 → 丢弃重建
    emit_event("kp://index/rebuild-required", &json!({
        "reason": stored.diff_reason(expected),
        "old_sig": stored, "new_sig": expected,
    }));
    rebuild_from_scratch(pool, expected)
}

fn rebuild_from_scratch(pool: DbPool, sig: &IndexSignature) -> Result<DbPool> {
    // 目标：同时满足 FR-SIG-03（可中断续做）与 FR-SIG-02（重建期间旧索引只读可用）
    // 1. 新建独立库文件 index.db.rebuild（全程不触碰正在服务的 index.db）
    // 2. 在 rebuild 库中写入 meta: rebuild_in_progress = <timestamp>
    // 3. 触发全量索引，全部写入 index.db.rebuild
    // 4. 完成后清除 rebuild_in_progress 并 fsync
    // 5. 原子切换：rename(index.db -> index.db.old)
    //              rename(index.db.rebuild -> index.db)
    //              删除 index.db.old 与其 -wal/-shm
    // 6. 启动时若发现 index.db.rebuild（上次中断）：丢弃它，按当前 index.db 状态重新开始
}
```

**关键点**：重建在**独立文件** `index.db.rebuild` 中进行，因此旧 `index.db` 全程可读，满足 PRD FR-SIG-02「重建期间可继续使用旧索引只读浏览」；`rebuild_in_progress` 标记必须在**重建完成时**才清除，若进程在重建中被杀，下次启动检测到 `index.db.rebuild` 即丢弃并重新开始（PRD FR-SIG-03、AC-REL-02），绝不会把半索引当作有效索引使用。两次 `rename` 的原子切换保证任何时刻 `index.db` 要么是完整旧库、要么是完整新库。

### 4.5 全局库迁移：版本化脚本

```rust
struct Migration {
    version: i32,
    description: &'static str,
    up: fn(&Connection) -> Result<()>,
}

const MIGRATIONS: &[Migration] = &[
    Migration { version: 1, description: "初始 schema", up: m001_init },
    // 后续版本追加于此，禁止修改已发布的迁移脚本
];

pub fn migrate(conn: &Connection) -> Result<()> {
    let current: i32 = conn
        .query_row("SELECT value FROM meta WHERE key='schema_version'", [], |r| r.get(0))
        .unwrap_or(0);

    // MIG-03：拒绝降级
    let latest = MIGRATIONS.last().map(|m| m.version).unwrap_or(0);
    if current > latest {
        return Err(AppError::DbError(format!(
            "数据库版本 v{current} 高于当前软件支持的 v{latest}，请升级 Knowl Pad"
        )));
    }

    for m in MIGRATIONS.iter().filter(|m| m.version > current) {
        // 每个迁移独立事务（MIG-02）
        let tx = conn.unchecked_transaction()?;
        (m.up)(&tx)?;
        tx.execute(
            "INSERT OR REPLACE INTO meta(key,value) VALUES('schema_version',?1)",
            [m.version.to_string()],
        )?;
        tx.commit()?;
        tracing::info!(version = m.version, desc = m.description, "全局库迁移完成");
    }
    Ok(())
}
```

| 纪律 | 说明 |
| --- | --- |
| 已发布脚本**只增不改** | 修改历史迁移会导致不同用户的库处于不同实际状态 |
| 每个迁移独立事务 | 失败时只回滚当前迁移，已完成的保留 |
| 迁移必须有测试 | MIG-05：构造 v(N-1) 库 → 执行迁移 → 断言 v(N) 结构与数据 |
| 降级明确拒绝 | MIG-03：不静默损坏，提示用户升级 |

### 4.6 索引签名实现

```rust
pub struct IndexSignature {
    pub schema_version: u32,
    pub parser_version: u32,
    pub tokenizer_version: String,     // jieba-rs crate 版本
    /// 词典指纹。jieba-rs 的词典**编译期内嵌**于二进制，运行时没有独立词典文件可哈希；
    /// 因此该值 = 构建期由 build.rs 通过 include_bytes! 对词典资源计算出的 SHA-256 前 16 位
    /// （并叠加 crate 版本与 features）。词典变更若无法感知，会破坏 AC-REL-03 的重建一致性。
    pub tokenizer_dict_hash: String,
    pub vault_root: String,            // 规范化绝对路径（SEC-10 / T-13）
    pub digest: String,                // 上述字段规范化拼接后的 SHA-256
}
```

`digest` 计算方式：字段按固定顺序以 `\n` 拼接（值本身不含换行，无需转义），取 SHA-256 十六进制。

**版本号维护纪律**：

| 变更类型 | 必须递增 |
| --- | --- |
| 索引库表结构（DDL） | `schema_version` |
| wikilink/tag/heading/frontmatter/block_id 任一解析规则（PRD §3.1 的 MD-* 规则） | `parser_version` |
| jieba-rs 升级 | `tokenizer_version` |
| jieba 词典变更（自定义词典或 crate 内置词典升级） | `tokenizer_dict_hash`（由 `build.rs` 在构建期对内嵌词典资源计算 SHA-256 前 16 位，写入编译期常量） |

> 这四项任一变化都会使既有索引失效并触发重建。这是**正确性优先于性能**的刻意选择：解析规则变了而索引不重建，会导致链接/标签数据与实际文件内容不一致，这类静默错误极难排查。

---

## 5. 核心引擎

### 5.1 Markdown 解析器

实现于 `domain/md_parse/`。设计目标：**纯函数、单次遍历、位置精确**。

#### 5.1.1 接口

```rust
/// 纯函数：输入文件字节，输出解析结果。无 IO、无全局状态、无副作用。
pub fn parse_note(bytes: &[u8]) -> ParsedNote;

pub struct ParsedNote {
    pub frontmatter: Option<Frontmatter>,
    pub frontmatter_error: Option<String>,
    pub links: Vec<ParsedLink>,
    pub tags: Vec<ParsedTag>,
    pub headings: Vec<ParsedHeading>,
    pub block_ids: Vec<ParsedBlockId>,
    pub plain_text: String,          // 供 FTS5 索引（已去标记、去代码块）
    pub warnings: Vec<ParseWarning>, // 非致命，不中断索引
}

pub struct ParsedLink {
    pub target_ref: String,          // '#' 前部分（已 trim，MD-WL-09）
    pub anchor: Option<Anchor>,      // Heading(String) | BlockId(String)
    pub alias: Option<String>,
    pub link_kind: LinkKind,         // Link | Embed
    pub line: u32,
    pub col: u32,
    /// 链接在原始字节中的精确区间 —— 链接改写（§6.1）依赖此项
    pub span: Span,
}

pub struct Span { pub start: usize, pub end: usize }
```

> **`span` 是设计关键**：链接改写必须精确替换 `[[...]]` 内的目标子串而不动别名与锚点（PRD FR-FILE-23）。若解析阶段不记录字节级区间，改写时就不得不用正则重新扫描全文——那会引入「解析」与「改写」两套不一致的匹配逻辑，是 bug 温床。记录 span 后，改写器只需做字节切片拼接，正确性由解析器单点保证。

#### 5.1.2 解析流程（单次遍历）

```
输入 bytes
   │
   ▼
① UTF-8 校验与 BOM 检测（NFR-PLAT-06）
   │  非 UTF-8 → warnings.push(InvalidEncoding)，按有损转换继续
   ▼
② frontmatter 提取（仅当第 1 行第 1 列为 "---"，MD-FM-01）
   │  找到闭合 "---" → YAML 解析 → 失败则 frontmatter_error + 降级
   │  正文起始偏移 body_start 确定
   ▼
③ 逐行扫描 body，维护状态机：
   │
   │  状态：Normal | FencedCode(lang) | IndentedCode
   │
   │  FencedCode 进出：行首 ``` 或 ~~~（≥3 个，可带语言标注）
   │  IndentedCode：行首 ≥4 空格且当前不在列表中
   │
   │  Normal 状态下按序处理：
   │    a. ATX 标题：^(#{1,6})\s+(.*)$  → headings（MD-H-01）
   │    b. Setext 标题：上一行非空 + 本行 ^(=+|-+)\s*$ → 回溯修正上一行为 heading
   │    c. 行内扫描（跳过行内代码 `...` 区间，MD-WL-05）：
   │         · 行内代码区间先行标记（正则或手写扫描 `+ 配对）
   │         · wikilink：!?\[\[ ... \]\] → links（记录 span）
   │         · 转义 \[[ 跳过（MD-WL-08）
   │         · tag：#[^\s#] 开头 + 合法字符集 → tags（MD-TAG-01/02）
   │         · 排除 "# " 标题形式（MD-TAG-04）
   │         · 排除已在行内代码区间的匹配
   │    d. Block ID：行尾 \^([a-zA-Z0-9-]+)\s*$ → block_ids（MD-BID-01）
   │    e. 累积 plain_text（跳过代码块内容，去除 MD 标记）
   ▼
④ frontmatter tags 与 aliases 合并入 tags（line = -1，MD-TAG-06）
   │  层级标签展开：#a/b/c → [a/b/c, a/b, a]（MD-TAG-03）
   ▼
⑤ 返回 ParsedNote
```

#### 5.1.3 实现要点

| 要点 | 说明 |
| --- | --- |
| **不使用完整 Markdown AST** | 结构化解析只需要链接/标签/标题/块ID 四类实体，构建完整 AST（如 `pulldown-cmark`）开销大且无收益。采用轻量行扫描 + 行内正则 |
| **代码块区间优先识别** | `code_fence.rs` 先产出全文的「代码区间列表」，wikilink/tag 匹配时用二分查找判断是否落在区间内。这比在状态机里逐字符判断更清晰、更易测 |
| **正则必须用 `regex` crate** | Rust 官方 regex 保证线性时间，**天然免疫 ReDoS**（对应 SEC-03 的 DoS 防护）。禁止引入回溯型正则引擎 |
| **span 用字节偏移** | 非字符偏移。改写时直接对 `&[u8]` 切片，避免 UTF-8 边界问题 |
| **解析失败永不抛错** | 任何异常降级为 warning（MD-FM-03）。解析器返回 `ParsedNote` 而非 `Result`——这是刻意的：单个文件的解析问题不应中断整库索引 |
| **超时保护** | 单文件解析设 10s 超时（SEC-03）。实现方式：工作线程中用 `tokio::time::timeout` 包裹；超时则跳过该文件并记 warning |
| **纯函数可测** | 无 IO 无状态，可用附录 B 的 25 个用例做穷尽单元测试（TEST-01） |

### 5.2 中文分词

#### 5.2.1 索引侧

```rust
pub struct Tokenizer {
    jieba: Arc<Jieba>,   // 全局单例（AppState 持有），词典加载仅一次
}

impl Tokenizer {
    /// 将原文转为空格分隔的分词结果，用于写入 note_fts.plain_text
    pub fn tokenize_for_index(&self, text: &str) -> String {
        self.jieba
            .cut(text, false)          // 非 HMM 模式：确定性更强，避免概率模型导致的索引不稳定
            .into_iter()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty() && !is_stop_token(t))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn is_stop_token(t: &str) -> bool {
    // 过滤纯空白与单字符标点，减少 FTS 索引体积
    t.chars().all(|c| c.is_whitespace() || c.is_ascii_punctuation() || is_cjk_punct(c))
}
```

#### 5.2.2 查询侧

```rust
/// 将用户查询转为 FTS5 MATCH 表达式
pub fn build_match_expr(&self, query: &str, mode: MatchMode) -> Result<String> {
    // 1. 提取高级语法前缀（path: file: tag: line:），剩余部分作为全文关键词
    let (filters, text) = extract_filters(query)?;   // 语法错误 → E_SEARCH_SYNTAX

    // 2. 处理引号短语（FR-SEARCH-24）：双引号内不拆词
    let phrases = extract_quoted(&text)?;            // 未闭合引号 → E_SEARCH_SYNTAX
    let rest = remove_quoted(&text);

    // 3. 处理排除项（FR-SEARCH-25）：- 前缀
    let (excludes, includes) = split_negation(rest);

    // 4. 对 includes 与 excludes 各自分词
    let inc_tokens = self.jieba.cut(&includes, false);
    let exc_tokens = self.jieba.cut(&excludes, false);

    // 5. 构造 MATCH 表达式
    let joiner = match mode { MatchMode::All => " AND ", MatchMode::Any => " OR " };
    let mut parts: Vec<String> = Vec::new();
    parts.extend(phrases.iter().map(|p| format!("\"{}\"", escape_fts(p))));
    parts.extend(inc_tokens.iter().map(|t| escape_fts(t)));
    let positive = parts.join(joiner);
    // FTS5 的 NOT 是【二元】运算符（expr1 NOT expr2），不能作为前缀单独出现。
    // 多个排除词用链式 `expr NOT a NOT b`（左结合）表达，语义为同时排除 a 与 b。
    let negative = exc_tokens.iter()
        .map(|t| escape_fts(t))
        .collect::<Vec<_>>().join(" NOT ");

    Ok(match (positive.is_empty(), negative.is_empty()) {
        (true, _) => return Err(AppError::SearchSyntax("查询内容为空".into())),
        (_, true) => positive,
        _ => format!("({positive}) NOT {negative}"),
    })
}
```

| 要点 | 说明 |
| --- | --- |
| **非 HMM 模式** | `cut(text, false)` 关闭 HMM 新词发现。理由：HMM 的概率输出可能随词典或版本微变，导致同一文本两次索引结果不同——这会让 AC-REL-03（重建一致性）失败。确定性优先 |
| **FTS5 转义** | `escape_fts` 必须处理 FTS5 的特殊字符（`"`、`*`、`^`、`:`、`(`、`)`、`-`、`+`），否则用户输入含这些字符时会产生语法错误或意外语义。分词后的 token 用双引号包裹最安全 |
| **高亮还原（FR-SEARCH-04）** | FTS5 的 `snippet()` 作用于**分词后**的文本，直接返回会带空格。实现方式：不用 `snippet()`，改为① 从 FTS 取得命中 rowid ② 读取原文 ③ 在原文中定位命中词（对每个 token 做原文子串查找）④ 截取 ±N 字符窗口并在前端高亮。这样保证片段与原文逐字一致 |
| **词典体积** | jieba 默认词典约 5MB，全量内置进入安装包。✅ **OPEN-02 已决（2026-09-20）：采用全量内置**，以换取完全离线可用与开箱即用的分词质量；`DEBT-03` 随之关闭——当前阶段不以打包体积为约束条件，故不采用精简词典或按需下载方案。**若未来重新引入体积约束**，改为精简词典会使索引结果变化，须同时递增索引签名版本以触发重建 |
| **中英混排** | jieba 对连续英文数字不切分（FTS-03），因此 `Rust学习笔记` 会切为 `Rust` / `学习` / `笔记`，搜索 `rust` 可命中（FTS5 大小写不敏感由 `unicode61` 保证） |

### 5.3 索引引擎

#### 5.3.1 任务队列与去重

```rust
pub enum IndexTask {
    /// 新增或修改（含内容重解析）
    Upsert { rel_path: String, mtime_ms: i64, seq: u64 },
    /// 删除
    Remove { rel_path: String, seq: u64 },
    /// 重命名/移动（等价于 Remove + Upsert，但可复用旧记录的链接裁决）
    Rename { from: String, to: String, seq: u64 },
}

pub struct IndexQueue {
    /// 待处理路径 → 最新任务（去重：同一文件多次变更只保留最后一个）
    pending: Mutex<HashMap<String, IndexTask>>,
    /// 单调递增序号，用于判定任务新旧
    seq: AtomicU64,
    /// 通知工作线程有新任务
    notify: Notify,
    /// 取消令牌（FR-SIG-03 / NFR-REL-10）
    cancel: CancellationToken,
}
```

**去重规则**：同一 `rel_path` 在队列中只保留 `seq` 最大的任务。这在文件监听风暴（如 Git 切换分支修改上千文件）下至关重要——避免对同一文件重复解析。

#### 5.3.2 全量索引流程

```
① 标记 meta.rebuild_in_progress = now()
② walkdir 遍历 vault_root
   · 跳过：.knowlpad/、.obsidian/、以 . 开头的目录（除非配置显示隐藏）、node_modules/、.git/
   · 分类：.md → note；图片/PDF/音视频扩展名 → attachment；其余 → other
   · 超 5MB 的 note → 跳过 + warning（SEC-11）
   · 收集 (rel_path, size, mtime) 三元组，total = 数量
③ 分块投递至工作线程池（chunk = 200）
   每个工作线程：
     for file in chunk:
        read bytes → parse_note() → tokenize_for_index()
        产出 IndexRow { file_meta, parsed, tokenized_text }
        通过 mpsc 发送至 DB 写线程
        每完成一个 → progress_tx.send()（节流层聚合）
④ DB 写线程：
   累积至 200 条或 500ms → BEGIN IMMEDIATE
     · INSERT INTO file (...)
     · 对 note：DELETE FROM note_fts WHERE rowid=? ; INSERT INTO note_fts(rowid, plain_text)
     · 批量 INSERT link / tag / file_tag / heading / block_id（暂不含 dst_file_id 与 status）
   COMMIT
⑤ 全部文件入库后 → 链接裁决阶段（§5.3.3）
⑥ 标签 ref_count 重算（一次 GROUP BY 更新）
⑦ 写入 meta.index_signature；DELETE meta.rebuild_in_progress
⑧ emit kp://index/completed { stats, duration_ms }
```

**进度上报节流**（EVT-02）：工作线程发送的是无节流的原子计数递增；由独立的节流任务每 100ms 读取当前值并 emit 一次。避免 10 万文件产生 10 万条事件。

#### 5.3.3 链接裁决（全局阶段）

链接的 `resolved`/`dangling`/`ambiguous` 状态**无法在单文件解析时判定**——需要全库的文件名视图。因此设计为独立的批量阶段：

```sql
-- 步骤 1：为每个 link 找出全部候选目标（大小写不敏感的 stem 匹配）
CREATE TEMP TABLE link_candidates AS
SELECT l.id AS link_id, f.id AS candidate_id, f.rel_path
FROM link l
JOIN file f ON (lower(f.stem) = lower(l.target_ref)
             OR lower(f.rel_path) = lower(l.target_ref)
             OR lower(f.rel_path) = lower(l.target_ref || '.md'))
WHERE f.deleted = 0
  -- embed（![[...]]）可指向附件；普通 link 只解析到笔记（FR-LINK-05）
  AND ( (l.link_kind = 'link'  AND f.kind = 'note')
     OR (l.link_kind = 'embed' AND f.kind IN ('note', 'attachment')) );

-- 步骤 2：别名匹配（frontmatter aliases）—— 需额外的 alias 表，见下
INSERT INTO link_candidates
SELECT l.id, f.id, f.rel_path
FROM link l
JOIN file_alias fa ON lower(fa.alias) = lower(l.target_ref)
JOIN file f ON f.id = fa.file_id
WHERE f.deleted = 0;

-- 步骤 3：按候选数量判定状态
--   0 个候选 → dangling
--   1 个候选 → resolved
--   >1 个候选 → 应用 MD-WL-04 消歧规则后：唯一则 resolved，仍多个则 ambiguous
```

**消歧规则实现（MD-WL-04）**：候选 > 1 时，在 Rust 侧对每个 link 执行：

```rust
fn disambiguate(link: &ParsedLink, src_rel_path: &str, candidates: &[FileRow]) -> Disambiguation {
    // 1. 完整路径精确匹配优先（若 target_ref 含 '/'）
    if link.target_ref.contains('/') {
        let exact: Vec<_> = candidates.iter()
            .filter(|c| eq_ignore_ascii_case(&c.rel_path, &link.target_ref)
                     || eq_ignore_ascii_case(&c.rel_path, &format!("{}.md", link.target_ref)))
            .collect();
        if exact.len() == 1 { return Resolved(exact[0].id); }
        if exact.len() > 1 { return Ambiguous(exact.iter().map(|c| c.id).collect()); }
        // 完整路径无匹配 → 继续走 stem 逻辑
    }
    // 2. 同目录优先
    let src_dir = parent_dir(src_rel_path);
    let same_dir: Vec<_> = candidates.iter().filter(|c| parent_dir(&c.rel_path) == src_dir).collect();
    if same_dir.len() == 1 { return Resolved(same_dir[0].id); }
    // 3. 最短路径优先
    let mut by_depth = candidates.to_vec();
    by_depth.sort_by_key(|c| (c.rel_path.matches('/').count(), c.rel_path.len()));
    let min_depth = by_depth[0].rel_path.matches('/').count();
    let tied: Vec<_> = by_depth.iter().take_while(|c| c.rel_path.matches('/').count() == min_depth).collect();
    if tied.len() == 1 { return Resolved(tied[0].id); }
    // 4. 仍歧义
    Ambiguous(by_depth.iter().map(|c| c.id).collect())
}
```

> **性能考量**：`link_candidates` 的 JOIN 在 10 万文件 × 平均 5 链接 = 50 万链接的规模下，若走全表扫描会很慢。因此 `file(stem)` 必须有索引（PRD §3.2.1 已有 `idx_file_stem`），且 `lower(stem)` 的函数索引可进一步优化：`CREATE INDEX idx_file_stem_lower ON file(lower(stem))`。该函数索引**已定稿在 PRD §3.2.1 的 DDL 中**（真相源），实现时直接使用，不再另行补充。

#### 5.3.4 增量索引

单文件变更的处理（`Upsert`）：

```
① 读取文件 → parse_note → tokenize
② BEGIN IMMEDIATE
     DELETE FROM file WHERE rel_path = ?     -- 外键 CASCADE 自动清理 link/tag/file_tag/heading/block_id
     DELETE FROM note_fts WHERE rowid = ?
     INSERT INTO file (...) VALUES (...)
     INSERT INTO note_fts (rowid, plain_text)
     批量 INSERT 该文件的 link（仅 src 侧）/ tag / heading / block_id
   COMMIT
③ 重算「指向该文件」与「该文件指向」的链接状态
     · 出链：用该文件的 target_ref 查 file 表裁决（局部，快）
     · 入链：其他文件中 target_ref 匹配该文件 stem/path/alias 的 link 需重算
       → SQL: UPDATE link SET dst_file_id=?, status='resolved'
              WHERE lower(target_ref) IN (lower(?stem), lower(?path), ...aliases)
                AND status IN ('dangling','ambiguous')
     · 反向：若该文件被删除/重命名，原本 resolved 指向它的 link 需改为 dangling
④ 更新 tag.ref_count（仅受影响的 tag_id）
⑤ emit kp://note/updated / kp://link/changed
```

**增量正确性的关键**：步骤 ③ 的入链重算范围必须精确。若文件名从 `A.md` 改为 `B.md`，则原先匹配 `A` 的 link 要变 dangling，原先匹配 `B` 的 dangling link 要变 resolved。这通过 `fs_ops.rs` 在重命名时传递 `(old_stem, new_stem)` 给索引引擎实现，不做全库扫描。对应 AC-LINK-02 的「未触发全量重建」断言。

### 5.4 文件监听

```rust
pub fn start_watcher(vault: &VaultHandle, queue: IndexQueue) -> Result<RecommendedWatcher> {
    let (tx, rx) = std::sync::mpsc::channel();
    let root = vault.root.clone();

    let mut watcher = notify::recommended_watcher(move |res: Result<Event, Error>| {
        if let Ok(event) = res { tx.send(event).ok(); }
    })?;

    watcher.watch(&root, RecursiveMode::Recursive)?;

    // 独立线程：去抖动 + 归并 + 过滤
    std::thread::spawn(move || {
        let mut buffer: HashMap<PathBuf, DebouncedState> = HashMap::new();
        let mut last_flush = Instant::now();

        loop {
            // 200ms 窗口（EVT-01）
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(event) => {
                    if should_ignore(&event, &root) { continue; }
                    merge_into(&mut buffer, event);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            // 窗口到期 → 冲刷
            if last_flush.elapsed() >= Duration::from_millis(200) && !buffer.is_empty() {
                for (path, state) in buffer.drain() {
                    if let Some(task) = state.into_index_task(&root) {
                        queue.push(task);
                    }
                }
                emit_merged_fs_events(&vault, &state_snapshot);
                last_flush = Instant::now();
            }
        }
    });

    Ok(watcher)
}

/// 单一真相源：遍历（§5.3.2）与监听（§5.4）**必须复用同一份忽略规则**。
/// 否则会出现「遍历跳过但监听不跳过」的索引风暴（例如 git checkout），或反之的索引遗漏。
pub const IGNORED_DIRS: &[&str] = &[".knowlpad", ".obsidian", ".git", "node_modules"];

fn should_ignore(event: &Event, root: &Path) -> bool {
    event.paths.iter().any(|p| {
        // 忽略 .knowlpad/ 自身变更（否则索引写入会触发监听死循环！）
        p.starts_with(root.join(".knowlpad"))
        // 忽略临时文件
        || p.file_name().map(|n| n.to_string_lossy().starts_with(".kp-tmp-")).unwrap_or(false)
        // 忽略 .obsidian/（FR-STORAGE-02）、.git/、node_modules/ 等，与遍历规则一致
        || p.components().any(|c| IGNORED_DIRS.contains(&c.as_os_str().to_string_lossy().as_ref()))
    })
}
```

**事件归并规则（EVT-01）**

| 窗口内的事件序列 | 归并结果 | 理由 |
| --- | --- | --- |
| Create → Modify → Modify | 单次 `Upsert` | 编辑器保存常触发多次 modify |
| Create → Remove | **丢弃**（净效果为无变化） | 临时文件场景 |
| Modify → Remove | `Remove` | 最终状态优先 |
| Remove → Create | `Upsert` | 视为内容替换 |
| Rename(from, to) | `Rename` 任务 | 可复用旧记录的链接裁决，比 Remove+Upsert 快 |

> **死循环防护是本模块最高优先级**：索引写入 `index.db` 位于 `.knowlpad/` 内，而 `.knowlpad/` 在 Vault 根下。若不过滤，每次索引写入都会触发监听 → 再次索引 → 无限循环。`should_ignore` 的第一条规则即为此存在。**M3 必须有针对性测试**：索引进行中人为在 Vault 内修改文件，断言不产生循环。

**inotify watch 上限处理（NFR-PLAT-09）**：Linux 默认 `fs.inotify.max_user_watches` 通常为 8192，大 Vault（数千目录）会耗尽。捕获 `notify::ErrorKind::MaxFilesWatch` 后：① 记录日志 ② 通过 `kp://index/failed` 上报，`detail` 含具体的 `sysctl` 修改建议 ③ 降级为「轮询模式」（每 5s 扫描 mtime），保证功能可用但性能下降。

---

## 6. 关键算法

本章覆盖三个正确性要求最高、一旦出错即损坏用户数据的模块。

### 6.1 批量链接改写器（最高风险模块）

对应 PRD FR-FILE-20~25、FR-TAG-07、SEC-12、NFR-REL-04、AC-FILE-01/02、AC-TAG-02。

#### 6.1.1 两阶段设计：预览与执行分离

改写**必须**分两个 Command 完成（PRD §5.3.3），不允许一步到位：

| 阶段 | Command | 产出 | 副作用 |
| --- | --- | --- | --- |
| 预览 | `link_rewrite_preview` | `preview_id` + 明细（文件数、处数、逐处 diff 片段） | **只读**，写入 `AppState.previews` 缓存（TTL 10 分钟） |
| 执行 | `link_rewrite_apply` | `operation_id` + 结果 | 备份 → 改写 → 重命名 → 重算索引 |

> **重命名与改写同属一次操作**：当改写由重命名/移动触发时，rename 规格随预览一并冻结，并在执行阶段作为同一事务的补偿步骤处理（§6.1.3 步骤 2.5）。**不存在**「只改名不改链接」或「只改链接不改名」的中间态。

分离的理由：① 满足 FR-FILE-21 的用户确认要求 ② 预览阶段的定位结果被冻结，执行阶段直接复用 `span`，**不重新扫描**——避免「预览时看到 N 处、执行时改了 M 处」的不一致 ③ 预览结果可供 UI 展示 diff，用户能真正理解将发生什么。

#### 6.1.2 预览阶段实现

```rust
pub fn build_preview(
    vault: &VaultHandle,
    from_ref: &str,                    // 如 "A" 或 "folder/A"
    to_ref: &str,                      // 如 "B" 或 "folder/B"
    rename_spec: Option<RenameSpec>,   // 由重命名/移动触发时携带，执行阶段纳入同一可回滚操作
) -> Result<RewritePreview> {
    // 1. 从索引库找出全部指向 from_ref 的链接（含 dangling，因为可能刚好要修复）
    let links = dao::link::find_by_target_ref(vault, from_ref)?;

    // 2. 按源文件分组
    let mut by_file: HashMap<i64, Vec<LinkRow>> = HashMap::new();
    for l in links { by_file.entry(l.src_file_id).or_default().push(l); }

    // 3. 逐文件计算精确的替换区间
    let mut edits: Vec<FileEdit> = Vec::with_capacity(by_file.len());
    let mut total_spans = 0;

    for (file_id, file_links) in by_file {
        let file = dao::file::get_by_id(vault, file_id)?;
        let bytes = fs::read(vault.root.join(&file.rel_path))?;

        // 关键：重新解析以获取代码区间，确保排除代码块内的匹配（FR-FILE-24）
        let parsed = parse_note(&bytes);
        let code_spans = collect_code_spans(&bytes);   // 代码块 + 行内代码区间

        let mut spans: Vec<ReplaceSpan> = Vec::new();
        for l in &file_links {
            // 双重校验：索引记录的 span 必须仍与当前文件内容匹配
            // （文件可能在索引后被外部修改 —— 见 6.1.5 陈旧检测）
            let Some(actual) = verify_span(&bytes, l.span, &l.target_ref) else {
                // span 失效 → 从解析结果中按 target_ref 重新定位
                match relocate(&parsed, &l.target_ref, &code_spans) {
                    Some(s) => s,
                    None => continue,   // 确实不存在（已被用户手改），跳过并记录 warning
                }
            };
            // 排除落在代码区间内的匹配
            if code_spans.iter().any(|c| overlaps(c, &actual)) { continue; }
            spans.push(actual);
            total_spans += 1;
        }

        if !spans.is_empty() {
            spans.sort_by_key(|s| s.start);          // 必须排序：从后往前替换
            edits.push(FileEdit { file_id, rel_path: file.rel_path.clone(), spans });
        }
    }

    let preview = RewritePreview {
        preview_id: Uuid::new_v4(),
        from_ref: from_ref.into(),
        to_ref: to_ref.into(),
        rename: rename_spec,           // 冻结 rename 规格，执行阶段直接复用（§6.1.3 步骤 2.5）
        file_count: edits.len(),
        span_count: total_spans,
        edits,
        created_at: OffsetDateTime::now_utc(),
        /// 记录各文件的 mtime，执行时校验是否被改动（6.1.5）
        mtimes: collect_mtimes(&edits, vault)?,
    };
    Ok(preview)
}
```

#### 6.1.3 执行阶段实现

```rust
pub fn apply_preview(
    vault: &VaultHandle,
    preview_id: Uuid,
    cancel: CancellationToken,
) -> Result<RewriteResult> {
    let preview = state.previews.remove(&preview_id)
        .ok_or(AppError::PreviewExpired)?;

    // TTL 校验
    if preview.created_at + Duration::minutes(10) < OffsetDateTime::now_utc() {
        return Err(AppError::PreviewExpired);
    }

    // ── 步骤 1：陈旧检测（6.1.5）──────────────────
    verify_mtimes_unchanged(vault, &preview.mtimes)?;   // 失败 → E_REWRITE_FAILED，不改任何文件

    let op_id = Uuid::new_v4();
    let backup_dir = vault.root.join(".knowlpad/backup")
        .join(format!("{}", op_id));

    // ── 步骤 2：备份（必须先于任何写入，SEC-12）────
    // 备份失败 → E_BACKUP_FAILED，操作中止，磁盘零改动
    fs::create_dir_all(&backup_dir)?;
    let mut backed_up: Vec<PathBuf> = Vec::new();
    for e in &preview.edits {
        let src = vault.root.join(&e.rel_path);
        let dst = backup_dir.join(&e.rel_path);
        if let Err(err) = fs::create_dir_all(dst.parent().unwrap()).and_then(|_| fs::copy(&src, &dst)) {
            cleanup_backup(&backed_up);          // 回滚已备份的（仅删备份，不动原文件）
            return Err(AppError::BackupFailed { path: e.rel_path.clone(), source: err });
        }
        backed_up.push(src);
    }

    // ── 步骤 2.5：若本次操作由重命名/移动触发，先执行 rename ──────
    // 顺序理由（FR-FILE-22）：rename 与链接改写必须同属一次可回滚操作。
    //   先 rename：若 rename 失败，磁盘上尚无任何链接被改写，直接中止即可；
    //   若先改写后 rename，rename 失败会留下「链接已指向 B、文件仍叫 A」的大规模悬空链接。
    let renamed: Option<RenameSpec> = match preview.rename.clone() {
        Some(spec) => {
            let from = vault.root.join(&spec.from);
            let to = vault.root.join(&spec.to);
            if let Err(err) = fs::rename(&from, &to) {
                cleanup_backup(&backed_up);
                return Err(AppError::IoFailure(format!("重命名失败，未改写任何链接：{err}")));
            }
            Some(spec)
        }
        None => None,
    };

    // 统一的失败补偿：回滚已改文件 + 撤销 rename；两者都失败则返回 RollbackFailed
    let compensate = |modified: &[PathBuf], renamed: &Option<RenameSpec>| -> Result<()> {
        let mut failures: Vec<(PathBuf, io::Error)> = Vec::new();
        if let Err(e) = rollback_all(vault, &backup_dir, modified) { failures.push((backup_dir.clone(), e)); }
        if let Some(spec) = renamed {
            let to = vault.root.join(&spec.to);
            let from = vault.root.join(&spec.from);
            if let Err(e) = fs::rename(&to, &from) { failures.push((to, e)); }
        }
        if failures.is_empty() { Ok(()) }
        else { Err(AppError::RollbackFailed { failures, backup_dir: backup_dir.clone() }) }
    };

    // ── 步骤 3：逐文件原子改写 ────────────────────
    let mut modified: Vec<PathBuf> = Vec::new();
    for e in &preview.edits {
        if cancel.is_cancelled() {
            compensate(&modified, &renamed)?;               // NFR-REL-10
            return Err(AppError::Cancelled);
        }
        let path = vault.root.join(&e.rel_path);
        match rewrite_file(&path, &e.spans, &preview.from_ref, &preview.to_ref) {
            Ok(()) => modified.push(path),
            Err(err) => {
                // 全有或全无：回滚全部已改文件，并撤销 rename（NFR-REL-04 / FR-FILE-22）
                compensate(&modified, &renamed)?;
                return Err(AppError::RewriteFailed {
                    failed_path: e.rel_path.clone(),
                    rolled_back: modified.len(),
                    source: err,
                });
            }
        }
        emit_progress(op_id, modified.len(), preview.edits.len());
    }

    // ── 步骤 4：保留备份供用户回滚，登记 operation_id（含 rename 规格）──
    // 备份不立即删除：FR-FILE-21 允许用户事后 link_rewrite_rollback；
    // link_rewrite_rollback 必须同时撤销 rename（先改回链接，再把文件改回原名）。
    register_operation(op_id, backup_dir.clone(), modified.clone(), renamed);

    Ok(RewriteResult { operation_id: op_id, file_count: modified.len(), span_count: preview.span_count })
}

/// 单文件改写：从后往前替换，保证前面的 span 偏移不失效
fn rewrite_file(path: &Path, spans: &[ReplaceSpan], from: &str, to: &str) -> Result<()> {
    let mut bytes = fs::read(path)?;
    // spans 已按 start 升序 → 反向遍历
    for s in spans.iter().rev() {
        // 仅替换 target_ref 子区间，保留 [[ ]]、别名、锚点（FR-FILE-23）
        let new_seg = build_replacement(&bytes[s.target_start..s.target_end], from, to);
        bytes.splice(s.target_start..s.target_end, new_seg.bytes().copied());
    }
    atomic_write(path, &bytes)?;      // §6.3
    Ok(())
}
```

#### 6.1.4 替换内容构造（保留别名与锚点）

`build_replacement` 必须只改目标引用部分：

| 原链接 | `from = "A"`, `to = "B"` | 结果 |
| --- | --- | --- |
| `[[A]]` | 目标 = `A` | `[[B]]` |
| `[[A\|我的别名]]` | 目标 = `A` | `[[B\|我的别名]]` ← **别名保留** |
| `[[A#某标题]]` | 目标 = `A` | `[[B#某标题]]` ← **锚点保留** |
| `[[A#^bid]]` | 目标 = `A` | `[[B#^bid]]` |
| `[[folder/A]]` | `from = "folder/A"`, `to = "folder/B"` | `[[folder/B]]` |
| `![[A]]` | — | `![[B]]` ← **嵌入前缀 `!` 保留** |

实现要点：`ReplaceSpan` 记录的 `target_start..target_end` **仅覆盖 `#` 之前、`[[` 或 `![[` 之后的那一段**，不包含 `[[`、`#`、`|`、`]]`。因此替换天然是局部的，无需字符串解析。这是 §5.1.1 中 `span` 设计的直接收益。

#### 6.1.5 陈旧检测（防 TOCTOU）

预览与执行之间可能间隔数秒（用户在看确认对话框），期间文件可能被外部修改。若不检测，`span` 偏移会错位，导致**改写位置错误、损坏无关内容**——这是最危险的失败模式。

```rust
fn verify_span(bytes: &[u8], span: Span, expected_target: &str) -> Option<ReplaceSpan> {
    // 校验 span 位置的实际内容是否仍是预期的 target_ref
    let actual = bytes.get(span.target_start..span.target_end)?;
    if !eq_ignore_ascii_case_bytes(actual, expected_target.as_bytes()) {
        return None;   // 位置内容不符 → span 已失效
    }
    // 校验前后定界符仍是 [[ 与 ]]/# /|
    if !ends_with_open_bracket(bytes, span.target_start) { return None; }
    Some(/* 完整 ReplaceSpan */)
}

fn verify_mtimes_unchanged(vault: &VaultHandle, mtimes: &HashMap<String, i64>) -> Result<()> {
    let mut changed = Vec::new();
    for (rel, expected) in mtimes {
        let actual = fs::metadata(vault.root.join(rel))?.modified_ms()?;
        if actual != *expected { changed.push(rel.clone()); }
    }
    if !changed.is_empty() {
        return Err(AppError::RewriteFailed {
            failed_path: changed.join(", "),
            rolled_back: 0,
            source: io::Error::new(io::ErrorKind::Other,
                "文件在预览后已被外部修改，请重新执行操作"),
        });
    }
    Ok(())
}
```

双重保护：mtime 整体校验（快速失败）+ 逐 span 内容校验（精确定位）。任一失败都不改动磁盘。

#### 6.1.6 回滚实现

```rust
fn rollback_all(vault: &VaultHandle, backup_dir: &Path, modified: &[PathBuf]) -> Result<()> {
    let mut failures = Vec::new();
    for path in modified {
        let rel = path.strip_prefix(&vault.root)?;
        let backup = backup_dir.join(rel);
        if backup.exists() {
            // 用原子写入恢复，保证回滚过程本身也是崩溃安全的
            if let Err(e) = atomic_write(path, &fs::read(&backup)?) {
                failures.push((path.clone(), e));
            }
        }
    }
    if !failures.is_empty() {
        // 回滚失败是最严重的状态：必须在 UI 显著提示并保留备份目录
        tracing::error!(?failures, "链接改写回滚失败，备份保留于 {:?}", backup_dir);
        return Err(AppError::RollbackFailed { failures, backup_dir: backup_dir.to_path_buf() });
    }
    Ok(())
}
```

> **回滚失败的处理**：这是唯一无法保证「用户数据完好」的路径。此时**必须**：① 返回专用错误码（在 PRD §5.2 的 `E_REWRITE_FAILED` 基础上扩展 `context.rolled_back` 与 `context.backup_dir`）② UI 弹出不可忽略的模态提示，明确告知哪些文件可能不一致、备份在何处 ③ 写入 ERROR 级日志。**禁止**静默降级为普通失败提示。

### 6.2 路径安全校验

对应 PRD SEC-02、AC-SEC-01、威胁 T-03/T-04。

```rust
pub struct PathGuard {
    /// 规范化后的 Vault 根（含符号链接解析）
    canonical_root: PathBuf,
}

impl PathGuard {
    /// 校验相对路径并返回安全的绝对路径
    pub fn resolve(&self, rel_path: &str) -> Result<PathBuf> {
        // ① 拒绝空路径
        if rel_path.is_empty() {
            return Err(AppError::PathOutsideVault("路径为空".into()));
        }

        // ② 拒绝绝对路径（AC-07：除 Vault 注册接口外一律相对路径）
        let p = Path::new(rel_path);
        if p.is_absolute() || rel_path.starts_with('/') || rel_path.contains(':') {
            return Err(AppError::PathOutsideVault("不接受绝对路径".into()));
        }

        // ③ 拒绝 NUL 字节（截断攻击）
        if rel_path.bytes().any(|b| b == 0) {
            return Err(AppError::PathEscapeDeny("路径含非法字符".into()));
        }

        // ④ 逐段检查，拒绝 "." 与 ".."（规范化前拦截，防止依赖平台规范化行为）
        for comp in p.components() {
            match comp {
                Component::ParentDir => {
                    return Err(AppError::PathOutsideVault("路径不得包含 ..".into()))
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(AppError::PathOutsideVault("不接受绝对路径".into()))
                }
                Component::CurDir => {
                    return Err(AppError::PathEscapeDeny("路径不得包含 .".into()))
                }
                Component::Normal(seg) => {
                    let s = seg.to_string_lossy();
                    // 段级校验：非空、非纯点号、不含平台非法字符
                    if s.is_empty() || s.trim_end_matches('.') .is_empty() {
                        return Err(AppError::InvalidFilename(s.into_owned()));
                    }
                    validate_segment(&s)?;      // FR-FILE-12 的字符集与保留名检查
                }
            }
        }

        // ⑤ 拼接并规范化（词法层面）
        let joined = self.canonical_root.join(p);
        let lexical = lexical_normalize(&joined);

        // ⑥ 前缀校验（词法层面第一道）
        if !lexical.starts_with(&self.canonical_root) {
            return Err(AppError::PathOutsideVault("路径越界".into()));
        }

        // ⑦ 符号链接解析后再次校验（关键：防 T-04 逃逸）
        //    仅在路径已存在时才能 canonicalize；不存在时用「最近的已存在祖先」策略
        let canonical = canonicalize_existing(&lexical)?;
        if !canonical.starts_with(&self.canonical_root) {
            tracing::warn!(?lexical, ?canonical, "符号链接指向 Vault 外，已拒绝");
            return Err(AppError::PathEscapeDeny("符号链接指向知识库外部".into()));
        }

        Ok(canonical)
    }
}

/// 路径可能尚不存在（新建文件），逐级向上找到已存在的祖先做 canonicalize
fn canonicalize_existing(path: &Path) -> Result<PathBuf> {
    let mut tail = Vec::new();
    let mut cur = path.to_path_buf();
    loop {
        match fs::canonicalize(&cur) {
            Ok(c) => {
                let mut full = c;
                for t in tail.iter().rev() { full = full.join(t); }
                return Ok(full);
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                match (cur.file_name(), cur.parent()) {
                    (Some(name), Some(parent)) => {
                        tail.push(name.to_os_string());
                        cur = parent.to_path_buf();
                    }
                    _ => return Err(e.into()),
                }
            }
            Err(e) => return Err(e.into()),
        }
    }
}
```

**为什么需要七步而非「一次 canonicalize + 前缀检查」**：

| 单独手段 | 可被绕过的方式 |
| --- | --- |
| 仅 `canonicalize` + 前缀检查 | 新建文件时目标不存在，`canonicalize` 直接失败（`NotFound`），若因此放行则绕过全部检查 |
| 仅词法检查 `..` | 符号链接：`vault/link` → `/etc`，词法上完全合法 |
| 仅 `starts_with` | Windows 下 `C:\vault` 与 `C:\vault-evil` 的字符串前缀关系；必须先规范化再比较**路径组件** |

七步中 ④ 拦截词法穿越、⑦ 拦截符号链接逃逸、⑥ 用组件级前缀比较拦截同前缀目录混淆。三者互补，缺一不可。

**审计要求**：全部拒绝路径（⑥⑦ 触发）必须写入 WARN 级日志，含 `lexical` 与 `canonical` 两个值。这既是安全审计线索，也是排查误拦截的依据。

**残留竞态（TOCTOU）**：`canonicalize` 校验通过到实际读写之间仍存在窗口，攻击者可在窗口内替换符号链接。缓解：① 校验后尽量以「已打开的句柄」或「canonical 父目录 + 末段」方式操作；② POSIX 下对末段使用 `O_NOFOLLOW`；③ Windows 下使用 reparse-point 语义打开。该残留风险登记为 `TR-10`（附录 C），不承诺完全消除。

### 6.3 原子写入

对应 PRD NFR-REL-01/02、§6.2.1 协议、AC-REL-01。

```rust
pub fn atomic_write(target: &Path, content: &[u8]) -> Result<()> {
    let parent = target.parent().ok_or_else(|| AppError::IoFailure("无父目录".into()))?;

    // 临时文件必须与目标同目录 —— 跨文件系统的 rename 不是原子操作
    let tmp = parent.join(format!(
        ".kp-tmp-{}-{}",
        std::process::id(),
        Uuid::new_v4().simple()
    ));

    // 步骤 1-3：写入 + fsync
    let written = (|| -> Result<()> {
        let mut f = OpenOptions::new()
            .write(true).create_new(true)     // create_new：临时文件名冲突即报错，不覆盖
            .open(&tmp)?;
        f.write_all(content)?;
        f.flush()?;
        f.sync_all()?;                        // 数据落盘，而非仅在 OS 缓存
        Ok(())
    })();

    // 记录目标文件原有权限位：直接给临时文件设 0600 会在 rename 后
    // **静默改变用户文件的权限**，因此改为「先记录、切换后恢复」。
    #[cfg(unix)]
    let prev_perms = fs::metadata(target).ok().map(|m| m.permissions());

    if let Err(e) = written {
        let _ = fs::remove_file(&tmp);        // 步骤 7：异常清理
        return Err(e);
    }

    // 步骤 4：备份由调用方负责（批量改写场景），单文件保存场景依赖 base_mtime 冲突检测

    // 步骤 5：rename（平台差异见下）
    if let Err(e) = platform::fs_atomic::replace(&tmp, target) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }

    // 步骤 6：fsync 父目录，确保目录项本身落盘
    //   缺少此步：断电后文件内容在盘上，但目录项丢失 → 文件"消失"
    if let Ok(dir) = fs::File::open(parent) {
        #[cfg(unix)]
        let _ = dir.sync_all();
        // Windows 上目录 fsync 无对应 API，依赖 NTFS 日志保证
    }

    // 恢复目标文件原有权限位（新建文件不适用，保持平台默认 + umask）
    #[cfg(unix)]
    if let Some(p) = prev_perms {
        let _ = fs::set_permissions(target, p);
    }

    Ok(())
}
```

**平台差异（`platform/fs_atomic.rs`，对应 RISK-09）**

| 平台 | `rename` 目标已存在时的行为 | 实现 |
| --- | --- | --- |
| POSIX（Linux/macOS） | 原子替换，旧目标被丢弃 | 直接 `fs::rename` |
| Windows | 目标存在时 `MoveFileEx` 需 `MOVEFILE_REPLACE_EXISTING` 标志；`std::fs::rename` 在 Rust 中已使用该标志，**但**目标为只读或被占用时失败 | 先尝试 `fs::rename`；失败且为 `AlreadyExists`/`PermissionDenied` 时，走「备份旧文件 → rename → 失败则还原备份」的三段式 |

```rust
#[cfg(windows)]
pub fn replace(tmp: &Path, target: &Path) -> Result<()> {
    match fs::rename(tmp, target) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            // 目标被占用（如另一程序打开）→ E_FILE_LOCKED，不强行处理
            Err(AppError::FileLocked(target.display().to_string()).into())
        }
        Err(e) => {
            // 其他失败：tmp 已在上层清理，target 未被触碰 → 原内容完整保留
            Err(e.into())
        }
    }
}
```

**崩溃安全性论证**（对应 AC-REL-01）：

| 崩溃时刻 | 磁盘状态 | 结果 |
| --- | --- | --- |
| 步骤 1-3 之间 | `target` 完整旧内容；`tmp` 部分写入 | ✅ 旧内容完整。`tmp` 残留在**用户目录**内（非 `.knowlpad/`），启动时按 `.kp-tmp-*` 模式清理 |
| 步骤 3 后、5 前 | `target` 完整旧内容；`tmp` 完整新内容 | ✅ 旧内容完整，`tmp` 清理 |
| 步骤 5 执行中 | rename 是原子的：要么旧、要么新 | ✅ 不存在中间态 |
| 步骤 5 后、6 前 | 文件内容在盘上，目录项可能未落盘 | ⚠️ 断电可能"丢失"新文件，但**旧内容或新内容二者之一完整**，不会出现半截文件。这满足 NFR-REL-01 的要求 |

> **临时文件位置的权衡**：`.kp-tmp-*` 必须与目标同目录（否则 rename 非原子）。这意味着用户目录中会短暂出现临时文件。缓解：① `create_new` 避免命名冲突 ② 命名含 PID + UUID 便于识别与清理 ③ 启动时扫描 Vault 清理残留 ④ 文件监听中过滤 `.kp-tmp-` 前缀（§5.4）避免触发索引。

---

## 7. 前端架构

### 7.1 状态管理分层

Pinia 4.x（仅 ESM）。按**数据归属**而非按视图划分 store，避免同一数据在多处重复缓存导致不一致。

| Store | 归属 | 数据内容 | 失效策略 |
| --- | --- | --- | --- |
| `useVaultStore` | 全局 | 当前 Vault 信息、Vault 列表、索引状态 | `kp://vault/changed`、`kp://index/*` |
| `useFileTreeStore` | Vault | 树节点（虚拟滚动数据源）、展开/折叠状态 | `kp://fs/*` 四个事件 → 局部节点增删改 |
| `useEditorStore` | Vault | 打开的标签页列表、活动标签、各标签的脏标记与滚动位置 | 用户操作；`kp://fs/removed` 时关闭对应标签 |
| `useNoteStore` | Vault | 已加载笔记的内容缓存（LRU，上限 20 篇） | `kp://note/updated`、`kp://fs/modified` |
| `useLinkStore` | Vault | 当前笔记的反链/出链（仅当前笔记，不全局缓存） | `kp://note/updated`、`kp://link/changed` |
| `useTagStore` | Vault | 标签树（含计数） | `kp://note/updated`、`kp://link/changed` |
| `useSearchStore` | 瞬时 | 查询串、结果、分页、加载态 | 每次查询整体替换 |
| `useGraphStore` | 瞬时 | 图谱数据、过滤条件、cytoscape 实例引用 | 视图关闭时**必须**置空并 destroy 实例（AC-GRAPH-04） |
| `usePreferenceStore` | 全局 | 全部偏好项（本地镜像） | 写入时防抖 500ms 提交（PERF-06） |
| `usePaletteStore` | 全局 | 注册的命令列表、最近使用 | 命令注册在模块初始化时静态完成 |

#### 7.1.1 缓存一致性规则

| 约束 | 内容 |
| --- | --- |
| `ST-01` | **同一份数据只在一个 store 中持有权威副本**。视图组件通过 getter 派生，不得自行缓存到 `ref` |
| `ST-02` | 事件驱动的失效必须**局部化**：`kp://note/updated { rel_path }` 只失效该笔记相关的缓存，禁止全量刷新（否则大库下每次保存都会引发整树重渲染） |
| `ST-03` | `useNoteStore` 用 LRU 限制内存（20 篇），超出淘汰最久未访问且非脏的项；脏项**禁止**淘汰（会丢用户输入） |
| `ST-04` | 全部 store 的 action 中调用 IPC 时**必须**处理错误分支（ERR-01），失败时回滚乐观更新或标记错误态 |
| `ST-05` | 禁止在 store 中直接 `invoke`，必须经 `core/ipc/`（FE-04） |

#### 7.1.2 乐观更新与冲突

编辑保存采用**悲观**策略（不乐观更新），因为 `note_write` 有 `base_mtime` 冲突检测：

```typescript
// features/editor/composables/useNoteSave.ts
async function save(note: OpenNote): Promise<SaveOutcome> {
  try {
    const res = await ipc.noteWrite({
      relPath: note.relPath,
      content: note.buffer,
      baseMtime: note.loadedMtime,     // 冲突检测依据
    });
    note.loadedMtime = res.newMtime;
    note.dirty = false;
    return { kind: 'saved' };
  } catch (err) {
    const e = asKpError(err);
    if (e.code === 'E_WRITE_CONFLICT') {
      // FR-EDITOR-34：外部修改冲突，交 UI 弹三选项
      return { kind: 'conflict', externalMtime: e.context.externalMtime };
    }
    if (e.code === 'E_IO_FAILURE') {
      note.dirty = true;               // 内容不丢（NFR-REL-08 / AC-REL-04）
      return { kind: 'failed', error: e };
    }
    throw e;                           // 其余错误上抛，由全局错误处理记录
  }
}
```

### 7.2 Markdown 渲染管线

```
笔记原文（string）
   │
   ▼
① markdown-it 15.0.2 解析 → HTML 字符串
   │   配置：html: true（允许原始 HTML，但下一步净化）
   │         linkify: false（wikilink 由自定义插件处理，且 linkify 是 DoS 面之一）
   │         typographer: false（smartquotes 是 DoS 面之一，见 SEC-03）
   │   自定义插件：wikilink 规则、tag 规则、block-id 锚点、frontmatter 剥离
   ▼
② DOMPurify 3.4.15 净化 → 安全 HTML
   │   配置逐次传参（禁止 setConfig，SEC-01）
   │   ADD_ATTR: ['data-kp-link','data-kp-tag','data-kp-anchor']  ← 供点击委托识别
   │   FORBID_TAGS: ['script','iframe','object','embed','form','input','style']
   │   FORBID_ATTR: ['onerror','onclick','onload', ...全部 on*]
   │   ALLOWED_URI_REGEXP: /^(?:(?:https?|mailto|asset|tauri):|[^a-z]|[a-z+.\-]+(?:[^a-z+.\-:]|$))/i
   ▼
③ 远程图片处理（SEC-08）
   │   遍历 <img>，src 为 http(s):// 的替换为占位组件（data-kp-remote-src 暂存）
   │   ⚠️ 应用内**永不加载**远程图片：点击占位符时仅调用系统浏览器打开该 URL
   │      （shell:allow-open 的 https 白名单），应用进程保持零网络请求（AC-SEC-03）
   │   本地附件 src 转为 asset:// 协议（需在 tauri.conf.json 配置 assetProtocol 及其 scope）
   │   全部 <img> 设 referrerPolicy="no-referrer"、loading="lazy"
   ▼
④ 插入 DOM（v-html 到此才允许使用）
   │
   ▼
⑤ 事件委托（不在每个链接上绑定监听器）
       容器级 click 监听 → 读 data-kp-* → 分派至路由/命令
```

**性能与安全的双重考量**

| 措施 | 理由 |
| --- | --- |
| `linkify: false` + `typographer: false` | 这两个选项正是 markdown-it DoS 漏洞（CVE 修复项）的触发面。本产品用 wikilink 而非自动 URL 识别，关闭它们既减少攻击面又提升解析速度 |
| 净化配置逐次传参 | 直接对应 PRD FR-EDITOR-42 与 v5 记录的 DOMPurify `setConfig()` 绕过漏洞 |
| 事件委托 | 一篇 5000 字笔记可能含数百个链接/标签，逐个绑定监听器会造成内存与性能问题；委托到容器只需一个监听器 |
| 渲染结果缓存 | 同一笔记内容未变时缓存净化后 HTML（以 content 哈希为键，LRU 20 项），避免滚动/切换标签时重复渲染 |
| **禁止**在 Web Worker 中渲染 | Worker 无 DOM，DOMPurify 需要 DOM 环境。若用 `jsdom` 模拟反而引入新依赖与不一致风险 |

### 7.3 虚拟滚动实现

PERF-02 要求文件树、搜索结果、反链列表、回收站、标签笔记列表在 > 200 项时启用虚拟滚动。

| 列表 | 数据规模 | 行高 | 方案 |
| --- | --- | --- | --- |
| 文件树 | 可达 10 万节点 | 固定 28px | **扁平化 + 虚拟滚动**：树结构展开为一维数组（含 `depth` 与 `visible` 标记），仅渲染可视窗口 ±10 行缓冲 |
| 搜索结果 | 通常 < 1000 | 可变（含片段） | 虚拟滚动 + 动态高度测量缓存 |
| 反链列表 | 可达数千 | 可变（上下文片段） | 同上；片段默认折叠，展开时才测量 |
| 回收站 | 通常 < 1000 | 固定 36px | 固定高度虚拟滚动 |
| 标签笔记列表 | 可达数千 | 固定 32px | 固定高度虚拟滚动 |

**文件树扁平化的关键**（AC-FILE-06 要求 10 万节点 50 FPS）：

```typescript
interface FlatNode {
  key: string;          // rel_path，作为虚拟滚动的稳定 key
  depth: number;
  name: string;
  kind: 'folder' | 'note' | 'attachment' | 'other';
  expanded: boolean;    // 仅 folder 有意义
  visible: boolean;     // 父链全部展开时为 true
  childCount?: number;
}

// 折叠/展开 = 更新 expanded + 重算后续节点的 visible（O(子树大小)）
// 过滤 = 重算 visible（匹配节点及其祖先链可见）
// 虚拟滚动只消费 visible === true 的项构成的数组
```

不采用递归组件（`<TreeNode>` 自嵌套）的原因：10 万节点下 Vue 的组件实例开销（每实例约 1–2KB）会直接吃掉数百 MB 内存，且 diff 成本随树深增长。扁平数组 + 虚拟滚动的内存占用与节点总数无关，只与可视行数有关。

### 7.4 编辑器内核适配层

见 §2.2 的 `KpEditorAdapter` 接口。补充实现约束：

| 约束 | 内容 |
| --- | --- |
| `ED-01` | 适配器**必须**在 `destroy()` 中释放全部资源（DOM 监听器、定时器、编辑器实例）。标签页关闭与视图卸载时调用 |
| `ED-02` | `on('change')` 回调必须节流（≤ 60Hz），避免每次按键触发 store 更新与脏标记计算 |
| `ED-03` | 链接/标签补全的触发逻辑在适配器**外部**实现（`composables/useSuggest.ts`），通过 `triggerSuggest` 与 `insertLink` 与内核交互。理由：补全数据来自 IPC，与内核无关，复用可避免 M8 换内核时重写 |
| `ED-04` | 快捷键注册走 `core/shortcut`（平台映射，NFR-PLAT-07），适配器不自行绑定 `keydown` |
| `ED-05` | MVP 的 `md-editor-v3` 与正式版 CodeMirror 6 **不共存于同一构建**：通过构建时环境变量 `VITE_EDITOR_ENGINE` 选择，避免包体膨胀与行为分歧 |
| `ED-06` | **代码高亮不引入任何第三方独立高亮库**（`shiki`/`highlight.js`/`prismjs` 等）。MVP 阶段用 `md-editor-v3` 内置高亮；M8 起编辑态用 `@codemirror/language` 的 `syntaxHighlighting()`、阅读态用 `@lezer/highlight` 的 `highlightCode()`，二者共用 `core/theme/` 的同一套 CSS 变量（`HL-05`）。切换内核时必须同步移除前一阶段的高亮实现与样式（`HL-06`），避免遗留死代码 |

### 7.5 前端性能预算

> ⚠️ **与「暂不考虑打包体积」的关系**（2026-09-20 决策）：下表中**首屏 JS / 懒加载 chunk 两项预算予以保留**。
> 理由：这两项度量的是**加载与解析耗时**，是达成 G1（冷启动 ≤3s）、G6（1 万条列表 ≥55fps）的手段，
> 而非「分发体积」约束。已解除约束的是**安装包分发体积**（jieba 词典 5MB 全量内置、
> `[profile.release]` 的 `opt-level` 由 `"s"` 改为 `3`，见 §3.5.1、`DEBT-03`）。
> **维护者不得以「不考虑体积」为由删除本节**——删除会使 G1 失去可度量的守护手段。

| 指标 | 预算 | 守护手段 |
| --- | --- | --- |
| 首屏 JS 体积（gzip） | < 400KB | `codeSplitting` 拆分；vendor 分离；CI 记录体积趋势，超预算 10% 告警 |
| 懒加载 chunk 单个体积 | < 200KB | 图谱（cytoscape）单独 chunk，仅打开时加载 |
| 组件重渲染 | 输入时 < 3 个组件 | Vue DevTools Profiler 抽查；用 `shallowRef` 存大对象 |
| 内存（标准库，5 标签） | < 300MB（NFR-PERF-13） | 笔记 LRU 20、图谱实例 destroy、事件监听注销（EVT-05） |
| 长任务 | 无 > 200ms 的主线程任务 | 大计算（如 10 万节点扁平化）分片 + `requestIdleCallback` |

---

## 8. IPC 实现

Command 与事件清单见 PRD §5.3/§5.4（不重复）。本章定义**实现方式与契约同步机制**。

### 8.1 Rust 侧 Command 实现范式

```rust
// commands/note.rs —— 薄壳层，严格遵循 RS-01（≤30 行、无业务逻辑）

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteWriteArgs {
    pub rel_path: String,
    pub content: String,
    pub base_mtime: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteResult {
    pub rel_path: String,
    pub new_mtime: i64,
    pub bytes_written: usize,
}

#[tauri::command]
pub async fn note_write(
    state: tauri::State<'_, AppState>,
    args: NoteWriteArgs,
) -> Result<WriteResult, AppError> {
    let vault = state.vault.read().await.clone().ok_or(AppError::VaultNotOpen)?;
    // 全部业务逻辑在 domain 层（RS-01/02）
    domain::note_io::write(&vault, &args.rel_path, args.content.as_bytes(), args.base_mtime).await
}
```

**范式约束**

| 约束 | 内容 |
| --- | --- |
| `CMD-01` | Command 函数体只做：取 `AppState` → 参数校验（类型层面已由 serde 完成）→ 调 `domain` → 返回。禁止出现 `if` 业务分支、SQL、`fs::` 调用 |
| `CMD-02` | 入参统一包裹为单一 `Args` 结构体（而非多个位置参数），便于向后兼容地新增可选字段 |
| `CMD-03` | serde 属性统一：Rust 侧 `#[serde(rename_all = "camelCase")]`，前端 TS 用 camelCase。与 PRD §5.1 `IPC-02`（v2.1 已修正为 camelCase）一致——理由见 §8.4 |
| `CMD-04` | 全部 Command 为 `async fn`（即使内部无 await），统一由 tokio 调度，避免同步 Command 阻塞 IPC 线程（AC-04） |
| `CMD-05` | 错误返回 `Result<T, AppError>`，`AppError` 实现 `serde::Serialize`，序列化为 PRD §5.2 的 `KpError` 结构 |
| `CMD-06` | 写操作 Command（PRD §5.3 标注 ⚠️ 的 **24** 个）必须在入口处记录 INFO 级审计日志（操作类型 + 相对路径 + 结果，**不含内容**，SEC-09） |

### 8.2 AppError 与错误码映射

```rust
// error.rs
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("当前没有打开的知识库")]
    VaultNotOpen,

    #[error("知识库路径不存在或不可访问：{0}")]
    VaultPathInvalid(String),

    #[error("操作路径超出知识库范围")]
    PathOutsideVault(String),

    #[error("路径包含非法内容")]
    PathEscapeDeny(String),

    #[error("文件不存在：{0}")]
    FileNotFound(String),

    #[error("目标已存在：{0}")]
    FileExists(String),

    #[error("文件已被外部修改，请选择处理方式：{rel_path}")]
    WriteConflict { rel_path: String, external_mtime: i64 },

    #[error("文件被其他程序占用：{0}")]
    FileLocked(String),

    #[error("文件超过大小上限（{limit} MB）：{path}")]
    FileTooLarge { path: String, limit: u64 },

    #[error("没有访问该文件的权限：{0}")]
    PermissionDenied(String),

    #[error("文件名不合法：{0}")]
    InvalidFilename(String),

    #[error("读写失败：{0}")]
    IoFailure(String),

    #[error("批量改写失败，已回滚 {rolled_back} 个文件：{failed_path}")]
    RewriteFailed { failed_path: String, rolled_back: usize, #[source] source: io::Error },

    #[error("备份创建失败，操作已中止：{path}")]
    BackupFailed { path: String, #[source] source: io::Error },

    #[error("回滚失败，备份保留于 {backup_dir}")]
    RollbackFailed { failures: Vec<(PathBuf, io::Error)>, backup_dir: PathBuf },

    #[error("预览已过期，请重新执行操作")]
    PreviewExpired,

    #[error("搜索语法错误：{0}")]
    SearchSyntax(String),

    #[error("数据库操作失败：{0}")]
    DbError(String),

    #[error("链接目标存在歧义，请指定具体笔记")]
    LinkAmbiguous,

    #[error("笔记元数据格式有误，已按普通正文处理")]
    ParseFrontmatter(String),

    #[error("恢复失败，原位置已被占用：{0}")]
    TrashRestoreConflict(String),

    #[error("索引签名不匹配，需要重建索引")]
    IndexSignatureMismatch,

    #[error("索引正在进行中，请稍后重试")]
    IndexBusy,

    #[error("更新包签名验证失败，已阻止安装")]
    UpdateSignature,

    #[error("更新包校验和不匹配，已阻止安装")]
    UpdateChecksum,

    #[error("网络不可用，更新检查已跳过")]
    UpdateNetwork(String),

    #[error("操作已取消")]
    Cancelled,

    #[error("内部错误：{0}")]
    Internal(String),

    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),

    #[error(transparent)]
    Tauri(#[from] tauri::Error),
}

impl AppError {
    /// 结构化上下文（写入 KpError.context，供前端做分支处理，见 PRD §5.2）
    pub fn context(&self) -> Option<serde_json::Value> {
        match self {
            Self::RewriteFailed { failed_path, rolled_back, .. } => Some(serde_json::json!({
                "failedPath": failed_path, "rolledBack": rolled_back,
            })),
            Self::RollbackFailed { failures, backup_dir } => Some(serde_json::json!({
                "failedCount": failures.len(),
                "backupDir": backup_dir.to_string_lossy(),
            })),
            Self::BackupFailed { path, .. } => Some(serde_json::json!({ "path": path })),
            Self::FileTooLarge { path, limit } => Some(serde_json::json!({ "path": path, "limit": limit })),
            Self::WriteConflict { rel_path, external_mtime } => Some(serde_json::json!({
                "relPath": rel_path, "externalMtime": external_mtime,
            })),
            _ => None,
        }
    }

    /// 映射为 PRD §5.2 的错误码（前端契约）
    pub fn code(&self) -> &'static str {
        match self {
            Self::VaultNotOpen => "E_VAULT_NOT_OPEN",
            Self::VaultPathInvalid(_) => "E_VAULT_PATH_INVALID",
            Self::PathOutsideVault(_) => "E_PATH_OUTSIDE_VAULT",
            Self::PathEscapeDeny(_) => "E_PATH_ESCAPE_DENY",
            Self::FileNotFound(_) => "E_FILE_NOT_FOUND",
            Self::FileExists(_) => "E_FILE_EXISTS",
            Self::WriteConflict { .. } => "E_WRITE_CONFLICT",
            Self::FileLocked(_) => "E_FILE_LOCKED",
            Self::FileTooLarge { .. } => "E_FILE_TOO_LARGE",
            Self::PermissionDenied(_) => "E_PERMISSION_DENIED",
            Self::InvalidFilename(_) => "E_INVALID_FILENAME",
            Self::IoFailure(_) => "E_IO_FAILURE",
            Self::RewriteFailed { .. } => "E_REWRITE_FAILED",
            Self::BackupFailed { .. } => "E_BACKUP_FAILED",
            Self::RollbackFailed { .. } => "E_REWRITE_FAILED",   // 复用码，context 区分
            Self::PreviewExpired => "E_PREVIEW_EXPIRED",
            Self::SearchSyntax(_) => "E_SEARCH_SYNTAX",
            Self::DbError(_) | Self::Sqlite(_) => "E_DB_ERROR",
            Self::LinkAmbiguous => "E_LINK_AMBIGUOUS",
            Self::ParseFrontmatter(_) => "E_PARSE_FRONTMATTER",
            Self::TrashRestoreConflict(_) => "E_TRASH_RESTORE_CONFLICT",
            Self::IndexSignatureMismatch => "E_INDEX_SIGNATURE_MISMATCH",
            Self::IndexBusy => "E_INDEX_BUSY",
            Self::UpdateSignature => "E_UPDATE_SIGNATURE",
            Self::UpdateChecksum => "E_UPDATE_CHECKSUM",
            Self::UpdateNetwork(_) => "E_UPDATE_NETWORK",
            Self::Cancelled => "E_CANCELLED",
            Self::Internal(_) | Self::Tauri(_) => "E_INTERNAL",
        }
    }
}

/// 序列化为前端 KpError
impl serde::Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("KpError", 4)?;
        st.serialize_field("code", &self.code())?;
        st.serialize_field("message", &self.to_string())?;          // 中文用户提示（ERR-02）
        st.serialize_field("detail", &format!("{self:?}"))?;        // 技术细节，仅入日志
        st.serialize_field("context", &self.context())?;            // 结构化上下文
        st.end()
    }
}
```

> **`message` 与 `detail` 的分离**（对应 PRD ERR-02）：`message` 由 `thiserror` 的 `#[error("...")]` 提供，必须是**中文、非技术、含可操作建议**；`detail` 是 `Debug` 输出，仅写日志与开发者工具，UI 不展示。这条分离纪律保证了用户看到的提示可读，同时开发者能拿到完整堆栈信息。

### 8.3 前后端契约同步（防止类型漂移）

59 个 Command × 各自的入参/返回类型，手工维护 TS 与 Rust 两份定义**必然**产生漂移。采用**手写 + CI 契约校验**：

```
Rust struct（唯一真相源）
   │
   │  【M0 决策】手写维护（不引入 codegen，见下方决策）
   ▼
src/core/ipc/commands.ts（手写，经 core/ipc 类型安全封装）
   │
   │  CI 门禁 17：IPC 契约一致性校验
   ▼
比对 TS 调用名与 Rust #[tauri::command] 名集合 → 缺失即 CI 失败
```

```javascript
// scripts/verify-ipc-contract.mjs（CI 门禁 17）
// 1. 扫描 src/core/ipc/commands.ts 中 TS 侧调用的命令名集合
// 2. 扫描 src-tauri/src 下全部 #[tauri::command] 名集合
// 3. TS 调用了 Rust 不存在的命令 → 退出码 1（阻断）
// 4. Rust 已实现但 TS 未封装 → 仅告警（不阻断）
```

> **M0 决策（2026-09-25）：不引入 codegen 工具，采用「手写 `commands.ts` + CI 契约校验」。**
> 依据：`tauri-specta` 稳定版 `1.0.2` 依赖 `tauri ^1.2.4`（Tauri 1.x，与本项目 Tauri 2.11.6 不兼容）；支持 Tauri 2 的 `2.0.0-rc.25` 仍是**预发布**，违反本项目「禁用预发布依赖」纪律（§3.5 纪律 1）。故按备选思路行事，但不实现自研宏 codegen，而以**名字集合契约**（门禁 17）防止漂移；待 `tauri-specta 2.x` 正式发布后再评估替换。风险项 `TR-01` 据此关闭。

### 8.4 命名风格的统一决策

PRD §5.1 的 `IPC-02` 原写「字段名统一 `snake_case`（serde 默认）」，而 Tauri 生态与 Vue/TS 惯例是 camelCase。二者必须择一，本文档做出**明确决策，PRD 已于 v2.1 同步修正**：

| 层面 | 规范 | 理由 |
| --- | --- | --- |
| Rust 结构体字段 | `snake_case`（Rust 语言惯例） | 符合 `cargo clippy` 与 Rust 社区规范 |
| **IPC 传输的 JSON 键** | **`camelCase`**（通过 `#[serde(rename_all = "camelCase")]`） | 前端 TS 直接消费，符合 JS 惯例；避免前端到处写 `rel_path` |
| TS interface 字段 | `camelCase` | 与传输格式一致，无需转换层 |
| Command 名 | `snake_case`（如 `note_write`） | Tauri 惯例，且 Command 名不是数据字段 |
| Event 名 | `kp://<域>/<事件>` kebab-case | PRD §0.2 已定 |

> **PRD 修正留痕**：PRD §5.1 `IPC-02` **已于 v2.1 修正**为「字段名在 IPC 传输中使用 camelCase（Rust 侧通过 `serde(rename_all = "camelCase")` 映射），Rust 结构体内部字段名保持 snake_case」。此修正记入 §12 勘误表 D-16。

---

## 9. 安全实现

威胁模型与安全需求见 PRD §6.3（15 项威胁、16 条 SEC 需求）。本章定义**四层防护的具体实现**。

### 9.1 第 1 层：进程与权限边界

#### 9.1.1 Capabilities 最小权限白名单

```json
// src-tauri/capabilities/default.json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Knowl Pad 主窗口最小权限集",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "core:path:default",
    "core:event:default",
    "core:window:default",
    "core:webview:default",

    "dialog:allow-open",
    "dialog:allow-message",
    "dialog:allow-ask",
    "dialog:allow-confirm",

    "updater:default",
    "process:allow-restart",

    {
      "identifier": "shell:allow-open",
      "allow": [
        { "url": "https://**" },
        { "url": "mailto:**" }
      ]
    }
  ]
}
```

**明确不授予**（对应 SEC-05、AC-SEC-02）：

| 权限 | 拒绝理由 |
| --- | --- |
| `fs:*`（全部） | 违反 PRD AC-01。全部文件操作走自定义 Command，统一施加 §6.2 路径校验与 §6.3 原子写入。若授予 `fs` 插件权限，一旦前端出现 XSS，攻击者可直接读写任意文件 |
| `shell:allow-execute` / `shell:allow-spawn` | 无执行外部程序的需求，等同 RCE 面 |
| `process:allow-exit` | 无强制退出需求；仅保留 `restart`（更新后重启） |
| `http:*` / `fetch` | 应用无任意网络请求需求；唯一的对外请求（更新检查）由 updater 插件在其自身权限内完成 |
| `core:webview:allow-create-webview-window` | 无多窗口需求（分栏在同一 Webview 内实现） |
| `notification:*` | V1.0 不使用系统通知 |

> **`shell:allow-open` 的协议白名单是安全关键**：不限定协议时，`file://`、`smb://`、自定义协议 URL 都可能被用于本地资源探测或协议处理器攻击。限定为 `https?://` 与 `mailto:` 后，笔记中的外部链接只能交系统浏览器打开网页或邮件客户端。

#### 9.1.2 CSP 配置

```json
// src-tauri/tauri.conf.json（节选）
{
  "app": {
    "security": {
      "csp": {
        "default-src": "'self'",
        "script-src": "'self'",
        "style-src": "'self' 'unsafe-inline'",
        "img-src": "'self' asset: https://asset.localhost blob: data:",
        "font-src": "'self' data:",
        "connect-src": "'self' ipc: http://ipc.localhost https://gitee.com",
        "object-src": "'none'",
        "base-uri": "'self'",
        "form-action": "'none'",
        "frame-ancestors": "'none'"
      },
      "dangerousDisableAssetCspModification": false,
      "freezePrototype": false
    },
    "withGlobalTauri": false
  }
}
```

| 指令 | 说明 |
| --- | --- |
| `script-src 'self'` | **无 `unsafe-inline`、无 `unsafe-eval`**（SEC-13）。这是对抗 XSS 的最后一道防线：即使 DOMPurify 被绕过，内联脚本也无法执行 |
| `style-src` 含 `unsafe-inline` | Vue 与 Tailwind 运行时需要注入 style；这是**已知妥协**，风险低于 script 内联（CSS 注入的主要危害是 UI 欺骗与数据外泄，后者已被 `img-src`/`connect-src` 限制）。记录于 §13.3 技术债 |
| `img-src` 不含 `https:` | 直接对应 SEC-08：远程图片默认不可加载。用户确认加载远程图片时，通过 Tauri 的 `asset:` 协议代理获取（而非直接 `https:`），使加载行为可控可审计 |
| `connect-src` 限定 `gitee.com` | 唯一允许的外部连接目标是更新源。`ipc:`/`http://ipc.localhost` 是 Tauri IPC 必需 |
| `object-src 'none'` / `frame-ancestors 'none'` / `form-action 'none'` | 关闭插件对象、禁止被 iframe 嵌套、禁止表单提交到外部 |
| `withGlobalTauri: false` | 不注入全局 `window.__TAURI__`，前端只能通过打包进来的 `@tauri-apps/api` 模块访问，缩小暴露面 |

> **`dangerousDisableAssetCspModification: false`**：保持 Tauri 自动为 `asset:` 协议追加 CSP 白名单的行为。若设为 `true`，需手工维护 asset 协议白名单，易漏配导致图片无法显示。

### 9.2 第 2 层：路径与文件系统边界

实现见 §6.2 `PathGuard`。补充**调用点强制约束**：

| 约束 | 内容 |
| --- | --- |
| `PATH-01` | `PathGuard` 实例**只能**由 `VaultHandle` 构造，且构造时 `canonical_root` 已由 `vault_open` 校验。**禁止**任何 domain 函数接收裸 `&Path` 后自行拼接 |
| `PATH-02` | `domain/` 与 `storage/` 中**禁止**出现 `fs::read` / `fs::write` / `File::open` 的直接调用，必须经 `note_io.rs` 或 `fs_ops.rs` 的封装函数（这些函数内部调用 PathGuard）。通过 `clippy` 自定义 lint 或 grep 门禁在 CI 中检查 |
| `PATH-03` | 全部路径参数在 IPC 边界即为 `String` 类型的相对路径，进入 domain 后第一步就转为经校验的 `PathBuf`。**禁止**在业务逻辑中途传递未校验的字符串路径 |
| `PATH-04` | 回收站、备份目录的路径由 `vault.rs` 统一生成（`.knowlpad/trash/`、`.knowlpad/backup/`），不接受外部输入拼接 |

**CI 门禁**（对应 PATH-02）：

```bash
# 检查 domain/storage 中是否存在裸文件 IO（排除 note_io.rs / fs_ops.rs / platform/）
grep -rn --include='*.rs' -E 'std::fs::(read|write|remove_file|rename|copy|create_dir)' \
     src-tauri/src/domain src-tauri/src/storage \
  | grep -v -E 'note_io\.rs|fs_ops\.rs|path_guard\.rs' \
  && echo "❌ 发现绕过封装的文件 IO" && exit 1 || echo "✅ 路径封装检查通过"
```

### 9.3 第 3 层：内容渲染边界

实现见 §7.2 渲染管线。补充净化配置的**集中管理**：

```typescript
// core/markdown/sanitize.ts —— 全项目唯一的净化入口
import DOMPurify, { type Config } from 'dompurify';

const SANITIZE_CONFIG: Config = Object.freeze({
  FORBID_TAGS: ['script', 'iframe', 'object', 'embed', 'form', 'input',
                'button', 'textarea', 'select', 'style', 'link', 'meta', 'base'],
  FORBID_ATTR: ['onerror', 'onclick', 'onload', 'onmouseover', 'onfocus',
                'onblur', 'onsubmit', 'onchange', 'oninput', 'srcdoc', 'formaction'],
  ALLOWED_URI_REGEXP: /^(?:(?:https?|mailto|asset|tauri|blob):|[^a-z]|[a-z+.\-]+(?:[^a-z+.\-:]|$))/i,
  ADD_ATTR: ['data-kp-link', 'data-kp-tag', 'data-kp-anchor', 'data-kp-remote-src',
             'target', 'rel', 'referrerPolicy', 'loading'],
  ADD_TAGS: ['img'],           // 允许 img（但 src 受 ALLOWED_URI_REGEXP 与远程处理约束）
  KEEP_CONTENT: true,          // 移除标签但保留其文本内容（避免内容凭空消失）
  RETURN_DOM: false,
  RETURN_TRUSTED_TYPE: false,
  // 明确不使用 setConfig（SEC-01 / FR-EDITOR-42）
});

/** 全项目唯一的 HTML 净化入口。禁止在任何其他位置直接调用 DOMPurify。 */
export function sanitizeHtml(dirty: string): string {
  // 配置逐次传参，不使用持久化 API
  return DOMPurify.sanitize(dirty, SANITIZE_CONFIG);
}

// 启动时的一次性自检：验证净化器确实生效（防止依赖被替换或配置失效）
export function selfTest(): void {
  const probes = [
    '<script>alert(1)</script>',
    '<img src=x onerror=alert(2)>',
    '<iframe src="https://evil.example"></iframe>',
    '<a href="javascript:alert(3)">x</a>',
    '<svg onload=alert(4)>',
    '<div onclick=alert(5)>x</div>',
  ];
  for (const p of probes) {
    const out = sanitizeHtml(p);
    if (/script|iframe|onerror|onclick|onload|javascript:/i.test(out)) {
      // 净化失效是安全关键故障：阻断应用启动而非静默继续
      throw new Error(`DOMPurify 自检失败，输入 ${p} 净化后仍含危险内容：${out}`);
    }
  }
}
```

**启动自检的设计意图**：依赖升级、配置误改、甚至供应链投毒都可能导致 DOMPurify 静默失效。`selfTest()` 在应用启动时执行，**失败即抛错阻断启动**（而非记录警告后继续）——因为此时继续运行等于把全部用户暴露于 XSS。对应 AC-SEC-01 的自动化验证。

### 9.4 第 4 层：更新与供应链边界

#### 9.4.1 更新签名验证

```rust
// domain/update.rs（基于 tauri-plugin-updater）
pub async fn check_and_install(app: &AppHandle, force: bool) -> Result<UpdateOutcome> {
    // 1. 拉取 update.json（强制 HTTPS，由 endpoint 配置保证）
    let updater = app.updater_builder()
        .on_download(|event| { emit_download_progress(app, event) })
        .build()?;

    match updater.check().await {
        Ok(Some(update)) => {
            // 2. SemVer 比对由插件内部完成
            if !force && is_skipped_version(&update.version)? {
                return Ok(UpdateOutcome::Skipped);
            }
            // 3. 用户确认（UI 层处理，此处假定已确认）
            // 4. 下载 + 签名验证 + SHA-256 校验
            //    ⚠️ 签名验证由 tauri-plugin-updater 内部强制执行：
            //       它用 tauri.conf.json 中的 pubkey 验证 minisign 签名，
            //       验签失败会返回错误，绝不会返回可安装的 payload。
            //       这是插件的核心安全设计，不可绕过、不可关闭。
            let downloaded = update.download_and_install(|_, _| {}, || {}).await
                .map_err(|e| match classify_update_error(&e) {
                    UpdateErrorKind::Signature => AppError::UpdateSignature,      // AC-UPDATE-02
                    UpdateErrorKind::Checksum  => AppError::UpdateChecksum,       // AC-UPDATE-03
                    UpdateErrorKind::Network   => AppError::UpdateNetwork(e.to_string()),
                    UpdateErrorKind::Other     => AppError::Internal(e.to_string()),
                })?;
            // 5. 清理下载缓存（FR-UPDATE-14）
            cleanup_update_cache(app)?;
            let _ = downloaded;
            Ok(UpdateOutcome::Installed)
        }
        Ok(None) => Ok(UpdateOutcome::UpToDate),
        Err(e) => {
            // FR-UPDATE-11：网络失败静默降级，不弹窗不阻塞启动
            if is_network_error(&e) {
                tracing::debug!(error = %e, "更新检查失败（网络），静默跳过");
                return Ok(UpdateOutcome::NetworkUnavailable);
            }
            Err(e.into())
        }
    }
}
```

**关于 v5 表述的修正**（对应 PRD FR-UPDATE-14 说明、§12 D-01/D-02）：

| v5 §11.7 的表述 | 实际情况 | 本方案的处理 |
| --- | --- | --- |
| 「更新元数据文件使用 **RSA 2048** 数字签名」 | `tauri-plugin-updater` 使用 **minisign/ed25519**，不是 RSA。`update.json` 中各平台的 `signature` 字段是 minisign 签名的 base64 编码 | 修正为 ed25519/minisign；密钥对由 `tauri signer generate` 生成，公钥入 `tauri.conf.json`，**私钥必须离线保管**（CI 中以 secret 注入） |
| 「使用 bsdiff 等二进制差异算法生成 patch」实现增量更新 | Tauri updater **不支持**二进制差分。它下载的是完整安装包（`.msi`/`.dmg`/`.deb`/`.AppImage`/`.tar.gz`） | V1.0 限定**全量更新**。若未来需要增量，需自研差分下载 + 本地重组，成本高，不列入路线图 |
| 「更新失败时**自动回滚到上一版本**，保留最近 1-2 个历史版本快照」 | 安装包替换由操作系统安装器完成。失败时旧版本**未被移除**，因此实际语义是「保持当前版本不变」，不存在"回滚"动作，也没有历史版本快照机制 | 修正为 FR-UPDATE-08「更新失败时保留当前版本可继续使用」。删除"历史版本快照"表述 |
| 「传输层强制 **TLS 1.3**」 | Tauri updater 基于 `reqwest`，TLS 版本由系统/`rustls` 决定，通常支持 1.2 与 1.3，**无法在应用层强制仅 1.3** | 修正为「强制 HTTPS（TLS 1.2+）」，见 SEC-04 |

#### 9.4.2 供应链防护

| 措施 | 实现 | 对应 |
| --- | --- | --- |
| 兼容范围声明 + lockfile 锁定 | `package.json` 声明向后兼容范围（`^`/`~`），**实际版本由 `pnpm-lock.yaml` 锁定**；`Cargo.toml` 声明 caret 范围，`Cargo.lock` 锁定。CI 用 `--frozen-lockfile` 严格按 lockfile 安装，不自行解析新版本。完整策略见 §3.6.1 | SEC-06 |
| Lockfile 入库 | `pnpm-lock.yaml`、`Cargo.lock` **均提交入库**（`.gitignore` 不得排除，§11.7.2）；门禁 8 用 `pnpm install --frozen-lockfile --dry-run` 校验与 manifest 一致 | SEC-06 |
| 升级受控 | 依赖升级是**显式动作**：`pnpm update <pkg>` / `cargo update -p <crate>` → 跑通全部门禁 → lockfile 变更**单独成 commit**。**禁止** CI 用非 frozen 安装，禁止把 lockfile 变更混入功能提交（§3.6.1） | SEC-06 / T-09 |
| 依赖审计门禁 | CI 执行 `pnpm audit --audit-level=high` + `cargo audit`，高危即失败 | SEC-06 / AC-SEC-04 |
| 停更包排除 | `vuedraggable` → `vue-draggable-next`（v5 已确立）；`serde_yaml`（已归档）→ `yaml-rust2`（§3.5.2） | R-16 |
| CI 权限最小化 | `GITEE_TOKEN` 仅授予 `projects` + `releases` 权限（v5 §11.6）；私钥仅在 release job 中以 secret 注入，PR 构建**不注入** | SEC-06 |
| 构建可复现性 | 固定 Node 版本（`.nvmrc`）、固定 Rust toolchain（`rust-toolchain.toml`）、`pnpm` 版本由 `packageManager` 字段锁定 | SEC-06 |

```toml
# rust-toolchain.toml —— 固定 Rust 版本，防止 CI 与本地构建差异
# ⚠️ 注意：TOML 注释符是 #，不是 //（本节原稿误用 //，已修正）
[toolchain]
channel = "1.90.0"        # 实际有效 MSRV，由 tauri 2.12 家族决定（2026-09-30 核实）
                          # 注意：Tauri 2.11.x 自身仅要求约 1.77（已核实 2.11.5 为 1.77.2），
                          # 但 2026-09-30 起由 tauri 2.12 家族抬到 1.90（time/image 的 1.88 已不再是约束）。见 DEBT-08
components = ["rustfmt", "clippy", "llvm-tools-preview"]   # llvm-tools 供 cargo-llvm-cov 使用（DEBT-10）
profile = "minimal"
```

### 9.5 日志与隐私

对应 SEC-09、SEC-16、威胁 T-12。

```rust
// 日志初始化（main.rs）
fn init_logging(config_dir: &Path) -> Result<()> {
    let log_dir = config_dir.join("logs");
    fs::create_dir_all(&log_dir)?;

    // 按日滚动，保留 14 天（PRD §2.4.2）
    let file_appender = tracing_appender::rolling::RollingFileAppender::new(
        tracing_appender::rolling::Rotation::DAILY,
        &log_dir,
        "knowl-pad.log",
    );

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env()
              .unwrap_or_else(|_| "info,tower_http=warn".into()))
        .with(tracing_subscriber::fmt::layer()
              .writer(file_appender)
              .with_ansi(false)
              // 关键：不记录 span 的 Debug 输出，防止意外泄露内容
              .fmt_fields(tracing_subscriber::fmt::format::DefaultFields::new()))
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))  // 仅 debug 构建
        .init();

    // 启动清理：删除 14 天前的日志
    cleanup_old_logs(&log_dir, 14)?;
    // 启动清理：删除 Vault 内残留的 .kp-tmp-* 临时文件（§6.3）
    Ok(())
}
```

**日志内容红线**（`core/logger` 与 Rust 侧共同遵守）：

| 允许记录 | 禁止记录 |
| --- | --- |
| 错误码、错误分类 | 笔记正文（任何片段） |
| 相对路径（**不含** Vault 绝对路径前缀） | frontmatter 内容 |
| 操作类型（read/write/rename/delete/rebuild） | 搜索查询词 |
| 耗时、文件大小、文件数量 | 标签名（可能含敏感信息，如 `#医疗/某疾病`） |
| 版本号、平台、CPU 核数 | Vault 绝对路径（仅在 `vault_open` 的 INFO 日志中记录一次，用于诊断） |
| 索引统计（各类实体计数） | 偏好设置的具体值 |

**CI 门禁**：grep 检查代码中是否存在将内容写入日志的模式（如 `tracing::*!(?content`、`tracing::*!(%text`），命中即告警。

**零遥测的验证方式**（AC-SEC-03）：在隔离网络环境（如 mitmproxy 透明代理 + 全量流量记录）运行完整功能遍历，断言除更新检查外零对外请求。此测试为**发布门禁**。

---

## 10. 跨平台实现

需求见 PRD §6.4。本章定义实现方式。

### 10.1 平台抽象层

```rust
// platform/mod.rs
pub trait PlatformAdapter {
    /// 原子替换（rename 语义差异，§6.3）
    fn atomic_replace(&self, tmp: &Path, target: &Path) -> Result<()>;
    /// 设置目录权限为仅当前用户可读写（SEC-15）
    fn restrict_dir_perms(&self, path: &Path) -> Result<()>;
    /// 路径显示规范化（Windows 长路径前缀处理）
    fn display_path(&self, path: &Path) -> String;
    /// 在系统文件管理器中显示
    fn reveal_in_file_manager(&self, path: &Path) -> Result<()>;
    /// 换行符默认风格
    fn default_line_ending(&self) -> &'static str;
    /// 原生菜单构建（macOS 需要）
    fn build_menu(&self, app: &AppHandle) -> Option<Menu>;
    /// 文件名非法字符集
    fn invalid_filename_chars(&self) -> &'static [char];
    /// 保留设备名（Windows 特有）
    fn reserved_names(&self) -> &'static [&'static str];
}

#[cfg(target_os = "windows")] pub use windows::WindowsAdapter as Adapter;
#[cfg(target_os = "macos")]   pub use macos::MacosAdapter as Adapter;
#[cfg(target_os = "linux")]   pub use linux::LinuxAdapter as Adapter;
```

**满足 PRD NFR-PLAT-15**：全部平台差异收敛于此，`domain/` 通过 trait 调用，不出现 `#[cfg(target_os)]`。

### 10.2 文件名校验（NFR-PLAT-04：最严格并集）

```rust
/// 三平台非法字符的并集 —— 保证 Vault 可跨平台拷贝
const INVALID_CHARS: &[char] = &[
    '\\', '/', ':', '*', '?', '"', '<', '>', '|',   // Windows 非法
    '\0',                                             // 全平台
    // 控制字符 0x00-0x1F 单独检查
];

/// Windows 保留名（即使 Vault 在 Linux 上也拒绝，保证可迁移性）
const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL",
    "COM1","COM2","COM3","COM4","COM5","COM6","COM7","COM8","COM9",
    "LPT1","LPT2","LPT3","LPT4","LPT5","LPT6","LPT7","LPT8","LPT9",
];

pub fn validate_filename(name: &str) -> Result<()> {
    // 1. 非空
    if name.is_empty() { return Err(AppError::InvalidFilename("文件名不能为空".into())); }
    // 2. 长度（保留余量给 Vault 路径前缀，NFR-PLAT-10）
    if name.chars().count() > 200 { return Err(AppError::InvalidFilename("文件名过长（上限 200 字符）".into())); }
    // 3. 控制字符
    if name.chars().any(|c| c.is_control()) { return Err(AppError::InvalidFilename("文件名含控制字符".into())); }
    // 4. 非法字符
    if let Some(c) = name.chars().find(|c| INVALID_CHARS.contains(c)) {
        return Err(AppError::InvalidFilename(format!("文件名不得包含字符 '{c}'")));
    }
    // 5. 首尾空格与点号（Windows 会静默去除，导致名称不一致）
    if name.starts_with(' ') || name.ends_with(' ') || name.ends_with('.') {
        return Err(AppError::InvalidFilename("文件名不得以空格开头或以空格/点号结尾".into()));
    }
    // 6. 保留名（含 "CON.md" 这类带扩展名的形式）
    let stem = name.split('.').next().unwrap_or(name);
    if RESERVED_NAMES.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        return Err(AppError::InvalidFilename(format!("'{stem}' 是 Windows 保留设备名".into())));
    }
    // 7. 全为点号（"." ".." "..."）
    if name.chars().all(|c| c == '.') { return Err(AppError::InvalidFilename("文件名不得仅由点号组成".into())); }
    Ok(())
}
```

> **为什么 Linux 上也执行 Windows 规则**：PRD NFR-PLAT-04 要求 Vault 可在三平台间自由拷贝。若在 Linux 上允许 `CON.md` 或 `a:b.md`，用户拷贝到 Windows 时文件会创建失败或行为异常，且这种损坏发生在软件之外，无法给出提示。**统一取最严格并集**是唯一能保证可迁移性的做法。对应 AC-FILE-03。

### 10.3 文件监听的平台差异

| 平台 | 底层机制 | 已知限制 | 处理 |
| --- | --- | --- | --- |
| Windows | `ReadDirectoryChangesW` | 网络驱动器上事件可能丢失；事件缓冲区溢出时 `notify` 报 `Rescan` | 收到 `Rescan` 时触发全量 mtime 扫描对账 |
| macOS | `FSEvents` | 事件有 ~1s 延迟；同一事件可能重复投递 | 200ms 去抖动窗口 + `pending` HashMap 去重（§5.4）已覆盖 |
| Linux | `inotify` | `max_user_watches` 默认 8192，大 Vault 耗尽；不监听挂载点变化 | 捕获 `MaxFilesWatch` → 上报含 `sysctl` 建议的错误 → 降级轮询模式（§5.4） |

**统一对账机制**：无论平台，每次 Vault 打开时都执行一次 **mtime 对账扫描**（比对索引库记录的 `mtime_ms` 与磁盘实际值），修复监听遗漏导致的索引漂移。这是 NFR-PERF-11 之外的一致性兜底。

### 10.4 打包配置

```json
// src-tauri/tauri.conf.json（bundle 节选）
{
  "bundle": {
    "active": true,
    "targets": "all",
    "identifier": "com.knowlpad.desktop",
    "icon": ["icons/32x32.png", "icons/128x128.png", "icons/icon.ico", "icons/icon.icns"],
    "shortDescription": "本地优先的笔记与知识管理软件",
    "longDescription": "Knowl Pad 是一款跨平台本地优先的笔记与知识管理软件，数据完全保存在你的电脑上，以纯 Markdown 文件存储。",
    "category": "Productivity",
    "windows": {
      "wix": { "language": ["zh-CN", "en-US"] },
      "nsis": { "languages": ["SimpChinese", "English"], "installMode": "currentUser" }
    },
    "macOS": {
      "minimumSystemVersion": "12.0",
      "category": "public.app-category.productivity",
      "signingIdentity": null,
      "providerShortName": null,
      "entitlements": null
    },
    "linux": {
      "deb": { "depends": ["libwebkit2gtk-4.1-0", "libgtk-3-0", "libayatana-appindicator3-1"] },
      "appimage": { "bundleMediaFramework": false }
    },
    "createUpdaterArtifacts": true
  }
}
```

| 平台 | 产物 | 说明 |
| --- | --- | --- |
| Windows | `.msi`（WiX）+ `.exe`（NSIS） | NSIS 用 `currentUser` 安装模式，避免要求管理员权限 |
| macOS | `.dmg` + `.app` | `signingIdentity: null` 表示未签名——对应 NFR-PLAT-13，**必须在 README 中明确告知用户 Gatekeeper 绕过方式及风险** |
| Linux | `.deb` + `.AppImage` | ✅ M0 核实：**Ubuntu 22.04 jammy** 与 **Debian 12 bookworm** 均存在 `libwebkit2gtk-4.1-0`、`libgtk-3-0`、`libayatana-appindicator3-1`（`deb.depends` 可直接使用）；**Fedora** 构建期包名按官方 prerequisites：`webkit2gtk4.1-devel`、`libappindicator-gtk3-devel`、`librsvg2-devel`、`openssl-devel`。RPM 运行时依赖由打包器自动推导 |
| 全平台 | updater artifacts | `createUpdaterArtifacts: true` 生成 `.sig` 签名文件与 `latest.json`，供 §9.4 更新流程使用 |

> **macOS 未签名的诚实披露**（NFR-PLAT-13）：无 Apple 开发者账号时，用户下载后首次打开会被 Gatekeeper 拦截。文档中**必须**说明「右键 → 打开」或 `xattr -d com.apple.quarantine` 的操作方式，并**明确告知这会绕过系统安全检查、仅在信任来源时使用**。禁止隐瞒此限制或暗示软件已签名。

---

## 11. 质量保障与发布

### 11.1 测试分层实现

测试策略见 PRD §8.3。本节定义各层的**技术实现方式**。

| 层级 | 实现要点 |
| --- | --- |
| **Rust 单元测试** | `domain/` 每个模块内嵌 `#[cfg(test)] mod tests`。解析器（`md_parse/`）用 PRD 附录 B 的 25 个用例做数据驱动测试：`#[test] fn b01_simple_wikilink()` ... 每个规则至少一正例一反例（TEST-01）。测试用 `tempfile` 创建临时 Vault，禁止依赖固定路径 |
| **Rust 集成测试** | `src-tauri/tests/` 目录。构造真实临时 Vault → 调用 domain 函数 → 断言数据库状态。覆盖全部 59 个 Command 的正向 + 错误路径（PRD §8.3） |
| **前端单元测试** | `vitest` + `@vue/test-utils` + `jsdom`。`core/utils` 纯函数覆盖率 ≥ 90%；核心组件（编辑器、文件树、搜索、反链面板）必测交互 |
| **契约测试** | `scripts/verify-ipc-contract.mjs`（§8.3）。CI 门禁，防止 TS/Rust 类型漂移与 Command 数量不符 |
| **E2E** | `tauri-driver` + `WebDriverIO`。覆盖 PRD §1.3 的 10 个核心场景。**注意**：`tauri-driver` 在 macOS 上支持受限（WebDriver 对 WKWebView 支持不完整），macOS E2E 可能需降级为关键路径人工验证，此限制须在 M0 确认并记录 |
| **安全测试** | `tests/security/`。① 路径穿越：对 §6.2 的六类恶意载荷逐一断言被拦截（AC-SEC-01）② XSS：用 §9.3 的 probe 集断言净化生效（AC-EDITOR-01）③ 零网络请求：mitmproxy 透明代理 + 流量断言（AC-SEC-03）④ Capabilities 审计：解析 `default.json` 断言不含宽权限（AC-SEC-02） |
| **可靠性测试** | `tests/reliability/`。故障注入：用脚本在写入过程中随机 `kill -9`，重启后断言文件完整性（AC-REL-01，1000 次循环）；索引中断恢复（AC-REL-02）；索引重建一致性（AC-REL-03） |
| **性能基准** | `tests/perf/`。用 `scripts/gen-fixture-vault.mjs` 生成标准/大/压力三档数据集，对 PRD §6.1.2 的 16 项指标逐项测量，结果写入 JSON 供 CI 趋势比对（回退 > 20% 阻断，PERF-08） |
| **扩展语法解析测试** | `tests/fixtures/syntax-compat/`。夹具为**依据 PRD §3.1 规则条款预先固化的结构化断言**（推导流程见 PRD 附录 B），比对「解析器输出 vs 夹具」，断言解析准确率 ≥ 99%（SJ-04）。**不依赖任何外部软件**，同时满足 PRD TEST-05 |

**基准数据集生成**（保证可复现，不入库大文件）：

```javascript
// scripts/gen-fixture-vault.mjs
// 参数：--tier standard|large|stress --out <dir>
// standard: 3000 篇 × 2KB + 15% 附件
// large:    20000 篇 × 4KB
// stress:   100000 篇
// 内容用确定性伪随机（固定 seed）生成，含可控比例的 wikilink、嵌套标签、
// frontmatter、代码块、标题、block id，以覆盖解析器全部分支
```

### 11.2 版本号管理

对应 PRD 无（属技术实现）。继承 v5 §11.1/11.2 的 SemVer 规范，补充**自动同步实现**。

版本号需在**四处**严格一致（v5 §11.2）：

| 位置 | 字段 |
| --- | --- |
| `src-tauri/tauri.conf.json` | `version` |
| `package.json` | `version` |
| `src-tauri/Cargo.toml` | `version` |
| Git Tag | `v<version>` |

```javascript
// scripts/bump-version.mjs —— 由 semantic-release 调用，保证四处同步
// 1. 读取新版本号（来自 semantic-release 的 nextRelease.version）
// 2. 写入 tauri.conf.json（JSON 解析→改 version→序列化，保留格式）
// 3. 写入 package.json（同上）
// 4. 写入 Cargo.toml（用 toml 解析器，仅改 [package].version，不动其他）
// 5. 校验四处一致，不一致则退出码 1
// 禁止手工修改任何一处版本号（v5 §11.2 的要求，用 CI 门禁守护）
```

**CI 门禁**：每次构建校验四处版本号一致，不一致即失败。

### 11.3 Git 提交规范（修正 v5）

v5 §11.3 的表格存在**损坏行与重复段落**（`feat!` 行的表格结构错乱，且整段 Conventional Commits 说明重复出现两次）。修正后的完整规范：

| Commit Type | 版本影响 | 示例 |
| --- | --- | --- |
| `fix:` | PATCH | `fix(editor): 修复代码块高亮异常` |
| `feat:` | MINOR | `feat(graph): 新增知识图谱力导向布局` |
| `perf:` | PATCH | `perf(index): 批量事务提交，索引提速 3×` |
| `refactor:` | 无 | `refactor(file-tree): 重构递归组件为扁平虚拟滚动` |
| `docs:` | 无 | `docs: 更新 PRD §4.5 搜索需求` |
| `chore:` | 无 | `chore(deps): 锁定 Rust 新增依赖版本` |
| `test:` | 无 | `test(md_parse): 补充附录 B 兼容性用例` |
| `build:` / `ci:` | 无 | `ci: 增加构建产物完整性校验` |
| `feat!:` 或含 `BREAKING CHANGE:` 脚注 | MAJOR | `feat!: 重构索引库 schema` / `fix(security)!: 收紧 Capabilities\n\nBREAKING CHANGE: 移除 fs 插件权限` |
| `revert:` | 视被回滚提交 | `revert: feat(graph): ...` |

**scope 约定**（对应 §2.2/§2.3 的模块划分）：`vault` `file` `editor` `link` `search` `graph` `tag` `attach` `palette` `trash` `settings` `update` `index` `parse` `security` `deps` `ci`。

### 11.4 CI/CD 流水线（历史设计：Gitee Go；实际已迁移 GitHub Actions）

v5 §11.5 的 YAML 存在**多处错误**：使用 `github.repository_owner`、`github.event.repository.name`、`github.ref_name` 等 GitHub Actions 变量（在 Gitee Go 中不存在）；`actions-rs/toolchain` 已停更；把 `pnpm ci` 当**依赖安装命令**用（pnpm 无内置 `ci` 子命令，安装应为 `pnpm install --frozen-lockfile`；注意 `pnpm ci` 会回落到运行本项目的门禁聚合脚本，详见 §11.4 修正表）；缺少本文档定义的全部质量门禁。修正版：

> ⚠️ **现状（2026-09-25，附录 D.3）**：Gitee Go 的变量语法（`GITEE_*`/`{GITEE_xxx}`）与流水线模型均与下方 YAML 不兼容，**实际 CI 已迁移至 `.github/workflows/`**（`ci.yml` + `release.yml` + `platform-smoke.yml`，经 `actionlint` 校验），门禁执行 `scripts/run-gates.mjs`。下方 YAML 仅作「门禁与结构设计」的历史留档，不再可直接运行；`DEBT-06` 已关闭。

```yaml
# .gitee/workflows/knowlpad.yml
name: Knowl Pad CI/CD

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

env:
  NODE_VERSION: '24.19.0'
RUST_TOOLCHAIN: '1.90.0'   # 实际 MSRV，由 tauri 2.12 家族决定（2026-09-30 核实，见 DEBT-08）

jobs:
  # ── 阶段 1：静态检查与测试（PR 与 push 均执行）──────────
  quality:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: 安装 pnpm
        uses: pnpm/action-setup@v4
        with: { version: 12.4.1 }

      - name: 安装 Node
        uses: actions/setup-node@v4
        with:
          node-version: ${{ env.NODE_VERSION }}
          cache: pnpm

      - name: 安装 Rust（dtolnay 替代已停更的 actions-rs）
        uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: ${{ env.RUST_TOOLCHAIN }}
          components: rustfmt, clippy, llvm-tools-preview

      - uses: Swatinem/rust-cache@v2
        with: { workspaces: src-tauri }

      - name: 安装 cargo-audit 与 cargo-llvm-cov（门禁 7 与覆盖率门禁所需，均不在 rustup components 中）
        run: |
          cargo install cargo-audit --locked
          cargo install cargo-llvm-cov --locked

      - name: 安装系统依赖（Tauri Linux 构建所需 + 门禁脚本依赖）
        run: |
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev \
                                  librsvg2-dev patchelf libssl-dev \
                                  ripgrep
          # ripgrep 为门禁 13（命名检查）与路径封装检查脚本的必需依赖。
          # ⚠️ ubuntu-latest runner 不预装 rg；两个脚本已内置依赖守卫，
          #    缺失时会以退出码 2 硬性失败（而非静默通过）。
          rg --version | head -1

      - run: pnpm install --frozen-lockfile          # 修正：不是 pnpm ci

      # ── 17 项质量门禁（PRD §8.4）─────────
      # 单一真相源：本地与 CI 执行同一脚本；下面的分步写法仅供理解门禁构成。
      # 实际 ci.yml 用一行：
      #   - run: node scripts/run-gates.mjs
      - run: pnpm typecheck                            # 门禁 1
      - run: pnpm lint                                 # 门禁 2
      - run: pnpm gate:clippy                          # 门禁 3
      - run: pnpm gate:fmt                             # 门禁 4
      - run: pnpm test:coverage                        # 门禁 5
      - run: pnpm gate:rust-test                       # 门禁 6
      - name: Rust domain 覆盖率门禁（≥ 85%，DEBT-10）
        run: pnpm gate:coverage
      - run: pnpm audit --audit-level=high             # 门禁 7
      - run: cargo audit --file Cargo.lock             # 门禁 7（工作区锁在仓库根）
      - run: pnpm gate:lockfile                        # 门禁 8
      - run: pnpm contract:verify                      # 门禁 17：IPC 契约
      - name: 路径封装检查（SEC PATH-02）
        run: bash scripts/check-path-encapsulation.sh
      - name: 命名一致性检查（门禁 13，PRD §0.2）
        run: bash scripts/check-naming.sh              # 扫描禁止写法，命中即失败

      # ── 门禁 9：生产构建 + Vite 8 chunk 完整性校验（§3.4.2 规避手段）──
      - name: 生产构建（重复 3 次比对产物一致性）
        run: |
          for i in 1 2 3; do
            pnpm build
            node scripts/verify-build-integrity.mjs --out dist-$i
            mv dist dist-$i
          done
          node scripts/compare-build-manifests.mjs dist-1 dist-2 dist-3

      # ── 门禁 10/11/12：性能、安全、可靠性 ─────────────
      - run: pnpm fixture:gen -- --tier standard --out /tmp/std-vault
      - run: pnpm test:perf -- --baseline .perf-baseline.json --max-regression 20  # 门禁 10：CI 仅做相对回退比对；绝对值达标在发布前于专用机器判定（附录 B.5）
      - run: pnpm test:security
      - run: pnpm test:reliability

  # ── 阶段 2：多平台构建与发布（仅 main 分支 push）────────
  release:
    needs: quality
    # ⛔ 已废弃（2026-09-25）：gitee.* 命名空间不存在（Gitee Go 用 GITEE_*）。
    #    本 YAML 已由 .github/workflows/ 取代，此行仅作历史留档（DEBT-06 已关闭）。
    if: gitee.ref == 'refs/heads/main' && gitee.event_name == 'push'
    strategy:
      matrix:
        include:
          - { platform: ubuntu-22.04,  target: x86_64-unknown-linux-gnu }
          - { platform: windows-latest, target: x86_64-pc-windows-msvc }
          - { platform: macos-latest,  target: aarch64-apple-darwin }
          - { platform: macos-13,      target: x86_64-apple-darwin }
    runs-on: ${{ matrix.platform }}
    steps:
      - uses: actions/checkout@v4
      # ...（pnpm/node/rust 安装同上，略）
      - run: pnpm install --frozen-lockfile

      - name: 语义化发版（生成版本号、CHANGELOG、同步四处版本）
        run: pnpm exec semantic-release   # 必须用 exec（本地已锁定的版本）；dlx 会绕过 lockfile
        env:
          GITEE_TOKEN: ${{ secrets.GITEE_TOKEN }}
          # ⚠️ 更新签名私钥仅在此 job 注入，PR 构建不注入（§9.4.2）
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_KEY_PASSWORD }}

      - name: 构建平台安装包
        run: pnpm tauri build --target ${{ matrix.target }}

      - name: 上传至 Gitee Release
        uses: release-files-to-gitee@v1        # 修正：v5 用的 yanglbme/gitee-release-action 需核实可用性
        with:
          gitee_token: ${{ secrets.GITEE_TOKEN }}
          # ⛔ 已废弃（2026-09-25）：Gitee Go 的系统变量是 GITEE_* 形式，引用语法为 {GITEE_xxx}，
          #    不存在 ${{ gitee.* }}；且其流水线不是 GitHub Actions 的 on/jobs/uses 模型。
          #    实际 CI 已迁移至 .github/workflows/（DEBT-06 已关闭，见附录 D.3）。
          #    以下 gitee.* 占位仅作历史留档，不再使用。
          owner: ${{ gitee.repository_owner }}   # ← 待替换为 {GITEE_xxx} 形式
          repo: ${{ gitee.repository_name }}     # ← 待替换为 {GITEE_xxx} 形式
          tag: ${{ gitee.ref_name }}             # ← 待替换为 {GITEE_xxx} 形式
          files: |
            src-tauri/target/${{ matrix.target }}/release/bundle/**/*
            src-tauri/target/${{ matrix.target }}/release/bundle/**/*.sig
```

| 修正项 | v5 的问题 | 本方案 |
| --- | --- | --- |
| 变量命名空间 | 用 `github.repository_owner` / `github.event.repository.name` / `github.ref_name` | 改用 `gitee.*`（**实际变量名需在 M0 按 Gitee Go 官方文档核实**，此处为占位；v5 §11.5 的注意事项本身就承认了这一点但未修正） |
| Rust 安装 | `actions-rs/toolchain@v1`（已停更多年） | `dtolnay/rust-toolchain@stable` |
| 依赖安装 | 把 `pnpm ci` 当**安装命令**用 | `pnpm install --frozen-lockfile`。⚠️ 澄清：pnpm **无内置 `ci` 子命令**（与 npm 的 `npm ci` 不同），故拿它当安装命令是错的；但 `pnpm ci` 本身会回落到运行 `package.json` 的 `ci` **脚本**（本项目的门禁聚合脚本，§3.6），二者用途不同，勿混淆（红线 R-17） |
| 系统依赖 | 完全缺失 → Linux 构建必然失败 | 补齐 `libwebkit2gtk-4.1-dev` 等 |
| 质量门禁 | 仅 `cargo build` + `pnpm build`，无任何测试/审计/lint | 补齐全部 17 项（PRD §8.4 的 13 项 + 4 项扩展） |
| 多平台 | 单 `ubuntu-latest`，却期望产出 `.msi`/`.dmg` → 不可能 | matrix 策略四平台 |
| 构建缓存 | 无 | `Swatinem/rust-cache` + pnpm store 缓存 |
| Release action | `yanglbme/gitee-release-action@main` 可用性未验证 | 标注需 M0 核实；给出 `semantic-release-gitee` 作为备选路径 |

> **诚实声明**：Gitee Go 的具体内置变量名、可用的 action 生态与 GitHub Actions 存在差异，本 YAML 的 `gitee.*` 变量与 `release-files-to-gitee` action **均需在 M0 按 Gitee Go 官方文档核实后替换为准确值**。本方案给出的是结构与门禁的完整设计，而非可直接运行的最终配置。**2026-09-25 更新**：该核实已完成并据此迁移 GitHub Actions（附录 D.3），本段保留为历史决策记录。

### 11.5 发布链路（GitHub Actions + Gitee Release 同步）

发布链路不使用 semantic-release（原因见下方作废说明），现状如下：

| 阶段 | 实现 |
| --- | --- |
| 版本号同步 | `scripts/bump-version.mjs <semver>`：同步 `package.json` / `src-tauri/tauri.conf.json` / `src-tauri/Cargo.toml`，并校验三处一致 |
| 触发 | 打 tag（`v*`）或 `workflow_dispatch` |
| 构建 | `.github/workflows/release.yml` 四平台矩阵（Windows / Ubuntu / macOS arm64 / macOS intel），`pnpm tauri build` 产出安装包与 `.sig` |
| updater 元数据 | `scripts/gen-latest-json.mjs` 分平台生成 → `scripts/merge-latest-json.mjs` 合并为 `latest.json` |
| GitHub Release | `gh release create <tag> --notes-file CHANGELOG.md artifacts/*` |
| Gitee Release 同步 | `scripts/publish-gitee-release.mjs`（Gitee OpenAPI v5；`--verify` 只读联调、`--dry-run` 预演、幂等复用 Release） |

#### 11.5.1 版本与变更日志自动化（DEBT-11 已关闭）

不引入 semantic-release：改用**零依赖自研脚本**，直接读 `git log` 解析 Conventional Commits（PRD CODE-09）。

| 脚本 | 职责 |
| --- | --- |
| `scripts/changelog.mjs` | 解析 `git log` → 过滤发布提交与 `[skip ci]` → 按「✨ 新功能 / 🐛 问题修复 / ⚡ 性能优化 / ⏪ 回滚」分组，破坏性变更单列；默认打印（dry-run），`--write` 写入 `CHANGELOG.md` 顶部 |
| `scripts/prepare-release.mjs` | 推断递增类型（破坏性→major、feat→minor、其余→patch）→ 调 `bump-version.mjs` 同步 `package.json`/`tauri.conf.json`/`Cargo.toml` 并校验一致 → 写 CHANGELOG → 打印 `git commit/tag/push` 后续命令。**工作区不干净即拒绝**，默认 dry-run |
| `scripts/bump-version.mjs` | 唯一的版本号写入点（四处同步 + 一致性校验） |

人工发布流程：

```bash
pnpm release:prepare            # 预演：看版本推断与 CHANGELOG 预览
pnpm release:prepare -- --write # 落盘：同步版本 + 写 CHANGELOG
git add -A && git commit -m "chore(release): x.y.z"
git tag vx.y.z && git push --follow-tags   # 触发 release.yml
```

> 回归测试：`tests/unit/changelog.spec.mjs`（18 项，覆盖解析/过滤/分组/推断/渲染/插入）；`node scripts/changelog.mjs --self-test` 可在无 git 环境下自检。

> **配置文件说明**：原设计的 `.releaserc.json` **已删除**——它引用的 6 个 semantic-release 插件在 `devDependencies` 中一个都未安装，属不可执行配置；且其中的 `semantic-release-gitee` 已违反 R-16。发布说明与版本号自动化暂缺，登记为 `DEBT-11`。
```

> ⛔ **M0 核实结论（2026-09-25，已升级为阻断项）**：`semantic-release-gitee@0.0.1-dev` 在 npm registry 上的时间为 **created 2022-02-10 / modified 2022-05-17**，即**已停更逾 4 年**，且仅发布过一个 `-dev` 版本。这**直接违反红线 R-16（禁止使用已停更的依赖）**，因此**不得进入 V1.0**。处置：改用 GitHub Actions（或自研脚本）调用 Gitee OpenAPI 上传 Release 资产，并同步维护 `latest.json` 与 `.sig`；`DEBT-06` 由「核实」升级为「必须替换」。

**历史沿革**：v5 §11.6 与本文档早期版本采用 `semantic-release` + `semantic-release-gitee` 编排发布；M0 核实确认后者已停更逾 4 年（R-16），遂改为「GitHub Actions 构建 + `gh release` + 自研 Gitee 同步脚本」。原文的 `releaseRules`、中文 CHANGELOG 分类、四处版本同步、`.sig`/`latest.json` 资产清单等设计要点，已由现方案中的 `bump-version.mjs` 与 `release.yml` 承接。

**updater 元数据文件名说明**：v5 §11.7 使用 `update.json`，而 Tauri 2 updater 的约定文件名是 `latest.json`（`createUpdaterArtifacts: true` 自动生成）。本方案统一采用 **`latest.json`**，`tauri.conf.json` 的 endpoint 相应配置为：

```json
"plugins": {
  "updater": {
    "pubkey": "<base64 编码的 minisign 公钥，由 tauri signer generate 产出>",
    "endpoints": [
      "https://gitee.com/<owner>/knowl-pad/releases/download/latest/latest.json"
    ],
    "windows": { "installMode": "passive" }
  }
}
```

### 11.6 发布前检查清单

每次发布（含 alpha/beta/rc）前**必须**逐项确认：

| # | 检查项 | 依据 |
| --- | --- | --- |
| 1 | CI 全部门禁通过（17 项） | PRD §8.4 |
| 2 | 三平台 × 20 项跨平台验收清单逐项通过 | PRD §6.4.1 |
| 3 | 安全测试集全通过（AC-SEC-01~05） | 发布门禁 |
| 4 | 可靠性测试集全通过（AC-REL-01~04） | 发布门禁 |
| 5 | 性能基准无 > 20% 回退 | PERF-08 |
| 6 | 扩展语法解析准确率 ≥ 99% | SJ-04 |
| 7 | 四处版本号一致 | §11.2 |
| 8 | CHANGELOG 已生成且内容准确 | §11.5 |
| 9 | 更新签名密钥为正式密钥（非测试密钥），私钥离线保管 | §9.4.2 |
| 10 | `latest.json` 中各平台 URL 可访问、签名与哈希正确 | §9.4.1 |
| 11 | 安装包在**干净虚拟机**上验证安装→启动→核心功能→更新全链路 | 防止开发环境污染导致的假通过 |
| 12 | macOS 未签名情况已在 README 明确披露绕过方式与风险 | NFR-PLAT-13 |
| 13 | 无遥测、无崩溃上报代码（grep 审计） | SEC-16 |
| 14 | 生产构建不含 sourcemap、devtools 已禁用 | SEC-14 |
| 15 | 依赖审计无高危 CVE | SEC-06 |

### 11.7 工程配置文件（补全）

v5 与本文档前序版本提到以下文件但**未给出内容**，导致 M0 无法直接初始化工程。本节补齐全部 7 个文件的可落地内容。

#### 11.7.1 `.nvmrc`

```
24.19.0
```

> 与 `package.json` 的 `engines.node`、CI 的 `node-version` 三处必须一致（§11.2 版本号同步纪律）。

#### 11.7.2 `.gitignore`

```gitignore
# ⚠️ lockfile 必须入库（红线 R-17 / SEC-06）：pnpm-lock.yaml 与 Cargo.lock
#    是「安装层面锁定」的载体，不得出现在本文件任何忽略规则中，也不得被
#    node_modules/ 等规则间接排除。依赖管理策略见 §3.6.1。

# ── Node / 前端 ─────────────────────────────
node_modules/
dist/
dist-ssr/
.vite/
*.local
.pnpm-store/

# ── Rust / Tauri ────────────────────────────
src-tauri/target/
target/
**/*.rs.bk
*.pdb

# ── 测试与覆盖率 ────────────────────────────
coverage/
*.lcov
test-results/
playwright-report/

# ── 密钥与凭据（严禁入库，SEC-06 / SEC-08）──
*.pem
*.key
tauri.*.key
tauri.*.pub
.env
.env.*
!.env.example

# ── 编辑器与系统 ────────────────────────────
.idea/
*.iml
.DS_Store
Thumbs.db
desktop.ini

# ── 本项目特有：测试用 Vault 夹具产物 ────────
tests/fixtures/**/.knowlpad/
!tests/fixtures/**/.keep
```

> **`!.env.example` 的作用**：忽略全部 `.env*` 但保留示例文件，确保新人能知道需要哪些变量而不会误提交真实密钥。

#### 11.7.3 `.npmrc`

```ini
# ⚠️ 不设置 save-exact：本项目依赖策略为「声明层面兼容范围、安装层面 lockfile 锁定」
# （§3.6.1）。save-exact=true 会让 `pnpm add` 写入精确版本（无 ^/~），与该策略冲突，
# 故显式不启用。精确版本由 pnpm-lock.yaml 锁定，CI 用 --frozen-lockfile 安装。

# pnpm 严格模式：未在 package.json 声明的依赖不可被 import（防幽灵依赖）
strict-peer-dependencies=false
auto-install-peers=true

# 隔离式 node_modules，与 pnpm 默认行为一致
node-linker=isolated

# 国内构建加速（CI 中可用环境变量覆盖）
# 仓库默认使用官方 registry；国内镜像属于**可选加速**，由开发者/CI 通过环境变量覆盖，
# 不写入仓库——避免第三方镜像影响 lockfile 完整性与 CI 复现性：
#   npm_config_registry=https://registry.npmmirror.com/ pnpm install --frozen-lockfile

# 安装脚本：false 表示【不忽略】postinstall 等脚本，即允许执行。
# rusqlite（bundled SQLite）等依赖需要构建脚本，故必须为 false。
ignore-scripts=false

# ⚠️ M0 实测：pnpm 12 不再读取 package.json 的 "pnpm" 字段（设置项已迁移到 pnpm-workspace.yaml），
#    且默认**不执行**依赖的构建脚本，遇到被忽略的构建脚本会直接以 ERR_PNPM_IGNORED_BUILDS 失败。
#    正确做法见 pnpm-workspace.yaml（§11.7.3b）。
```

生成 pnpm-workspace.yaml（pnpm 12 起设置项的归属地；缺失会导致 `pnpm install` 失败）：

```yaml
packages:
  - '.'

# 显式允许需要构建脚本的依赖（当前仅 esbuild）。
# 这是把 T-09 的攻击面从「全量放开」收敛为「白名单」的落点之一。
allowBuilds:
  esbuild: true

# 未被允许的依赖若带有构建脚本，此处降级为警告而非 install 失败。
strictDepBuilds: false
```

> **M0 实测（2026-09-25）**：不提供该文件时 `pnpm install` 报 `ERR_PNPM_IGNORED_BUILDS`；`allowBuilds` 必须是 **map** 形式（`包名: true`），写成列表会报 YAML 解析错误。

#### 11.7.3b pnpm-workspace.yaml（pnpm 12 新增必需项）

见上方代码块。CI 与本地均依赖该文件；`.github/workflows/ci.yml` 中的 `pnpm install --frozen-lockfile` 会在缺失时失败。

> **`ignore-scripts=false` 的含义与权衡**：`false` = 不忽略安装脚本（即**允许** postinstall 执行），这是 `rusqlite`（bundled SQLite）等需要构建脚本的依赖能正常工作的前提。它确实保留了一定的 postinstall 投毒面（威胁 T-09），但本策略的防护来自 **lockfile 锁定 + `pnpm audit` / `cargo audit` 审计门禁 + frozen 安装**（§3.6.1、门禁 7/8），而非靠全局禁用脚本。若未来引入不可信依赖，应改用 pnpm 的 `onlyBuiltDependencies` **白名单**精确控制哪些包可运行构建脚本，而非把 `ignore-scripts` 全局改为 `true`（那会直接破坏 rusqlite 构建）。

#### 11.7.4 `.prettierrc.json`

```json
{
  "semi": false,
  "singleQuote": true,
  "trailingComma": "all",
  "printWidth": 100,
  "tabWidth": 2,
  "arrowParens": "always",
  "endOfLine": "lf",
  "vueIndentScriptAndStyle": false,
  "plugins": ["prettier-plugin-tailwindcss"],
  "overrides": [
    {
      "files": ["*.md"],
      "options": { "proseWrap": "preserve", "printWidth": 120 }
    }
  ]
}
```

> `endOfLine: "lf"` 是**跨平台必需**项：防止 Windows 开发者提交 CRLF 导致 CI 的 `prettier --check` 与 `git diff` 噪声（对应 PRD NFR-PLAT 系列）。

#### 11.7.5 `eslint.config.js`（flat config，ESLint 9）

```js
import js from '@eslint/js'
import tseslint from 'typescript-eslint'
import pluginVue from 'eslint-plugin-vue'
import vueParser from 'vue-eslint-parser'

export default tseslint.config(
  { ignores: ['dist/**', 'node_modules/**', 'src-tauri/target/**', 'coverage/**'] },

  js.configs.recommended,
  ...tseslint.configs.strictTypeChecked,
  ...tseslint.configs.stylisticTypeChecked,

  // Vue SFC：script 块交给 TS parser，模板交给 vue parser
  ...pluginVue.configs['flat/recommended'],
  {
    files: ['**/*.vue'],
    languageOptions: {
      parser: vueParser,
      parserOptions: {
        parser: tseslint.parser,
        project: './tsconfig.json',
        extraFileExtensions: ['.vue'],
      },
    },
  },

  {
    languageOptions: {
      parserOptions: { project: ['./tsconfig.json', './tsconfig.node.json'] },
    },
    rules: {
      // ── 安全相关：本项目为本地文件应用，必须从严 ──
      'no-eval': 'error',
      'no-implied-eval': 'error',
      'no-new-func': 'error',
      // 禁止直接触碰 Tauri 原始 API，必须走 src/core/ipc/client.ts 封装（技术方案 §8.3）；
      // 例外：src/core/ipc/** 自身，见文件末尾的例外配置块
      'no-restricted-imports': [
        'error',
        {
          paths: [
            {
              name: '@tauri-apps/api/core',
              message: '禁止直接调用 invoke()，请改用 src/core/ipc/client.ts 的类型安全封装',
            },
          ],
        },
      ],
      // 禁止前端出现文件路径拼接逻辑（路径校验是 Rust 侧职责，SEC-12）
      'no-restricted-syntax': [
        'error',
        {
          selector: "CallExpression[callee.name='join']",
          message: '路径拼接必须在 Rust 侧完成（§6.2 路径安全），前端只传递 Vault 相对路径',
        },
        // Vite 8 回归 #1（§3.4.2）：对带别名成员的字符串 const enum 会生成
        // 错误的反转映射代码。因已决策不回退 Vite（§3.4.3），此项升级为硬性门禁：
        // 跨边界枚举一律用 `as const` 对象字面量 + 联合类型替代。
        // 这同时更符合 isolatedModules / verbatimModuleSyntax 的单文件转译约束。
        {
          selector: 'TSEnumDeclaration[const=true]',
          message:
            'Vite 8 生产回归（§3.4.2 #1）：禁止使用 const enum，请改用 `as const` 对象字面量 + 联合类型',
        },
      ],
      '@typescript-eslint/no-floating-promises': 'error',
      '@typescript-eslint/no-misused-promises': 'error',
      '@typescript-eslint/consistent-type-imports': 'error',
      'vue/multi-word-component-names': 'off',

      // ── 红线自动化（PRD §8.2）────────────────────────────
      // R-04：禁止用 v-html 渲染未净化 HTML。本项目渲染用户 Markdown，
      // 必须走 core/markdown 的净化管线，不得在模板里直接 v-html。
      'vue/no-v-html': 'error',
      // R-05：禁止修改 DOMPurify 全局配置（一旦被改，全站净化策略失效）。
      // 需要定制净化行为时，必须在 core/markdown 内用独立实例的 sanitize 选项。
      'no-restricted-properties': [
        'error',
        {
          object: 'DOMPurify',
          property: 'setConfig',
          message:
            'R-05 红线：禁止使用 DOMPurify.setConfig()，请在 core/markdown 中通过 sanitize() 的第三参传入局部配置',
        },
        {
          object: 'DOMPurify',
          property: 'clearConfig',
          message: 'R-05 红线：禁止清除 DOMPurify 全局配置',
        },
      ],
    },
  },

  // 例外 1：core/ipc 是唯一允许直接触碰 @tauri-apps/api 的目录（FE-04）
  {
    files: ['src/core/ipc/**/*.ts'],
    rules: { 'no-restricted-imports': 'off' },
  },

  // 例外 2：唯一允许使用 v-html 的位置——净化后的安全渲染组件（§7.2 步骤 ④）。
  //         该组件内部必须先调用 sanitizeHtml()，禁止拼接未经净化的 HTML。
  {
    files: ['src/core/markdown/**/*.vue', 'src/shared/components/KpSafeHtml.vue'],
    rules: { 'vue/no-v-html': 'off' },
  },

  // 不属于任何 tsconfig project 的文件：关闭类型化规则，避免 parserOptions.project 报错
  {
    files: ['eslint.config.js', 'scripts/**/*.mjs', 'vitest.config.ts', '*.config.ts'],
    ...tseslint.configs.disableTypeChecked,
  },
)
```

> 两条 `no-restricted-*` 规则是**架构纪律的自动化执行**：把「IPC 必须走封装」「路径校验只在 Rust 侧」这两条红线（PRD R 系列）从文档约定变成 lint 门禁，避免开发过程中被无意破坏。

#### 11.7.5b `husky/` + `lint-staged` 配置（Git pre-commit hook）

```json
// package.json 中的 lint-staged 配置（husky 9 不再需要 husky.config.js）
{
  "lint-staged": {
    "*.{ts,vue}": ["eslint --fix", "prettier --write"],
    "*.{json,md,css,yml,yaml}": ["prettier --write"],
    "*.rs": ["rustfmt --edition 2024"]
  }
}
```

```bash
# husky 9 初始化（一次性，M0 执行）
pnpm dlx husky init
# 生成 .husky/pre-commit，内容如下：
```

```sh
# .husky/pre-commit
pnpm lint-staged
```

> husky 9 简化为 `.husky/` 目录下的 shell 脚本，不再需要 `husky.config.js`。`lint-staged` 在提交时自动对暂存文件执行 ESLint + Prettier + rustfmt，对应 PRD CODE-03。`pnpm install` 时 husky 自动安装 Git hook（通过 `package.json` 的 `"prepare": "husky"` 脚本）。

#### 11.7.6 `vitest.config.ts`

```ts
import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'
import { resolve } from 'node:path'

export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: { '@': resolve(fileURLToPath(new URL('./src', import.meta.url))) },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    include: ['tests/unit/**/*.spec.ts', 'src/**/*.{test,spec}.ts'],
    exclude: ['tests/e2e/**', 'tests/perf/**', 'node_modules/**'],
    coverage: {
      provider: 'v8',
      reporter: ['text', 'lcov', 'json-summary'],
      // 阈值对应 PRD §8.3 的覆盖率门禁（CI 门禁 6）
      // 全局底线 + 目录级阈值（对应 PRD §8.3：core/utils ≥ 90%）
      thresholds: {
        'src/core/utils/**': { lines: 90, functions: 90, branches: 85, statements: 90 },
        lines: 80,
        functions: 80,
        branches: 75,
        statements: 80,
      },
      exclude: [
        '**/*.d.ts',
        '**/main.ts',
        'src/core/ipc/**', // 纯转发层，由集成测试覆盖（FE-04）
        'tests/**',
      ],
    },
    // 所有 IPC 调用在单测中被 mock，禁止真实触达 Rust
    setupFiles: ['tests/unit/setup.ts'],
  },
})
```

> `tests/unit/setup.ts` 的职责：mock `window.__TAURI_INTERNALS__`，使组件单测不依赖 Rust 进程。这是前端可独立测试的前提。

#### 11.7.7 CI secrets 注入方式（补齐 `DEBT-06` 的操作细节）

更新签名私钥（`TAURI_SIGNING_PRIVATE_KEY`）**严禁**写入仓库。CI 中的注入方式：

| 平台 | 注入语法 | 说明 |
| --- | --- | --- |
| Gitee Go | `${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}` | 在仓库「设置 → 凭据管理」中创建，⚠️ **变量名语法待 M0 核实**（Gitee Go 与 GitHub Actions 不完全一致） |
| GitHub Actions（备选） | `${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}` | 语法已确认 |

**四条硬性要求**：

1. 私钥仅在 release job 中注入，**lint/test job 不得持有**（缩小暴露面）。
2. 私钥口令 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 同样走 secrets，不得为空字符串（空口令等于无保护）。
3. CI 日志中必须确认私钥**未被回显**——Gitee Go 默认遮蔽 secrets，但若通过 `echo` 拼接进其他变量则会泄露，需在 M0 实测验证。
4. 本地开发使用**独立的测试密钥对**，正式私钥离线保管（对应 §11.6 检查项 9）。

> 生成命令：`pnpm tauri signer generate -w ~/.knowlpad-signing/knowl-pad.key`（✅ M0 实测可用，@tauri-apps/cli 2.11.5 生成成功），产出的公钥填入 `tauri.conf.json` 的 `updater.pubkey`（必须是**公钥内容**，不能是文件路径）。签名环境变量：`TAURI_SIGNING_PRIVATE_KEY`（或 `TAURI_SIGNING_PRIVATE_KEY_PATH`）与 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。

#### 11.7.8 CI 辅助脚本（补齐 CI 引用但缺内容的两个脚本）

§11.4 的 CI 配置引用了 `check-path-encapsulation.sh` 与 `check-naming.sh`，此前**只被引用未给出内容**。二者均为纯文本扫描脚本，无需 Node 依赖，可在三平台 CI runner（bash 环境）下运行。

**`scripts/check-naming.sh`**（门禁 13，对应 PRD §0.2）

```bash
#!/usr/bin/env bash
# 命名一致性检查：扫描 PRD §0.2 定义的禁止写法。
# 退出码非 0 即阻断 CI。
set -euo pipefail

# ── 依赖守卫（必须放在最前面）──────────────────────
# ⚠️ ripgrep 并非 runner 预装：GitHub Actions 的 ubuntu-latest 不含 rg，
#    Windows/macOS 开发机通常也不含。若缺失时不显式报错，下方的
#    `if hits=$(rg ...)` 会因退出码 127 被判定为「无命中」，
#    导致门禁静默通过、形同虚设。故此处硬性失败。
if ! command -v rg >/dev/null 2>&1; then
  echo "❌ 缺少依赖：ripgrep (rg)。请先安装（CI 见 §11.4 的安装步骤）："
  echo "   Ubuntu/Debian: sudo apt-get install -y ripgrep"
  echo "   macOS:         brew install ripgrep"
  echo "   Windows:       winget install BurntSushi.ripgrep.MSVC"
  exit 2
fi

# 扫描范围：源码与文档，排除产物目录、依赖、以及规范说明自身所在文件。
# ⚠️ 只纳入**实际存在**的目录：ripgrep 对不存在的路径会以退出码 2 失败，
#    而 `if hits=$(rg ...)` 会把任何非零退出码当成「无命中」，导致门禁静默通过。
SCAN_PATHS=()
for p in src src-tauri/src tests scripts docs; do
  [[ -d "$p" ]] && SCAN_PATHS+=("$p")
done
if [[ ${#SCAN_PATHS[@]} -eq 0 ]]; then
  echo "❌ 未找到任何待扫描目录，门禁不可判定"; exit 2
fi
EXCLUDES=(
  --glob '!**/target/**'
  --glob '!**/node_modules/**'
  --glob '!**/dist/**'
  --glob '!**/dist-*/**'
  # ⚠️ 以下三个文件必须排除：它们为定义/执行规范而必须列举禁止写法，
  #    否则脚本会扫描到自身内容并必然失败（自指悖论）
  #    用 `!**/<basename>` 而非 `!docs/<basename>`：rg 的 --glob 匹配语义
  #    依赖调用时的根路径，按 basename 排除更稳健。
  --glob '!**/Knowl-Pad-PRD.md'              # §0.2 命名规范定义处
  --glob '!**/Knowl-Pad-完整技术方案.md'      # §11.7.8 内嵌了本脚本源码
  --glob '!**/AGENTS.md'                     # 红线摘要同样列举了禁止写法（自指）
  --glob '!**/check-naming.sh'               # 本脚本自身
)

violations=0

# rg 封装：退出码 0=命中，1=无命中，≥2=执行错误。
# ⚠️ 执行错误**必须**显式失败，绝不能当成"无命中"（否则门禁静默通过、形同虚设）。
rg_scan() {
  local out status
  out=$(rg -n "${EXCLUDES[@]}" "$@" "${SCAN_PATHS[@]}" 2>&1) && status=0 || status=$?
  if [[ $status -ge 2 ]]; then
    echo "❌ ripgrep 执行失败（exit $status），门禁不可判定："
    echo "$out" | head -5 | sed 's/^/   /'
    exit 2
  fi
  printf '%s' "$out"
}

check() {
  local pattern="$1" reason="$2"
  local hits
  # -n 输出行号便于定位；--fixed-strings 避免正则元字符误判
  hits=$(rg_scan --fixed-strings "$pattern")
  if [[ -n "$hits" ]]; then
    echo "❌ 命名违规：发现 '$pattern'（$reason）"
    echo "$hits" | sed 's/^/   /'
    violations=$((violations + 1))
  fi
}

# ── 产品名禁止写法 ────────────────────────────────
check 'KnowlPad'   '驼峰无空格；仅允许出现在安装包产物文件名中'
check 'knowlPad'   '小驼峰'
check 'Knowl-pad'  '产品名语境下的错误大小写'

# ── 标识符禁止写法 ────────────────────────────────
# 注意：Rust 模块名 knowl_pad:: 是语言强制约定，属合法例外，故此处
# 仅检查「作为包名/crate 名字符串」出现的下划线写法
hits=$(rg_scan --regexp '"name"\s*[:=]\s*"knowl_pad"' --regexp '^name\s*=\s*"knowl_pad"')
if [[ -n "$hits" ]]; then
  echo "❌ 命名违规：包名/crate 名应为 'knowl-pad' 而非 'knowl_pad'"
  echo "$hits" | sed 's/^/   /'
  violations=$((violations + 1))
fi

# ── 数据目录禁止写法（应为 .knowlpad，无分隔符）──────
check '.knowl-pad'  '数据目录应为 .knowlpad（无分隔符）'
check '.knowl_pad'  '数据目录应为 .knowlpad（无分隔符）'

if [[ $violations -gt 0 ]]; then
  echo ""
  echo "命名一致性检查失败，共 $violations 类违规。规范见 PRD §0.2。"
  exit 1
fi

echo "✅ 命名一致性检查通过"
```

> **已知局限（如实登记）**：
> 1. **自指排除**：脚本必须排除三个文件——PRD（§0.2 定义规范）、技术方案（§11.7.8 内嵌本脚本源码）、以及脚本自身。否则它会扫描到自己列举的禁止写法而**必然失败**。这已在上方的 `EXCLUDES` 中处理，但代价是这三个文件的正文命名违规无法被自动捕获，需评审时人工把关。
> 2. **文档名豁免**：交付文档前缀 `Knowl-Pad-`（PRD §0.2 明确规定）与禁止写法 `Knowl-pad` 仅大小写不同。脚本扫描的是小写 `Knowl-pad`，故合法文档名不会误报；但这也意味着大小写错误的文档名（`Knowl-pad-PRD.md`）**会被正确捕获**。
> 3. **自然语言歧义**：正文散文中把产品名写成 `Knowl-Pad` 的情况，与合法的文件名前缀难以自动区分，故该模式**未纳入扫描**（PRD §0.2 已用文字规范约束）。
>
> 这三项局限使门禁 13 是**辅助手段而非完备保证**，不可将其视为命名合规的唯一防线。

**`scripts/check-path-encapsulation.sh`**（SEC PATH-02，补充 ESLint 规则覆盖不到的场景）

```bash
#!/usr/bin/env bash
# 前端路径封装检查：确保「路径校验只在 Rust 侧」这条红线未被破坏。
# ESLint 的 no-restricted-syntax 只能覆盖 AST 可识别的调用形式，
# 本脚本补充扫描字符串字面量中的危险模式。
set -euo pipefail

# ── 依赖守卫（必须放在最前面）──────────────────────
# ⚠️ ripgrep 并非 runner 预装。若缺失时不显式报错，下方
#    `if hits=$(rg ...)` 会因退出码 127 被判定为「无命中」，
#    导致门禁静默通过、形同虚设。故此处硬性失败。
if ! command -v rg >/dev/null 2>&1; then
  echo "❌ 缺少依赖：ripgrep (rg)。请先安装（CI 见 §11.4 的安装步骤）："
  echo "   Ubuntu/Debian: sudo apt-get install -y ripgrep"
  echo "   macOS:         brew install ripgrep"
  echo "   Windows:       winget install BurntSushi.ripgrep.MSVC"
  exit 2
fi

violations=0

# 只扫描前端源码，Rust 侧路径操作是合法的
SCAN_PATHS=(src)
EXCLUDES=(--glob '!**/*.d.ts' --glob '!src/core/ipc/**')

# rg 封装：退出码 0=命中，1=无命中，≥2=执行错误（必须显式失败，不得视为"通过"）
rg_scan() {
  local out status
  out=$(rg -n "${EXCLUDES[@]}" "$@" "${SCAN_PATHS[@]}" 2>&1) && status=0 || status=$?
  if [[ $status -ge 2 ]]; then
    echo "❌ ripgrep 执行失败（exit $status），门禁不可判定："
    echo "$out" | head -5 | sed 's/^/   /'
    exit 2
  fi
  printf '%s' "$out"
}

check() {
  local pattern="$1" reason="$2"
  local hits
  hits=$(rg_scan --regexp "$pattern")
  if [[ -n "$hits" ]]; then
    echo "❌ 路径封装违规：$reason"
    echo "$hits" | sed 's/^/   /'
    violations=$((violations + 1))
  fi
}

# 1. 绝对路径字面量：前端不应出现盘符或根路径开头的字符串
check "\"[A-Za-z]:[\\\\/]"            '前端出现 Windows 绝对路径字面量（应由 Rust 侧解析）'
check "'[A-Za-z]:[\\\\/]"             '前端出现 Windows 绝对路径字面量（应由 Rust 侧解析）'
check '"/(Users|home|root)/'          '前端出现 Unix 绝对路径字面量（应由 Rust 侧解析）'

# 2. 路径回溯片段：前端不得自行拼接 ../ 或 ..\
#    ⚠️ 必须排除模块导入语句——TS/ESM 的相对导入（from '../lib/ipc'）
#    是合法且普遍写法，若不排除会导致门禁全量误报。
#    实现方式：先用 rg 粗筛出含 ../ 的行，再用 grep -v 剔除导入语句。
#    不用 `rg -P`（PCRE2）：部分发行版打包的 ripgrep 未启用该特性，会直接报错。
traversal_hits=$(rg_scan --regexp '\.\.[\\/]' \
  | grep -Ev '^[^:]+:[0-9]+:\s*(import|export)\b' \
  | grep -Ev '\bfrom\s+['"'"'"]' \
  | grep -Ev '\brequire\s*\(' \
  || true)
if [[ -n "$traversal_hits" ]]; then
  echo "❌ 路径封装违规：前端出现路径回溯片段（路径穿越风险，SEC-02）"
  echo "   （模块导入语句已排除，以下均为可疑的运行时路径拼接）"
  echo "$traversal_hits" | sed 's/^/   /'
  violations=$((violations + 1))
fi

# 3. 直接触碰 Tauri fs 插件（本项目明确不引入 tauri-plugin-fs，PRD AC-01）
check '@tauri-apps/plugin-fs'         '禁止引入 tauri-plugin-fs，文件操作必须走自定义 Command'

# 4. 绕过 IPC 封装直接调用 invoke
hits=$(rg_scan --regexp 'invoke\s*[<(]')
if [[ -n "$hits" ]]; then
  echo "❌ IPC 封装违规：禁止在 src/core/ipc/ 之外直接调用 invoke()"
  echo "$hits" | sed 's/^/   /'
  violations=$((violations + 1))
fi

if [[ $violations -gt 0 ]]; then
  echo ""
  echo "路径封装检查失败，共 $violations 类违规。详见技术方案 §6.2 与 §7.3。"
  exit 1
fi

echo "✅ 路径封装检查通过"
```

> **与 ESLint 的分工**：`eslint.config.js`（§11.7.5）的 `no-restricted-imports` / `no-restricted-syntax` 在 **AST 层**拦截 `import` 与函数调用；本脚本在**文本层**补充拦截字符串字面量中的绝对路径与 `../` 片段——后者 ESLint 难以可靠识别。两层共同保证「前端永不构造文件路径」这条架构红线（PRD R 系列、SEC-02、SEC-12）。
>
> **误报处理**：若前端确需在 UI 上**展示**路径字符串（如显示当前 Vault 位置），该字符串应来自 Rust 侧 Command 的返回值而非字面量，故正常代码不会触发误报。如出现误报，用 `// kp-naming-allow` 注释豁免并在评审中说明，**不得**直接放宽脚本规则。

---

## 12. 对源文档（v5）的勘误与修正留痕

本节完整记录 v5 中**技术性错误**与本文档的修正。继承关系：v5 的依赖锁定矩阵与安全审计结论**全部有效并被继承**；以下条目为需要修正或补充的部分。

| # | v5 位置 | v5 的表述 | 问题 | 本文档的修正 |
| --- | --- | --- | --- | --- |
| **D-01** | §11.7 | 「增量更新（Delta）：使用 bsdiff 等二进制差异算法生成 patch」 | Tauri updater 插件**不支持**二进制差分更新，下载的是完整安装包 | V1.0 限定全量更新（PRD FR-UPDATE-14、本文档 §9.4.1） |
| **D-02** | §11.7 | 「更新失败时自动回滚到上一版本，保留最近 1-2 个历史版本快照」 | 应用二进制替换由 OS 安装器完成，失败时旧版本未被移除；不存在"回滚"动作，也无历史快照机制 | 修正为「更新失败时保持当前版本不变」（PRD FR-UPDATE-08） |
| **D-03** | §11.7 | 「更新元数据文件使用 RSA 2048 数字签名」 | Tauri updater 使用 **minisign/ed25519**，非 RSA | 修正为 ed25519/minisign（§9.4.1） |
| **D-04** | §11.7 | 「传输层强制 TLS 1.3 加密」 | TLS 版本由 `reqwest`/系统栈决定，应用层无法强制仅 1.3 | 修正为「强制 HTTPS（TLS 1.2+）」（PRD SEC-04） |
| **D-05** | §11.7 | `update.json` 文件名与其格式示例 | Tauri 2 updater 约定文件名为 `latest.json`；且示例中缺少 `.sig` 与 updater 所需的 `version`/`notes`/`pub_date`/`platforms` 之外的字段说明 | 统一采用 `latest.json`，由 `createUpdaterArtifacts: true` 自动生成（§11.5） |
| **D-06** | §11.5 CI YAML | 使用 `github.repository_owner`、`github.event.repository.name`、`github.ref_name`；`actions-rs/toolchain@v1`；把 `pnpm ci` 当依赖安装命令；单 `ubuntu-latest` 却期望产出 `.msi`/`.dmg`；无系统依赖安装；无测试门禁 | GitHub Actions 变量在 Gitee Go 中不存在；`actions-rs` 已停更；pnpm 无内置 `ci` 子命令、不能当安装命令用；Linux 无法产出 Windows/macOS 安装包；缺 WebKitGTK 依赖则构建必失败 | 完整重写 CI 配置（§11.4），matrix 四平台、`dtolnay/rust-toolchain`、`pnpm install --frozen-lockfile`、补齐系统依赖与 17 项门禁 |
| **D-07** | §11.3 | Conventional Commits 表格中 `` `feat! `` 行结构损坏（三列错位），且整段说明与表格**重复出现两次** | 文档质量问题，直接复制会导致规范不清 | 修正为完整的 10 类 commit type 表格（§11.3） |
| **D-08** | §5 `package.json` | 缺少 `@tailwindcss/vite`（Tailwind 4 的 Vite 集成方式）、测试工具链、Lint 工具链、CI 脚本；`shadcn-vue` 误置于 `dependencies` | 无法支撑 PRD §8.3/§8.4 的测试与门禁要求；shadcn-vue 是 CLI 工具而非运行时依赖 | 补齐并修正（§3.6） |
| **D-09** | §6 `Cargo.toml` | 仅锁 `tauri`/`tauri-build`/`rusqlite`/`serde`/`serde_json` 五个 crate | **不足以实现任何功能**：缺文件监听、目录遍历、中文分词、哈希、异步运行时、错误处理、日志、YAML 解析、正则等全部必需依赖 | 补充 19 个新增 crate 的选型与理由；版本号已于 **2026-09-19** 经 crates.io 官方 API 核实并锁定（§3.5.2、§3.5.4）。**始终未编造任何版本号** |
| **D-09b** | §6 `tauri-build` | 锁定 `2.1.0` | 版本过期，与 `tauri` 不配套 | 修正为 `2.7.0`（随 tauri 2.12.0 配套，crates.io 核实） |
| **D-09c** | 隐含 MSRV | 未明确项目 MSRV，前序草稿写 `1.82` | **会导致构建失败**：MSRV 由依赖的实际 `rust-version` 决定。2026-09-19 为 `1.88`（`time`/`image`），2026-09-30 随 tauri 2.12 家族升至 **`1.90`** | 修正 `rust-version` 与 `rust-toolchain.toml` 为 `1.90`；权衡登记为 `DEBT-08` |
| **D-09d** | §9.4.2 `rust-toolchain.toml` | 代码块使用 `//` 作为注释符 | **TOML 不支持 `//`**，仅支持 `#`，该文件原样落地会解析失败 | 改用 `#` |
| **D-09e** | 前端依赖 | 虚拟滚动、日期库未选型；`cytoscape-fcose` 版本「待核实」 | 无法实现 PRD 的万级列表性能指标与图谱布局 | 选型 `@tanstack/vue-virtual 3.13.39`、`dayjs 1.11.23`；核实 `cytoscape-fcose 2.2.0`（peerDep `cytoscape ^3.2.0`，与 `3.34.3` 兼容） |
| **D-10** | §5 注意 | 「Vite 8 要求 `@vitejs/plugin-vue` 升级至 6.0.0+」 | 表述正确，但 v5 的 `package.json` 中写的是 `"@vitejs/plugin-vue": "^6.0.0"` 而 dependencies 里未列 `@tailwindcss/vite` | 已在 §3.6 补齐 |
| **D-11** | §9 建议 | 「如果对构建稳定性有极高要求，建议保持 Vite 6.4.3」与「Vite 强制锁定 >= 8.x，自动覆盖 6.4.3 中的安全修复」 | 两处建议**互相矛盾**：前者建议回退 6.4.3，后者称 8.x 覆盖了 6.4.3 的安全修复（暗示 6.4.3 仍有漏洞） | **2026-09-21 已决策：采用 Vite 8、不回退**（§3.4.3、`OPEN-01` 关闭）。三项生产回归的规避手段升级为硬性门禁。矛盾消解——不再保留 6.4.3 这条可能不安全的退路 |
| **D-12** | §10.1 | XSS 防护示例代码仅 `DOMPurify.sanitize(md.render(...))` | 未说明**禁止 `setConfig()`**——而这正是 v5 §10.4 自己记录的 3.4.11 前漏洞成因。防护方案与安全审计结论未打通 | 显式列为 P0 需求（PRD FR-EDITOR-42 / SEC-01）+ 代码实现约束（§9.3）+ 启动自检 |
| **D-13** | §4.2 | pinia 4.0.3「仅支持 ESM，Store 编写方式不变」 | 正确，但未提示 `"type": "module"` 是前置条件 | 已在 §3.2.2 标注项目本身即 ESM |
| **D-14** | 全文 | 未涉及中文全文搜索的分词方案 | SQLite FTS5 的 `unicode61` **不对中文分词**，直接实现会导致中文搜索几乎不可用——对一款以中文知识库为主要场景的产品而言是致命缺陷 | 设计 jieba-rs 预分词方案（PRD §3.4、本文档 §5.2） |
| **D-15** | 全文 | 未定义任何功能需求、数据模型、IPC 契约、验收标准 | v5 实质是「依赖清单 + 安全审计 + 发布流程」，不含软件设计 | 由 PRD v2.1 完整补齐；本文档补齐架构与实现设计 |
| **D-16** | PRD §5.1 `IPC-02` | 原文写「字段名统一 `snake_case`（serde 默认）」 | 与技术方案 §8.4 的 camelCase 决策冲突；前端 TS 惯例为 camelCase，snake_case 传输键会导致前端到处写 `rel_path` 等不符合 JS 惯例的字段名 | **PRD 已于 v2.1 修正**：IPC 传输的 JSON 键统一 camelCase（Rust 侧通过 `#[serde(rename_all = "camelCase")]` 映射），Rust 结构体内部字段名保持 snake_case。§8.1 `CMD-03` 中的冲突声明已同步移除 |

---

## 12.1 开发可用性修订记录（v6.2，2026-09-25）

本版在 v6.1 基础上做了一次**面向"能否直接开工"**的系统修订。与 §12 的「v5 勘误」不同，本节记录的是**三份文档之间**以及**文档与可执行配置之间**的不一致与错误。完整逐条清单见 `docs/history/评审修订说明.md`。

| 类别 | 问题数 | 典型修正 |
| --- | --- | --- |
| S1 阻塞（不修则 CI 必红/代码必错） | 8 | IPC 封装路径统一为 `src/core/ipc/`；tsconfig 去掉与 `rootDir` 互斥的测试 include；CI 补 `cargo-audit`/`cargo-llvm-cov`；门禁 8 弃用 pnpm 不支持的 `--dry-run`；发布链路改用 `pnpm exec`、补 `@semantic-release/exec`、assets 纳入 `Cargo.lock`；命名门禁排除 `AGENTS.md`；`vue/no-v-html` 与类型化 lint 的范围修正 |
| S2 契约与数量不一致 | 10 | Command 总数 58→**59**、写操作 21→**24**；事件名 `kp://file/changed`→`kp://fs/modified`；Capabilities 与 SEC-05/AC-SEC-02 对齐（`process:allow-restart`、去掉 `http://`/`file://`、删除 notification 插件）；覆盖率阈值口径；夹具与 setup 路径；CodeMirror 依赖引入时机；`.npmrc` 严格模式表述；备份保留与回滚的矛盾 |
| S2 算法与实现正确性 | 15 | FTS5 改用普通表以支持 DELETE；`NOT` 改二元链式；embed 可裁决到附件；**重命名与链接改写合并为一次可回滚操作**；回收站 manifest 与 `.knowlpad/` 可删除性；重建期间旧索引可用；`AppError::context()` 与错误码映射；watcher 忽略规则与遍历对齐；远程图片加载机制；`file_reveal` 归属 Rust 侧；拼音映射表；文件树分页；TS/TSX 高亮降级；原子写入权限位；词典指纹 |
| S3 安全/供应链/度量/治理 | 13 | `semantic-release-gitee` 风险标注；registry 改回官方；`file_tag` 主键冲突；TOCTOU 登记 `TR-10`；函数索引回写 PRD；SJ-04 判定口径；`pnpm ci` 聚合门禁；性能门禁口径；架构图版本号同步；"已核实版本"降级为 M0 复核 |

**M0 核实后的事项状态**（2026-09-25；完整结果见附录 D）：

| # | 事项 | 状态 |
| --- | --- | --- |
| 1 | Gitee Go 变量语法与流水线模型 | ⛔ **已确认不兼容**（应为 `GITEE_*`/`{GITEE_xxx}`），YAML 必须整体重写（`DEBT-06`） |
| 2 | `deb.depends` 包名 | ✅ Ubuntu 22.04 三个包名已核实存在；⚠️ Debian 12 / Fedora 待核 |
| 3 | Vite 8 `minify` 与 `advancedChunks` 键名 | ✅ 已实测：`minify` 取 `oxc` 合法（默认值）；`advancedChunks` 废弃 → `codeSplitting` |
| 4 | updater `pubkey` 生成 | ✅ 命令与签名环境变量已实测；⚠️ 正式公钥仍需 `tauri signer generate` 产出后填入 |
| 5 | 依赖版本与 `0.x` 适配关系 | ✅ 已用真实 lockfile 与编译验证回写（含 `r2d2_sqlite 0.35.0` + `rusqlite 0.40.2`）；⚠️ 工具链大版本落后需决策（附录 D.3） |
| 6 | `semantic-release-gitee` 维护状态 | ⛔ **已确认于 2022-02 停更逾 4 年**，违反 R-16，必须替换 |

---

## 12.2 M0 收尾修订记录（v6.3，2026-09-29）

在 v6.2 基础上，把 M0 阶段遗留的契约缺口与文档分叉收口：

| # | 类别 | 修订 |
| --- | --- | --- |
| 1 | IPC 契约缺口 | 前端 `commands.ts` 声明的 `vault_close` 在 Rust 侧缺失；补实现并注册（`src-tauri/src/commands/vault.rs`、`main.rs`），并补 `system_info` 的 TS 封装 |
| 2 | 契约校验缺陷 | `scripts/verify-ipc-contract.mjs` 的正则要求泛型，漏检 `callVoid('...')`；已修正并新增「Rust 已实现但 TS 未封装」告警 |
| 3 | 门禁扩充 | `scripts/run-gates.mjs` 新增**门禁 17：IPC 契约一致性（TS↔Rust）**；PRD §8.4 与 AGENTS.md 同步为 17 项（13 基准 + 4 扩展） |
| 4 | 契约生成决策 | 评估 `tauri-specta`：稳定版 `1.0.2` 仅面向 Tauri 1.x，支持 Tauri 2 的 `2.0.0-rc.25` 仍为预发布；正式决定「手写 `commands.ts` + CI 契约校验」，关闭 `TR-01`（§8.3） |
| 5 | Gitee 遗留 | §11.4 标注为历史设计（实际已迁移 `.github/workflows/`）；`DEBT-06` 关闭；附录 A 同步 |
| 6 | M0 核实回写 | §3.3 `vue-tsc`/TS 6、§3.5 `Cargo.lock` 逐项复核、§3.5.4 测试依赖、`DEBT-08` MSRV 1.90 均回写为「M0 已验证」 |
| 7 | 架构图 | 页脚移除过时的「待 M0 核实」四项，改为 M0 核实完成结论与后续 M9/人工项 |

> 完整逐条记录见 `docs/history/M0收尾修订说明.md`。门禁现状：**17 项**（见附录 D 与仓库根 `gate-report.json`）。

## 13. 实施路线与技术债

### 13.1 里程碑与本文档章节的对应

里程碑规划见 PRD §9.1。本节补充**每个里程碑的技术交付物与验证方式**：

| 里程碑 | 本文档相关章节 | 关键技术验证 |
| --- | --- | --- |
| **M0 工程初始化** | §2 工程结构、§3.3 TS 配置、§3.4 Vite 8 验证、§3.5.4 Cargo.toml、§11.4 CI、§11.7 配置文件 | ① Vite 8 三项回归的规避验证（§3.4.3 的 4 项）② **Rust 1.88 toolchain 下全依赖 `cargo check` 通过**（版本号已于 2026-09-19 核实锁定，此步验证 features 与 major bump 后的 API 签名）③ 三平台空窗口启动 ④ CI 门禁跑通（当时 16 项，门禁 17 契约校验随后补入）⑤ `DEBT-08` 决策：确认 MSRV 1.88 在目标构建环境可用 |
| **M1 存储与 Vault** | §4 数据架构、§6.2 路径安全、§6.3 原子写入、§9.1 Capabilities | ① AC-REL-01 强杀一致性 1000 次 ② AC-SEC-01 路径穿越全拦截 ③ 全局库迁移测试 |
| **M2 文件树与基础编辑** | §7.3 虚拟滚动、§7.4 编辑器适配层 | ① AC-FILE-06 十万节点 50 FPS ② AC-EDITOR-02/05/06 |
| **M3 解析与索引引擎** | §5.1 解析器、§5.2 分词、§5.3 索引、§5.4 监听 | ① 附录 B 兼容性 ≥ 99% ② AC-REL-03 索引重建一致 ③ 监听死循环防护测试 ④ inotify 降级测试 |
| **M4 链接与反链** | §6.1 批量改写器 | ① AC-FILE-01/02 ② AC-LINK-01~05 ③ 回滚失败路径的 UI 提示验证 |
| **M5 搜索与标签** | §5.2 分词查询侧、§7.2 渲染管线 | ① AC-SEARCH-01~06 ② AC-TAG-01~04 |
| **M6 图谱与附件** | §7.5 前端性能预算 | ① AC-GRAPH-01~04 ② AC-ATTACH-03 零网络请求 |
| **M7 安全加固与回收站** | §9 全部 | ① AC-SEC-01~05 全通过 ② DOMPurify 启动自检 ③ AC-TRASH-01~04 |
| **M8 编辑器正式版** | §7.4 ED-01~06、§3.2.4 代码高亮方案 | ① **先行 spike**：`legacy-modes` 的 StreamParser 能否经包装供 `highlightCode()` 使用（决定语言覆盖是约 14 种还是约 100 种，`DEBT-09`/`TR-09`）② AC-EDITOR-01/03/04/07 ③ NFR-PERF-07 ④ **高亮切换验证**：编辑态与阅读态代码块配色一致（`HL-05`）；`md-editor-v3` 及其内置 highlight.js 样式已完全移除、无死代码（`HL-06`）；阅读态高亮输出仍经 DOMPurify 净化（`HL-01`，用 AC-EDITOR-01 的 XSS 用例复验）⑤ `HL-07` 行数阈值实测标定 |
| **M9 更新与发布** | §9.4、§11.2~11.6 | ① AC-UPDATE-01~05 ② 干净虚拟机全链路 ③ §11.6 发布清单 |
| **M10 V1.0** | 全部 | PRD SJ-01~06 |

### 13.2 关键技术债登记

| 债项 | 描述 | 影响 | 偿还计划 |
| --- | --- | --- | --- |
| `DEBT-01` | MVP 阶段用 `md-editor-v3`，仅支持工具栏 + 分屏预览，**缺少 Live Preview（所见即所得）能力** | 早期版本编辑体验平庸：源码与渲染结果分栏显示，不如单栏即时渲染流畅 | M8 替换为 CodeMirror 6 并实现 Live Preview（适配层已隔离，§7.4） |
| `DEBT-02` | `style-src` 含 `unsafe-inline`（§9.1.2） | CSS 注入的理论风险（UI 欺骗） | 评估 Tailwind 4 与 Vue 是否能改为 nonce 方案；V1.1 |
| ~~`DEBT-03`~~ ✅ **已关闭**（2026-09-20） | jieba 词典全量内置，安装包增大约 5MB | ~~分发体积~~ 不再构成约束 | **决策**：`OPEN-02` 定为全量内置，且当前阶段不以打包体积为约束（`[profile.release]` 的 `opt-level` 已由 `"s"` 改为 `3`，见 §3.5.1）。故本项不再作为债务。**保留编号不重排**，避免其他章节引用失配 |
| `DEBT-04` | 索引库 `link_candidates` 用 TEMP TABLE 做全局裁决，10 万文件规模下可能慢 | 大库首次索引时间 | M3 实测；若超 NFR-PERF-03 的 8min 预算，改为分批 + 函数索引 `lower(stem)` |
| `DEBT-05` | macOS E2E 受 `tauri-driver` 限制，可能降级为人工验证 | 回归风险 | M0 确认限制范围；评估 `WebDriverAgent` 或其他方案 |
| `DEBT-06` | ~~Gitee Go 的 action 生态弱于 GitHub Actions，CI 配置可能需多轮调试~~ ✅ **已关闭（2026-09-25）**：CI 已迁移至 `.github/workflows/`（`ci.yml` + `release.yml` + `platform-smoke.yml`，`actionlint` 通过），发布由 `scripts/publish-gitee-release.mjs` 同步 Gitee Release | M0 工期 | 见附录 D.3 第 1/2 项 |
| `DEBT-07` | ✅ **已关闭（2026-09-19）**：`file_alias` 表原在 PRD §3.2.1 的 DDL 中未定义，但 §5.3.3 的链接裁决依赖它 | Schema 不完整 | 已补入 PRD §3.2.1：`file_alias(file_id, alias)` + `idx_alias_lower`，并同步更新 PRD §3.2.2 的查询模式表。本文档 §5.3.3 的裁决 SQL 现已有对应 schema 支撑 |
| `DEBT-08` | **MSRV 两次抬升**：2026-09-19 由 `time 0.3.55` / `image 0.25.10` 抬到 `1.88`；2026-09-30 因 tauri 2.12 家族（`tauri-utils 2.10.0`、`muda 0.20.0` 等）再次抬到 **`1.90`** | ① CI 与所有开发者 toolchain 必须 ≥ 1.90；② 部分企业内网离线镜像可能未同步；③ 依赖升级会继续抬高 MSRV，需靠 MSRV 感知解析守住 | ✅ **已处置**：`.cargo/config.toml` 启用 `resolver.incompatible-rust-versions = "fallback"` + 工作区 `resolver = "3"`，解析不再越界；每次抬高 MSRV 需同步更新 `rust-toolchain.toml`、3 个 workflow 与本文档矩阵 |
| `DEBT-09` | **阅读态代码高亮的语言覆盖不确定**（2026-09-21 评估发现）。Lezer 原生 grammar（`@lezer/*`）仅约 **14 种**语言；`@codemirror/legacy-modes` 虽含约 **100 种**，但它导出的是 **StreamParser**（旧式流式接口），能否经 `StreamLanguage.define()` 包装后供 `highlightCode()` 使用**未经验证** | 若 legacy-modes 链路不可行，阅读态仅 14 种语言可高亮，SQL/Bash/Ruby/TOML/PowerShell 等常见语言（`@lezer/sql`、`@lezer/bash` 已确认**不存在**，npm 404）只能纯文本降级——对技术笔记用户是明显体验缺口 | **M8 启动时先做最小 spike**：取 `legacy-modes/mode/shell` 走通一次 `highlightCode()`。① 可行 → 按需注册约 100 种语言，本债项关闭；② 不可行 → 在「接受 14 种上限」与「阅读态改用 highlight.js/shiki（需正式变更 `ED-06`）」之间做产品决策。详见 §3.2.4「语言覆盖」 |
| `DEBT-10` | PRD §8.3 要求 Rust `domain/` 覆盖率 ≥ 85%，而门禁 6 原先只跑 `cargo test`，**无覆盖率度量与阈值强制** | 覆盖率承诺不可验证，domain 层质量无法随迭代守护 | **本版已落地**：`rust-toolchain.toml` 与 CI 增加 `llvm-tools-preview`，CI 安装 `cargo-llvm-cov` 并执行 `cargo llvm-cov --fail-under-lines 85`；本地由 `pnpm gate:rust` 覆盖 |
| ~~`DEBT-11`~~ ✅ **已关闭（2026-09-30）** | ~~发布说明与版本号自动化暂缺~~：已落地**零依赖自研方案**——`scripts/changelog.mjs`（解析 Conventional Commits → 按类型分组渲染）+ `scripts/prepare-release.mjs`（推断递增类型 → 调 `bump-version.mjs` 同步四处版本 → 写入 `CHANGELOG.md`；默认 dry-run，`--write` 才落盘，工作区不干净时拒绝执行） | ~~CHANGELOG 需人工维护、版本号需手工同步~~ | 已由 `pnpm release:prepare` 覆盖；回归测试见 `tests/unit/changelog.spec.mjs`（18 项）。人工 review 仍作为最后一道确认（发布是显式动作） |

> `DEBT-07` 是本文档编写过程中发现的 **PRD 缺项**，已在此显式登记。它必须在 M3 开始前补入 PRD，否则别名匹配（MD-WL-02、AC-EDITOR-04）无法实现。

### 13.3 已知妥协清单

| 妥协 | 内容 | 理由 |
| --- | --- | --- |
| `synchronous = NORMAL` | 系统断电时可能丢失最后一批未 checkpoint 的**索引**事务 | 索引可重建，性能收益远大于风险。**笔记内容**的安全性由 §6.3 的 fsync 独立保证，不依赖此设置 |
| `style-src 'unsafe-inline'` | CSS 注入的理论风险 | Vue/Tailwind 运行时必需；`script-src` 无 inline 已阻断主要 XSS 路径（DEBT-02） |
| `linkify: false` | 纯 URL 文本不自动转为链接 | 减少 DoS 面（SEC-03）；用户可显式写 `[[...]]` 或 `[](url)` |
| `typographer: false` | 不自动转换引号为排版引号 | 减少 DoS 面；且自动改写用户输入的字符违背"所见即所存" |
| 非 HMM 分词 | 无法识别词典外的新词 | 确定性优先——索引必须可重建且逐次一致（AC-REL-03） |
| 文件名统一取三平台最严格并集 | Linux 用户无法创建 `a:b.md` | 保证 Vault 跨平台可迁移（NFR-PLAT-04），这是本地优先产品的核心价值 |
| 无增量更新 | PATCH 版本也需下载完整安装包 | Tauri updater 不支持差分（D-01）；自研成本高，收益有限 |
| 无崩溃自动上报 | 用户遇到崩溃需手动反馈日志 | 零遥测承诺（SEC-16）优先于问题排查便利 |

### 13.4 AGENTS.md 的定位

为避免历史文档中出现的「技术栈信息在三处维护导致分叉」问题，明确三层文档的职责边界：

| 文档 | 职责 | 禁止内容 |
| --- | --- | --- |
| **PRD** | 需求真相源：功能、数据模型、接口契约、验收标准、非功能指标 | 实现细节（具体代码、库的内部用法） |
| **本文档（技术方案）** | 实现真相源：架构、算法、配置、CI/CD、勘误 | 需求定义（若与 PRD 冲突以 PRD 为准） |
| **AGENTS.md** | AI 编程工具的**索引与红线**：指向 PRD 与技术方案的具体章节；列出红线（R-01~R-17）、关键命令、禁止事项 | **禁止**复制粘贴技术栈版本号、模块清单、接口清单——只做指针引用，避免第二处维护点 |

---

## 附录 A：配置清单汇总

| 文件 | 章节 | 状态 |
| --- | --- | --- |
| `package.json` | §3.6 | ✅ 可直接使用（测试/lint 工具版本以 `^` 约束，M0 锁定） |
| `tsconfig.json` | §3.3.2 | ✅ 可直接使用 |
| `vite.config.ts` | §3.4.4 | ✅ **M0 已校准**：`minify: 'oxc'` 合法（且是默认值）；`advancedChunks` 已废弃 → 改用 `codeSplitting`；实测构建通过 |
| `Cargo.toml` | §3.5.4 | ✅ **M0 实测可编译**：`cargo check` 在 rustc 1.98.1 与 **1.90.0** 下均通过（含 `rusqlite` bundled、`jieba-rs`、`image`、`r2d2_sqlite`） |
| `rust-toolchain.toml` | §9.4.2 | ✅ `1.90.0`（由 tauri 2.12 家族的实际 MSRV 决定，见 `DEBT-08`） |
| `.nvmrc` | §11.7.1 | ✅ `24.19.0` |
| `.gitignore` | §11.7.2 | ✅ 可直接使用 |
| `.npmrc` | §11.7.3 | ✅ 可直接使用（锁定 pnpm 严格模式与 registry） |
| `.prettierrc.json` | §11.7.4 | ✅ 可直接使用 |
| `eslint.config.js` | §11.7.5 | ✅ 可直接使用（flat config，ESLint 9） |
| `vitest.config.ts` | §11.7.6 | ✅ 可直接使用 |
| `scripts/check-naming.sh` | §11.7.8 | ✅ **M0 实测通过**（正例/反例/缺失扫描路径三种情况）。已修复「扫描路径缺失时静默通过」缺陷；**需预装 ripgrep**（guard 会以退出码 2 硬失败） |
| `scripts/check-path-encapsulation.sh` | §11.7.8 | ✅ **M0 实测通过**（正确放行 `src/core/ipc/**`，正确拦截绝对路径/`../`/裸 `invoke`） |
| `tauri.conf.json`（security 节） | §9.1.2 | ✅ 可直接使用 |
| `tauri.conf.json`（bundle 节） | §10.4 | ⚠️ Ubuntu 22.04 三个包名已核实存在；**Debian 12 / Fedora 仍需核实** |
| `tauri.conf.json`（updater 节） | §11.5 | ⚠️ 生成命令已实测可用（`pnpm tauri signer generate`）；`pubkey` 仍需正式密钥产出后填入 |
| `capabilities/default.json` | §9.1.1 | ✅ 可直接使用 |
| ~~`.releaserc.json`~~ | §11.5 | ⛔ **已删除**：插件依赖未安装（不可执行）+ 含 R-16 违规包；发布改由 `.github/workflows/release.yml` 与 `scripts/publish-gitee-release.mjs` 承担 |
| `.gitee/workflows/knowlpad.yml` | §11.4 | ⛔ **已废弃（2026-09-25）**：M0 确认 Gitee Go 变量/模型不兼容；实际 CI 为 `.github/workflows/`（经 `actionlint` 校验），本文件不再存在（`DEBT-06` 已关闭） |

**图例**：✅ 可直接使用 ｜ ⚠️ 需核实局部项 ｜ ⛔ 含占位，核实前不可用

## 附录 B：性能基准实现

PRD §6.1.2 定义 16 项指标（`NFR-PERF-01`~`16`），基准数据集见 PRD §6.1.1（标准库 3000 篇 / 大库 20000 篇 / 压力库 100000 篇）。本节给出**全部 16 项**的测量方法——每项指标都必须可自动化测量，否则无法纳入 CI 门禁。

| 指标 | 测量方式 | 工具 / 埋点位置 |
| --- | --- | --- |
| `NFR-PERF-01` 冷启动 | 进程 spawn → 首帧可交互的时间戳差 | `tauri-driver` + 前端埋点 |
| `NFR-PERF-02` 打开 Vault | `vault_open` 调用 → 文件树首次渲染完成 | 前端埋点（`onMounted` 后 `requestAnimationFrame`） |
| `NFR-PERF-03` 全量索引 | `kp://index/completed` 事件的 `duration_ms` | Rust 侧计时（`Instant`） |
| `NFR-PERF-04` 搜索响应 | 输入停顿（debounce 结束）→ 结果 DOM 呈现 | 前端 `performance.mark` |
| `NFR-PERF-05` 快速打开 | 按下 `Ctrl+O` → 候选列表首屏渲染完成 | 前端 `performance.mark`（fuse.js 匹配耗时单独记录） |
| `NFR-PERF-06` 打开笔记 | 点击文件树节点 → CodeMirror 可接受输入 | 前端埋点（编辑器 `focus` 事件） |
| `NFR-PERF-07` 输入延迟 | 按键 `keydown` → 对应字符渲染的 `requestAnimationFrame` 回调 | 前端埋点（连续 100 次取 **P95**） |
| `NFR-PERF-08` 反向链接面板 | 切换笔记 → 反链面板列表渲染完成（500 条夹具） | 前端埋点；SQL 侧另记 `idx_link_dst` 查询耗时 |
| `NFR-PERF-09` 单文件保存 | `note_write` 调用 → fsync 返回（Rust 侧） | Rust `Instant` 包裹 `write + sync_all` |
| `NFR-PERF-10` 增量索引 | 单文件变更入队 → 索引写入完成 | Rust 侧队列打点（取 P95） |
| `NFR-PERF-11` 外部变更反映 | 外部写入/新增文件（测试脚本）→ 前端收到 `kp://fs/modified` / `kp://fs/created` 并刷新 | 端到端：脚本写文件 + 前端埋点 |
| `NFR-PERF-12` 图谱渲染 | 打开全局图谱 → cytoscape `layoutstop` 事件 | 前端埋点；压力库下改为验证**降级提示已触发**（FR-GRAPH-09） |
| `NFR-PERF-13` 内存 | 常驻内存（打开 5 篇笔记后的稳态值） | Windows `GetProcessMemoryInfo` / macOS `task_info` / Linux `/proc/self/status`，Rust 侧采样 |
| `NFR-PERF-14` 空闲 CPU | 无操作 30s 后的进程 CPU 时间增量 ÷ 墙钟时间 | 同上（取两次采样的 `utime+stime` 差值） |
| `NFR-PERF-15` 滚动帧率 | 文件树 10 万节点匀速滚动时的帧计数 ÷ 时间窗口 | 前端 `requestAnimationFrame` 计数 |
| `NFR-PERF-16` 重命名+改写 | `file_rename` 调用 → 改写事务提交完成（300 处引用夹具） | Rust `Instant`；另记预览阶段与执行阶段各自耗时 |

**测量纪律**：

1. **统计口径**：延迟类指标（04/05/06/07/08/09/10）取 **P95** 而非平均值——平均值会掩盖卡顿长尾，而用户感知的是最差体验。每项**连续采样 ≥ 30 次**（07 项 ≥ 100 次）。
2. **预热**：所有测量前需完成一次预热运行并丢弃其结果，避免 JIT/缓存冷启动污染数据。
3. **基准数据集生成器**：三档夹具由 `scripts/gen-fixture-vault.mjs` 确定性生成（固定随机种子），保证每次运行的库结构完全一致，否则指标不可比。
4. **回归判定**：结果写入 `.perf-result.json`，与仓库中的 `.perf-baseline.json` 比对；**任一指标回退 > 20% 则 CI 失败**（PERF-08、门禁 10）。基线文件在每次有意优化后由维护者更新并提交。
5. **环境隔离**：CI 中运行性能测试的机器负载不可控，故 CI **只做回退比对**，绝对值达标判定（PRD §6.1.1 的基准环境）在发布前于专用机器上人工执行（§11.6 检查项 5）。

## 附录 C：风险登记册（技术视角）

PRD §10.1 已列 10 项产品/技术风险。此处补充**纯实现层面**的风险：

| ID | 风险 | 缓解 |
| --- | --- | --- |
| `TR-01` | ~~`tauri-specta` 与 Tauri 2.11 / TS 6 不兼容，契约自动生成方案落空~~ ✅ **已关闭（2026-09-25）**：稳定版 `1.0.2` 依赖 `tauri ^1.2.4`（Tauri 1.x）；支持 Tauri 2 的 `2.0.0-rc.25` 为预发布，违反禁用预发布纪律。M0 决策采用「手写 `commands.ts` + CI 契约校验（门禁 17）」，待 2.x 正式版再评估 | — |
| `TR-02` | ~~`serde_yaml` 已归档，替代方案的维护状态未知~~ ✅ **已解决（2026-09-19）**：核实确认 `serde_yaml` 与社区接续版 `serde_yml` **均已废弃**，`yaml-rust2 0.13.0` 为当前唯一活跃维护的纯 Rust YAML 解析器 | 选型已锁定 `yaml-rust2`（§3.5.2）。若后续需要 serde `Deserialize` 派生能力，改评估 `noyalib`；自研 frontmatter 子集解析器（约 300 行）保留为最终备选 |
| `TR-03` | SQLite FTS5 在 `rusqlite` 的 `bundled` feature 下是否默认启用需确认 | M1 验证 `PRAGMA compile_options` 含 `ENABLE_FTS5`；若未启用需在 `bundled` 基础上加编译特性 |
| `TR-04` | Windows 上 `mmap_size` PRAGMA 的实际效果与稳定性未知 | M1 在 Windows 上实测；异常则 Windows 平台单独设 `mmap_size = 0` |
| `TR-05` | `walkdir` 遍历 10 万文件时若遇到深层嵌套或符号链接循环可能卡死 | 设置 `max_depth`（默认 64）、`follow_links(false)`、`skip_cycles` |
| `TR-06` | CodeMirror 6 的 Markdown 语法定义与本项目扩展语法（wikilink/tag/block-id）的集成复杂度被低估 | M8 前做技术预研 spike；若不可行则 Live Preview 降级为「分屏预览」发布（对应 PRD OPEN-03） |
| `TR-09` | **阅读态高亮的语言覆盖不足**：`@lezer/*` 仅约 14 种语言，`@lezer/sql`、`@lezer/bash` 确认不存在；`legacy-modes` 的约 100 种语言能否接入 `highlightCode()` **未验证**（StreamParser 与 LRParser 接口不同） | M8 最小 spike 验证；不可行则按 `DEBT-09` 的两个选项做产品决策。**同时**：`highlightCode()` 为同步函数，超大代码块需 `HL-07` 的行数守卫，阈值经实测标定 |
| `TR-07` | `notify` 的 `RecommendedWatcher` 在 Linux 上默认用 inotify，但某些文件系统（如 NFS、部分 FUSE）不支持 | 检测失败后降级轮询模式（§5.4） |
| `TR-08` | 索引期间用户同时编辑文件，导致解析结果与磁盘内容不一致 | 索引写入前用 `mtime` 二次校验，不一致则重新入队（§5.3.4 的增量机制天然覆盖） |
| `TR-10` | 路径校验的 TOCTOU 竞态：`canonicalize` 通过后、实际读写前符号链接可能被替换 | 以已打开句柄 / canonical 父目录 + 末段操作；POSIX 用 `O_NOFOLLOW`、Windows 用 reparse-point 语义；残留风险登记在册（§6.2） |

---

## 附录 D：M0 核实结果（2026-09-25）

本节记录 M0 阶段**在真实环境中执行**的核实项与结论。执行环境：Windows + Node v24.19.0 + pnpm 12.4.2 + rustc 1.98.1（x86_64-pc-windows-msvc，VS 18 Community / MSVC 14.51 / Windows SDK 10.0.26100）+ ripgrep 15.2.0。

### D.1 结论总览

| 核实项 | 方法 | 结论 |
| --- | --- | --- |
| 前端依赖可安装 | pnpm install（官方 registry） | ✅ 全部解析成功；vite→8.3.1、typescript→6.0.3、vue-tsc→3.3.11、@types/node→24.13.6 |
| Vite 8 配置键 | 生成最小 Vue 工程并 vite build | ✅ minify 取值 oxc 合法（且是默认值）；build.rolldownOptions.output.advancedChunks **已废弃并告警** → 应改 codeSplitting（结构相同，实测均正确产出 vendor chunk） |
| TS 6 + vue-tsc 类型检查 | vue-tsc --noEmit（§3.3.2 的 tsconfig） | ✅ 通过 |
| const enum 回归（RISK-01 #1） | 构造带别名成员的字符串 const enum 并构建 | ✅ **未复现**：成员被正确内联（var c=s("ok")），无反转映射。仍保留 ESLint 禁用规则作为廉价护栏 |
| writeBundle 产物遗漏（RISK-01 #2） | 连续 3 次生产构建 + 产物清单比对 + 引用完整性校验 | ✅ 三次产物清单完全一致；index.html 引用的 chunk 全部存在 |
| Rust 依赖可编译 | cargo check（rustc 1.98.1） | ✅ 33 个 crate 全部通过（含 rusqlite bundled、jieba-rs、image、r2d2_sqlite） |
| MSRV 1.90（DEBT-08） | rustup toolchain install 1.90.0 + cargo +1.90.0 check | ✅ **通过**，1.90 足以编译当前依赖集 |
| Tauri 及插件可编译 | cargo check（tauri 2.12.0 + updater/dialog/shell/opener/process） | ✅ 通过 |
| 依赖安全审计（门禁 7） | cargo install cargo-audit + cargo audit --json | ✅ **0 vulnerabilities / 0 warnings**（182 个依赖） |
| Lockfile 门禁（门禁 8） | pnpm install --frozen-lockfile / --dry-run / --lockfile-only | ✅ 三者均可用；**--dry-run 受 pnpm 12.4.2 支持**，故恢复为最初写法 |
| updater 密钥生成 | pnpm tauri signer generate | ✅ 成功生成密钥对；签名环境变量为 TAURI_SIGNING_PRIVATE_KEY(_PATH) + _PASSWORD |
| Linux 包名 | packages.ubuntu.com（jammy） | ✅ libwebkit2gtk-4.1-0、libgtk-3-0、libayatana-appindicator3-1 均存在；Debian/Fedora 待核 |
| 门禁脚本 | 构造正例/反例/缺目录三种树运行 | ✅ 修正后可正确拦截违规、正确豁免 AGENTS.md 与 src/core/ipc/**；**发现并修复**缺目录时静默通过的缺陷 |
| 完整脚手架与门禁 | 搭建 Vue3+TS6+Vite8+Pinia+Router 前端、Tauri 2 薄壳、独立 `kp-domain` crate，跑本地门禁（当时 16 项，门禁 17 契约校验随后补入） | ✅ **16/16 全部通过**；`kp-domain` 行覆盖率 **94.70%**（阈值 85%）；IPC 契约一致（详见 D.4） |
| 脚手架迁移与 CI 落地 | 把 `m0-verify/` 迁移到仓库根，并用 `actionlint` 校验工作流 | ✅ 迁移后根目录 **16/16 再次全通过**；`actionlint` 通过（并据此修掉失效的 `macos-13` 标签） |
| Gitee 发布联调 | 对公开仓库实测读路径，并给脚本加 `--verify` | ✅ 端点可达（`releases` / `tags/{tag}` / `attach_files` 均 200）；`--verify` 实测退出码 0（已存在）与 2（尚无） |
| 三平台窗口冒烟 | 新增 `KP_SMOKE=1` 模式 + GitHub Actions 三平台矩阵 | ✅ Windows 本机实测通过；Linux（Xvfb）/ macOS 由 `platform-smoke.yml` 在 runner 上执行，工作流已过 `actionlint` |

### D.2 M0 发现并已修正的问题

| # | 问题 | 证据 | 处置 |
| --- | --- | --- | --- |
| M0-1 | Vite 8.3 中 advancedChunks 已废弃 | 构建输出 WARN advancedChunks option is deprecated, please use codeSplitting instead | 全文改用 codeSplitting（§3.4.4、§7.5） |
| M0-2 | crate 名写成 r2d2-sqlite，Cargo 解析失败；且版本 0.25.0 依赖的是 rusqlite ^0.32 | cargo 报 no matching package named r2d2-sqlite found；crates.io 依赖元数据 | 改为 r2d2_sqlite = "0.35.0"（依赖 rusqlite ^0.40），并实测编译通过 |
| M0-3 | sha2 0.11 的 finalize() 不再实现 LowerHex | 编译错误 E0277 | 在 major bump 复核条目中补充手动 hex 编码说明 |
| M0-4 | 门禁脚本在任一扫描目录缺失时静默通过 | 移除 tests/ 后 rg 退出码为 2，脚本仍输出通过并 exit 0 | 两个脚本改为「只扫描存在的目录 + rg 退出码 ≥2 立即失败」 |
| M0-5 | Gitee Go 变量语法与流水线模型不兼容 | 官方帮助中心：系统变量为 GITEE_*，引用 {GITEE_xxx} | CI 章节标注阻断，DEBT-06 保持打开 |
| M0-6 | semantic-release-gitee 自 2022-02 起停更 | npm registry 的 time 字段（created 2022-02-10 / modified 2022-05-17） | 升级为红线 R-16 阻断项，必须替换 |
| M0-7 | 版本号小幅漂移 | crates.io / npm registry | thiserror 2.0.20→2.0.21、tokio-util 0.7.15→0.7.19、tempfile 3.14.0→3.27.0、pnpm 12.4.1→12.4.2 |
| M0-8 | jieba-rs 0.11.0 没有 hmm cargo feature | crates.io features 列表（default/default-dict/textrank/tfidf） | HMM 只能通过 cut(text, false) 关闭；修正 PRD 表述 |
| M0-9 | §9.2 的内联路径封装检查片段用 &&/|| 串联，grep 报错时也输出「通过」 | 在缺目录的树上实测输出通过 | 该片段仅作示意；真正的门禁以 §11.7.8 的脚本为准（CI 已如此调用） |
| M0-10 | **pnpm 12 不再读 package.json 的 `pnpm` 字段，且默认忽略依赖构建脚本** | `pnpm install` 报 `ERR_PNPM_IGNORED_BUILDS`；提示 "The pnpm field in package.json is no longer read" | 新增 `pnpm-workspace.yaml`，用 `allowBuilds: { esbuild: true }` + `strictDepBuilds: false`；已写入 §11.7.3/§11.7.3b |
| M0-11 | **Windows Smart App Control 阻断 cargo 产物执行**（首次执行时） | `cargo check/test` 报 `os error 4551 应用程序控制策略已阻止此文件`；`VerifiedAndReputablePolicyState=1`；同一二进制复制到别处仍被阻断（按文件信誉判定） | **重试后自行解除**：同机再次执行时 clippy/test/build/覆盖率全部正常，判定为 SAC 的瞬时按文件信誉阻断，非代码缺陷。CI 侧不受影响 |
| M0-12 | **性能基准用单次 P50，在繁忙机器上抖动达 35%** | 同机连续 3 次 P50 = 6.29 / 8.23 / 8.49 ms，产生假回退 | 改为 **min(批次中位数)**（7 批 × 20 次渲染取最小中位数）并重设基线：机器噪声只会抬高测量值，最小值最接近真实开销。已更新 §11.4/附录 B 的判定口径 |
| M0-13 | **发布矩阵里的 `macos-13` runner 标签已不可用** | `actionlint` 报 `label "macos-13" is unknown`（可用 Intel 标签为 `macos-15-intel`、`macos-26-intel` 等） | 改为 `macos-15-intel`；并把 `actionlint` 纳入 CI 步骤，防止再次引入失效标签 |
| M0-14 | **Gitee 对不存在的 tag 返回 `HTTP 200 + body "null"` 而非 404** | 实测 `GET /releases/tags/<不存在的 tag>` → 200、body 长度 4（`null`） | 发布脚本按「尚未创建」处理（`--verify` 退出码 2）；已写入联调手册 |
| M0-15 | **三平台窗口启动此前只能在 Windows 人工验证** | CI 里跑 GUI 需要窗口服务器：Linux 无 X11、Windows/macOS runner 会话可用性不确定 | 给应用加 `KP_SMOKE=1` 冒烟模式（窗口出现即断言并退出），新增 `platform-smoke.yml` 三平台矩阵（Linux 用 `xvfb-run` + `WEBKIT_DISABLE_COMPOSITING_MODE`）。本机实测：`KP_SMOKE_OK window=main visible=true`，exit 0 |

### D.3 M0 未完成 / 待决策项（不得带占位进入 M1）

| # | 事项 | 状态 |
| --- | --- | --- |
| 1 | Gitee Go 流水线重写（或迁移 GitHub Actions） | ✅ **已完成迁移**：CI 落在 `.github/workflows/`，经 `actionlint` 校验通过；`.gitee/README.md` 说明缘由与回退路径（`DEBT-06` 关闭） |
| 2 | semantic-release-gitee 替代方案落地 | ✅ **已完成**：`scripts/publish-gitee-release.mjs`（Gitee OpenAPI v5），支持 `--verify` / `--dry-run` / 幂等复用；读路径端点已实测 |
| 3 | Debian 12 / Fedora 的包名 | ✅ **已核实**：Debian 12 bookworm 三个运行库均存在；Fedora 构建期包名见 §10.4 |
| 4 | macOS / Linux 上的空窗口启动（三平台验收） | ✅ **已可自动化**：新增 `.github/workflows/platform-smoke.yml`，三平台矩阵构建后以 `KP_SMOKE=1` 启动并断言输出 `KP_SMOKE_OK`（Linux 走 Xvfb，macOS/Windows 用 runner 自带会话）。Windows 本机已实测通过。⚠️ §6.4.1 中依赖人工观察的条目（高 DPI、Gatekeeper 提示、系统文件管理器行为、安装/卸载）仍需人工执行 |
| 5 | 开发工具链大版本落后 | ✅ **决策已出**：M0 保持已测通的声明范围，升级收拢为 M1 的独立分组任务，逐项清单与执行约定见 `docs/history/依赖升级决策.md` |
| 6 | 完整脚手架与门禁端到端跑通 | ✅ **已完成并迁移到仓库根**：16/16 门禁在根目录再次全通过，IPC 契约一致 |

### D.4 完整脚手架与门禁落地结果（仓库根目录）

脚手架结构（已可运行）：

```
knowl-pad/（仓库根）
├── src/                      Vue3 + TS6 前端（app / core / features / shared 分层）
│   ├── core/ipc/             invoke 类型安全封装（FE-04、ESLint 例外点）
│   ├── core/markdown/        DOMPurify 净化管线 + 启动自检
│   ├── core/utils/           toHex / clamp / formatBytes（覆盖 ≥90%）
│   └── features/vault/       Pinia store + 组件
├── crates/kp-domain/         Rust 领域层（无 Tauri 依赖，可独立单测与覆盖率）
│   ├── src/path_guard.rs     七步路径校验（SEC-02）
│   ├── src/note_io.rs        原子写入 / 冲突检测 / 临时文件清理（NFR-REL-01）
│   └── tests/reliability.rs  并发读取一致性（AC-REL-01 本地等价物）
├── src-tauri/                Tauri 2 薄壳：commands / state / error_wrapper / capabilities
├── scripts/                  门禁与发布脚本（run-gates / verify-* / publish-gitee-release / bump-version）
├── tests/                    unit / security / perf 测试集
├── .github/workflows/        ci.yml（13 门禁）+ release.yml（构建+GitHub Release+Gitee 同步）
└── pnpm-workspace.yaml       pnpm 12 设置（allowBuilds 白名单）
```

本地门禁执行结果（`node scripts/run-gates.mjs`）：

| 门禁 | 结果 | 说明 |
| --- | --- | --- |
| 1 类型检查 | ✅ | `vue-tsc --noEmit` + 测试工程 |
| 2 前端 Lint | ✅ | `eslint . --max-warnings 0`（0 error / 0 warning） |
| 3 前端构建 + 完整性 | ✅ | Vite 8.3 `codeSplitting` + chunk 引用校验 |
| 4 Rust clippy | ✅ | `cargo clippy --workspace --all-targets -- -D warnings`，0 warning |
| 5 Rust fmt | ✅ | `cargo fmt --all --check` |
| 6 Rust 测试 | ✅ | 30 个用例通过（kp-domain 单测 25 + 可靠性 2 + bin 3） |
| 7 Rust 覆盖率 | ✅ | `kp-domain` 行覆盖 **94.70%** / 函数 98.04%（阈值 85%） |
| 8 vitest 覆盖率 | ✅ | 全局 ≥80%，`core/utils` ≥90% |
| 9 依赖审计 | ✅ | `pnpm audit --audit-level=high` + `cargo audit`（0 漏洞） |
| 10 Lockfile | ✅ | `pnpm install --frozen-lockfile --dry-run` |
| 11 Rust 应用构建 | ✅ | `cargo build` 成功（tauri 2.12.0 + shell/opener/dialog/process） |
| 12 性能基准 | ✅ | 渲染 P50 相对基线无回退 |
| 13 安全测试集 | ✅ | 10 条 XSS 探针 + 启动自检 + Capabilities/CSP 审计 |
| 14 可靠性测试集 | ✅ | 并发读取一致性 + 临时文件清理（2 用例），通过 |
| 15 命名一致性 | ✅ | 通过（含 AGENTS.md 豁免） |
| 16 路径封装检查 | ✅ | 通过 |

**结论（重试后）**：**16/16 全部通过**，报告见仓库根 `gate-report.json`。首次执行时 5 项 Rust 门禁被 Smart App Control 瞬时阻断（M0-11），重试后自行解除；期间一并修正了 clippy 死代码、crate 路径解析、Tauri 宏的 never-type-fallback 以及性能基准噪声（M0-12）等问题。
---

**文档结束**

> 本技术方案与《Knowl-Pad-PRD》v2.3 配套使用。PRD 定义「做什么」，本文档定义「怎么做」。
>
> **依赖版本核实状态（2026-09-19 核实 / 2026-09-20 更新管理策略）**：Rust 侧 19 个 crate + 7 个 Tauri 插件 + 5 个继承项、前端侧 4 个新增库，**已全部经 crates.io / npm 官方 API 核实，`TBD` 占位已清零**。`Cargo.toml`（§3.5.4）现可用于构建。
>
> **依赖管理策略（§3.6.1）**：声明层面用向后兼容范围（`package.json` 的 `^`/`~`、`Cargo.toml` 的 caret），安装层面由 `pnpm-lock.yaml` 与 `Cargo.lock` 锁定精确版本；两个 lockfile 均提交入库，CI 与生产一律用 `pnpm install --frozen-lockfile` 安装，升级依赖为显式动作且 lockfile 变更单独成 commit。
>
> **同期决策（2026-09-20）**：① `OPEN-02` 已决——jieba 词典全量内置，`DEBT-03` 关闭；② 暂不考虑打包体积，`[profile.release]` 的 `opt-level` 由 `"s"` 改为 `3`、`lto` 改为 `"fat"`（§3.5.4）；③ MVP 阶段代码高亮**无需单独配置**，`md-editor-v3` 已内置 highlight.js 集成（§3.2.4）。
>
> **同期决策（2026-09-21）**：① **Vite 确定采用 8.x、取消回退路径**（§3.4.3、PRD `OPEN-01` 关闭）——三项生产回归的规避手段升级为硬性门禁，其中「禁用 `const enum`」已落地为 ESLint 规则 `TSEnumDeclaration[const=true]`（§11.7.5，选择器写法已核实）；② **跟进上游更新**：vue 3.5.43、vue-router 5.3.1、dompurify 3.4.15、@tauri-apps/cli 2.11.5、@vue/devtools-api 8.2.1、@vitejs/plugin-vue 6.0.9、vue-tsc 3.3.11、tauri 2.11.6、tauri-plugin-updater 2.12.0（逐项见 §3.2、§3.5、§3.6.1）；③ **TypeScript 刻意停留 6.0.3**，不升 7.0.2（§3.2.1）；④ **`md-editor-v3` 刻意停留 `^6.5.6`**，不升 7.0.0（M8 即被替换，§3.2.4）；⑤ **M8 起代码高亮统一用 CodeMirror 6 / Lezer 原生方案**，不引入任何第三方高亮库——编辑态用 `syntaxHighlighting()`、阅读态用 `@lezer/highlight` 的 `highlightCode()`（已核实该 API 真实存在），详见 §3.2.4「代码高亮方案」与约束 `ED-06`、`HL-01`~`HL-06`。
>
> **CodeMirror 6 子包版本已锁定**（2026-09-21，§3.2.4）：`@codemirror/state` 6.7.5、`view` 6.43.12、`language` 6.12.4、`commands` 6.11.1、`search` 6.7.2、`autocomplete` 6.20.3、`lang-markdown` 6.5.2、`lang-javascript` 6.2.5；`@lezer/highlight` 1.2.3、`markdown` 1.7.2、`javascript` 1.5.5。**注意易错点**：`HighlightStyle` / `syntaxHighlighting` 属于 `@codemirror/language`，不在 `@lezer/highlight` 中。其余 `@lezer/<语言>` parser（python/css/html/json/xml 等）按实际支持的语言清单在 M8 前逐个核实后引入。
>
> **M0 核实结果（2026-09-25，完整记录见附录 D）**：④ updater 密钥生成命令与签名变量已实测通过；③ Vite 8 键名已实测校准（`minify` 的 `oxc` 取值合法、`advancedChunks` 废弃 → `codeSplitting`）；② Ubuntu 22.04 的三个 `deb.depends` 包名均已核实存在（Debian/Fedora 待核）；① **Gitee Go 变量语法已确认不兼容**（应为 `GITEE_*`/`{GITEE_xxx}`），流水线 YAML 必须重写（`DEBT-06`）。另新增两项阻断结论：**`semantic-release-gitee` 已停更逾 4 年，R-16 违规，必须替换**；**门禁脚本存在「扫描路径缺失即静默通过」缺陷，已修复**。
>
> **开发可用性修订（2026-09-25，v6.2）**：本版针对跨文档一致性、门禁可执行性与算法正确性做了系统修订，共 60+ 处，逐条留痕见 `docs/history/评审修订说明.md` 与本节 §12.1。要点：① 统一前端 IPC 封装路径为 `src/core/ipc/`（并修正 ESLint/脚本的排除项与 v-html 例外）；② `tsconfig` 去掉与 `rootDir` 互斥的测试 include 并新增 `tsconfig.test.json`；③ CI 补齐 `cargo-audit`/`cargo-llvm-cov`、修正门禁 8 命令、`pnpm dlx` 改为 `pnpm exec`、发布 assets 纳入 `Cargo.lock`；④ 修正 FTS5 contentless 表、`NOT` 运算符语法、附件 embed 裁决、重命名与链接改写的原子性、回收站与 `.knowlpad/` 可删除性的矛盾、重建期间旧索引可用性；⑤ 统一 Command 计数为 59、写操作 24，并同步架构图版本号。
>
> **M0 收尾修订（2026-09-29，v6.3）**：① 补实现并注册 Rust 命令 `vault_close`（修复 IPC 契约缺口），修正 `verify-ipc-contract.mjs` 对无泛型 `callVoid('...')` 的漏检；② 新增 **门禁 17：IPC 契约一致性（TS↔Rust）**，门禁总数 16→17（PRD §8.4 与 AGENTS.md 同步）；③ 评估 `tauri-specta` 后决定「手写 `commands.ts` + CI 契约校验」，关闭 `TR-01`（§8.3）；④ 关闭 `DEBT-06`（Gitee Go 已迁移 `.github/workflows/`）并回写 §11.4；⑤ 回写 §3.3/§3.5/§3.5.4 的 M0 核实结论与 `DEBT-08`（保持 MSRV 1.88）。完整记录见 §12.2 与 `docs/history/M0收尾修订说明.md`。
>
> 本文档相对 v5 的 16 项修正（D-01~D-16）已在 §12 完整留痕。v5 的依赖锁定矩阵与安全审计结论全部有效并被继承。
