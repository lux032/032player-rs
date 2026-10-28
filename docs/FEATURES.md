# 功能对齐清单

以 Electron 版 `/workspace/032musicplayer` 中 **032 服务端可用** 的功能为准。
路径相对 `032musicplayer/src`，`ctrl` = `renderer/api/music032/music032-controller.ts`。
行为细节、能力位、已知限制以 `032musicplayer/MUSIC032_CHANGES.md` 为准。

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
| 能力位：`features.browse` / `lyrics` / `lyricsOnline` / `lyricsTranslation` / `playlistArtwork` / `playlistItemOps` / `trackWorks` / `dailyMixes` / `playbackContext` / series，`playback.clientKinds`，`limits.maxPageSize`（500） | `shared/api/music032/music032-types.ts` `M032Capabilities` | M0 | [x] |

## 播放

| 功能 | 参考 | 里程碑 | 完成 |
|------|------|--------|------|
| 原始文件流 `/api/v1/tracks/{id}/stream`（bit-perfect 前提） | `ctrl` `getStreamUrl` | M0 | [x] |
| 可选转码 `/api/v1/tracks/{id}/transcode.{format}` | 同上 | M2 | [ ] |
| mpv：播放 / 暂停 / seek / 音量 / 静音 | `main/features/core/player/index.ts` | M0 | [ ] |
| 输出设备选择、独占模式（播放中可切换） | `main/features/core/player/audio-exclusive.ts` | M0 | [x] 设置页 |
| 设置持久化：音量、输出设备、独占模式、歌词面板开关（`settings.json`） | `renderer/store/settings.store.ts` | M1 | [x] |
| 缓冲状态（`paused-for-cache`）、载入失败（`end-file` reason=error） | `main/features/core/player/index.ts` `createMpv` | M1 | [x] |
| 无缝播放（预加载下一首） | `renderer/features/player/audio-player/mpv-player.tsx` | M1 | [x] mpv `append` |
| 淡入淡出（如保留） | `mpv-player.tsx` fade | 待定 | [ ] |
| 队列：替换、下一首播放、加到末尾、移除、清空 | `renderer/store/player.store.ts`、`features/player/context/` | M1 | [x] |
| 队列拖动排序 | 同上（`dnd-kit`） | M2 | [ ] 队列面板暂用上一个 / 下一个 |
| 随机、单曲 / 列表循环 | 同上 | M1 | [x] |
| 续播位置（`Song.resumePositionMs`） | `useResumePosition` | M3 | [ ] |
| 睡眠定时 | `features/player/components/sleep-timer-button.tsx` | M1 | [x] |
| 随机播放全部（曲目视图"随机全部"；browse 走 `sort=random`，旧服务端抽样窗口） | `features/player/components/shuffle-all-modal.tsx`、`ctrl` `getRandomSongList` | M2 | [x] |
| Auto DJ（如保留） | `features/player/auto-dj/` | 待定 | [ ] |
| 播放条：封面、标题、艺人、专辑、音质（`FLAC / 24bit / 48kHz`）、进度、音量 | `features/player/components/playerbar.tsx`、`renderer/utils/audio-quality.ts` | M0 / M4 | [x] 播放条收藏按钮未做 |
| 全屏播放器（原图封面不带 `size`，`size=0` 会被拒） | `features/player/components/full-screen-player.tsx`、`ctrl` `getImageUrl` | M4 | [ ] |

## 播放上报（apiRevision 3）

| 功能 | 参考 | 里程碑 | 完成 |
|------|------|--------|------|
| 会话状态机：start / pause / resume / seek / buffering / heartbeat / end，seq，重试 | `renderer/api/music032/music032-playback-session.ts` | M0 | [x] |
| 播放器信号适配层 | `renderer/api/music032/music032-playback-adapter.ts` | M1 | [x] |
| 接线：`POST /api/v1/playback/events` | `features/player/hooks/use-music032-playback-reporter.ts` | M1 | [x] |
| 会话恢复快照、resume_invalid 持久化（应用重启可续） | session 文件头注释 | M1 | [x] |
| 重启恢复队列和播放位置（停止状态，播放时从原位置继续） | `player.store.ts` `partialize`、`use-queue-restore.ts` | M1 | [x] |
| clientKind：capabilities `playback.clientKinds` 含 `desktop` 时报 `desktop`，会话内固定 | `MUSIC032_CHANGES.md` 播放上报接线 | M1 | [x] |
| 每日推荐 context 只在全新 start 发送；加入队列的曲目不带 context | `MUSIC032_CHANGES.md` 每日推荐 | M3 | [ ] |
| 退出时尽力发送 `client_closed` | 同上 | M1 | [x] |
| 测试：移植 `scripts/music032/playback-reporter.test.mjs` | - | M0 | [x] |

