# 设计文档

## 背景

参见 `proposal.md` 了解背景与目标，参见 `specs/window-frame/spec.md` 了解对外行为规范。已通过多页面视觉原型（本地 `prototypes/window-frame/index.html` 与 `prototypes/window-frame/chassis.html`）确认最终方向为 **方案 D（一体外壳画布 / Chrome 外框）**。

当前 `032player-rs` 的关键代码结构与约束：

- `Cargo.toml`：使用 `slint 1.18.1`（已启用 `unstable-winit-030`、`raw-window-handle-06`），`winit 0.30.13` 为实际窗口后端；工作区全局设置 `unsafe_code = "deny"`。
- `crates/app/ui/app.slint`：`AppWindow` 目前无 `no-frame` 设置，依赖操作系统原生标题栏；内部为三栏布局（`sidebar` + `pages` + `right` 覆盖/停靠右栏）加底部 `PlayerBar`，另含 `FullScreenPlayer`、`LoginView`、右键菜单与模态弹窗。
- `crates/app/src/main.rs`：通过 `slint::BackendSelector::new().select()?` 启动后端，窗口显示后调用 `keep_on_screen(&window)` 校验顶部可触达并调用 `system::install(&window, ...)` 注册托盘、SMTC 和 `on_close_requested`。
- `crates/app/ui/search.slint` 与 `crates/app/src/ui/search.rs`：`SearchState` 为全局状态（含 `query-input`、`active-query`、`tab` 及 `search(string)` 回调），目前由左侧栏顶部按钮或快捷键 `Ctrl+K` 跳转至 `page == 6`，再在 `SearchPage` 顶部渲染独立的 340px `SearchField`。
- `tools/ui-snapshot`：基于 `slint::platform::software_renderer::MinimalSoftwareWindow` 直接加载 `crates/app/ui/app.slint` 渲染 PNG 回归图，不依赖 winit 窗口对象。

## 目标与非目标

**目标：**

- 在 `crates/app` 内以零 `unsafe` 方式落地 **方案 D（一体外壳画布）**，覆盖主页、列表页、详情页、搜索页、设置页、全屏播放器、窄屏 1280 及登录页全部场景，同时支持 032 Dark / 032 Light 与跟随系统主题。
- 收敛全局搜索、返回导航与右栏开关到 `38px` 顶壳，彻底移除重复搜索框与悬浮遮挡按钮，同时保持现有快捷键、搜索防抖、历史滚动恢复、拖拽调整栏宽、关闭到托盘与单实例激活全部行为不退化。
- 在 Windows 11 上通过 `winit 0.30` 的 `WindowExtWindows` 安全 API 同步原生圆角、外框着色与阴影，并在 Windows 10 / Linux / macOS 与 `tools/ui-snapshot` 上平滑工作。

**非目标：**

- 本期不引入 Win32 `SetWindowSubclass` / `WM_NCCALCSIZE` / `WM_NCHITTEST` 消息钩子（因此不实现「鼠标悬停自绘最大化按钮弹出 Win11 Snap Layouts 分屏菜单」）；保持 `unsafe_code = "deny"` 零豁免。
- 不改动 `crates/api`、`crates/core`、`crates/player`、`crates/session`、`crates/sonos`、`crates/lyrics`、`crates/asio` 或 `crates/os` 的公共契约。
- 不增加尚无历史栈支撑的「前进」按钮，也不新增设置项开关（直接以一体外壳替代旧系统标题栏）。

## 关键决策

### 决策 1：采用纯 Slint + winit（零 `unsafe`）实现无边框窗口与 Windows 11 DWM 同步

经实查 `i-slint-backend-winit 1.18.1` 与 `winit 0.30.13` 源码：

