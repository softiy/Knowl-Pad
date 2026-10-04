# AGENTS.md — Knowl Pad

> 本文件是 **AI 编程工具的指令入口**，不是需求文档、也不是实现文档。
>
> **职责边界（技术方案 §13.4 强制约定）**：本文件只做**指针引用**与**红线声明**。
> 禁止在此复制粘贴技术栈版本号、模块清单、IPC 接口清单、DDL——那些内容一旦在此重复，
> 就会形成第二处维护点并与真相源分叉。**所有细节一律指向 PRD 与技术方案的具体章节。**

---

## 1. 三层文档职责（先读这个，再动手）

| 文档 | 角色 | 何时查 |
| --- | --- | --- |
| **`docs/Knowl-Pad-PRD.md`** | **需求真相源** | 要改「做什么」：功能行为、数据模型、接口契约、验收标准、性能与安全指标 |
| **`docs/Knowl-Pad-完整技术方案.md`** | **实现真相源** | 要改「怎么做」：架构、算法、配置文件、CI/CD、勘误留痕 |
| **本文件** | 索引与红线 | 每次开始编码前通读一遍，确认自己没有触碰红线 |

**冲突裁决规则**：若技术方案与 PRD 冲突，**以 PRD 为准**，并同步修正技术方案。
若本文件与上述任一文档冲突，**以那两份文档为准**——本文件不得自行定义需求或实现。

---

## 2. 三条架构底线（不可协商）

完整表述见 PRD §1.2 的 P-1~P-5 与 §2.1~§2.5。以下三条是**最常被无意破坏**的，特别提示：

1. **Markdown 文件是唯一真相源**（PRD P-1 / §2.3）
   索引库是纯派生数据。**禁止**在索引库存放任何无法从 `.md` 重建的信息。
   自检方式：删掉 `.knowlpad/` 后重建，结果必须与删除前逐表一致（PRD SJ-03、AC-06）。

2. **永不静默破坏用户数据**（PRD P-2）
   任何写操作必须原子化；任何批量改写必须先备份、可预览、可回滚、失败即中止并保留原状。
   实现细节见技术方案 §6.1（链接改写）、§6.3（原子写入）。

3. **不执行任何第三方代码**（PRD P-4 / X-01）
   本项目**没有插件系统**，这是核心决策，不是遗漏。渲染用户 Markdown 的 HTML 输出必须经 DOMPurify 净化。
   **禁止**提出、设计或实现任何形式的插件机制、用户脚本、远程代码加载。

---

## 3. 红线清单（违反即视为缺陷）

**权威定义在 PRD §8.2 的 R-01~R-17。** 此处仅列编号与一句话摘要，便于快速自查；
措辞以 PRD 为准，**不要**依据本文件的摘要做实现决策。

| # | 一句话摘要 | 权威出处 |
| --- | --- | --- |
| R-01 | 前端不得直接读写文件（必须走自定义 Command） | PRD §8.2 / AC-01 |
| R-02 | 前端不得引入 WASM SQLite 或直连数据库 | PRD §8.2 / AC-02 |
| R-03 | 索引库不得存储无法从 Markdown 重建的数据 | PRD §8.2 / AC-06 |
| R-04 | 不得用 `v-html` 渲染未净化的 HTML | PRD §8.2 / SEC-01 |
| R-05 | 不得使用 `DOMPurify.setConfig()` | PRD §8.2 / SEC-01 |
| R-06 | 不得非原子写入（禁止直接 `fs::write` 覆盖用户文件） | PRD §8.2 / NFR-REL-01 |
| R-07 | 不得静默覆盖用户数据（重名、外部冲突、批量改写） | PRD §8.2 / P-2 |
| R-08 | IPC/主线程同步操作 > 16ms 须异步化；任何 > 50ms 的阻塞即缺陷 | PRD §8.2 / AC-04 |
| R-09 | 不得硬编码平台路径或分隔符 | PRD §8.2 / NFR-PLAT-02 |
| R-10 | 不得授予 Tauri 宽泛的文件 / shell 权限 | PRD §8.2 / SEC-05 |
| R-11 | 不得加入任何遥测、统计、崩溃自动上报 | PRD §8.2 / SEC-16 |
| R-12 | 不得在日志中记录笔记正文或搜索词 | PRD §8.2 / SEC-09 |
| R-13 | 不得自动加载远程资源 | PRD §8.2 / SEC-08 |
| R-14 | 不得引入插件系统或任何第三方代码执行机制 | PRD §8.2 / X-01 |
| R-15 | 不得 `catch {}` 空吞错误 | PRD §8.2 / ERR-01 |
| R-16 | 不得使用已停更的依赖 | PRD §8.2 / SEC-06 |
| R-17 | CI / 生产不得用会自行解析新版本的安装命令（`pnpm install`、`npm install`），必须 `pnpm install --frozen-lockfile`；lockfile 变更须单独成 commit | PRD §8.2 / SEC-06 |

