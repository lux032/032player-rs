# 架构

初稿，M0 实现时按实际情况修改。

## Workspace 布局

```
032player-rs/
  Cargo.toml            # [workspace]
  crates/
    api/                # 032 HTTP 客户端 + 类型（无 UI、无 mpv）
    session/            # 播放上报状态机（纯逻辑，可注入时钟 / 传输 / 存储）
    lyrics/             # LRC / 逐字解析、翻译显示合并、注音 / 罗马音
    core/               # 包名 client-core（`core` 与标准库冲突）。队列、设置、登录态、图片缓存，
                        # 串起 api + session；播放器通过 trait 注入
    player/             # libmpv 封装（只有这里依赖 libmpv）
    os/                 # SMTC / MPRIS / 托盘 / 单实例 / 更新
    app/                # Slint UI，二进制入口
```

依赖方向：`app -> {core, player, os}`，`core -> {api, session, lyrics}`。
`core` 不依赖 `player` / `os`：播放器和系统集成以 trait 形式在 `core` 定义，由 `app` 注入实现。
这样 `api` / `session` / `lyrics` / `core` 都能在无 libmpv、无显示的机器上 `cargo test`。

## 选型（暂定）

| 用途 | crate | 备注 |
|------|-------|------|
| UI | `slint` | D2，M0 验证 |
| 异步运行时 | `tokio` | UI 线程之外跑网络和解码 |
| HTTP | `reqwest`（rustls） | |
| 序列化 | `serde`、`serde_json` | 类型对照 `music032-types.ts` 和服务端 `internal/storage` |
| 错误 | `thiserror`（库）、`anyhow`（app） | |
| 播放 | `libmpv2` | Windows 随包带 `libmpv-2.dll` |
| 媒体控制 | `souvlaki` | SMTC / MPRIS / macOS 一套 API |
| 托盘 | `tray-icon` | |
| 凭据存储 | `keyring` | API Token、媒体凭据 |
| 设置 / 状态 | `serde` + TOML / JSON 文件，`directories` 定位 | |
| 缓存 | 磁盘图片缓存 + 内存 LRU（`lru` 或 `moka`） | |
| 图片解码 | `image`（后台线程，按显示尺寸缩放） | |
| 日语分词 | `lindera`（替代 kuromoji） | M3 |
| 罗马音 | `wana_kana` | M3 |
| i18n | `slint` 内置 `@tr()` + gettext，或 `fluent` | M4 决定 |
| 日志 | `tracing` + `tracing-subscriber`（滚动文件） | |
| 更新 | 待定（`self_update` / velopack） | M4 |
| 文件对话框 | `rfd`（`xdg-portal`，无 GTK 依赖） | M2 歌单封面 |

## 关键设计

### 线程模型

- UI 线程只做渲染和事件分发。
- `core` 在自己线程上的 tokio 运行时里跑一个循环，持有全部状态：
  UI 发 `Command`（`Commander` 可克隆），引擎信号以 `EngineEvent` 进来，后台任务结果走内部通道；
  状态变化以 `Event` 推给 sink，`app` 在 sink 里用 `slint::invoke_from_event_loop` 切回 UI 线程。
  `core` 不依赖 Slint，UI 侧状态放在 UI 线程的 thread_local 里。
- 播放进度：`player` 观察 mpv 的 `time-pos`，`core` 按 250 ms 步长节流后只更新进度相关的少数属性，
  不触发整页重建（这正是 Electron 版的痛点之一）。
- 切歌时序：`loadfile replace` 之后、新文件 `FileLoaded` 之前，上一首遗留的进度 / 时长 / EOF 一律丢弃。
- 无缝播放：当前文件 `FileLoaded` 后，把队列的"自动下一首"`append` 到 mpv 播放列表（`prefetch-playlist`），
  队列或循环模式变化时先 `clear_upcoming` 再重新 `append`。EOF 时若下一首就是预加载的那首，mpv 已经自己切过去，
  `core` 只更新状态不再 `load`。`FileLoaded` 带回 mpv `path`，与预期不符就重新 `load`（队列恰好在曲目边界被改的竞态）。
- 视图：一个 `View` 枚举决定主页显示什么（六个侧栏列表、专辑 / 艺人 / 歌单详情、搜索、相似歌曲）。
  所有视图都产出同一形状：`ViewHeader`（标题、副标题、详情、封面、已加载 / 总数、工具栏）+ `ViewRow`（行）
  + `ViewTracks`（行对应的曲目，`PlayRow(index)` 用它做队列）。`core::view` 负责取数和组装，
  `app` 只有一个 `ViewPane` 组件按"网格 / 文本行"渲染。
- 列表：登录后展示专辑网格；每个列表按 500 / 页拉取，滚动到接近底部再要下一页（`Command::LoadMore`）。
  每次换视图或换查询 revision +1，旧请求的页直接丢弃；失败保留已加载的行，再滚动会重试。
  `sort=random` 的 seed 按查询（不含分页）记忆，回到顶部换新，最多 50 个。