- Slint `Window` 声明 `no-frame: true; resize-border-width: 6px;` 后，`i-slint-backend-winit` 会将 winit 窗口设为 `decorations(false)`，并自带八方向边缘光标切换与 `drag_resize_window(dir)` 缩放逻辑。
- `winit 0.30` 在 Windows 上对 `decorations(false)` 仍保留 `WS_CAPTION | WS_THICKFRAME` 并通过内置 `WM_NCCALCSIZE` 将最大化窗口裁切到 `MonitorFromRect -> rcWork`（不会盖住任务栏）。
- 通过 `crates/app/src/window_chrome.rs`（新增模块）封装所有 winit 交互与 Windows DWM 同步：
  - 启动前通过 `slint::BackendSelector::new().with_winit_window_attributes_hook(...)` 在 Windows 上预置 `with_corner_preference(CornerPreference::Round)`、`with_undecorated_shadow(true)` 与初始 `with_border_color(...)`，避免窗口首帧闪烁。
  - 窗口显示后通过 `WinitWindowAccessor::on_winit_window_event` 监听 `WindowEvent::Focused(focused)`、`WindowEvent::Resized(_)`、`WindowEvent::ScaleFactorChanged` 与 `WindowEvent::ThemeChanged`，实时回写 `AppWindow` 的 `window-active` 与 `window-maximized` 属性，并在 Windows 上同步更新 `CornerPreference`（最大化时 `DoNotRound`，常规时 `Round`）和 `DWMWA_BORDER_COLOR`（跟随当前有效主题：`Theme.border` 暗 `#25252e` / 亮 `#dedee5`，失焦可保持同色或柔和降对比）。
  - 顶壳拖拽区回调 `window-drag()` -> `winit_window.drag_window()`；最小化 `window-minimize()` -> `set_minimized(true)`；最大化切换 `window-toggle-maximize()` -> `set_maximized(!is_maximized())`；关闭 `window-close()` -> 复用 `system.rs` 抽出的共用关闭处理函数（先 `ui::record_window_now()`，再根据 `close_to_tray && tray_available` 隐藏或 `quit_event_loop()`）。

**替代方案**：在 `crates/os` 写 `unsafe` Win32 子类化钩子拦截 `WM_NCCALCSIZE` / `WM_NCHITTEST` 以支持悬停最大化弹出 Win11 Snap Layouts。暂不采用的原因：需维护高 DPI 下的非客户区命中测试表并处理与 `winit` 内置 `WM_NCCALCSIZE` 的交互冲突，复杂度与回归风险显著高于收益；且当前架构留出了独立扩展边界，后续如有强需求可在 `crates/os` 单独补齐。

### 决策 2：一体外壳画布几何结构与图层分层（`crates/app/ui/app.slint`）

将 `AppWindow` 根容器重构为：

```text
AppWindow (no-frame: true, resize-border-width: 6px, background: Theme.chrome)
+-- outer-frame (1px inner/outer border when !window-maximized, 0px when maximized)
    +-- VerticalLayout
        +-- TitleBar (height: 38px, background: Theme.chrome, z-order above modals)
        |   +-- Left:  [16px icon] ["032player" when !small] [| < crumb when logged-in & !full]
        |   |          (or [| v "正在播放 · <title>"] when full-open)
        |   +-- Mid:   [TopSearchBar max 380px when logged-in & !full] (surrounded by drag area)
        |   +-- Right: [PanelToggleBtn | when logged-in & !full] [Min] [Max/Restore] [Close]
        +-- body-slot (vertical-stretch: 1, background: Theme.chrome)
            +-- LoginView / restoring: padded 8px left/right/bottom -> rounded 10px Theme.bg pane
            +-- Logged-in normal:
            |   +-- HorizontalLayout (padding-right: 8px, spacing: 8px)
            |   |   +-- sidebar (Theme.chrome, no right divider line; top search removed)
            |   |   +-- main-pane (rounded 10px, 1px Theme.border, clip: true, background: Theme.bg)
            |   |   |   +-- pages (existing pages; floating back & narrow floating open-panel removed)
            |   |   +-- docked right spacer (width: panel-docked ? panel-w : 0px)
            |   +-- PlayerBar (height: bar-h, background: Theme.chrome, border-top removed)
            +-- Right panel (`right`, y = top-bar-h, height = body-h - bar-h):
            |   +-- rounded 10px, 1px Theme.border, clip: true, background: Theme.panel
            |   +-- wide/medium: docked at right edge (8px from window right); resize handle sits inside the 8px gap!
            |   +-- narrow (<= 1280): overlay drawer at right edge (8px from window right) with drop-shadow
            +-- FullScreenPlayer (y = top-bar-h, x = 8px, width = root.width - 16px, height = body-h - bar-h):
            |   +-- rounded 10px, 1px Theme.border, clip: true, background: Theme.stage
            |   +-- inner top-left floating close circle removed (moved to TitleBar left)
            +-- Modals & ContextMenuOverlay:
                +-- Modals cover `y = top-bar-h..root.height` so TitleBar remains draggable & closable
```

