# Gitee Release 发布联调手册

| 项目 | 内容 |
| --- | --- |
| 适用脚本 | `scripts/publish-gitee-release.mjs`（替代已停更的 semantic-release-gitee） |
| 关联 | `docs/history/CI替代方案.md`、`.github/workflows/release.yml`、红线 R-16 |
| 端点核实 | 2026-09-25 实测（Gitee OpenAPI v5，读路径匿名可用） |

## 0. 前置条件

| 项 | 说明 |
| --- | --- |
| Gitee 私人令牌 | **仅勾选 `projects`**（仓库读写）。Gitee 没有独立的 releases scope |
| `GITEE_OWNER` / `GITEE_REPO` | 仓库变量（GitHub Actions 里配为 Variables） |
| `GITEE_TOKEN` | 仓库 Secret；**不要**写进任何文件 |
| 签名密钥 | 仅正式发布需要：`TAURI_SIGNING_PRIVATE_KEY` + `_PASSWORD`（私钥离线保管，测试用独立密钥对） |

## 1. 只读联调：确认端点可达（无需 token）

脚本的读路径已实测（2026-09-25，公开仓库匿名可用）：

```bash
node scripts/publish-gitee-release.mjs --owner mindspore --repo mindspore --tag v0.1.0-alpha --verify
```

实测输出：

```text
✅ 端点可达 · Release 已存在：id=62730 tag=v0.1.0-alpha name=v0.1.0-alpha
   已上传附件 0 个
exit=0
```

对应三个端点（Gitee API v5）：

| 操作 | 路径 |
| --- | --- |
| 按 tag 查 Release | `GET /repos/{owner}/{repo}/releases/tags/{tag}` |
| 列出附件 | `GET /repos/{owner}/{repo}/releases/{release_id}/attach_files` |
| 创建 Release | `POST /repos/{owner}/{repo}/releases` |
| 上传附件 | `POST /repos/{owner}/{repo}/releases/{release_id}/attach_files`（multipart） |

## 2. 带 token 的 `--verify`（改仓库前先跑）

```bash
export GITEE_TOKEN=...        # 或用 Actions Secrets
export GITEE_OWNER=<owner>
export GITEE_REPO=<repo>
node scripts/publish-gitee-release.mjs --tag v0.1.0 --verify
```

**退出码语义**（可用于脚本判定）：

| 退出码 | 含义 | 处置 |
| --- | --- | --- |
| 0 | 端点可达且该 tag 已有 Release | 可继续上传资产（脚本会复用，不重复创建） |
| 2 | 端点可达，该 tag 尚无 Release | 首次发布属正常 |
| 1 | token 无效/权限不足（401/403）或网络/端点异常 | 检查令牌是否勾选 `projects` |

## 3. dry-run：只打印请求，不发写请求

```bash
node scripts/publish-gitee-release.mjs --tag v0.1.0 --notes-file CHANGELOG.md --assets "artifacts/**/*" --dry-run --owner <owner> --repo <repo>
```

实测输出（示例）：

```text
发布目标：<owner>/<repo>  tag=v0.1.0  资产 5 个  [DRY-RUN]
→ GET  https://gitee.com/api/v5/repos/<owner>/<repo>/releases/tags/v0.1.0?access_token=<TOKEN>
→ POST https://gitee.com/api/v5/repos/<owner>/<repo>/releases
  form: access_token=<TOKEN>&tag_name=v0.1.0&name=v0.1.0&body=...
→ POST https://gitee.com/api/v5/repos/<owner>/<repo>/releases/<ID>/attach_files  file=... (91607 bytes)
完成：[DRY-RUN] 新建资产 5 个，跳过 0 个
```

## 4. 真实发布

**推荐路径**：打 tag 后由 `.github/workflows/release.yml` 自动完成
（4 平台构建 → `gh release create` → `publish-gitee-release.mjs` 同步 Gitee）。

**手动路径**（应急）：

```bash
node scripts/publish-gitee-release.mjs \
  --tag v0.1.0 --name "v0.1.0" --notes-file CHANGELOG.md \
  --assets "artifacts/**/*" --target main
```

## 5. Gitee 已知行为与坑（实测）

1. **不存在的 tag 返回 `HTTP 200 + body "null"`**，不是 404。脚本已按「尚未创建」处理（退出码 2）。
2. **附件重名**：脚本按返回体中的 `exist/已存在/already` 判定为已上传并跳过，保证重复执行幂等。
3. **`assets` 与 `attach_files` 不是一回事**：前者含 Gitee 自动生成的源码包，后者才是上传的资产。
4. **不要用 `process.exit()` 硬退出**：带未关闭 HTTP socket 的硬退出在 Windows 上会 fail-fast（`0xC0000409`）。脚本改用 `process.exitCode`。
5. 令牌只从环境变量读取，**不接受命令行明文传入**，避免进入 shell 历史与 CI 日志。

## 6. 验收清单

- [ ] `--verify` 返回 0 或 2（不是 1）
- [ ] `--dry-run` 打印的资产清单与 `artifacts/` 实际文件一致
- [ ] 正式发布后 Gitee Release 页面可见全部资产（含 `.sig` 与 `latest.json`）
- [ ] 客户端 updater 指向 Gitee 的 `latest.json` 能检测到新版本并通过签名校验
- [ ] CI 日志中 grep 不到私钥内容
