//! 窗口创建参数与运行入口。
//!
//! **为什么用无边框窗口（`with_decorations(false)`）**：
//! 1. 参考截图里 CrystalStudio 用的是 Avalonia 自绘标题栏（深金色标题栏 + 右侧 `− □ ×`），
//!    OS 原生标题栏会被画成系统样式，与截图不符；
//! 2. 任务书 §七-2 要求自截图**恰好 1500×950**。带原生边框时 `GetWindowRect` 会比客户区大
//!    一圈（约 16×39），截出来就不是 1500×950 了。
//!
//! 代价：窗口不可通过边框缩放；已用「标题栏拖拽 = `ViewportCommand::StartDrag`」保住移动能力。
//! 最小尺寸仍按任务书设置（1100×720），由 `with_min_inner_size` 交给 winit。
//!
//! # 本单变化：**先装载、再开窗**
//! [`MireditApp::new`] 会先把地图与图库装好（失败也照样开窗，只是状态栏/画布显示原因）。
//! 这样窗口标题、标题栏、状态栏、画布**第一帧就是真实数据**，不会先闪一帧"未命名 256×256"。

use crate::app::MireditApp;
use crate::cli::Cli;
use crate::metrics as m;

/// 构造 eframe 的原生窗口选项（标题用真实文档标题）。
pub fn native_options(title: &str) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size([m::WINDOW_W, m::WINDOW_H])
            .with_min_inner_size([m::WINDOW_MIN_W, m::WINDOW_MIN_H])
            // 自绘标题栏：关掉 OS 装饰，客户区即 1500×950
            .with_decorations(false)
            .with_resizable(true),
        ..Default::default()
    }
}

/// 启动 GUI（阻塞直到窗口关闭）。
///
/// 中文字体在**创建 App 的那一帧之前**装好（`CreationContext` 里就有 `egui_ctx`），
/// 否则首帧会先按 egui 内置字体排一遍，屏幕上会闪一帧方框。
pub fn run(cli: &Cli) -> eframe::Result {
    let app = MireditApp::new(cli);
    let title = app.window_title().to_string();
    eframe::run_native(
        &title,
        native_options(&title),
        Box::new(move |cc| {
            crate::fonts::install(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
}
