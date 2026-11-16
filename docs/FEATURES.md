# 功能对齐清单

以 Electron 客户端 [032musicplayer](https://github.com/lux032/032musicplayer) 中 **032 服务端可用** 的功能为准。
下面的路径相对该仓库的 `src`，`ctrl` = `renderer/api/music032/music032-controller.ts`。
行为细节、能力位、已知限制以该仓库的 `MUSIC032_CHANGES.md` 为准。本地有检出时按这些相对路径对照。

里程碑列见 `PLAN.md`。勾选表示 Rust 版已完成。

## 连接与认证

| 功能 | 参考 | 里程碑 | 完成 |
|------|------|--------|------|
| 登录：服务器 URL + API Token + 媒体凭据（两者必填且不能相同） | `ctrl` `authenticate` | M0 | [x] api |
| 校验 `GET /api/v1/capabilities`，`apiRevision` 必须为 3 | `ctrl` `M032_API_REVISION` | M0 | [x] |
| 媒体凭据校验：取 1 首曲目，Range `bytes=0-0` 请求流，401 即无效；空库跳过 | `ctrl` `verifyMediaToken` | M0 | [x] |
| Bearer 请求；非 2xx 解析 `{error:{code,message}}` | `renderer/api/music032/music032-api.ts` | M0 | [x] |
| 401 触发重新登录（保留媒体凭据，只需重填 API Token） | 同上、`renderer/store/auth.store.ts` | M1 | [x] |
| 登录态持久化：两个凭据存系统凭据库，启动自动登录；表单留空沿用已保存值；退出登录清空两者 | `auth.store.ts` `logout`、`edit-server-form.tsx` | M1 | [x] |
| 媒体 URL 统一追加 `mediaToken` 查询参数 | `shared/api/music032/music032-normalize.ts` `m032MediaUrl` | M0 | [x] |
| 能力位：`features.browse` / `lyrics` / `lyricsOnline` / `lyricsTranslation` / `playlistArtwork` / `trackWorks` / `dailyMixes` / `playbackContext` / series，`playback.clientKinds`，`limits.maxPageSize`（500） | `shared/api/music032/music032-types.ts` `M032Capabilities` | M0 | [ ] |

## 播放

| 功能 | 参考 | 里程碑 | 完成 |
|------|------|--------|------|
| 原始文件流 `/api/v1/tracks/{id}/stream`（bit-perfect 前提） | `ctrl` `getStreamUrl` | M0 | [x] |
| 可选转码 `/api/v1/tracks/{id}/transcode.{format}` | 同上 | M2 | [x] 有损格式拖动进度时带 `offsetMs` 重新请求 |
| mpv：播放 / 暂停 / seek / 音量 / 静音 | `main/features/core/player/index.ts` | M0 | [ ] |
| 输出设备选择、独占模式（播放中可切换） | `main/features/core/player/audio-exclusive.ts` | M0 | [x] 设置页 |
| 设置持久化：音量、输出设备、独占模式、歌词面板开关（`settings.json`） | `renderer/store/settings.store.ts` | M1 | [x] |
| 缓冲状态（`paused-for-cache`）、载入失败（`end-file` reason=error） | `main/features/core/player/index.ts` `createMpv` | M1 | [x] |
| 无缝播放（预加载下一首） | `renderer/features/player/audio-player/mpv-player.tsx` | M1 | [x] mpv `append` |
| 淡入淡出：播放 / 暂停 300 ms 渐变（`audioFadeOnStatusChange`）；不做交叉淡化 | `mpv-player.tsx` fade | M4 | [x] mpv `volume-gain` |
| 队列：替换、下一首播放、加到末尾、移除、拖动排序、清空 | `renderer/store/player.store.ts`、`features/player/context/` | M1 | [x] 拖动排序在 M2 完成 |
| 随机、单曲 / 列表循环 | 同上 | M1 | [x] |
| 续播位置（`Song.resumePositionMs`） | `useResumePosition` | M3 | [x] 接着听 / 最近在听，条目第一次播放时生效 |
| 睡眠定时 | `features/player/components/sleep-timer-button.tsx` | M1 | [x] |
| 随机播放全部 | `features/player/components/shuffle-all-modal.tsx`、`ctrl` `getRandomSongList` | M2 | [x] 100 首 |
| Auto DJ | `features/player/auto-dj/` | - | - 不做 |
| 均衡器 / 压缩器、ReplayGain | `settings/components/playback/eq-settings.tsx`、`mpv-settings.tsx` | - | - 不做（2026-11-06） |
| 播放条：封面、标题、艺人、专辑、音质（`FLAC / 24bit / 48kHz`）、进度、音量、收藏 | `features/player/components/playerbar.tsx`、`renderer/utils/audio-quality.ts` | M0 / M4 | [ ] |
| 全屏播放器（原图封面不带 `size`，`size=0` 会被拒；页签歌词 / 接下来播放 / 相关歌曲，不做可视化和配置弹窗） | `features/player/components/full-screen-player.tsx`、`ctrl` `getImageUrl` | M4 | [x] |

## 播放上报（apiRevision 3）

| 功能 | 参考 | 里程碑 | 完成 |
|------|------|--------|------|
| 会话状态机：start / pause / resume / seek / buffering / heartbeat / end，seq，重试 | `renderer/api/music032/music032-playback-session.ts` | M0 | [x] |
| 播放器信号适配层 | `renderer/api/music032/music032-playback-adapter.ts` | M1 | [x] |
| 接线：`POST /api/v1/playback/events` | `features/player/hooks/use-music032-playback-reporter.ts` | M1 | [x] |
| 会话恢复快照、resume_invalid 持久化（应用重启可续） | session 文件头注释 | M1 | [x] |
| 重启恢复队列和播放位置（停止状态，播放时从原位置继续） | `player.store.ts` `partialize`、`use-queue-restore.ts` | M1 | [x] |
| clientKind：capabilities `playback.clientKinds` 含 `desktop` 时报 `desktop`，会话内固定 | `MUSIC032_CHANGES.md` 播放上报接线 | M1 | [x] |
| 每日推荐 context 只在全新 start 发送；加入队列的曲目不带 context | `MUSIC032_CHANGES.md` 每日推荐 | M3 | [x] 随队列保存 |
| 退出时尽力发送 `client_closed` | 同上 | M1 | [x] |
| 测试：移植 `scripts/music032/playback-reporter.test.mjs` | - | M0 | [x] |

## 曲库

| 功能 | 参考 | 里程碑 | 完成 |
|------|------|--------|------|
| 专辑列表：`/browse/albums`（sort、order、yearFrom/yearTo、random+seed）；无 browse 回退 `/albums` | `ctrl` `getAlbumList`、`MUSIC032_CHANGES.md` 列表接口 | M0 / M2 | [x] |
| 曲目列表：`/browse/tracks`（sort 含 album/added/duration/random） | `ctrl` `getSongList` | M0 / M2 | [x] |
| 艺人列表：`/browse/artists` / `/artists` | `ctrl` `getArtistList` | M2 | [x] |
| random seed：按查询（不含分页）记忆，offset 0 换新 seed，最多记 50 个 | `ctrl` | M2 | [x] |
| 各列表支持的排序项 | `features/shared/components/list-sort-by-dropdown.tsx` `MUSIC032` | M2 | [x] |
| 筛选：收藏、艺人、年份区间 | `features/shared/components/list-filters.tsx` | M2 | [x] 收藏走收藏页；艺人筛选原从艺人页「更多」进入，v0.2.3 去掉该按钮后暂无入口 |
| 专辑详情（`trackCount` 超出嵌入曲目时用 `/tracks?album=` 分页补齐）、专辑音质汇总 | `ctrl` `getAlbumDetail` | M2 | [x] |
| 艺人详情、作品集、热门歌曲 | `ctrl` `getAlbumArtistDetail` / `artistDiscography` / `getTopSongs` | M2 | [x] 热门歌曲不显示（v0.2.3） |
| 收藏：专辑 / 曲目 / 艺人；收藏页走 `/favorites/albums|tracks`，客户端排序筛选 | `ctrl` `favoriteAlbums` / `favoriteSongs` | M2 | [x] 曲目页签可播放 / 随机播放整个列表 |
| 搜索 | `ctrl` `search` | M2 | [x] |
| 相似歌曲 `/tracks/{id}/similar` | `ctrl` `similarTracks`、`features/similar-songs/` | M2 | [x] |
| 封面 / 艺人图 / 作品海报（2:3）URL，按尺寸请求 | `ctrl` `getImageUrl` | M0 | [x] |

## 歌单

| 功能 | 参考 | 里程碑 | 完成 |
|------|------|--------|------|
| 列表（`durationMillis`，未知时隐藏时长）、详情、曲目 | `ctrl` `getPlaylist*` | M2 | [x] |
| 新建、改名、删除 | `ctrl` `createPlaylist` / `updatePlaylist` / `deletePlaylist` | M2 | [x] |
| 添加 `items/append`（含专辑单曲右键入口）、移除 `items/remove`、排序 `order`、替换 | `ctrl` | M2 | [x] 替换只在 api（UI 用不到） |
| 封面上传 / 删除（`features.playlistArtwork`） | `ctrl` `uploadPlaylistImage` | M2 | [x] |
| 同一首歌只出现一次（服务端跳过重复） | `MUSIC032_CHANGES.md` | - | - |

## 032 专属页面

| 功能 | 参考 | 里程碑 | 完成 |
|------|------|--------|------|
| 032 主页：`/home`、`/home/works`、`/home/songs`、`/home/chronicle`、`/home/spotlight/random`、最近播放；404/501 回退普通主页 | `features/home/music032/`、`032music-server/docs/api/home.md` | M3 | [x] 无主页接口时侧栏不显示主页、默认进专辑 |
| 作品列表（搜索、类型、排序、无限滚动）/ 详情（关联专辑、曲目用途、系列横幅） | `features/works/`、`032music-server/docs/api/works.md` | M3 | [x] 列表后台分页全量加载 |
| 系列列表 / 详情（按类型分组成员、系列专辑）；无能力时回退网页 | `features/series/`、`docs/api/series.md` | M3 | [x] 成员用类型角标代替分组标题 |
| 每日推荐：列表、详情、生成中 5 秒轮询、评价、换一个、另存为歌单、不再推荐 | `features/daily-mix/`、`032music-server/docs/daily-mix-design.md` | M3 | [x] |
| 播放记录 `/playback/history`（无限滚动、清空） | `features/playback-history/` | M3 | [x] |
| 正在播放标识：`/tracks/{id}/works`（`features.trackWorks`），旧服务端用会话映射 | `features/shared/components/playing-indicator/` | M3 | [x] |
| 「在网页中打开」（作品 / 系列编辑等） | 各页面 | M3 | [x] |

## 歌词

| 功能 | 参考 | 里程碑 | 完成 |
|------|------|--------|------|
| 获取 `/tracks/{id}/lyrics`（来源由服务端决定，显示 `sourceName` / 匹配信息） | `ctrl` `getLyrics`、`features/lyrics/api/lyrics-api.ts` | M0（逐行）/ M3 | [x] 显示 `sourceName` |
| 同步滚动、点击跳转、非同步歌词 | `features/lyrics/synchronized-lyrics.tsx` | M3 | [x] 手动滚动后 4 秒恢复居中 |
| 逐字卡拉 OK | `features/lyrics/synchronized-karaoke-lyrics.tsx`、`split-word-cue.ts` | M3 | - 不做：032 歌词只有逐行时间 |
| 搜索 `/lyrics/search`、预览 `/lyrics/remote/{source}/{id}`、确定 `PUT .../lyrics/source`、清除 `DELETE` | `features/lyrics/components/lyrics-search-form.tsx` | M3 | [x] |
| 翻译 `POST /tracks/{id}/lyrics/translation {targetLanguage}`；`translation_disabled` 时刷新能力位 | `features/lyrics/api/lyric-translate.ts` | M3 | [x] `translation_disabled` 后本次会话隐藏翻译 |
| 显示方式：bilingual / translated | `features/lyrics/utils/translation-display-modes.ts` | M3 | [x] 另有自动翻译、目标语言 |
| 偏移（仅当前会话） | - | M3 | [x] 按曲目 |
| 假名注音、罗马音（原在主进程 kuromoji） | `main/features/core/lyrics/furigana.ts` | - | 删除（M4；去掉 lindera 词典，包更小） |
| 中文行用中文字体 | 提交 ec619cb0 | M3 | [ ] |

## 系统集成

| 功能 | 参考 | 里程碑 | 完成 |
|------|------|--------|------|
| SMTC（Windows）/ MPRIS（Linux）/ macOS Now Playing、媒体键 | `main/features/linux/mpris.ts`、`main/features/core/player/media-keys.ts` | M1 | [x] `souvlaki` |
| 托盘、最小化到托盘 | `main/index.ts` | M1 | [x] Windows；关闭到托盘（最小化到托盘未做），Linux 托盘未做 |
| 单实例 | `main/index.ts` | M1 | [x] |
| 窗口内快捷键及自定义（录入 / 清除 / 恢复默认 / 冲突检测） | `renderer/store/settings.store.ts` `hotkeys` | M4 | [x] 设置页；空格播放 / 暂停，切歌、进度、音量、随机、循环、队列、搜索 |
| 全局快捷键 | `main/index.ts` | - | - 不做（2026-11-06） |
| 更新提示（读本仓库 GitHub Releases，只提示和给链接） | `main/fork-config.ts` | M4 | [x] |
| 设置导出 / 导入 | `renderer/components/export-import-settings-modal/` | 待定 | [ ] |

## Sonos 投放

参考 Android 客户端 [MusicBridge](https://github.com/lux032/MusicBridge) 的 032 队列模式。路径相对该仓库的
`app/src/main/java/com/lux032/musicbridge`。Electron 版没有这个功能，以 MusicBridge 行为为准。

| 功能 | 参考 | 里程碑 | 完成 |
|------|------|--------|------|
| SSDP 发现（每个 IPv4 网卡各发一次）+ ZoneGroupTopology 按编组协调器合并房间；手填音箱地址兜底 | `sonos/SonosDiscovery.kt`、032 服务端 `internal/upnp/discovery.go` | M4 | [x] `sonos` crate |
| 播放条"投放"按钮（独占旁边）：本机 / 房间列表、重新搜索；选择后记住，重启自动连回 | `state/AppStatePlaybackCoordinator.kt` `switchPlaybackTarget` | M4 | [x] |
| 整个队列写入音箱自己的队列（`x-rincon-queue`，`AddMultipleURIsToQueue` 每批 16），音箱自己连播，关掉本程序不影响；超过 2000 首按窗口推送 | `SonosController.replaceQueue`、`state/SonosQueue.kt` | M4 | [x] |
| 转码策略：Opus（Sonos 不能解码）转 MP3 320k / FLAC；> 48 kHz 无损转 MP3 / FLAC 48k / 原样；其他 Sonos 不支持的格式转 MP3；FLAC 先预热服务端缓存，失败回退 MP3，后续 FLAC 后台预热 | `state/SonosPlan.kt`、`music/Music032Warmup.kt`、`ui/settings/sections/SonosAdvancedSection.kt` | M4 | [x] 设置页"Sonos 投放" |
| 每秒轮询音箱状态，按 Track 序号 + URI 映射回队列；播放 / 暂停 / 进度跟随；进度条和歌词在两次轮询之间按时钟走 | `startPlaybackReporting` | M4 | [x] |
| 控制：播放 / 暂停、上一首 / 下一首 / 队列点播（队列内跳转）、拖动进度（实时 MP3 带 `offsetMs` 重开该条目）、单曲 / 列表循环（`SetPlayMode`）、随机（客户端顺序，重推队列）、音量（组音量，10 秒同步一次） | `togglePause`、`seekPlayback`、`updatePlaybackMode`、`applyVolumeChange` | M4 | [x] |
| 下一首播放 / 加到末尾 / 移除 / 拖动排序 / 清空 同步到音箱队列（单步编辑，其余重推） | `enqueueIntoSonosQueue`、032 网页 `cast.js` `diffQueue` | M4 | [x] |
| 032 播放上报：沿用同一个上报器；本机与 Sonos 互切同一首不重新计数；音箱自己切歌按完成 / 替换结束；单曲循环绕回算新的一次；从未出声报错误 | README「032 播放会话」 | M4 | [x] |
| 接管音箱上已在播放的 032 队列（本程序上次推送、网页播放器推送），按 mediaToken 或地址识别；循环模式一并同步 | `adoptSonosQueueIfPlaying`、`resolveAdoptableQueue` | M4 | [x] |
| 睡眠定时：分钟到点暂停音箱；"本曲 / 本专辑结束后"在音箱切到下一首时停止 | `SleepTimerController` | M4 | [x] |
| 服务器地址是 localhost / 127.0.0.1 时，推给音箱的 URL 换成本机局域网地址 | 032 服务端 `castBaseURL` | M4 | [x] |
| Plex、非队列的单曲推送模式（`playTrack`） | MusicBridge Plex 兼容 | - | - 不做（D4 只支持 032） |
| 普通 DLNA 渲染器 | 032 服务端 `internal/upnp/generic.go` | - | - 不做 |

## 外观与本地化

| 功能 | 参考 | 里程碑 | 完成 |
|------|------|--------|------|
| 032 Light / 032 Dark，跟随系统，默认强调色 `#5b47e0` / `#8d7dff` | `shared/themes/music032-*`、`shared/themes/music032/` | M4 | [x] 默认深色 |
| 侧边栏：主页、专辑、曲目、艺人、作品、系列、播放记录、歌单、收藏（无流派 / 文件夹 / 电台） | `features/sidebar/`、`use-music032-sidebar-items.ts` | M2 | [x] |
| zh-Hans / zh-Hant / en | `i18n/locales/` | - | - 只做简体中文（2026-11-06） |

## 待定（默认不做，见 PLAN.md）

远程控制、Discord RPC、可视化、自定义主题、普通 DLNA（Sonos 已做，见上）、标签编辑器、其余语言、Web 播放器引擎（Rust 版只用 mpv）。

## 明确不做

其他服务端（Jellyfin / Navidrome / Subsonic / Plex）、jukebox、网络电台、Web / Docker 构建、移动端布局。
