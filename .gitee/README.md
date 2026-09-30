# 已迁移：CI 不再使用 Gitee Go

本目录**不再承载流水线**。

## 背景

M0 核实结论（见 `docs/history/CI替代方案.md`）：

1. Gitee Go 的系统变量是 `GITEE_*`，引用语法为 `{GITEE_xxx}`；原先按 GitHub Actions 写的
   `${{ gitee.repository_owner }}` / `${{ gitee.ref_name }}` **不存在**。
2. Gitee Go 的流水线模型不是 `on/jobs/uses`，原 YAML 无法直接运行。
3. `semantic-release-gitee` 自 2022-02 起停更逾 4 年，违反红线 R-16，已由
   `scripts/publish-gitee-release.mjs` 取代。

## 现状

- **构建与质量门禁**：`.github/workflows/ci.yml`（ubuntu 全量 16 项门禁 + windows 冒烟）。
- **发布**：`.github/workflows/release.yml`（4 平台矩阵 → GitHub Release → 同步 Gitee Release）。
- **分发与自动更新源**：仍然是 **Gitee Release**（`latest.json` 与 `.sig` 由发布脚本上传），
  与 PRD FR-UPDATE 系列一致。

若组织强制要求回到 Gitee Go，请按 `docs/history/CI替代方案.md` §6 的回退步骤重写，并把所有
`${{ ... }}` 替换为 `{GITEE_xxx}` 形式。