> **红线有自动化门禁兜底**：R-01 / R-09 由 `scripts/check-path-encapsulation.sh` 扫描（技术方案 §11.7.8）；
> 命名规范由 `scripts/check-naming.sh` 扫描（PRD §0.2、门禁 13）；
> R-17 由门禁 8（`pnpm install --frozen-lockfile --dry-run` 校验 lockfile 与 manifest 一致，M0 实测支持）兜底；
> R-04 / R-05 部分由 ESLint 规则覆盖（技术方案 §11.7.5）。
> **但门禁不完备**——不得因为「CI 没报错」就认为红线未被触碰。

---

## 4. 模块边界纪律（最容易随迭代退化）

| 层 | 约束编号 | 权威出处 |
| --- | --- | --- |
| 前端 feature 划分与依赖方向 | `FE-01`~`FE-05` | PRD §2.5.1 |
| Rust 分层（`commands` / `domain` / `storage` / `platform`） | `RS-01`~`RS-06` | PRD §2.5.2 |
| 目录结构（前端 / 后端 / 脚本） | — | 技术方案 §2.1、§2.2、§2.3 |

**两条最常被破坏的边界**：

- `RS-01`：`commands/` 是**薄壳**——只做参数反序列化、调 `domain/`、映射错误。
  **禁止**在 Command 里写 SQL、文件 IO、业务分支。单函数建议 ≤ 30 行。
- `FE-04`：`features/` **禁止**直接调用 `@tauri-apps/api` 的 `invoke`，必须走 `core/ipc/` 的类型安全封装。

依赖方向严格单向：`commands → domain → storage`。禁止反向依赖与循环依赖（`RS-05`）。

---

## 5. 编码规范（摘要）

**权威定义在 PRD §8.1 的 CODE-01~CODE-11。** 以下仅列 AI 编码时最易忽略的四条：

- **CODE-01**：TypeScript 开启 `strict`；**禁止 `any`**，确需时用 `unknown` + 类型收窄。
- **CODE-04**：Vue 组件用 `<script setup lang="ts">`；**禁止**选项式 API 与 mixin。
- **CODE-07**：**禁止**提交 `console.log` / `dbg!` / `println!` 到生产代码，调试日志走统一 logger。
- **CODE-10**：注释解释「为什么」而非「做什么」。复杂算法（链接改写、分词、索引一致性）**必须**有设计说明注释。

命名一律遵循 **PRD §0.2**（产品名 `Knowl Pad`、包名 `knowl-pad`、数据目录 `.knowlpad`、
Bundle ID `com.knowlpad.desktop`、Rust 模块 `knowl_pad::*`）。该节是唯一权威，且有 CI 门禁 13 兜底。

---

## 6. 关键命令

