# CI/CD 替代方案：GitHub Actions 构建 + 同步 Gitee Release

| 项目 | 内容 |
| --- | --- |
| 文档版本 | v1.0（2026-09-25） |
| 关联 | PRD §8.4（门禁）、技术方案 §11.4/§11.5、`DEBT-06`、红线 R-16/R-17 |
| 替代对象 | Gitee Go 流水线 YAML（原 `.gitee/workflows/knowlpad.yml`）与 `semantic-release-gitee` |

## 1. 为什么要替代

M0 核实结论（技术方案 附录 D）：

1. **Gitee Go 变量语法不兼容**：Gitee Go 的系统变量是 `GITEE_*`，引用语法为 `{GITEE_xxx}`；原 YAML 使用的 `${{ gitee.repository_owner }}` / `gitee.ref_name` **不存在**。
2. **流水线模型不同**：Gitee Go 不是 GitHub Actions 的 `on/jobs/uses` 模型，原 YAML 无法直接运行。
3. **生态可用性未定**：原引用的 `release-files-to-gitee` / `yanglbme/gitee-release-action` 可用性未核实。
4. **`semantic-release-gitee` 已停更**：npm registry 显示 last publish = 2022-02-10，modified = 2022-05-17，**逾 4 年无维护**，直接违反红线 **R-16**。

因此本方案把 **构建与质量门禁放在 GitHub Actions**（生态成熟、矩阵构建开箱即用），把 **分发仍落在 Gitee Release**（PRD 规定 Gitee Release 是应用唯一的对外网络目标，也是 updater 的唯一下载源），两者之间用**自研 Node 脚本调用 Gitee OpenAPI v5** 同步。

## 2. 架构

```
  开发者 push / PR
        │
        ▼
  GitHub Actions: ci.yml           ← 13 项门禁（PR + main）
        │
        │ 打 tag v*
        ▼
  GitHub Actions: platform-smoke.yml   ← 三平台窗口启动冒烟（KP_SMOKE=1）
        │
  GitHub Actions: release.yml
        ├── build（4 平台矩阵）
        │     pnpm tauri build --target <triple>
        │     产出 .msi/.exe/.dmg/.AppImage/.deb + *.sig
        ├── GitHub Release（gh release create）
        └── publish-gitee-release.mjs  ──► Gitee OpenAPI v5
                                             ├── 创建/复用 Release（tag）
                                             └── 上传全部资产（含 .sig 与 latest.json）
```

**关键点**：应用的 updater endpoint 仍指向 **Gitee Release 的 `latest.json`**，与 PRD FR-UPDATE 系列一致；GitHub Release 只是构建产物的中转与备份渠道。

## 3. 变量与密钥映射（Gitee Go → GitHub Actions）

| 语义 | Gitee Go（原） | GitHub Actions（新） |
| --- | --- | --- |
| 仓库所有者 | `{GITEE_REPO}` 拆分 | `${{ vars.GITEE_OWNER }}`（自定义变量）或 `github.repository_owner` |
| 仓库名 | `{GITEE_REPO}` | `${{ vars.GITEE_REPO }}` |
| 分支 | `{GITEE_BRANCH}` | `${{ github.ref_name }}` / `github.ref` |
| 提交 | `{GITEE_COMMIT}` | `${{ github.sha }}` |
| 标签 | `{GITEE_TAG}` | `${{ github.ref_name }}`（`refs/tags/*` 触发） |
| 凭据 | Gitee Go 凭据管理 | Repository secrets / variables |

**必须配置的 Secrets / Variables**：

| 名称 | 类型 | 用途 | 权限范围 |
| --- | --- | --- | --- |
| `GITEE_TOKEN` | Secret | 调用 Gitee OpenAPI v5 创建 Release 与上传附件 | Gitee 私人令牌，勾选 **`projects`**（仓库读写）即可；Gitee **没有**独立的 `releases` scope |
| `GITEE_OWNER` | Variable | Gitee 空间地址（用户名/组织 path） | — |
| `GITEE_REPO` | Variable | Gitee 仓库 path | — |
| `TAURI_SIGNING_PRIVATE_KEY` | Secret | updater 签名私钥 | 仅 release job 注入；PR 构建不注入 |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Secret | 私钥口令（**不得为空**） | 同上 |