- 封面：专辑网格是自定义 Model，`row_data` 只对可见行调用，在那里才发封面请求；
  请求尺寸 = 卡片逻辑尺寸 x 缩放比例，向上取最接近的服务端尺寸（256 / 512 / 768 / ...）；
  `core` 缓存压缩字节（LRU 400），在 blocking 线程解码并缩放到显示尺寸，UI 只做 `SharedPixelBuffer` 拷贝；
  UI 侧最多保留 96 张解码封面，超出丢弃最早的，滚回来再从字节缓存解码。

### 登录态

- 服务器地址存 `data_dir/session.json`；API Token 和媒体凭据存系统凭据库（服务名 `032player`，
  键 `api-token@{baseUrl}` / `media-token@{baseUrl}`）。`core` 定义 `SecretStore` trait，
  `os::secrets::Keyring`（`keyring` 4：Windows 凭据管理器 / macOS 钥匙串 / Linux Secret Service，纯 Rust）由 `app` 注入。
  凭据库调用可能阻塞，`core` 只在 `spawn_blocking` 里调用；打不开时只记日志，不保存。
- 启动发 `Command::Restore`：两个凭据都在就自动登录。登录表单里留空的凭据取已保存值。
- 已保存的凭据被服务端拒绝时删掉对应那个。会话中任一请求 401（列表、歌词、上报）：
  删 API Token、保留媒体凭据，停止播放回到登录页。用户退出登录：两个都删。
- 每次登录 / 退出递增 generation，旧会话的列表分页和上报定时器结果直接丢弃。

### 系统集成

- `os` 不依赖 `core`，`app/src/system.rs` 在 UI 线程上把两边接起来：core 事件同步到媒体控件和托盘；
  系统回调（其他线程）经 `slint::invoke_from_event_loop` 回到 UI 线程，再发 `Command`。
- 关闭窗口：开了"关闭时最小化到托盘"且有托盘则隐藏，否则 `quit_event_loop`。主循环是 `run_event_loop_until_quit`。
- 单实例只用标准库：锁文件 + 本机回环端口，不引入额外依赖。

### api

- `Client { base_url, api_token, media_token, http }`，方法按资源分组（albums、tracks、browse、artists、
  playlists、favorites、similar、works、series、home、daily_mixes、lyrics、playback）。
- 歌单写接口对照 `032music-server/docs/api/playlist-operations.md`：append / remove / order 带 `expectedRevision`，
  409 `playlist_conflict` 说明别人先改了，重新读取再试。
- 统一错误 `ApiError { status, code, message }`；401 单独可识别，交给 `core` 触发重新登录。
- 分页上限 500（`limits.maxPageSize`）。
- 先只移植用到的类型，字段用 `Option` 容忍旧服务端缺字段。

### session

- 1:1 移植 `music032-playback-session.ts` 和 `music032-playback-adapter.ts`，保持语义与服务端
  `playback-reporter.js` 一致。
- sans-IO：状态机不碰网络。驱动方循环 `poll_transmit()` 取事件、发 HTTP、`handle_outcome()` 回填结果；
  `keepalive` 事件（退出时的 `client_closed`）不需要回填。同一时刻最多一个事件在途，等价于参考实现的串行队列。
- 定时器（心跳、重试）通过 `Scheduler` trait 注入，到期后驱动方调 `on_timer(id)`。存储、uuid、clientKind 同样注入。
- 测试用假实现，对照 `scripts/music032/playback-reporter.test.mjs` 逐条移植（`crates/session/tests/`）。

### player

- 一个 mpv 实例，事件循环在专用线程，向 `core` 发 `PlayerEvent`（时间、暂停、缓冲、结束 + 原因、错误）。
- 无缝播放用 mpv playlist 预追加下一首（`loadfile ... append`）或 `--prefetch-playlist`，M1 实测决定。
- 独占：Windows `audio-exclusive=yes` + WASAPI；设备列表来自 `audio-device-list`。
- libmpv 来源：Windows 由 `crates/player/build.rs` 下载固定版本的 shinchiro mpv-dev 包
  （SourceForge `mpv-player-windows/libmpv` 永久保留，sha256 校验），解压到 `target/libmpv/`，
  `libmpv.dll.a` 复制为 `mpv.lib` 供 MSVC 链接，`libmpv-2.dll` 复制到输出目录。
  `MPV_LIB_DIR` 指定本地目录可跳过下载。Linux / macOS 链接系统 libmpv。

### 图片

- 按显示尺寸向服务端请求（`size` 参数，原图不带参数）。
- 后台下载 + 解码 + 磁盘缓存，UI 只拿 `slint::Image`。列表滚动时取消不可见项的请求。
- 行模型是惰性的（`app/src/rows.rs`）：Slint 只为即将绘制的行调用 `row_data`，所以"请求封面"只发生在
  可见行。这一点是硬约束 - 如果改成在 Rust 里预先构建全部行，封面缓存（96 张）比列表短时会形成
  "淘汰 -> 重绘 -> 重新请求"的循环。
- 已解码封面放在 `app/src/artwork.rs`（按 `{px}|{path}` 分键，尺寸变化超过 125% / 60% 才整体重解码）；
  键的在途去重、淘汰顺序放在 `core::artwork_keys::KeyCache`（可无显示器测试），
  图片到达时只重绘用到该封面的行。
