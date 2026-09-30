# Knowl Pad

> 本地优先（local-first）的跨平台 Markdown 笔记应用。
> **Markdown 文件是唯一真相源**：索引库是纯派生数据，删掉后可随时从 `.md` 完整重建。

**当前状态：M0（工程脚手架 + 17 项质量门禁 + 三平台窗口冒烟）已完成；功能开发尚未开始。**

---

## 文档（唯一真相源）

| 文档 | 作用 |
| --- | --- |
| [`docs/Knowl-Pad-PRD.md`](docs/Knowl-Pad-PRD.md) | **需求真相源**：功能行为、数据模型、接口契约、验收标准、性能与安全指标 |
| [`docs/Knowl-Pad-完整技术方案.md`](docs/Knowl-Pad-完整技术方案.md) | **实现真相源**：架构、算法、配置文件、CI/CD、勘误与决策留痕 |
| [`docs/AGENTS.md`](docs/AGENTS.md) | AI 编码工具的指令入口（索引 + 红线），不含需求或实现细节 |
| [`docs/history/`](docs/history/README.md) | 修订记录、审计、决策与 CI 证据（**非真相源**，仅供追溯） |

> 技术栈与依赖版本矩阵以技术方案 §3.2 / §3.5 为准；门禁清单以 PRD §8.4 为准。**本文件不复制这些内容**，避免形成第二处维护点。

---

## 快速开始

```bash
# 依赖安装（必须用 pnpm；CI 与生产一律 --frozen-lockfile）
pnpm install --frozen-lockfile

# 启动桌面应用（含热重载）
pnpm tauri dev

# 仅前端（浏览器调试，IPC 不可用）
pnpm dev
```

工具链硬约束（**低于此版本必然构建失败**）：

- Rust **1.90+**（由仓库根 `rust-toolchain.toml` 固定；MSRV 由 tauri 2.12 家族决定，见技术方案 `DEBT-08`）
- Node **24.19.0** + pnpm（见 `.nvmrc` 与 `package.json` 的 `engines` / `packageManager`）
- 门禁脚本需要 `ripgrep`（缺失时以退出码 2 硬性失败，不会静默通过）

---

## 质量门禁

共 **17 项**（PRD §8.4）：类型检查、前端 Lint、Rust clippy/fmt/test、前端与 Rust 测试覆盖率、
依赖安全审计、lockfile 一致性、生产构建、性能基准、安全与可靠性测试集、命名一致性、
前端构建完整性、domain 覆盖率、路径封装检查、IPC 契约一致性。

```bash
pnpm ci    # 本地一次跑完全部门禁（= node scripts/run-gates.mjs）
```

CI 见 [`.github/workflows/`](.github/workflows/)：`ci.yml`（17 项门禁 + Windows 冒烟）、
`platform-smoke.yml`（三平台窗口启动冒烟）、`release.yml`（多平台构建 + 发布）。

---

## 参与开发

1. **先读真相源**：动代码前读 `docs/AGENTS.md` 的「红线清单」与「模块边界纪律」；
2. **提交信息**遵循 Conventional Commits（`feat` / `fix` / `perf` …）；
3. **禁止直接推送 `main`**：须经 PR + Code Review，且 CI 门禁全绿；
4. 修 bug 必须附回归测试；改 IPC 接口必须同步契约测试（门禁 17）；
5. 发现文档矛盾请按技术方案 §13.2 登记 `DEBT-NN`，**不要悄悄绕过**。

---

## 许可

**MIT OR Apache-2.0** 双许可，与两个 crate 的 `license` 字段一致：

- [LICENSE-MIT](LICENSE-MIT)
- [LICENSE-APACHE](LICENSE-APACHE)

除非你明确另行声明，任何有意提交以纳入本项目的贡献，均按上述双许可授权（与 Rust 生态惯例一致）。
