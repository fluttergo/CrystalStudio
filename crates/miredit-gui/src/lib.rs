//! `miredit-gui` —— Crystal Studio 桌面版 GUI（地图编辑器，仿上游运行期界面）。
//!
//! 产品显示名固定 `Crystal Studio`（2026-11-06 用户裁定）；`miredit-gui` 只是 crate 名。
//!
//! **本单范围**：把中间那块空画布换成**真实地图渲染** —— 打开 `.map`、从图库取帧、
//! 按客户端规则合成、平移/缩放、图层开关、单元格信息与状态栏联动。
//!
//! ## 2026-11-05 追加：完整菜单系统 + 深色模式
//!
//! 菜单栏那 5 个静态标签升级成**真能用的菜单**（官方 5 菜单 39 项，逐项三态：已实现 /
//! 需要先开图 / 未实现+原因），并新增「视图 → 深色模式」与设置持久化
//! （`%LOCALAPPDATA%\miredit\settings.json`）。细节见 [`menu`] 模块文档与
//! [`theme`] 的两套色板。
//!
//! ## 唯一视觉基准
//! `layout/refs/crystalstudio-main-runtime-1500x950.png`（1500×950 真机运行截图）
//! + `layout/crystalstudio-ui.json`（几何/颜色/图标路径真值）。
//!
//! ⚠ 明确不采信 `App.axaml` 的深色 `#14171D`：那是 XAML 声明值，
//! 运行期 `ThemeService` 会按系统 accent 重算，**上屏是浅色 + 金色强调**。
//!
//! ## 模块划分
//! ### 数据与状态
//! - [`mapdoc`]   地图 + 图库装载（失败一律显式报错，绝不空白）；
//!   **地图解析走 `miredit_core::Document::load`**（九种格式 + 宽松尾块），
//!   宿主不再自己 `mir2_map::Map::parse`（那只认 12/14/36 三档）
//! - [`banner`]   **常驻告警黄条**：`Document::edit_warning()` 的原文钉在画布顶部
//!   （未解尾块 / 格式未承诺编辑语义；不是 toast、不自动消失）
//! - [`palette`]  资源面板：真资源库枚举 / 按需解码缩略图 / 虚拟滚动 / 图号搜索 / 点选设笔刷
//! - [`stamp_panel`] 资源面板「分类」页签 = 印章浏览器：9,159 枚 `.X` 按官方中文分类分组 /
//!   名称搜索 / 46×32 缩略图（LRU 缓存 256）/ 分页 96 / 选中
//! - [`stamp_place`] **印章放置**：`.X → StampCellInput` 的唯一适配 / 读盘 / 待放置态 /
//!   `register_stamp` → `place_stamp_by_id` 落位链 / `Alt` 悬停轮廓的几何
//! - [`selection_ops`] **矩形选区 + 剪贴板**：选择工具（工具列第 5 个）/ `Ctrl+C/X/V` /
//!   `Delete` / `Ctrl+Shift+V` 填充（**无官方依据，已记档**）/ 状态栏「选区·剪贴板」文案
//! - [`canvas_overlay`] 画布叠加层：金色虚线选区框 + 半透明填充 / 印章待放置轮廓（**只画轮廓**）
//! - [`grid`]      **单元格网格**（视图 → 单元格网格 / `G` / `--cell-grid`）：世界格 48×32 的
//!   1px 边界线，只画视口内可见范围，`scale < 0.5` 不画（默认关，偏离官方的理由见模块文档 §五）
//! - [`edit`]     编辑闭环：吸管 / 笔刷（N×N）/ 橡皮擦（N×N）/ **填充（Bucket）** / 撤销 / 重做 / 脏标记（纯数据层，可单测）
//! - [`save`]     保存：**原子替换**写回（写同目录临时文件 → 校验 → `rename`）
//! - [`canvas_view`] 画布视口：世界像素 ↔ 屏幕像素、**重渲调度与纹理缓存**、缩放/平移
//! - [`cellinfo`] 单元格检查器取数（纯函数，可在无窗口测试里逐字段核对）
//! - [`screen_text`] 运行期文案装配（模板 + 真实数据，模板全在 [`ui_text`]）
//! - [`cli`]      命令行开关解析
//! - [`editrun`]  无窗编辑驱动器（`--edit-*` / `--save`，验收取证用，不开窗）
//! - [`selfcheck`] 截图落盘 / 同视点分块比对 / 单元格探针（取证用，不参与日常绘制）
//! - `acceptance_tests`（`#[cfg(test)]`）验收门 2/3/4/5/6 的**逐字节**取证：
//!   真实 `0.map` 副本上放真实印章 / 撤销逐字节 / 复制粘贴 3×3 / 剪切 3×3 /
//!   Type5 拒绝（注入验证，见该模块文档 §三）；
//!   另有 `formats.rs` 的**九格式验收**：全仓 1,158 张开图零失败 / 未改即存零差异 /
//!   九种格式合成样本全部能开且原样存回 / Type5 真装载路径 / 67 张带尾块图的黄条
//!
//! ### 界面骨架（原有）
//! - [`ui_text`]  **界面文案总表**（全量中文，代码里不许再出现界面字面量）
//! - [`fonts`]   中文字体加载（系统单面 TTF → egui 首选字体；拿不到即 exit(2)）
//! - [`text_fit`] 文字标签的「位置 + 裁剪盒」（静态 [`text_fit::Label`] / 动态 [`text_fit::DynLabel`]）
//! - [`theme`]   颜色（**两套色板**：浅色 [`theme::LIGHT`] / 深色 [`theme::DARK`]，
//!   绘制处一律取 [`theme::ui`]，切换只改一个开关）
//! - [`metrics`] 几何常量（每条注明取自截图的坐标或 JSON 路径）
//! - [`layout`]  把常量算成一组矩形
//! - [`icon_path`] 极小的 SVG path `d` → 顶点解析器（含曲线细分与耳切三角化）
//! - [`icons`]   10 个工具按钮的矢量图标
//! - [`paint`]   绘制原语
//! - [`panel`]   面板通用骨架
//! - [`titlebar`] / [`menubar`] / [`statusbar`] / [`canvas`] / [`dock_left`] / [`dock_right`]
//! - [`menu`]    **完整菜单系统**（5 菜单 39 项 / 三态 / 弹层 / 快捷键浮层 / 深色模式 /
//!   设置持久化），子模块见该模块文档
//! - [`app`]     应用主体（布局编排 + 交互 + 重渲调度 + 自动化收尾）
//! - [`window`]  窗口创建参数
//!
//! ## 依赖纪律
//! 只用 `egui` / `eframe` 与三个同 workspace 的库（`mir2-map` / `mir2-assets` / `miredit-render`）；
//! **没有引入任何新的图形库**。PNG 读写复用 `miredit_render::png_io`（它本来就带着 `png`），
//! 截图的像素来源是 egui 自己的 `ViewportCommand::Screenshot`。

pub mod app;
pub mod banner;
pub mod canvas;
pub mod canvas_overlay;
pub mod canvas_view;
pub mod cellinfo;
pub mod cli;
pub mod dock_left;
pub mod dock_right;
pub mod edit;
pub mod editrun;
pub mod export;
pub mod fonts;
pub mod grid;
pub mod icon_path;
pub mod icons;
pub mod layout;
pub mod maplist;
pub mod mapdoc;
pub mod menubar;
pub mod menu;
pub mod metrics;
pub mod paint;
pub mod palette;
pub mod panel;
pub mod save;
pub mod screen_text;
pub mod selection_ops;
pub mod selfcheck;
pub mod stamp_panel;
pub mod stamp_place;
pub mod stamp_probe;
pub mod statusbar;
pub mod text_fit;
pub mod theme;
pub mod titlebar;
pub mod ui_text;
pub mod window;

#[cfg(test)]
mod acceptance_tests;
#[cfg(test)]
mod maplist_tests;
#[cfg(test)]
mod stamp_panel_tests;