关键联动修正：

- `np-art-side` 与 `FullScreenPlayer` 高度计算必须扣除 `top-bar-h`（`38px`），避免右栏封面或全屏三栏纵向被压缩溢出。
- 侧栏与主画布之间、主画布与右栏画布之间的 `8px` 缝隙同时承担宽度拖拽热区（`wide` 宽屏下 `col-resize`），悬停时在缝隙正中浮现 `2px` `Theme.accent` 柔和提示条，不再侵占内容区像素。
- 路径提示（`crumb`）直接由 Slint 响应式属性从 `root.full-open`、`root.page`、`root.detail.kind`、`root.detail.title`、`root.np-title` 派生，避免在 Rust 多处路由回调里重复拼接同步。
- 顶壳拖拽与双击实现：在 `TitleBar` 底层铺满一个 `TouchArea`（置于左右中交互控件之下），单击按下调用 `window-drag()`，双击调用 `window-toggle-maximize()`；左右中各按钮和搜索输入框置于上层自然拦截点击，不触发误拖。注意 Slint 的 `TouchArea` 在收到 `down` 时若立刻调用 `drag_window()`，系统移动循环会接管鼠标导致 `double-clicked` 收不到 - 因此需采用**轻量移动阈值（如 4px）或原生双事件兼容策略**：
  - 方案：在拖拽 `TouchArea` 记录 `press-x / press-y` 与 `moved` 标志，仅当按住左键且位移 `>= 4px` 时触发一次 `window-drag()`；纯点击抬起不触发 `drag_window()`，从而让 Slint 的 `double-clicked` 在双击顶壳空白区时稳定触发 `window-toggle-maximize()`！这是一个非常关键的跨平台防坑细节。

### 决策 3：全局搜索框收进顶壳并消除双搜索框

- 在 `TitleBar` 中部放置 `TopSearchBar`（高度 `28px`、圆角 `8px`、最大宽度 `380px`，绑定 `SearchState.query-input`）：
  - 未聚焦且为空时显示搜索图标 + 占位文案 `搜索曲库、艺人、作品...` + 快捷键徽章 `root.search-shortcut`（如 `Ctrl+K`）。
  - 聚焦或非空时显示高亮边框（`Theme.accent` 55% alpha + 外晕）、`TextInput` 编辑区以及右侧清空按钮 `x`。
  - 交互流程：
    1. 点击搜索槽或按下 `search` 快捷键（`shortcuts.rs` 改为调用 `w.invoke_focus_top_search()`）-> 聚焦顶壳 `TextInput`；
    2. 编辑文本（300 ms 防抖）或按 Enter -> 若当前不在搜索页（`page != 6`）自动导航到 `Route::Page(6)`，并触发 `SearchState.search(SearchState.query-input)`；
    3. 点清空按钮 `x` -> 清空 `SearchState.query-input` 并调用 `SearchState.search("")`（重置搜索结果为空态，保持聚焦方便继续输入）；
    4. 在顶壳搜索框按 `Escape` -> 失焦交还给 `shortcut-scope`；若当前处于搜索页且无输入内容或再次按 `Escape` 且 `can-go-back` 为真，可返回上一页。
- `crates/app/ui/search.slint`：移除 `SearchPage` 顶部冗余的 `SearchField` 与「搜索」按钮，保留查询结果标题（`“xxx”的搜索结果` / 空态引导 `输入关键词搜索歌手、专辑、歌曲或作曲家`）与分类 `Tab` 栏，使搜索页更清爽并腾出更多首屏结果空间。
- `crates/app/ui/app.slint`：移除 `sidebar` 顶部的旧「搜索曲库」按钮，侧栏顶部直接从「我的媒体库」起步。

### 决策 4：颜色 Token 与图标扩展（`theme.slint` / `controls.slint`）

- `theme.slint` 在 `Theme` 新增语义 token（同时提供 Dark / Light 两套值）：
  - `win-border`（暗 `#2c2c38` / 亮 `#cbcbe0`）、`win-border-inactive`（暗 `#1d1d25` / 亮 `#dcdce6`）
  - `win-btn-hover`（暗 `#ffffff14` / 亮 `#11111311`）、`win-close-hover`（暗 `#d93848` / 亮 `#c42b44`）
  - `search-focus-border`（暗 `#8d7dff8c` / 亮 `#5b47e073`）
