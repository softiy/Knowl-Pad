# MSRV 提升至 1.90 与依赖回归（2026-09-30）

> 本文件是**决策与操作记录**，不是真相源。规则已固化到技术方案 §3.5.1 / §3.5.4 / §3.6.1 / §9.4.2 / §11.4 / §13.2、PRD §2.2 与 AGENTS.md §7.1。

## 1. 背景：两条可选路线

CI #1 三平台全部因 MSRV 失败（详见 `CI首次运行问题与修复-2026-09-30.md`）：

```
error: rustc 1.88.0 is not supported by the following packages:
  tauri@2.12.0 / tauri-utils@2.10.0 / muda@0.20.0 ... requires rustc 1.90
```

| 路线 | 做法 | 代价 |
| --- | --- | --- |
| A（**本次采纳**） | 把 toolchain 升到 **1.90**，依赖保持当前稳定档（tauri 2.12.0 家族） | 开发者与 CI 必须 ≥ 1.90 |
| B（已于同日实现并验证） | 保持 1.88，把整个 Tauri 家族降级到 2.11.6 一档 | 停留在旧档，需持续手动压制依赖 |

**决策**：采纳 A。理由：Tauri 家族的 `rust-version` 会随版本持续抬升，长期压制依赖版本的成本高于升级 toolchain；
且 MSRV 感知解析（`.cargo/config.toml`）已能保证解析结果不越界，升级是**显式且可复现**的动作。

## 2. 变更清单

| 文件 | 变更 |
| --- | --- |
| `src-tauri/rust-toolchain.toml` | `channel = "1.88.0"` → **`"1.90.0"`** |
| `src-tauri/Cargo.toml` / `crates/kp-domain/Cargo.toml` | `rust-version = "1.88"` → **`"1.90"`** |
| `.github/workflows/{ci,platform-smoke,release}.yml` | `RUST_TOOLCHAIN: '1.88.0'` → **`'1.90.0'`** |
| `.cargo/config.toml` | 保留 MSRV 感知解析（`incompatible-rust-versions = "fallback"`），注释改为说明 1.90 |
| `Cargo.lock` | 重新解析（MSRV = 1.90） |
| 文档 | 技术方案（§3.5.1 矩阵、§3.5.4 Cargo.toml、§3.6.1 Rust 规则、§9.4.2 toolchain、§11.4 CI、§12 勘误 D-09b/D-09c、§13.2 DEBT-08、附录 A）、PRD §2.2、架构图、AGENTS.md §7.1 |

## 3. 依赖版本变化

| 包 | 路线 B（1.88） | **路线 A（1.90，本次）** |
| --- | --- | --- |
| `tauri` | 2.11.6 | **2.12.0** |
| `tauri-build` | 2.6.3 | **2.7.0** |
| `tauri-utils` | 2.9.3 | **2.10.0** |
| `tauri-runtime` / `-wry` | 2.11.3 / 2.11.4 | **2.12.0 / 2.12.0** |
| `tauri-codegen` / `-macros` / `-plugin` | 2.6.3 各 | **2.7.0 各** |
| `muda` | 0.19.3 | **0.20.0** |
| `tauri-plugin-dialog` | 2.7.3 | **2.8.0** |
| `tauri-plugin-shell` | 2.3.6 | **2.4.0** |
| `tauri-plugin-opener` | 2.5.5 | **2.7.0** |
| `tauri-plugin-process` | 2.3.1 | **2.4.0** |
| `time` | 0.3.55（其自身 MSRV 1.88） | 0.3.55（**不再是 MSRV 瓶颈**） |

包总数：496（路线 B）→ **483**（路线 A）。

## 4. 验证

- `cargo +1.90.0 check --workspace --all-targets --locked`（与 CI 相同的 toolchain）
- GitHub Actions：见本次提交对应的 CI 运行（门禁 17 项 + 三平台窗口冒烟）

## 5. 遗留提醒

1. **`rust-toolchain.toml` 位于 `src-tauri/`**：rustup 按当前目录向上查找，因此在**工作区根**或 `crates/kp-domain/` 直接运行 `cargo` 时**不会**应用该固定，会用开发者默认 toolchain（本机为 1.98.1）。
   CI 因显式设置 `RUST_TOOLCHAIN` 不受影响。**建议后续把该文件移到工作区根**，让全仓一致；本次未移动，避免触发本机全量重建。
2. 每次依赖升级后应确认 `rust-version` 是否被抬高；MSRV 感知解析只保证「不越界」，不负责提醒「可以降级」。