```bash
# ── 安装（必须用 pnpm；版本由 package.json 的 packageManager 字段锁定）──
# 依赖管理策略：声明层面兼容（package.json 用 ^/~ 范围），安装层面锁定
# （pnpm-lock.yaml 锁精确版本）。CI 与生产一律用 --frozen-lockfile，禁止用
# 会自行解析新版本的裸 `pnpm install` / `npm install`。升级依赖是显式动作，
# lockfile 变更单独成 commit。详见技术方案 §3.6.1（红线 R-17）。
pnpm install --frozen-lockfile

# ── 开发 ──────────────────────────────────────────
pnpm tauri dev              # 启动桌面应用（含热重载）
pnpm dev                    # 仅前端（浏览器调试，IPC 不可用）

# ── 质量检查（提交前自查；编号与 PRD §8.4 一致；flags 只在 package.json 维护）──
pnpm ci                     # ⚠️ 一次性跑完全部门禁（= node scripts/run-gates.mjs）
                            #    与 npm 的 `npm ci`（安装命令）无关，勿混淆；CI 中的安装命令
                            #    一律是 `pnpm install --frozen-lockfile`（红线 R-17）。
pnpm typecheck              # 门禁 1：TypeScript 类型检查
pnpm lint                   # 门禁 2：前端 Lint
pnpm gate:clippy            # 门禁 3：Rust Lint
pnpm gate:fmt               # 门禁 4：Rust 格式
pnpm test:coverage          # 门禁 5：前端单元/组件测试 + 覆盖率
pnpm gate:rust-test         # 门禁 6：Rust 单元/集成测试
cargo audit --file Cargo.lock   # 门禁 7（工具直调；需先 cargo install cargo-audit --locked）
pnpm gate:lockfile          # 门禁 8：Lockfile 一致性
pnpm tauri build            # 门禁 9：生产构建
pnpm test:perf              # 门禁 10：性能基准
pnpm test:security          # 门禁 11：安全测试集（AC-SEC）
pnpm test:reliability       # 门禁 12：可靠性测试集（AC-REL）
pnpm gate:naming            # 门禁 13：命名一致性
pnpm build:verify           # 门禁 14：前端构建 + 完整性
pnpm gate:coverage          # 门禁 15：Rust domain 覆盖率 ≥ 85%
pnpm gate:path-encapsulation # 门禁 16：路径封装检查
pnpm contract:verify        # 门禁 17：IPC 契约一致性（TS ↔ Rust）
pnpm gate:rust              # 仅 Rust 侧聚合（门禁 3/4/6/7/15），改 Rust 代码时用

# ── 构建 ──────────────────────────────────────────
pnpm build                  # 前端产物
pnpm tauri build            # 门禁 9：完整安装包
pnpm build:verify           # 构建 + Vite 8 chunk 完整性校验

# ── 发布准备（DEBT-11 已关闭）─────────────────────
pnpm changelog:gen          # 依据 Conventional Commits 打印变更日志（dry-run）
pnpm release:prepare        # 推断版本 → 同步四处 + 写 CHANGELOG（dry-run；--write 落盘）

# ── PR 流程（分支保护已启用，禁止直推 main）───────
pnpm pr                     # 推送分支 → 开 PR → 等必需检查 → 合并 → 同步 main（可续跑）
pnpm pr -- status           # 查看当前 PR 与必需检查状态
pnpm pr -- open             # 只推送分支并开 PR（不等待、不合并）
pnpm pr -- merge            # 等待检查全绿后合并

# ── 测试夹具与基准 ─────────────────────────────────
pnpm fixture:gen -- --tier standard --out /tmp/std-vault   # 生成基准数据集
pnpm test:perf -- --baseline .perf-baseline.json           # 性能基准比对
```

**完整的 `scripts` 定义见技术方案 §3.6；CI 门禁 17 项（PRD §8.4 的 13 项基准 + 4 项扩展）的完整清单见 PRD §8.4。**

---

## 7. 工具链硬约束

> 按 §13.4 约定，本节**不复制版本号清单**（权威矩阵在技术方案 §3.2 / §3.5 与 PRD §2.2）。
> 以下两条是会**直接导致构建失败**的环境门禁，必须知晓：

1. **Rust MSRV = 1.90**
   由 `tauri 2.12.0` 家族（含 `tauri-utils 2.10.0`、`muda 0.20.0`）的 `rust-version` 决定；
   `time` / `image` 的 1.88 已不再是约束（2026-09-30 更新）。
   低于 1.90 的 toolchain **必然构建失败**。固定方式见**仓库根**的 `rust-toolchain.toml`（技术方案 §9.4.2）——必须放根目录，否则工作区根/`crates/` 下的 `cargo` 不会应用该固定。
   相关权衡登记为 `DEBT-08`（技术方案 §13.2）。
   ⚠️ 依赖解析由 `.cargo/config.toml` 的 MSRV 感知解析（+ 工作区 `resolver = "3"`）保证不会选到高于 1.90 的依赖；
   门禁与 CI 的 cargo 命令一律带 `--locked`（Rust 版 R-17）。**不要删除 `.cargo/config.toml`**，详见技术方案 §3.6.1。