- `controls.slint` 的 `IconKind` 新增 4 个轻量几何图标（沿用统一线条风格）：`win-min`、`win-max`、`win-restore`、`panel-right`（右栏开关）。
- `PlayerBar`（`player_bar.slint`）：移除顶部 `1px` 分隔线（因为上方圆角画布本身已有底边框线，再画一条会变双线）。

### 决策 5：`tools/ui-snapshot` 兼容与视觉回归

- `tools/ui-snapshot` 使用 `MinimalSoftwareWindow`，不运行 winit 事件循环；因此 `AppWindow` 新增的 `window-active`（默认 `true`）与 `window-maximized`（默认 `false`）及窗口控制回调在 `ui-snapshot` 中开箱即用，无空指针或未绑定问题。
- 扩展 `tools/ui-snapshot` 覆盖新外壳下的关键画面：主页（暗/亮、2300/1600/1280）、专辑列表（右栏展开/收起）、专辑详情（顶壳返回与路径提示）、搜索页（顶壳搜索框聚焦）、全屏舞台与登录页，用于每次改 `.slint` 后肉眼对比 `prototypes/window-frame/chassis.html` 原型。

## 风险与权衡

- **[风险] Windows 11 `set_undecorated_shadow(true)` 在浅色主题下窗口最顶缘可能露出一根 1px 黑线（winit `event_loop.rs:1194` 的 `rgrc[0].top += 1` hack）** -> 缓解措施：常规态在 `window_chrome.rs` 中将 `set_border_color` 设为当前主题 `Theme.border`（最大化时关闭阴影并清除 DWM 边框着色），并列为 **[Win]** 必验项；若实机浅色模式下顶部 1px 线明显刺眼，可在 Windows 11 上测试关闭 `set_undecorated_shadow` 仅靠 `CornerPreference::Round` + `DWMWA_BORDER_COLOR`，或按亮/暗主题切换。
- **[风险] 顶壳按住拖动 4px 阈值与双击最大化手感** -> 缓解措施：4px 逻辑位移在 100%-150% DPI 下约 4-6 物理像素，并使用 `drag-area` 局部 400ms 双击判定（避免 Slint 全局点击计数把邻近控件点击 + 空白区点击误判为双击），同时非最大化态为 `4px` 客户端缩放边带预留内缩、最大化态清零 `resize-border-width` 保证贴角点击不吞事件。
- **[风险] 顶壳 `TextInput` 抢占全局快捷键（如空格暂停/播放）与其他编辑器截获 `Ctrl+K`** -> 缓解措施：在 `shortcut-scope` 捕获阶段放行带修饰键的全局搜索快捷键（如 `Ctrl+K`），点击侧栏、主画布、播放条、右栏或顶壳空白区以及按 `Escape` / 切页 / 进全屏 / 开弹窗时立即交还焦点给 `shortcut-scope`，且 `Escape` 返回过滤按键重复事件防止长按连退。
- **[风险] macOS 上 `winit::Window::drag_resize_window` 返回 `NotSupported`** -> 缓解措施：本仓库主力平台为 Windows、其次 Linux、macOS 尽力而为；在 `#[cfg(target_os = "macos")]` 下关闭客户端 `resize-border-width`（`disable-client-resize = true`）避免边带吞点击，后续如需完善 macOS 原生缩放可接入透明标题栏 + 原生边框。
- **[风险] Linux 平铺窗口管理器（如 Sway / Hyprland / i3）用户可能习惯无边框或无窗口按钮** -> 缓解措施：Slint `no-frame: true` 在 Wayland/X11 下表现一致且圆角画布内边距克制（8px），在非最大化与平铺满屏态下都不破坏布局；若后续收到 Linux 用户反馈，可再考虑设置项切换系统标题栏。
- **[风险] `AppWindow` 移除 `sidebar` 搜索钮与 `SearchPage` 内搜索框后，既有快捷键/截图测试断言受影响** -> 缓解措施：在 `tasks.md` 中明确同步更新 `tools/ui-snapshot` 坐标与回归用例，确保 `cargo test`、`clippy` 和 `ui-snapshot` 全绿。
