# Proposal

## Why

当前 `032player-rs` 使用系统默认窗口标题栏与外框，Windows 11 深色模式下顶部呈现突兀的浅灰黑横条（`#202020`）与灰白外框，与播放器自身的 v3 Editorial 三栏暗紫黑/浅雾配色和结构完全割裂，既影响整体质感，也浪费了一整行垂直空间。需要将窗口外框与顶部控制融为播放器外壳的一部分，使边框本身和界面元素保持统一的色彩、圆角、层次与交互语言。

## What Changes

- 采用经原型确认的 **方案 D（一体外壳画布 / Chrome 外框）** 重构窗口外壳：
  - 将 `AppWindow` 切换为无系统标题栏框架（`no-frame: true` + `resize-border-width`），由顶壳（`38px`）、左侧导航栏和底部播放条共同构成连续的 `Theme.chrome` 外壳；
  - 主内容区（`Theme.bg`）与右侧「正在播放」栏（`Theme.panel`）作为带 `10px` 圆角、`1px solid Theme.border` 描边、彼此间距 `8px` 的内嵌画布呈现；全屏播放器（`Theme.stage`）与登录页（`Theme.bg`）同样作为单块圆角画布嵌入外壳中，保证全场景视觉统一。
- 收敛顶壳与周边入口，消除冗余与位置冲突：
  - 顶壳左侧放置 `032player` 品牌标志、返回按钮 `<`（替代原详情页漂浮在封面左上角的圆形返回钮）以及轻量路径提示（如 `专辑 / <名>`、`艺人 / <名>`；全屏舞台下切换为收起按钮与正在播放提示）；
  - 顶壳居中放置全局搜索框（默认显示 `搜索曲库、艺人、作品...` 与快捷键标签 `Ctrl+K`），点击或按快捷键直接聚焦输入并进入搜索结果页；同步移除左侧栏顶部旧「搜索曲库」按钮以及 `SearchPage` 内重复的第二套大搜索输入框；
  - 顶壳右侧在 `- □ ×` 窗口按钮左侧放置「正在播放栏开关」图标按钮（以 `1px` 竖线分隔），宽屏与窄屏统一使用，并取代窄屏下漂浮在主区右上角的圆形歌词入口；
  - 窗口按钮、外框边线与顶壳元素响应 032 Dark / 032 Light 主题切换、最大化状态（外圆角与外阴影归零、图标切为还原）以及失焦降噪状态。
- Windows 底层默认采用 **纯 Slint + winit（零 `unsafe`）** 实现窗口交互与原生融合：
  - 利用 `winit 0.30` 的安全扩展（`WindowExtWindows`）在 Windows 11 上同步原生圆角（`CornerPreference::Round` / 最大化时 `DoNotRound`）、外框着色（`set_border_color` 跟随 `Theme.border`）与阴影（`set_undecorated_shadow`），通过 `drag_window()` 与顶壳双击支持系统拖动、Aero Snap 边缘贴边和最大化切换，并通过 `on_winit_window_event` 同步聚焦与最大化状态；
  - Linux / macOS 复用 Slint 无边框窗口拖拽与八向边缘缩放能力。
- 明确非目标：本期不引入 Win32 `WM_NCCALCSIZE` / `WM_NCHITTEST` 消息钩子（因此不含「鼠标悬停最大化按钮弹出 Win11 分屏布局菜单」），保持零 `unsafe` 与低风险落地；如后续有强需求再独立增强。

## Capabilities

### New Capabilities

- `window-frame`: 桌面客户端的一体外壳画布窗口框架、自定义融合顶壳（品牌标、返回与路径提示、全局搜索收敛、右栏开关、窗口控制按钮）、拖拽/缩放/最大化/失焦行为、全场景（常规三栏、右栏收起、全屏舞台、窄屏 1280 浮层、登录页）适配及 Windows 11 DWM 外框同步。

### Modified Capabilities

<!-- 无既有 spec 需求变更 -->

## Impact

- `crates/app/ui/`：`app.slint`（`AppWindow` 无边框外壳、38px 顶壳组件、圆角画布容器、移除旧悬浮返回钮与窄屏悬浮右栏钮及侧栏搜索按钮）、`theme.slint`（补充外框/窗口按钮/搜索聚焦态颜色 token）、`controls.slint`（新增窗口最小化/最大化/还原/右栏切换图标）、`search.slint`（移除页内重复搜索输入框，与顶壳全局搜索输入联动）、`fullscreen.slint`（配合顶壳收起键与舞台画布边框）。
- `crates/app/src/`：`main.rs`（`BackendSelector` 窗口属性钩子与初始 Windows DWM 属性设置）、`system.rs` / 新增 `window_chrome.rs`（最小化、最大化切换、关闭复用既有托盘逻辑、拖动、聚焦/最大化/主题变化时同步 DWM 外框颜色与圆角）、`ui/wire.rs`、`ui/shortcuts.rs`（`Ctrl+K` 聚焦顶壳搜索框）、`ui/actions.rs`（同步顶壳路径提示文本）。
- `tools/ui-snapshot/`：同步新增 `AppWindow` 属性/假数据，并补充方案 D 常规 / 亮色 / 搜索 / 全屏 / 窄屏 / 登录态截图回归校验。
- 依赖与合规：不新增第三方 crate，不引入 `unsafe` 代码，继续满足 `unsafe_code = "deny"` 以及无 libmpv / 无显示环境下的 `cargo test` 要求。