2. **Node = 24.19.0，包管理器 = pnpm**
   `.nvmrc` 与 `package.json` 的 `engines` / `packageManager` 三处必须一致（技术方案 §11.2）。
   **禁止**用 npm / yarn 安装依赖——会生成错误的 lockfile 并破坏门禁 8。

3. **门禁脚本依赖 `ripgrep`**
   `check-naming.sh` 与 `check-path-encapsulation.sh` 需要 `rg`。两个脚本都内置了依赖守卫，
   缺失时以**退出码 2 硬性失败**（不会静默通过）。本地安装方式见技术方案 §11.7.8。

4. **环境地雷：宿主时钟回拨会让 node 子进程 abort（本机已取证，2026-10-04）**
   症状：`Assertion failed: new_time >= loop->time, file src\win\core.c, line 327`，退出码 **`0xC0000409`**
   （PowerShell 里显示 `-1073740791` 或 `3221226505`）；常见于 `vitest`、以及门禁批量派生 node 的场景，
   表现为「一批命令在 0.2 秒内集体失败」。
   根因：本机是虚拟化桌面（`HypervisorPresent: True`），宿主/内核对 guest 校时**含向后跳变**
   （实例：`2026-10-04 10:02:59` 内核把系统时间**回拨 171 秒**，事件 `Kernel-General` Id=1；
   `W32Time`/`vmictimesync`/`autotimesvc` 均为 Stopped，校时来自 PID 4 与某个 svchost 组件）。
   libuv 的单调时钟断言因此失败——**与项目代码无关**；GitHub runner 不受影响，**CI 始终是权威**。
   判定命令（失败时刻附近若有"时间增量更改"即为本因）：
   `Get-WinEvent -FilterHashtable @{LogName='System'; ProviderName='Microsoft-Windows-Kernel-General'; Id=1} -MaxEvents 10 | ForEach-Object { '{0:MM-dd HH:mm:ss}  {1}' -f $_.TimeCreated, ($_.Message -replace "`r?`n",' ') }`
   处置：① 本地优先**直连** `node scripts/xxx.mjs`，不经 `cmd.exe`——`scripts/run-gates.mjs` 与
   `scripts/lib/spawn-tool.mjs` 已内建「优先原生 .exe + pipe 捕获回显 + 崩溃码重试一次」；
   ② `pnpm test` / `pnpm test:coverage` 走 `scripts/run-with-retry.mjs`，**只对该崩溃码**重试（会打印重试次数）；
   ③ 看到非零退出码，先判断**是不是崩溃码**再下结论；④ **禁止**为了让本地"变绿"而放宽门禁或覆盖率阈值。

---

## 8. 动手前必查的章节索引

| 你要做的事 | 先读 |
| --- | --- |
| 新增 / 修改一个 IPC Command | PRD §5.3（Command 清单）、§5.2（错误码）、技术方案 §8（IPC 实现与契约生成） |
| 写 Markdown 解析逻辑 | PRD §3.1（解析规则 MD-WL / MD-TAG / MD-FM）、附录 B（测试集）、技术方案 §5 |
| 改数据库 schema | PRD §3.2 / §3.3（DDL）、§3.6（迁移策略）、技术方案 §4（连接与事务） |
| 实现文件写入 / 重命名 / 删除 | 技术方案 §6.1（链接改写两阶段）、§6.3（原子写入协议）、PRD §6.2（可靠性需求） |
| 做搜索 | PRD §3.4（中文分词方案）、§4.5、技术方案 §5（索引引擎） |
| 加权限 / 碰 Capabilities | PRD §6.3（威胁模型与 SEC 需求）、技术方案 §9.1 |
| 处理跨平台路径 / 回收站 | 技术方案 §6.2（路径安全）、§10（跨平台）、PRD §6.4 |
| 改 CI / 发布流程 | 技术方案 §11.4（CI）、§11.5（发布链路与版本自动化）、§11.7（配置文件） |
| 看当前已知技术债与妥协 | 技术方案 §13.2（DEBT 登记）、§13.3（已知妥协）、§12（对 v5 的勘误） |
| 确认某功能是否在本版范围 | PRD §1.4（In / Out of Scope）、§9.3（V1.1 候选，**不承诺**） |

