# 规划

整理自 2026-10-28 的评估讨论。

## 背景

Electron 版 032musicplayer 的规模（2026-10）：

| 部分 | 行数 | 说明 |
|------|------|------|
| `src/renderer`（React UI） | 约 14.4 万 | 重写成本主要在这里 |
| `src/main`（Electron 主进程） | 约 9 千 | mpv、MPRIS、托盘、更新、远程控制 |
| `src/shared` | 约 1.8 万 | 类型、normalize、主题 |
| 032 专属 API + 上报 | 约 3.1 千 | 纯逻辑，容易移植 |

技术栈：Electron 43、React 19、Mantine 9、react-query、zustand、motion、OverlayScrollbars、
react-window，播放走 `node-mpv`（mpv 子进程 + IPC socket）。

要点：**音频本来就是原生 mpv**，bit-perfect / 独占模式不依赖 JS。Rust 重写改善的是
UI 流畅度和资源占用（内存、启动、包体），不是音质。

## 两类"性能不好"

1. 内存高、启动慢、包体大：Electron 自带 Chromium 的固有成本。
2. 滚动卡、切页慢、播放时掉帧：React 层问题（进度 tick 引起大范围重渲染、
   `player.store.ts` 订阅粒度、动画、滚动条库、react-query 持久化缓存）。

## 路线对比

| 路线 | 工作量 | 解决 1 | 解决 2 | 备注 |
|------|--------|--------|--------|------|
| 0. 原地优化 Electron 版 | 1-3 周 | 否 | 大部分 | 投入产出比最高，可在 Electron 仓库里独立进行 |
| 1. Tauri 2 + 复用 React | 1.5-3 月 | 是 | 否 | 只重写主进程；Linux WebKitGTK 可能更慢 |
| 2. 全原生 Rust | 6-12 人月对齐 | 是 | 是 | 本仓库选的路线 |

**决定：本仓库做路线 2，定位为实验项目。** Electron 版继续用，路线 0 的优化可另行在
Electron 仓库做，两者不冲突。

## 已定决策

| # | 决策 | 状态 |
|---|------|------|
| D1 | 独立仓库 `/workspace/032player-rs`，与 Electron 版分开 | 已定 |
| D2 | UI 框架 Slint（GPL-3 授权，与本项目一致） | 暂定，M0 验证后确认 |
| D3 | 播放用 libmpv（`libmpv2` crate），进程内调用，不走 socket | 已定 |
| D4 | 只支持 032 Music Server；不做多服务端抽象 | 已定 |
| D5 | 主力平台 Windows，其次 Linux，macOS 尽力而为 | 已定 |
| D6 | 语言先做 zh-Hans / zh-Hant / en | 已定 |

UI 框架备选（D2 在 M0 不过关时再选）：

| 框架 | 优点 | 缺点 |
|------|------|------|
| Slint | 声明式 `.slint`，内置虚拟化 ListView，Windows / IME 成熟，内存低 | 复杂自定义布局（主页轮换、逐字歌词）要多花功夫 |
| gpui + gpui-component | 性能最强，组件库自带表格 / 虚拟列表 / 主题 | API 不稳定，文档少 |
| iced | Elm 架构，社区大 | 无内置虚拟列表 |

## 待用户确认的范围

以下 Electron 版功能是否要做，未定（默认 M4 之后再说）：

- 远程控制（内置 HTTP 服务 + 远程 SPA）
- Discord Rich Presence
- 可视化
- 自定义主题（导入 CSS 的那套无法沿用，要重新设计）
- DLNA 投放
- 标签编辑器（本地 taglib）
- 其余 36 种语言

## 里程碑

每个里程碑结束时用户在 Windows 上实测，决定是否继续。

### M0 - 可行性验证（约 2 周）

目标：用最少代码回答"值不值得继续"。细粒度进度见 `TASKS.md`。

- [x] Cargo workspace 骨架（crate 见 `ARCHITECTURE.md`）、CI（fmt / clippy / test）
- [x] `api`：类型移植（够用即可：专辑、曲目、capabilities、错误体）、Token 登录、
      `/browse/albums`、`/browse/tracks`（无 browse 时回退 `/albums`、`/tracks`）、流 URL（带 `mediaToken`）、封面 URL
- [x] `session`：移植 `music032-playback-session.ts` + `scripts/music032/playback-reporter.test.mjs`
- [x] `player`：libmpv 播放 / 暂停 / seek / 下一首、进度事件、输出设备、独占模式（播放已实测；独占模式待 M1 设置页入口）
- [x] `app`：登录页、专辑网格、曲目列表（1 万行虚拟列表 + 封面异步加载）、播放条、一段中日文滚动歌词
- [ ] 实测并记录到本文件：冷启动时间、空闲 / 播放 / 滚动时内存、滚动帧率，与 Electron 版对比

通过标准（建议）：内存不到 Electron 版的 1/3，1 万行滚动无明显掉帧，中日文显示和输入法正常。

### M1 - 能日常使用

状态：代码完成（2026-10-28），待用户 Windows 验证（清单见 `TASKS.md`）。最小化到托盘、Linux 托盘未做。

- 队列（随机、循环、下一首播放、加到末尾）、无缝播放、睡眠定时
- 播放上报接线（start / pause / resume / seek / buffering / heartbeat / end，会话恢复，clientKind `desktop`）
- 系统集成：SMTC / MPRIS / 媒体键、托盘、单实例
- 设置持久化、登录态持久化（Token 存系统凭据库）

### M2 - 曲库浏览

状态：代码完成（2026-10-28），待用户 Windows 验证（清单见 `TASKS.md`）。艺人的收藏入口与艺人筛选、“整表替换”歌单没做。

- [x] 专辑 / 曲目 / 艺人 / 歌单列表：排序、升降序、随机 seed、年份区间、仅收藏、搜索
- [x] 专辑 / 艺人 / 歌单详情，收藏（专辑 / 曲目），搜索
- [x] 歌单增删改、排序、封面上传
- [x] 相似歌曲
- [x] 随机播放全部

### M3 - 032 专属页面

- 032 主页（聚焦卡、接着听、最近添加、作品轮换、主题曲轮换、编年、歌单、页脚统计、空库引导）
- 作品列表 / 详情，系列列表 / 详情
- 每日推荐（评价、换一个、另存为歌单、不再推荐、playbackContext）
- 播放记录（含清空）
- 歌词：同步滚动、逐字、翻译（双语 / 只看译文）、搜索和纠正、偏移、假名注音、罗马音
- 正在播放标识（`/tracks/{id}/works`）

### M4 - 打磨发布

- 032 Light / Dark 主题、强调色
- 全屏播放器、音质标识
- 自动更新、安装包（Windows 优先）
- i18n 三语补全

## 风险

| 风险 | 应对 |
|------|------|
| 中日文字体回退、输入法 | M0 必测 |
| 逐字卡拉 OK 歌词渲染 | M0 先做逐行，M3 再做逐字；不行就自绘 |
| 大量封面图的解码和缓存 | 后台线程解码 + 磁盘缓存 + 按尺寸请求（服务端支持 `size`，注意 `size=0` 会被拒） |
| 主页等复杂布局 | M3 再做，必要时简化 |
| libmpv 在 Windows 的分发 | 随包带 `libmpv-2.dll`，构建脚本下载 |
| 工作量远超预期 | 实验项目，每个里程碑可停 |
