# 032player-rs

[032 Music Server](https://github.com/lux032/032music-server) 的原生桌面客户端。用 Rust 和 Slint 写界面，播放走进程内的 libmpv。

给已经搭好 032 Music Server、想在桌面上浏览曲库并听歌的人。只连接这一套服务端。Jellyfin、Navidrome、Subsonic、Plex 不在范围内。界面是简体中文。

当前版本 **0.2.15**。日常听歌可以用。代码签名、多语言和最小化到托盘等还没做完，所以没有标成 1.0。

## 下载

Windows 安装包和便携 zip 在 [Releases](https://github.com/lux032/032player-rs/releases)。最新是 [v0.2.15](https://github.com/lux032/032player-rs/releases/tag/v0.2.15)。

主力平台是 Windows。Linux 可以按下面的方式从源码构建。macOS 尽力而为，没有作为正式发布平台验证。

## 现在能做什么

- 用服务器地址、API Token 和媒体凭据登录。正常使用时凭据写在操作系统凭据库里，下次启动自动登录。
- 浏览专辑、曲目、艺人、歌单、收藏，以及搜索。
- 播放：libmpv、队列、随机、循环、无缝切歌、睡眠定时、可选转码、独占输出、播放 / 暂停时的淡入淡出。
- 歌词：同步滚动、翻译、搜索和纠正、偏移。
- 032 专属页面：主页、作品、系列、每日推荐、播放记录。
- 系统媒体控制（Windows SMTC、Linux MPRIS）和媒体键。
- Sonos 投放：本机直接控制音箱。
- 浅色 / 深色主题。

功能对照和完成情况见 [docs/FEATURES.md](docs/FEATURES.md)。

## 还没做完

- 自动更新只做到启动时查看 GitHub Releases 并给出链接，不会自动安装。这项提示以后可能拿掉。
- 安装包没有代码签名。
- 界面只有简体中文。
- macOS 没有完整验证。
- 最小化到托盘还没做。Windows 上可以关闭到托盘；Linux 托盘没有。

明确不做的还有均衡器、ReplayGain、逐字卡拉 OK、全局快捷键、普通 DLNA，以及其他服务端。见 [docs/PLAN.md](docs/PLAN.md) 和 [docs/FEATURES.md](docs/FEATURES.md)。

部分播放和界面逻辑移植自 Electron 客户端 [032musicplayer](https://github.com/lux032/032musicplayer)（基于 Feishin）。

## 文档

| 文档 | 内容 |
|------|------|
| [docs/PLAN.md](docs/PLAN.md) | 路线、已定决策、里程碑 |
| [docs/FEATURES.md](docs/FEATURES.md) | 功能对照清单 |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | crate 划分和关键设计 |
| [docs/TASKS.md](docs/TASKS.md) | 任务记录（完成打勾） |
| [AGENTS.md](AGENTS.md) | 编码约定和开发环境 |

## 构建

```bash
cargo test --workspace --exclude player --exclude app   # 无界面 crate
cargo run -p app                                         # 需要 libmpv
```

Windows 首次构建会自动下载 libmpv（约 30 MB，sha256 校验，缓存在 `target/libmpv/`）。
已有 libmpv 时设置 `MPV_LIB_DIR` 指向含 `mpv.lib` / `libmpv-2.dll` 的目录即可跳过下载。
Linux 需要系统 libmpv 和 fontconfig 开发包（如 `libmpv-dev`、`libfontconfig1-dev`）。

API Token 和媒体凭据在正常使用时保存在操作系统凭据库。开发时可以用环境变量预填登录页，三项都有时自动登录；不设也可以在界面里填写。

```powershell
$env:M032_SERVER_URL = "https://music.example.com"
$env:M032_API_TOKEN = "..."
$env:M032_MEDIA_TOKEN = "..."
cargo run -p app --release
```

日志级别用 `RUST_LOG` 控制（默认 `info`）。

## 许可

GPL-3.0（部分逻辑移植自 GPL-3.0 的 032musicplayer / Feishin）。
