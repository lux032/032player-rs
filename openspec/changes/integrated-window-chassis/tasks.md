# Tasks

## 1. 主题 Token、图标与顶壳组件基础

- [x] 1.1 在 `crates/app/ui/theme.slint` 补充方案 D 外壳语义颜色 token（`win-border`、`win-border-inactive`、`win-btn-hover`、`win-close-hover`、`search-focus-border`，同步覆盖 032 Dark 与 032 Light），并在 `crates/app/ui/controls.slint` 的 `IconKind` 新增 `win-min`、`win-max`、`win-restore`、`panel-right` 矢量图标，通过 `cargo check -p app-ui` 验证 Slint 编译通过。
- [x] 1.2 在 `crates/app/ui/` 新增或拆分顶壳组件（`38px` `TitleBar` 与居中 `TopSearchBar`）：实现左侧品牌标 + 返回按钮 `<` + 轻量路径提示（全屏态切为收起按钮 + 正在播放提示）、居中全局搜索输入框（未聚焦显示占位与 `search-shortcut`、聚焦显示高亮边框与一键清空按钮）、右侧右栏开关图标 + 竖线 + 最小化/最大化还原/关闭按钮、底层 4px 位移防误触拖动与双击最大化 `TouchArea`，以及未登录/窄屏/全屏/失焦/最大化视觉态，通过 `cargo check -p app-ui` 验证。

## 2. 一体外壳画布布局与全页面接入

- [x] 2.1 重构 `crates/app/ui/app.slint` 的 `AppWindow`：开启 `no-frame: true` 与 `resize-border-width: 6px`，以 `Theme.chrome` 作为连续外框并按非最大化/最大化切换外框边线；将主内容区（`Theme.bg`）、右侧停靠/浮层「正在播放」栏（`Theme.panel`）、`FullScreenPlayer`（`Theme.stage`）与 `LoginView`（`Theme.bg`）统一包裹为 `10px` 圆角、`1px solid Theme.border`、彼此间距 `8px` 的内嵌画布；同步扣除 `top-bar-h` 更新 `np-art-side`、右栏与全屏高度，将宽屏侧栏/右栏宽度拖拽手柄安置在 `8px` 缝隙内，并让模态弹窗遮罩从顶壳下方开始（不挡住窗口操控），通过 `cargo check -p app-ui` 验证。
- [x] 2.2 清理冗余入口与画布边缘收口：移除 `app.slint` 中侧栏顶部旧「搜索曲库」按钮、详情页左上角悬浮圆形返回按钮和窄屏主区右上角悬浮歌词入口；移除 `crates/app/ui/player_bar.slint` 顶部的 `1px` 分隔线避免与圆角画布底边形成双线；移除 `crates/app/ui/fullscreen.slint` 内左上角悬浮收起圆钮（改由顶壳左侧承担）；移除 `crates/app/ui/search.slint` 顶部的重复 `SearchField` 与「搜索」按钮并保留标题与分类 `Tab`，通过 `cargo check -p app-ui` 验证。

## 3. Rust 窗口控制、搜索焦点与 Windows 11 DWM 同步

- [x] 3.1 新增 `crates/app/src/window_chrome.rs` 并在 `crates/app/src/main.rs` 与 `crates/app/src/system.rs` 接入：在 `BackendSelector::with_winit_window_attributes_hook` 中预置 Windows 11 圆角、阴影与初始外框色；通过 `WinitWindowAccessor::on_winit_window_event` 实时同步 `window-active`、`window-maximized`、Windows 11 `CornerPreference` 与 `DWMWA_BORDER_COLOR`（跟随主题与系统主题变化）；接线 `window-drag`、`window-minimize`、`window-toggle-maximize` 与 `window-close`（复用 `system.rs` 共用关闭/托盘隐藏与 `record_window_now` 逻辑），全程零 `unsafe`，通过 `cargo check -p app` 与（若工具链已装）`cargo check -p app --target x86_64-pc-windows-msvc` 验证。
- [x] 3.2 更新 `crates/app/src/ui/wire.rs`、`crates/app/src/ui/shortcuts.rs` 与 `crates/app/src/ui/search.rs`：实现 `focus-top-search()` 供 `Ctrl+K` 快捷键与点击顶壳搜索槽直接聚焦顶壳输入；输入防抖/回车自动导航至搜索页并提交查询；清空按钮重置搜索态；`Escape` 失焦交还 `shortcut-scope` 避免抢占空格播放/暂停；主题切换时触发 Windows DWM 外框颜色刷新，通过 `cargo clippy --workspace --all-targets -- -D warnings` 验证。

## 4. 快照回归、文档同步与全工作区检查

- [x] 4.1 同步更新 `tools/ui-snapshot` 适配新的 `AppWindow` 属性与布局坐标（含歌词返回等点击坐标回归测试），补充渲染一体外壳画布的关键视图（主页暗/亮、专辑列表含右栏收起、专辑详情、搜索聚焦、全屏舞台、窄屏 1280 与登录页）并人工检查生成 PNG 与 `prototypes/window-frame/chassis.html` 原型一致（`cargo run --manifest-path tools/ui-snapshot/Cargo.toml -- /tmp`）。
- [x] 4.2 更新 `docs/TASKS.md`（汇总本功能的 **[Win]** 实机验证清单：Win11 暗/亮模式外框色与 1px 顶边阴影表现、100%/150% DPI 缩放、顶壳拖动/Aero Snap/双击最大化、最大化不盖任务栏、八向缩放、关闭到托盘与重启恢复几何位置、顶壳搜索与快捷键焦点交还），并运行全套工作区检查确认全部通过：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace --exclude player --exclude app --exclude app-ui`。

## 后续跟进

- 在 Windows 11 实机（含 150% 缩放与暗/亮主题）逐项完成 `docs/TASKS.md` 汇总的 **[Win]** 验收清单；若浅色模式下 `set_undecorated_shadow(true)` 导致顶部 1px 细线明显，按 `design.md` 风险缓解项现场微调并复验。
- 验证完成后运行 `/openspec-verify-change` 与 `/openspec-archive-change` 归档 `integrated-window-chassis`。