## 4. Gitee OpenAPI v5 调用约定（脚本依据）

| 操作 | 方法与路径 | 关键参数 |
| --- | --- | --- |
| 按 tag 查询 Release | `GET /api/v5/repos/{owner}/{repo}/releases/tags/{tag}` | `access_token` |
| 创建 Release | `POST /api/v5/repos/{owner}/{repo}/releases` | `access_token`、`tag_name`(必填)、`name`、`body`、`target_commitish`、`prerelease` |
| 上传附件 | `POST /api/v5/repos/{owner}/{repo}/releases/{release_id}/attach_files` | `access_token`、`file`（multipart/form-data） |
| 列出 Release | `GET /api/v5/repos/{owner}/{repo}/releases` | `access_token`、`page`、`per_page` |

> ⚠️ 端点路径依据 Gitee API v5 的公开约定整理（`access_token` 可放在表单或查询串）。**M0 未用真实 token 联调**，首次发布前必须用 `--dry-run` 打印请求后，再做一次真实 smoke（见 §7 验证步骤）。

## 4.1 前置：pnpm 12 的构建脚本白名单（M0 实测）

pnpm 12 起：
1. 设置项**不再读取** `package.json` 的 `pnpm` 字段，迁移到 `pnpm-workspace.yaml`；
2. 默认**不执行**依赖的构建脚本，遇到被忽略的构建脚本会直接以 `ERR_PNPM_IGNORED_BUILDS` 使 `pnpm install` 失败。

因此仓库根目录必须存在：

```yaml
# pnpm-workspace.yaml
packages:
  - '.'
allowBuilds:
  esbuild: true      # 必须允许，否则 vite/vitest 链路安装失败
strictDepBuilds: false
```

> 注意 `allowBuilds` 是 **map**（`包名: true`），写成列表会导致 YAML 解析错误。CI 与本地依赖同一文件。

## 5. 迁移步骤

1. 在 GitHub 仓库 Settings → Secrets and variables 中创建上表 5 个条目。
2. 删除或归档 `.gitee/workflows/knowlpad.yml`，避免两套 CI 并存产生假绿。
3. 提交 `.github/workflows/ci.yml` 与 `.github/workflows/release.yml`。
4. 在 `package.json` 补充脚本（`gate:*`、`test:security`、`test:reliability`、`test:perf`），见 `scripts/run-gates.mjs`。
5. 首次以 `workflow_dispatch` 跑 release job 的 **dry-run**（脚本 `--dry-run`）验证 Gitee 联调。
6. 确认 `latest.json` 由 Gitee Release 地址可访问后，打印下一版本触发真实发布。

## 6. 回退（若必须留在 Gitee Go）

若组织策略要求只能在 Gitee Go 上跑：把 `ci.yml` 的每个 `run:` 原样搬进 Gitee Go 的任务步骤（Gitee Go 的模型是「流水线 → 阶段 → 任务」，非 `jobs/uses`），并把所有 `${{ ... }}` 替换为 `{GITEE_xxx}` 形式；矩阵构建需拆成 4 条流水线（Gitee Go 无 GitHub 的 `strategy.matrix`）。**工作量大且生态弱，仅在强制要求时采用**。

## 7. 验证清单

- [ ] `ci.yml`：PR 触发后全部门禁（`node scripts/run-gates.mjs`）全绿。
- [ ] `platform-smoke.yml`：ubuntu（Xvfb）/ macOS / Windows 三平台均打印 `KP_SMOKE_OK`。
- [ ] `release.yml`：4 平台矩阵各自产出安装包与 `.sig`。
- [ ] `publish-gitee-release.mjs --dry-run` 输出的请求体与预期一致。
- [ ] 真实发布后：Gitee Release 页面能看到全部资产；`latest.json` 可下载且内容与 GitHub 侧一致。
- [ ] 客户端 updater 指向 Gitee 的 `latest.json` 能检测到新版本并通过签名校验。
- [ ] 私钥未在 CI 日志中回显（grep 日志确认）。