## 曲库

| 功能 | 参考 | 里程碑 | 完成 |
|------|------|--------|------|
| 专辑列表：`/browse/albums`（sort、order、yearFrom/yearTo、random+seed）；无 browse 回退 `/albums` | `ctrl` `getAlbumList`、`MUSIC032_CHANGES.md` 列表接口 | M0 / M2 | [x] |
| 曲目列表：`/browse/tracks`（sort 含 album/added/duration/random） | `ctrl` `getSongList` | M0 / M2 | [x] |
| 艺人列表：`/browse/artists` / `/artists` | `ctrl` `getArtistList` | M2 | [x] |
| random seed：按查询（不含分页）记忆，offset 0 换新 seed，最多记 50 个 | `ctrl` | M2 | [x] |
| 各列表支持的排序项（browse-only 项在无 browse 时隐藏） | `features/shared/components/list-sort-by-dropdown.tsx` `MUSIC032` | M2 | [x] |
| 筛选：仅收藏（`/favorites/*`）、年份区间（`yearFrom`/`yearTo`）；艺人筛选未做 | `features/shared/components/list-filters.tsx` | M2 | [x] 部分 |
| 专辑详情（`trackCount` 超出嵌入曲目时用 `/tracks?album=` 分页补齐）、专辑音质汇总 | `ctrl` `getAlbumDetail` | M2 | [x] |
| 艺人详情（发行 + 热门歌曲）、作品集 | `ctrl` `getAlbumArtistDetail` / `artistDiscography` / `getTopSongs` | M2 | [x] 详情内联 |
| 收藏：专辑 / 曲目 / 艺人（`PUT`/`DELETE .../favorite`） | `ctrl` `createFavorite` / `deleteFavorite` | M2 | [x] 曲目与专辑；艺人入口未做 |
| 收藏页走 `/favorites/albums|tracks`，客户端排序筛选 | `ctrl` `favoriteAlbums` / `favoriteSongs` | M2 | [x] |
| 搜索 | `ctrl` `search` | M2 | [x] |
| 相似歌曲 `/tracks/{id}/similar` | `ctrl` `similarTracks`、`features/similar-songs/` | M2 | [x] 曲目右键 |
| 封面 / 艺人图 / 作品海报（2:3）URL，按尺寸请求 | `ctrl` `getImageUrl` | M0 | [x] |

## 歌单

| 功能 | 参考 | 里程碑 | 完成 |
|------|------|--------|------|
| 列表（`durationMillis`，未知时隐藏时长）、详情、曲目 | `ctrl` `getPlaylist*` | M2 | [x] |
| 新建、改名、删除 | `ctrl` `createPlaylist` / `updatePlaylist` / `deletePlaylist` | M2 | [x] |
| 添加 `items/append`（曲目 / 整张专辑）、移除 `items/remove`、排序 `order` | `docs/api/playlist-operations.md` | M2 | [x] 替换未做（整表替换） |
| 封面上传 / 删除（`features.playlistArtwork`） | `ctrl` `uploadPlaylistImage` | M2 | [x] 上传走文件对话框 |
| 同一首歌只出现一次（服务端跳过重复） | `MUSIC032_CHANGES.md` | - | - |

## 032 专属页面