---

## 9. 开发纪律

1. **先查真相源，再动手**。本文件不含需求与实现细节，不要依据本文件的摘要做设计决策。
2. **不臆造版本号**。新增依赖必须联网核实当前稳定版（crates.io / npm 官方 API），
   并同步更新技术方案 §3.2 / §3.5 的矩阵。历史教训见技术方案 §12 的 D-09 系列。
3. **不扩大范围**。PRD §1.4 的 Out of Scope（X-01~）明确排除的功能，
   不得「顺手实现」——尤其是插件系统（R-14）。
4. **测试跟着改**。修 bug 必须附回归测试（PRD TEST-03）；改接口必须同步契约测试（门禁 17）。
5. **发现文档矛盾就登记，不要悄悄绕过**。按技术方案 §13.2 的格式新增 `DEBT-NN` 条目，
   或直接修正真相源并在 §12 留痕。
6. **禁止直接推送 `main`**（PRD CODE-09）：分支保护已启用（PR 必需、2 项必需检查 = 门禁 17 项 + Windows 冒烟、
   `strict` 要求分支最新、禁止强推/删除、`enforce_admins` 开启）。流程：新建分支 → 本地 `pnpm ci` 全绿 →
   推送**分支** → 开 PR（模板含自检清单）→ CI 全绿后合并。PR 模板见 `.github/pull_request_template.md`。

7. **里程碑收尾必做「交付行对账」**。把计划文档里每条 `交付：` 逐项列出，状态**只能**写
   「✅ 已实现（提交号）」或「顺延（顺延到哪个里程碑 + 依据 + 登记在哪）」，**不允许留空**。
   > 教训（M1，2026-10-01）：计划的 WP3 交付行写了「打开时恢复上次 Vault」（FR-VAULT-06，P1），
   > 实现没做、完成报告的顺延表里也没写——**三次自检（17 门禁 / CI 全绿 / 验收清单 11/11）
   > 全部没抓到**，最终由独立上下文审查发现。根因是「对账基准用了『我做了什么』而不是『承诺了什么』」。
8. **顺延只能落在真相源或台账**：PRD 口径/勘误表、完成报告的顺延表、DEBT 台账——三者至少一处。
   **PR 描述不算**（它是过程记录，没有人会在判定里程碑是否完成时回读它）。
   > 同一教训：FR-VAULT-06 的"顺延"当时只写在 PR #12 的描述里，等于没写。
9. **验收清单是子集，必须在报告里声明覆盖范围**。例如「本清单覆盖 AC 与质量门禁，
   **不覆盖** P1 功能承诺，后者见交付行对账表」——否则会出现「清单全绿 = 里程碑完成」的假信号。
10. **关键里程碑收尾做一次独立上下文审查**（新会话/子代理，基线取里程碑末次提交）。
    自证只能验证一致性，**发现不了自己漏掉的那一行**；M1 的独立审查抓到 2 条 major + 1 条 P1 缺口。

---

## 10. 里程碑

**M0~M10 共 11 个里程碑**，完整规划、交付物与阶段验收（DoD）见 **PRD §9.1**，
关键路径与并行关系见 **PRD §9.2**。

当前项目状态（2026-10-02）：**M0 ✅、M1 ✅**，下一步 **M2（文件树与基础编辑）**。
M1 交付与 11 项验收结论见 [M1完成报告](history/M1完成报告-2026-10-01.md)；
独立审查的 14 条发现与逐条处置见 [独立审查与修复](history/独立审查与修复-2026-10-01.md)。

最高技术风险环节：**M3（解析器）** 与 **M4（链接改写）**——PRD §9.2 已明确提示，
建议优先建立测试集后再动实现代码。

各里程碑的依赖版本引入时机见技术方案 §3.5.2 的「引入阶段」列。
