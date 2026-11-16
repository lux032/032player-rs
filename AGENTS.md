# AGENTS.md

## 通用约定

- 不使用长破折号 "—"，用普通 "-"。
- 文档用中文，代码标识符和注释用英文。
- Conventional Commits（`feat(api): ...`、`fix(player): ...`）。scope 用 crate 名：`api`、`session`、`player`、`core`、`lyrics`、`os`、`sonos`、`app`，文档用 `docs`，构建用 `build` / `ci`。
- 只在明确要求时提交。
- YAGNI：先做 `docs/FEATURES.md` 里列出的功能，不为其他服务端留抽象。

## 先读

1. `docs/PLAN.md` - 已定决策和当前里程碑；`docs/TASKS.md` - 当前任务清单，完成后打勾。
2. `docs/ARCHITECTURE.md` - crate 边界。
3. 做某个功能前，在 `docs/FEATURES.md` 找到对应条目和 Electron 版源码位置，**以 Electron 版行为为准**。

## 参考来源

行为以这些项目为准。它们是独立仓库，只读，不要为了迁就本仓库去改它们。本地如果有一份检出，按下面的相对路径查找；没有固定的兄弟目录。公开仓库里的同名路径是同一份参考。

| 项目 | 相对该项目根目录 | 用途 |
|------|------------------|------|
| [032musicplayer](https://github.com/lux032/032musicplayer) | （仓库根） | Electron 客户端，行为基准 |
| 同上 | `MUSIC032_CHANGES.md` | 032 相关改动、能力位、已知限制的总表 |
| 同上 | `src/shared/api/music032/music032-types.ts` | API 响应类型 |
| 同上 | `src/renderer/api/music032/` | HTTP 客户端、controller、播放上报状态机 |
| 同上 | `scripts/music032/*.test.mjs` | 上报状态机测试，移植成 Rust 测试 |
| [032music-server](https://github.com/lux032/032music-server) | `internal/storage`、`docs/api/*.md` | 服务端（Go）。结构体和接口文档是接口权威 |
| 同上 | `internal/http/assets/playback-reporter.js` | 上报协议参考实现 |

## 开发环境说明

- 文档和界面用简体中文。主力平台是 Windows，其次 Linux，macOS 尽力而为。
- 无界面 crate 必须能在没有 libmpv 和显示器的机器上 `cargo test`。依赖 libmpv / UI 的代码只放在 `player` / `os` / `app`；
  `core`（包名 `client-core`）不依赖它们，需要时在 `core` 定义 trait，由 `app` 注入。
- `player` 可以只做 `cargo check`（不链接 libmpv）。已安装 `x86_64-pc-windows-msvc` 标准库时，
  `cargo check -p player --target x86_64-pc-windows-msvc` 可检查 Windows 专属代码（不链接）。
  依赖 reqwest 的 crate 在没有 MSVC 的环境里可能编不过（aws-lc 需要 MSVC 编译 C），交给 CI 的 Windows job。
- Linux 上编译 Slint 需要 fontconfig / freetype 开发包。运行界面还需要系统 libmpv，见 README。
- 界面检查：`cargo run --manifest-path tools/ui-snapshot/Cargo.toml -- /tmp` 用软件渲染器把
  `crates/app/ui/app.slint` 填假数据渲染成 PNG（登录页、专辑、曲目）。改了 `.slint` 后要同步更新假数据。
- 实际播放和完整窗口要在装有 libmpv 的图形环境里跑，Windows 上的结果最有参考价值。
  Windows 编译慢：标 **[Win]** 的项先攒着，一个大模块完成后在 `TASKS.md` 汇总成一份验证清单，再集中验证。

## 检查

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --exclude player --exclude app
```