| 功能 | 参考 | 里程碑 | 完成 |
|------|------|--------|------|
| 032 主页：`/home`、`/home/works`、`/home/songs`、`/home/chronicle`、`/home/spotlight/random`、最近播放；404/501 回退普通主页 | `features/home/music032/`、`032music-server/docs/api/home.md` | M3 | [ ] |
| 作品列表（搜索、类型、排序、无限滚动）/ 详情（关联专辑、曲目用途、系列横幅） | `features/works/`、`032music-server/docs/api/works.md` | M3 | [ ] |
| 系列列表 / 详情（按类型分组成员、系列专辑）；无能力时回退网页 | `features/series/`、`docs/api/series.md` | M3 | [ ] |
| 每日推荐：列表、详情、生成中 5 秒轮询、评价、换一个、另存为歌单、不再推荐 | `features/daily-mix/`、`032music-server/docs/daily-mix-design.md` | M3 | [ ] |
| 播放记录 `/playback/history`（无限滚动、清空） | `features/playback-history/` | M3 | [ ] |
| 正在播放标识：`/tracks/{id}/works`（`features.trackWorks`），旧服务端用会话映射 | `features/shared/components/playing-indicator/` | M3 | [ ] |
| 「在网页中打开」（作品 / 系列编辑等） | 各页面 | M3 | [ ] |

## 歌词

| 功能 | 参考 | 里程碑 | 完成 |
|------|------|--------|------|
| 获取 `/tracks/{id}/lyrics`（来源由服务端决定，显示 `sourceName` / 匹配信息） | `ctrl` `getLyrics`、`features/lyrics/api/lyrics-api.ts` | M0（逐行）/ M3 | [ ] |
| 同步滚动、点击跳转、非同步歌词 | `features/lyrics/synchronized-lyrics.tsx` | M3 | [ ] |
| 逐字卡拉 OK | `features/lyrics/synchronized-karaoke-lyrics.tsx`、`split-word-cue.ts` | M3 | [ ] |
| 搜索 `/lyrics/search`、预览 `/lyrics/remote/{source}/{id}`、确定 `PUT .../lyrics/source`、清除 `DELETE` | `features/lyrics/components/lyrics-search-form.tsx` | M3 | [ ] |
| 翻译 `POST /tracks/{id}/lyrics/translation {targetLanguage}`；`translation_disabled` 时刷新能力位 | `features/lyrics/api/lyric-translate.ts` | M3 | [ ] |
| 显示方式：bilingual / translated | `features/lyrics/utils/translation-display-modes.ts` | M3 | [ ] |
| 偏移（仅当前会话） | - | M3 | [ ] |
| 假名注音、罗马音（原在主进程 kuromoji） | `main/features/core/lyrics/furigana.ts` | M3 | [ ] |
| 中文行用中文字体 | 提交 ec619cb0 | M3 | [ ] |

## 系统集成

| 功能 | 参考 | 里程碑 | 完成 |
|------|------|--------|------|
| SMTC（Windows）/ MPRIS（Linux）/ macOS Now Playing、媒体键 | `main/features/linux/mpris.ts`、`main/features/core/player/media-keys.ts` | M1 | [x] `souvlaki` |
| 托盘、最小化到托盘 | `main/index.ts` | M1 | [x] Windows；关闭到托盘（最小化到托盘未做），Linux 托盘未做 |
| 单实例 | `main/index.ts` | M1 | [x] |
| 全局快捷键 | `main/index.ts` | M4 | [ ] |
| 自动更新（读本仓库 GitHub Releases） | `main/fork-config.ts` | M4 | [ ] |
| 设置导出 / 导入 | `renderer/components/export-import-settings-modal/` | 待定 | [ ] |

## 外观与本地化

| 功能 | 参考 | 里程碑 | 完成 |
|------|------|--------|------|
| 032 Light / 032 Dark，跟随系统，默认强调色 `#5b47e0` / `#8d7dff` | `shared/themes/music032-*`、`shared/themes/music032/` | M4 | [ ] |
| 侧边栏：专辑、曲目、艺人、歌单、收藏（M2）；主页、作品、系列、播放记录在 M3（无流派 / 文件夹 / 电台） | `features/sidebar/`、`use-music032-sidebar-items.ts` | M2 / M3 | [x] M2 部分 |
| zh-Hans / zh-Hant / en | `i18n/locales/` | M4 | [ ] |

## 待定（默认不做，见 PLAN.md）

远程控制、Discord RPC、可视化、自定义主题、DLNA、标签编辑器、其余语言、Web 播放器引擎（Rust 版只用 mpv）。

## 明确不做

其他服务端（Jellyfin / Navidrome / Subsonic / Plex）、jukebox、网络电台、Web / Docker 构建、移动端布局。
