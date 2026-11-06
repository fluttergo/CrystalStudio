//! 导出：**选区 PNG**（文件 → 导出选区(E)…）与**小地图 PNG**（文件 → 导出小地图(M)…）。
//!
//! # 一、为什么需要「分块渲染」，而不是一次 `try_render_region_scaled_with`
//!
//! `miredit-render` 的两条护栏（`types.rs:32,35,48,95`，**本 crate 一个字都不改**）：
//!
//! | 常量 | 值 | 管什么 |
//! |---|---|---|
//! | `MAX_SIDE` | 32768 px | 1× 合成画布单边 |
//! | `MAX_PIXELS` | 256,000,000 px | 1× 合成画布总像素 |
//! | `SCALED_MAX_BYTES` | 64 MiB | **目标图**字节数（缩放档） |
//! | `INTERMEDIATE_MAX_BYTES` | 128 MiB | **1× 中间图**字节数（缩放档） |
//!
//! 而 `try_render_region_scaled_with` 的流程是「**先按 1× 合成整幅，再重采样**」
//! （`scale.rs` 模块文档 §二）⇒ 整图 1× 中间图是绕不过去的。`0.map` 实测 **700×700 格**：
//!
//! ```text
//! 1× 中间图 = 700*48 × 700*32 = 33600 × 22400
//!           33600 > MAX_SIDE(32768)         ⇒ DimensionOverflow
//!           33600*22400 = 752,640,000 > MAX_PIXELS(256M) ⇒ DimensionOverflow
//! @0.25 目标 = 8400 × 5600 × 4 = 188,160,000 B > SCALED_MAX_BYTES(64 MiB)
//! ```
//!
//! ⇒ **任何档位单次调用都渲不出 700×700 整图**。所以本模块把窗口切成若干块，
//! 每块单独调 [`miredit_render::try_render_region_scaled_with`]（**采样 100% 由 render crate 做，
//! 本文件一行滤镜都没有**），再把各块按整数像素偏移拼回整幅。
//!
//! # 二、拼接为什么是**逐字节精确**的（两条依据）
//!
//! **① 缩放核按整块对齐**：`盒式_流式` 把输出像素 `j` 映到源像素 `[j*sw/dw, (j+1)*sw/dw)`；
//! 本模块的块边长恒为 `TILE_CELLS`（64 或 32，都能被缩小档的 4 / 2 整除），
//! 而世界格像素 `48 / 32` 也都能被 4 整除 ⇒ 每块内第 `j` 个输出像素覆盖的源矩形
//! **恰好**是整幅图里第 `(块起点/倍率 + j)` 个输出像素覆盖的同一块源像素。
//! ⇒ 缩小档（`0.25` / `0.5`）拼出来与「整图一次渲」逐字节相同。
//! `One` 档更直接：`try_render_region_scaled_with(.., One)` 内部 `return compose::render(..)`
//! （`scale.rs:174-176`），压根不进重采样。
//!
//! **② 精灵可能跨块**：`compose::render` 的三层循环**只遍历窗口内的格**
//! （`compose.rs:115/149/176` 的 `for dx in 0..cells_w`，没有任何外扩边距），
//! 而单帧可能比一格大得多 —— 地表 96×64（2×2 格）、表层帧宽可任意。
//! ⇒ 若直接按块切，块边界上会**丢像素**（块内最后一列的宽帧被画布裁掉）
//! 或**少画**（块外第一列的大帧本该溢进来）。
//! 本模块的处理：**每块各算一个扩边格数**，向四周多渲那么多格，再裁回块本身。
//! 扩边不是拍脑袋的常量，而是**按窗口里每一格的真实帧尺寸算出来的**
//! （见 [`tile_margins`]）；上限也由 render crate 的护栏反算（见 [`margin_cap`]）。
//!
//! # 三、原子写（不许留半个文件）
//!
//! PNG 编码复用 [`miredit_render::png_io::write_rgba_png`]（与 `miredit-web` 的
//! `/api/tiles` **同一个编码器**，见 `miredit-web/src/routes/mod.rs:78-108`），
//! 但那个 API 只有「写文件」形态、没有「返回 `Vec<u8>`」形态 ⇒ 先写到系统临时目录、
//! 读回字节、立刻删掉，再把字节交给 [`crate::save::save_atomic`]
//! （`tmp → write_all → sync_all → 读回逐字节核对 → rename → 尽力 fsync 父目录`）。
//! ⇒ 失败时目标路径上**不会出现半个文件**，临时文件也由 `save_atomic` 自己清掉。
//!
//! # 四、选档 + 两段式（小地图）
//!
//! **@acfbbf8a 回源订正（2026-11-06）**：上游新增小地图尺寸面板，官方尺寸口径已可得
//! ⇒ 小地图**缺省**改走 [`export_minimap_sized`] + [`MinimapSize::Default`]
//! （`W*12 × H*8`，官方 `MinimapExporter.cs:20`）；三档预设与显式像素见
//! [`MinimapPreset`] / [`MinimapSize`]，官方 `ValidSize` 的 Rust 等价换算见
//! [`MINIMAP_MAX_PIXELS`]，官方 16 级 mip 到本仓 4 档的有据映射见 [`pick_official_scale`]。
//!
//! 本节下面的「长边两段式」是**本仓兼容链**（`--minimap-max-side`），
//! 由 [`export_minimap`] 原样保留，护栏与逐字节输出都不变；它**不再是官方缺省口径**。
//!
//! 见 [`pick_minimap_scale`]：从大到小试 `One → Half → Quarter`，取**第一个**同时
//! 满足「两边 ≤ [`MINIMAP_MAX_SIDE`]」且「整幅输出过得了 [`MAX_EXPORT_SIDE`] /
//! [`MAX_EXPORT_BYTES`]」的档位。`Two` 永远不选 —— 小地图放大没有意义。
//!
//! 最小档仍超目标长边时，**不报错、也不超限交付**，而是走**第二段面积平均缩放**
//! （[`downscale_to_fit`] → [`fit_ratio`]），把整幅缩到目标长边以内，
//! 且**长宽比与 1× 渲染逐位相等**（判据 `w*ch*32 == h*cw*48`）。
//!
//! ## 为什么第二段用 `compare::resize_to`，而不是 `resample::盒式_流式`
//!
//! 两条都是 render crate 自己的面积平均实现，但**只有前者是公开的**：
//!
//! | 入口 | 可见性 | 能不能从本 crate 调 |
//! |---|---|---|
//! | `resample::盒式_流式` | `lib.rs:162` `mod resample;` + `resample.rs:165` `pub(crate)` | **不能**（私有模块 + crate 内可见）|
//! | `compare::resize_to` | `lib.rs:166` `pub mod compare;` + `compare.rs:154` `pub fn` | **能** |
//!
//! 两者语义同类（面积平均、确定性、不预乘）；内存上 `resize_to` 更省
//! （额外只有每个目标像素一个 `[u64; 4]` 累加器，峰值 ≈ 源图 + 目标图），
//! 而 `resample::缩放`（整幅预乘版）在 `8400×5600` 上要 `47 MP × 32 B ≈ 1.5 GB` —— 那条不能碰。
//! ⇒ 本模块固定用 `compare::resize_to`，**不**去动 `miredit-render` 的可见性
//! （为一个已有等价公开入口再加一条 API 表面，只会多一个无人消费的契约）。
//!
//! # 五、诚实边界
//!
//! - 整幅输出超 [`MAX_EXPORT_BYTES`] / [`MAX_EXPORT_SIDE`] ⇒ **报错**，不截断、不降级；
//! - 官方尺寸链（[`export_minimap_sized`]）的 `0` / 超大宽高 ⇒ **中文原因报错**，
//!   不静默截断、不偷偷降级到别的档位（见 [`valid_minimap_size`]）；
//! - 官方预设要求的档位过不了本仓更严的内存护栏 ⇒ **中文原因报错**，不静默换档；
//! - 帧大到某一块需要的外扩超过 [`margin_cap`] ⇒ **报错**（那时拼接不再精确）；
//! - 没有选区 / 没有地图 / 没有图库 ⇒ 各自的**中文原因**，一个文件都不写。
//!
//! # 六、门态（`doors_open`）**透传**（2026-11-05 补：闭门边界收口）
//!
//! 本模块曾经**只出闭门图**（走 `try_render_region_scaled`，它内部写死
//! `RenderOptions::default()`）⇒ 「画布开着门、导出是闭门」，同一张图两副样子，
//! 用户会以为导出坏了。现在门态**从调用方一路透到底**：
//!
//! ```text
//! 菜单「视图 → 开启门」/ --open-doors   →  宿主真源（app.doors_open）
//!                                          ↓
//!                          export_selection(.., doors_open) / export_minimap(.., doors_open)
//!                                          ↓
//!                    render_tiled(.., doors_open)  →  try_render_region_scaled_with(
//!                                                        .., RenderOptions::with_doors_open(doors_open))
//! ```
//!
//! 缺省 **`false`**（= 闭门，`RenderOptions::default()`）⇒ 与接线前**逐字节相同**；
//! 判据：`0.map` 的 `(106,316)` 起 3×3 导出仍是 `32,842 B / sha256 e1acd638…4335`。
//!
//! ## 为什么门态还必须进 [`tile_margins`]（否则分块拼接会破）
//!
//! 门开合**只改前景层帧号**（`帧号 += door_offset & 0x7F`，见 `miredit-render` 的
//! `RenderOptions` 文档 §三/§四），而**门开帧与闭门帧的尺寸可以不同**（实测 `0.map`
//! 就有这种格）。本模块的扩边是按「每一格真实帧的像素矩形」算出来的（§二 ②）——
//! 若扩边仍按**闭门**帧尺寸算，开门态下大出好几格的门开帧就会跨过块边界却拿不到扩边
//! ⇒ 块边界上丢像素/少画，「分块拼接 == 整图一次渲」当场不成立。
//! ⇒ [`tile_margins`] 与 [`render_tiled`] 收**同一个** `doors_open`，两边算的是同一批帧。
//!
//! ## 门偏移 / 帧号**只有一份实现**（2026-11-05 收口）
//!
//! 本模块**不再自带** `帧号` / `门偏移`：两者直接调 [`miredit_render::帧号`] /
//! [`miredit_render::门偏移`]（render 侧已从 `pub(crate)` 提成 `pub` 并再导出）。
//! 起因就是这里曾各抄一份（跨 crate 取不到），抄件注释写着「改口径两边一起改」——
//! **两份必然漂，而漂移的症状正是「画布开门、导出闭门」**。
//! 判据：`grep -rn "0x7F" crates/miredit-gui/src/export.rs` 只剩注释，一行实现都没有。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use miredit_core::Selection;
use miredit_render::{
    png_io::write_rgba_png, try_render_region_scaled_with, CollisionOverlay, LayerMask,
    MapLighting, RenderOptions, RenderScale, Viewport, CELL_H, CELL_W, INTERMEDIATE_MAX_BYTES,
    SCALED_MAX_BYTES, 帧号, 门偏移,
};
use mir2_assets::{LibRef, SpriteSource};
use mir2_map::Map;

use crate::mapdoc::DocState;
use crate::save;

/// 分块边长（格）。**必须能被缩小档的 4 / 2 整除**（模块文档 §二 ①），
/// 且单块的 1× 中间图 / 目标图都要留在 render crate 的护栏内：
///
/// | 档位 | 块边长 | 1× 中间图 | 目标图 |
/// |---|---|---|---|
/// | `One` / `Half` / `Quarter` | 64 格 | 3072×2048×4 = 24 MB ≤ 128 MiB ✅ | `Half` 1536×1024 ✅ |
/// | `Two` | 32 格 | 1536×1024×4 = 6 MB ✅ | 3072×2048×4 = 24 MB ≤ 64 MiB ✅ |
///
/// `Two` 必须更小块：64 格 `@2×` 的目标是 `6144×4096×4 = 100 MB > 64 MiB` ⇒ 会被拒。
pub const TILE_CELLS: u32 = 64;

/// `Two` 档专用的块边长（理由见 [`TILE_CELLS`] 的表格）。
pub const TILE_CELLS_TWO: u32 = 32;

/// 扩边格数的**搜索上界**（真上限由 render crate 的护栏反算，见 [`margin_cap`]）。
///
/// 为什么不是一个写死的"安全阀"：实测被写死的数打脸两次 ——
/// 第一版按 8 格设，`0.map` 的 3×3 选区里就有 **13 格**外溢的物件（一帧高 ≈ 448 px）；
/// 改成 24 格后整图导出又撞到 **27 格**（第 17 块）。
/// 正确的做法是**让护栏自己算上限**：只要「块 + 2×扩边」还过得了
/// `try_render_region_scaled_with` 的检查就允许；过不去才报错（不硬渲、不静默拼缺像素的图）。
///
/// 48 只是**搜索起点**：实际有效上限对 `Quarter` / `Half` 是 **41 格**、对 `One` 是 48 格、
/// 对 `Two`（块 32）只有 5 格 —— 因为前三者还要受 1× 中间图 / 目标图字节上限约束。
pub const MAX_MARGIN_CELLS: u32 = 48;

/// 该档位下**允许的最大扩边格数** = 最大的那个仍过护栏的 `m`（用 [`fit_tile`] 反算）。
///
/// `TILE_CELLS = 64` 时的实际值（算式见 [`fit_tile`] 的护栏）：
///
/// | 档位 | 卡在哪一条 | 最大 `m` |
/// |---|---|---|
/// | `Quarter` / `Half` | 1× 中间图 `(64+2m)²×1536 ≤ 128 MiB` | **41** |
/// | `One` | 单边 `(64+2m)×48 ≤ 32768` | **48**（顶到搜索起点）|
/// | `Two`（块 32） | 目标图 `(32+2m)×96 ≤ 4096` | **5** |
fn margin_cap(tile: u32, scale: RenderScale) -> u32 {
    for m in (0..=MAX_MARGIN_CELLS).rev() {
        if fit_tile(tile, m, scale).is_ok() {
            return m;
        }
    }
    0
}

/// 小地图**本仓兼容链**的目标长边（像素）。选档时要求输出两边都不超过它；
/// 最小档仍超 ⇒ 再走一次**面积平均缩放**（[`downscale_to_fit`]）把整幅缩到它以内。
///
/// ⚠ **原判断「原版不可得 ⇒ 自定值」已失效，@acfbbf8a 回源订正（2026-11-06）**：
/// 官方尺寸口径见 `I:\cs-new\MinimapExporter.cs` + `MinimapExportWindow.cs`：
///
/// 1. **默认输出尺寸**：`width = document.Width * 12`、`height = document.Height * 8`
///    （`MinimapExporter.cs:20`），**不是 2048 长边**；
/// 2. **三档预设**：以 `Width*48` / `Height*32` 为基准除以 `{4, 2, 1}`，选中预设时
///    `checked(document.Width * 48 / divisor)` / `checked(document.Height * 32 / divisor)`
///    （`MinimapExportWindow.cs:36-38`）；
/// 3. **合法尺寸判据**：`w > 0 && h > 0 && (long)w*h <= Array.MaxLength / 4`
///    （`MinimapExporter.cs:15-16`）；本仓等价换算、并叠加更严的既有护栏，见 [`MINIMAP_MAX_PIXELS`]；
/// 4. **精度/换算公式**：`scale = min(width/(W*48.0), height/(H*32.0))`、
///    `level = scale >= 1 ? 0 : clamp((int)floor(log2(1/scale)), 0, 16)`
///    （`MinimapExporter.cs:47-48`）。**官方用 16 级 overview mip，本仓只有 4 档**
///    ⇒ [`pick_official_scale`] 按「最接近且不越界」做**有据映射，不是自定值**。
///
/// ⇒ 2048 仅保留给本仓旧开关 `--minimap-max-side` 的兼容链，**官方缺省不再经过它**；
/// 新 CLI / 菜单默认走 [`export_minimap_sized`] + [`MinimapSize::Default`]。
/// 改这个数**不会**改变「采样由谁做」：第一段仍是 `try_render_region_scaled_with`，
/// 第二段是 render crate 的面积平均缩放 —— 本模块一行滤镜都没有。
pub const MINIMAP_MAX_SIDE: u32 = 2048;

/// `--minimap-max-side` 允许的最小值。
///
/// ⚠ **原判断「自定值」已失效，@acfbbf8a 回源订正（2026-11-06）**：
/// 官方 `MinimapExporter.ValidSize` 的下界条件就是 `width > 0 && height > 0`；
/// **64 不再是官方判据**，只保留为本仓旧「长边两段式」开关的局部下限：
/// 该路径要求保持 `cw*48 : ch*32` 的精确整数比，64 能确保 `fit_ratio` 不退化成 0 宽/0 高
/// （最简比 `3:2` 时 k=21 ⇒ 63×42）。官方尺寸链 [`export_minimap_sized`] 不读这个数，
/// 其合法性由 [`MINIMAP_MAX_PIXELS`] + 本仓更严的内存护栏共同裁决。
pub const MINIMAP_MAX_SIDE_MIN: u32 = 64;

/// 官方 `ValidSize(w,h)` 的**本仓 Rust 等价上限**（像素数，不是字节数）。
///
/// 官方（.NET）：`w > 0 && h > 0 && (long)w*h <= Array.MaxLength / 4`
/// （`I:\cs-new\MinimapExporter.cs:15-16`）。`Array.MaxLength = 0x7FFFFFC7`，
/// `/4 = 536,870,897` 个 `u32`，对应 **RGBA8 恰为 `2,147,483,588 B = 2 GiB - 4 B`**；
/// 本仓 `ExportImage.rgba: Vec<u8>` 按字节分配，`isize::MAX` 在 64 位是
/// `0x7FFF_FFFF_FFFF_FFFF`（9,223,372,036,854,775,807 B），令
/// `n = (isize::MAX as u64) / 4 = 2,305,843,009,213,693,951`
/// ⇒ `n * 4 <= isize::MAX`，这就是 Rust 侧「一次 `Vec<u8>` 能表达的最大 RGBA 像素数」。
/// **等价换算，不是抄 .NET 数字**。
///
/// ⚠ 本仓既有 `MAX_EXPORT_SIDE = 32768` 与 `MAX_EXPORT_BYTES = 512 MiB` 更严
/// （`32768² = 1,073,741,824` 像素，但字节护栏先卡到 `134,217,728` 像素），
/// 故 [`valid_minimap_size`] 在官方上限之外还会保留这些护栏；**没有放宽内存护栏**。
pub const MINIMAP_MAX_PIXELS: u64 = (isize::MAX as u64) / 4;

/// 官方三档小地图预设（基准 `W*48 × H*32`，除以 4 / 2 / 1）。
///
/// @acfbbf8a `MinimapExportWindow.cs:21,36-38`：
/// `Small (¼)` / `Medium (½)` / `Large (1:1)`，选中时写入
/// `checked(document.Width * 48 / divisor)` 与 `checked(document.Height * 32 / divisor)`。
/// 本枚举的 `divisor()` 就是那三个除数，不另造倍率。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MinimapPreset {
    /// `Small (¼)`：`W*48/4 × H*32/4` = `W*12 × H*8`（与官方默认同尺寸）。
    Small,
    /// `Medium (½)`：`W*48/2 × H*32/2` = `W*24 × H*16`。
    Medium,
    /// `Large (1:1)`：`W*48 × H*32`。
    Large,
}

impl MinimapPreset {
    /// 官方 `new[] { 4, 2, 1 }[Preset.SelectedIndex]` 的除数。
    pub const fn divisor(self) -> u32 {
        match self {
            MinimapPreset::Small => 4,
            MinimapPreset::Medium => 2,
            MinimapPreset::Large => 1,
        }
    }

    /// CLI 名与 `--minimap-preset` 的取值逐字对应；不是 UI 显示名。
    pub const fn key(self) -> &'static str {
        match self {
            MinimapPreset::Small => "small",
            MinimapPreset::Medium => "medium",
            MinimapPreset::Large => "large",
        }
    }

    /// `small|medium|large` → 枚举；其它写法一律 `None`（由调用方报中文原因）。
    pub fn parse(s: &str) -> Option<Self> {
        let key = s.trim();
        [Self::Small, Self::Medium, Self::Large]
            .into_iter()
            .find(|p| p.key().eq_ignore_ascii_case(key))
    }
}

/// 一次小地图导出的**尺寸口径**。
///
/// `Default` / `Preset` / `Explicit` 分别对应官方 `MinimapExporter.ExportAsync` 的
/// 「两个可空参数都缺省」/「尺寸面板预设」/「手动填宽高」。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MinimapSize {
    /// 官方默认：`W*12 × H*8`（`MinimapExporter.cs:20`）。
    #[default]
    Default,
    /// 官方三档预设：`W*48/divisor × H*32/divisor`。
    Preset(MinimapPreset),
    /// 官方尺寸面板里手填的宽 / 高（两个都必填；缺一个报中文原因）。
    Explicit { width: u32, height: u32 },
}

impl MinimapSize {
    /// 按地图格数算出**精确像素尺寸**（宽、高）。
    ///
    /// 官方预设用 C# `checked(...)`；本仓用 `u64` 算完再 `u32::try_from`，
    /// 溢出返回中文原因，不静默截断。
    pub fn resolve(self, cells_w: u32, cells_h: u32) -> Result<(u32, u32), String> {
        let (width, height) = match self {
            Self::Default => (
                u64::from(cells_w) * 12,
                u64::from(cells_h) * 8,
            ),
            Self::Preset(p) => {
                let d = u64::from(p.divisor());
                (
                    u64::from(cells_w) * u64::from(CELL_W) / d,
                    u64::from(cells_h) * u64::from(CELL_H) / d,
                )
            }
            Self::Explicit { width, height } => (u64::from(width), u64::from(height)),
        };
        let to_u32 = |v: u64, side: &str| {
            u32::try_from(v).map_err(|_| {
                format!("小地图导出尺寸溢出：{side} = {v} px 超过 u32 上限（官方 checked 语义）")
            })
        };
        Ok((to_u32(width, "宽")?, to_u32(height, "高")?))
    }
}

/// 单次导出的**输出**单边上限（像素）。超过 ⇒ 报错（不截断）。
pub const MAX_EXPORT_SIDE: u32 = 32_768;

/// 单次导出的**输出**字节上限（512 MiB）。
///
/// 与 render crate 的 `SCALED_MAX_BYTES`（64 MiB，管**单次调用**的目标图）不是一回事：
/// 本模块把整幅切成多块，每块都远小于 64 MiB，但**拼起来的那张整图**由这里封顶。
/// `0.map @Quarter` = `8400×5600×4 = 188 MB` ⇒ 在限内；`700×700 @Half` = 752 MB ⇒ 报错。
pub const MAX_EXPORT_BYTES: u64 = 512 * 1024 * 1024;

/// 一帧的像素尺寸（只为算外溢，不带像素数据）。
type FrameSize = (u32, u32);

/// 帧尺寸缓存：同一个 `(库, 帧号)` 只问一次图库。
///
/// 为什么必须有：算外溢要扫窗口里**每一格的三层帧**（700×700 = 147 万次），
/// 而 `SpriteSource::frame` 的契约是**每次返回一份新解码的像素**（`frames.rs:5-9`）
/// ⇒ 不缓存的话为了拿一个宽高会把同一帧解码上千遍。
#[derive(Default)]
struct SizeCache<'a> {
    src: Option<&'a dyn SpriteSource>,
    slots: HashMap<(LibRef, u32), Option<FrameSize>>,
}

impl<'a> SizeCache<'a> {
    fn new(src: &'a dyn SpriteSource) -> Self {
        SizeCache {
            src: Some(src),
            slots: HashMap::new(),
        }
    }

    /// 取 `(库, 帧号)` 的帧尺寸；取不到返回 `None`（**缺帧不是外溢**，按 0 算）。
    fn size(&mut self, lib: LibRef, index: u32) -> Option<FrameSize> {
        if let Some(hit) = self.slots.get(&(lib, index)) {
            return *hit;
        }
        let got = self
            .src
            .and_then(|s| s.frame(lib, index))
            .map(|f| (f.width, f.height));
        self.slots.insert((lib, index), got);
        got
    }
}

/// 一张导出的 RGBA8 位图（行优先，`rgba.len() == width*height*4`）。
///
/// 为什么不直接复用 [`miredit_render::Rendered`]：它标了 `#[non_exhaustive]`
/// （`types.rs:457`），外部 crate **不能**用结构体字面量构造 ⇒ 拼接结果只能自带一个类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportImage {
    /// 像素宽。
    pub width: u32,
    /// 像素高。
    pub height: u32,
    /// 行优先 RGBA8。
    pub rgba: Vec<u8>,
    /// 实际用了几块（`1` = 一次渲完，没分块）。
    pub tiles: u32,
    /// 实际用的扩边格数（`0` = 窗口里所有帧都不溢出自己那一格）。
    pub margin: u32,
}

/// 一次导出的回执（状态栏 / 命令行诊断都读它）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReport {
    /// 真正落盘的路径。
    pub path: PathBuf,
    /// 像素宽。
    pub width: u32,
    /// 像素高。
    pub height: u32,
    /// PNG 文件字节数。
    pub bytes: u64,
    /// 用的档位。
    pub scale: RenderScale,
    /// 分块数。
    pub tiles: u32,
    /// 扩边格数（逐块不同，这里报**最大**的那个）。
    pub margin: u32,
    /// 是否走了第二段面积平均缩放（小地图才可能 `true`）。
    pub two_stage: bool,
    /// 小地图是否走官方尺寸链（[`export_minimap_sized`]）；
    /// `false` = 本仓旧「`--minimap-max-side` 长边两段式」兼容链。
    pub official_size: bool,
    /// 这次导出用的**门态**（`false` = 闭门）。
    ///
    /// 为什么回执要带它：这是「画布开着门、导出是闭门」那个不一致收口后的**自证字段** ——
    /// 命令行回执里打出 `开门=1` 就能与画布截图对上；不带的话，两张导出图
    /// （`32,842 B` vs `32,812 B`）只能靠肉眼猜哪张是开门的。
    pub doors_open: bool,
    /// 这次导出用的**地图光照档位**（缺省 [`MapLighting::Day`] = 不画遮罩 ⇒ 与接线前逐字节相同）。
    pub lighting: MapLighting,
}

// ---------------------------------------------------------------------------
// 一、像素换算（与 `RenderScale::pixel_size` 同口径，纯整数）
// ---------------------------------------------------------------------------

/// 档位的分数表示 `(分子, 分母)`：`0.25 = 1/4`、`0.5 = 1/2`、`1 = 1/1`、`2 = 2/1`。
///
/// 用整数而不是 `f32`：拼接要算「第几格落在第几个输出像素」，浮点一旦有
/// `0.30000000000000004` 这类尾巴，块与块之间就会错一列。
const fn ratio(scale: RenderScale) -> (u64, u64) {
    match scale {
        RenderScale::Quarter => (1, 4),
        RenderScale::Half => (1, 2),
        RenderScale::One => (1, 1),
        RenderScale::Two => (2, 1),
    }
}

/// `cells` 个格 × 每格 `cell_px` 像素，在 `scale` 档下的**整数**像素数。
///
/// 恒等式：`48 % 4 == 0`、`32 % 4 == 0` ⇒ 四个档位的乘积都是整数（不存在四舍五入），
/// 与 `RenderScale::pixel_size` 的 `round(...)` 结果相同（有测试钉住）。
fn px_of(cells: u32, cell_px: u32, scale: RenderScale) -> u32 {
    let (num, den) = ratio(scale);
    (cells as u64 * cell_px as u64 * num / den) as u32
}

/// 一块 `cells_w × cells_h` 格在 `scale` 档下的输出像素尺寸。
pub fn scaled_size(cells_w: u32, cells_h: u32, scale: RenderScale) -> (u32, u32) {
    (
        px_of(cells_w, CELL_W, scale),
        px_of(cells_h, CELL_H, scale),
    )
}

// ---------------------------------------------------------------------------
// 二、选档
// ---------------------------------------------------------------------------

/// 该档位下「整幅输出」是否还过得了本模块自己的护栏（[`MAX_EXPORT_SIDE`] / [`MAX_EXPORT_BYTES`]）。
///
/// 为什么选档要看这个：`max_side` 是**用户给的目标**，它可能比护栏大得多
/// （`--minimap-max-side 99999`）。若只看 `max_side`，`700×700` 会选 `One`
/// ⇒ 33600×22400（3 GB / 单边超 32768）⇒ `render_tiled` 当场报错。
/// 选档必须只在**真渲得出来**的档位里挑，否则「用户把上限调大」反而变成失败。
fn 档位输出可行(cw: u32, ch: u32, scale: RenderScale) -> bool {
    let Some((w, h)) = scale.pixel_size(cw, ch) else {
        return false;
    };
    w <= MAX_EXPORT_SIDE
        && h <= MAX_EXPORT_SIDE
        && u64::from(w) * u64::from(h) * 4 <= MAX_EXPORT_BYTES
}

/// 小地图导出选哪一档：`One → Half → Quarter` 里**第一个**同时满足
///
/// 1. 两边都 ≤ `max_side`（用户给的目标长边），且
/// 2. 整幅输出过得了 [`MAX_EXPORT_SIDE`] / [`MAX_EXPORT_BYTES`]（[`档位输出可行`]）
///
/// 的档位；都不行就用 `Quarter`（最小档，之后由 [`downscale_to_fit`] 走第二段面积平均缩放）。
///
/// `Two` 不参与：小地图是「整图缩略」，放大没有意义（而且 `Two` 只会更快撞护栏）。
///
/// 举例（`0.map` 实测 700×700）：
///
/// | `max_side` | 选档 | 理由 |
/// |---|---|---|
/// | `2048` | `Quarter` | 三档都超 2048；`Quarter` 是最小档 |
/// | `99999` | `Quarter` | `One` 33600 px 超 `MAX_EXPORT_SIDE`；`Half` 752 MB 超 `MAX_EXPORT_BYTES` ⇒ 只剩 `Quarter` |
///
/// ⚠ @acfbbf8a 回源订正后，这条**只服务本仓旧开关 `--minimap-max-side`**；
/// 官方缺省 / 预设 / 指定像素走 [`pick_official_scale`] + [`export_minimap_sized`]。
pub fn pick_minimap_scale(cells_w: u32, cells_h: u32, max_side: u32) -> RenderScale {
    for s in [RenderScale::One, RenderScale::Half, RenderScale::Quarter] {
        if !档位输出可行(cells_w, cells_h, s) {
            continue;
        }
        if let Some((w, h)) = s.pixel_size(cells_w, cells_h) {
            if w <= max_side && h <= max_side {
                return s;
            }
        }
    }
    RenderScale::Quarter
}

/// 最大公约数（[`fit_ratio`] 用；只对非零入参调用）。
const fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// 把 `src_w × src_h` 按「**精确**保持地图格比例 `cw : ch`」缩到两边都 ≤ `max_side`。
///
/// # 返回
///
/// - `Ok(None)`：已经装得下（不缩、**也绝不放大**）；
/// - `Ok(Some((w, h)))`：要缩到这个尺寸，且 `w*ch*32 == h*cw*48` **逐位成立**；
/// - `Err(_)`：`max_side` 小到连最简比都放不下（此时无法在保持比例的前提下满足它）。
///
/// # 为什么不能简单按「长边缩到 max_side」算
///
/// 那样算出来的两个整数会**破坏长宽比**：`8400×5600` 按长边缩到 2048 得到
/// `2048×1365`，而 `2048/1365 = 1.500366…` ≠ `3/2` —— 验收判据
/// `w*ch*32 == h*cw*48` 当场就红。
///
/// 正确做法：先把 `cw*48 : ch*32` 化成最简整数比 `a : b`，再取**最大整数倍** `k`
/// （受 `max_side` 与源图尺寸双向约束）⇒ 输出 `(a*k, b*k)` 与 1× 渲染同比例。
/// `0.map`（700×700）⇒ `a:b = 3:2`，`k = 682` ⇒ **2046×1364**。
pub fn fit_ratio(
    src_w: u32,
    src_h: u32,
    cw: u32,
    ch: u32,
    max_side: u32,
) -> Result<Option<(u32, u32)>, String> {
    if src_w == 0 || src_h == 0 || cw == 0 || ch == 0 {
        return Ok(None);
    }
    let w0 = u64::from(cw) * u64::from(CELL_W);
    let h0 = u64::from(ch) * u64::from(CELL_H);
    let g = gcd(w0, h0).max(1);
    let (a, b) = (w0 / g, h0 / g);
    if u64::from(max_side) < a.max(b) {
        return Err(format!(
            "目标长边 {max_side} px 太小：这张图的比例是 {a}:{b}，保持比例至少要 {} px",
            a.max(b)
        ));
    }
    // 最大整数倍：两边都不超 max_side，也不超源图（不放大）
    let k = (u64::from(max_side) / a)
        .min(u64::from(max_side) / b)
        .min(u64::from(src_w) / a)
        .min(u64::from(src_h) / b);
    let (tw, th) = (k * a, k * b);
    if tw == u64::from(src_w) && th == u64::from(src_h) {
        return Ok(None);
    }
    Ok(Some((tw as u32, th as u32)))
}

/// 该档位下块边长取多少格（见 [`TILE_CELLS`] / [`TILE_CELLS_TWO`]）。
const fn tile_cells_for(scale: RenderScale) -> u32 {
    match scale {
        RenderScale::Two => TILE_CELLS_TWO,
        _ => TILE_CELLS,
    }
}

// ---------------------------------------------------------------------------
// 三、扩边格数（**实测帧尺寸**，不是猜的常量）
// ---------------------------------------------------------------------------

// 帧号 / 门偏移**本文件不定义**，直接调 `miredit_render::{帧号, 门偏移}`
// （2026-11-05 收口：render 侧已从 `pub(crate)` 提成 `pub` 并再导出）。
//
// 本文件曾各抄一份（因为跨 crate 取不到），抄件注释写着「改口径两边一起改」——
// 两份必然漂，而**漂移的症状正是「画布开门、导出闭门」**那个 bug。
// 判据：`grep -rn "0x7F" crates/miredit-gui/src/export.rs` 现在只剩注释，一行实现都没有。
//
// 为什么只共享这两个、不共享锚点算式：`0x7FFF` / `0x7F` 是**没有常量可依的魔法掩码**
// （`0x80` 还是「运行期已开门」标志位），抄错一位就静默画错帧；而
// `px = dx*48` / `py = dy*32` 是从公开常量 `CELL_W` / `CELL_H` 直接乘出来的，
// 常量一改两边同时变 ⇒ 抄了也漂不了，没必要为它多开一条跨 crate API。

/// **每块各需要向外扩多少格**（按块网格返回，`index = tj*nx + ti`）。
///
/// # 为什么需要扩边
///
/// `compose::render` 只遍历窗口内的格、且把帧按**原始尺寸**贴上去（`compose.rs:136/163/190`）：
///
/// | 层 | 锚点 | 覆盖范围（世界格 `(x,y)`）|
/// |---|---|---|
/// | 地表 | 左上 `(x*48, y*32)`，只偶偶格 | 向右 `帧宽`、向下 `帧高` |
/// | 中层 | 左上 `(x*48, y*32)` | 向右 `帧宽`、向下 `帧高` |
/// | 表层 | **底边** `(x*48, (y+1)*32 - 帧高)` | 向右 `帧宽`、向**上** `帧高` |
///
/// 一帧若比一格大，就会画出自己那一格：块边界上会**丢像素**（块内宽帧被画布裁掉）
/// 或**少画**（块外大帧本该溢进来）。把块向四周多渲 `m` 格再裁回来即可 ——
/// 只要 `m` 覆盖「所有能溢进本块的格到本块的距离」。
///
/// # 为什么逐块算，而不是全窗口取一个最大值
///
/// **实测**：`0.map` 里存在外溢 **30 格**的物件（≈1488 px 宽）。若全窗口取最大值，
/// 121 块每块都要扩 30 格 ⇒ 单块 `64+60=124` 格、重复渲染系数 `(124/64)² ≈ 3.8`，
/// 而且任意一个大物件都会拖慢整张图的导出。逐块算之后，只有**真正挨着**那个物件的
/// 一两块扩 30 格，其余块扩 0～1 格 ⇒ 又快又准。
///
/// # 怎么算（一遍扫完窗口，不重复扫）
///
/// 对窗口里每一格、每一层的帧，算出它的**像素矩形** `R`（世界格内相对窗口原点），
/// 再算出 `R` 与哪些块相交（块是规则的网格 ⇒ 直接整除即可，不需要遍历所有块），
/// 对这些块取「该格到块矩形」的切比雪夫距离作为所需扩边，逐块取最大值。
/// 一格能碰到的块通常只有 1～4 个，所以整趟是 `O(窗口格数)`。
///
/// **只看窗口内的格**（这是正确性的必要条件，不是省事）：窗口外的格在整图渲染里
/// 本来就不画（`compose.rs:115` 的 `for dx in 0..cells_w`），而 [`expand`] 会把扩边区
/// **夹回窗口** ⇒ 窗口外的格永远不会被画进来。
///
/// 本函数**不判上限**：上限由 [`render_tiled`] 按「这次到底分不分块」决定
/// （单块时扩边被夹回窗口，多大都无害）。
///
/// # `doors_open` 为什么是必填参数
///
/// 表层帧号在开门态下会 `+= 门偏移(cell, options)`（见 [`门偏移`]），**而门开帧与
/// 闭门帧的尺寸可以不同** ⇒ 扩边必须按**将要真正画出去的那一帧**算，否则块边界上会
/// 丢像素/少画（模块文档 §六）。传错这个参数不会报错、只会悄悄拼错图，所以它**没有
/// 缺省值**：调用方必须显式表态。
fn tile_margins(
    map: &Map,
    src: &dyn SpriteSource,
    mask: &LayerMask,
    origin: (i32, i32),
    cells: (u32, u32),
    tile: u32,
    options: RenderOptions,
) -> Vec<u32> {
    let nx = cells.0.div_ceil(tile) as usize;
    let ny = cells.1.div_ceil(tile) as usize;
    let mut out = vec![0u32; nx * ny];
    if nx == 0 || ny == 0 {
        return out;
    }
    let mut cache = SizeCache::new(src);
    // `options` 由调用方（[`render_tiled`]）直接递进来：扩边按**真正会画出去的那一帧**
    // 算，与真渲用的是同一份 `RenderOptions`（门态 + 光照档位都在此，不许拆成两个参数）。

    // 块网格的像素步长（一块 `tile` 格 = `tile*48` × `tile*32` px）
    let step_x = i64::from(tile) * i64::from(CELL_W);
    let step_y = i64::from(tile) * i64::from(CELL_H);

    // 一层的帧矩形 `(x0, y0, x1, y1)`（相对窗口原点的像素坐标，半开区间）
    let bump = |rect: (i64, i64, i64, i64), dx: u32, dy: u32, out: &mut [u32]| {
        let (x0, y0, x1, y1) = rect;
        if x1 <= x0 || y1 <= y0 {
            return; // 0×0 空帧：什么也不画，也就没有外溢
        }
        // 与哪些块相交：块 `ti` 覆盖像素 `[ti*step_x, (ti+1)*step_x)`
        let ti0 = (x0 / step_x).max(0) as usize;
        let ti1 = (((x1 - 1) / step_x).max(0) as usize).min(nx - 1);
        let tj0 = (y0 / step_y).max(0) as usize;
        let tj1 = (((y1 - 1) / step_y).max(0) as usize).min(ny - 1);
        for tj in tj0..=tj1 {
            for ti in ti0..=ti1 {
                // 该块在窗口内的格矩形（闭区间）
                let bx0 = (ti as u32) * tile;
                let by0 = (tj as u32) * tile;
                let bx1 = (bx0 + tile).min(cells.0) - 1;
                let by1 = (by0 + tile).min(cells.1) - 1;
                // 该格到块矩形的切比雪夫距离（在块内 ⇒ 0）。
                //
                // 一维「点到区间 `[lo, hi]` 的距离」= `lo.saturating_sub(v)` 与
                // `v.saturating_sub(hi)` 取大者：点在区间左侧 ⇒ 前者是 `lo - v`、后者 0；
                // 点在区间内 ⇒ 两个都是 0；点在右侧 ⇒ 前者 0、后者是 `v - hi`。
                //
                // 原先写成 `if v < lo { lo - v } else if v > hi { v - hi } else { 0 }`，
                // clippy `implicit_saturating_sub` 报的是最后那个 `… { v - hi } else { 0 }` 尾巴。
                // 没有只照它把尾巴换掉（那样会留下一个只在左侧成立的手写减法分支），
                // 而是两边都用 `saturating_sub`：三个区间由同一个式子覆盖，从构造上不可能
                // 下溢，也没有"手滑把 lo/hi 写反就静默绕回成天文数字"的余地。
                let mx = bx0.saturating_sub(dx).max(dx.saturating_sub(bx1));
                let my = by0.saturating_sub(dy).max(dy.saturating_sub(by1));
                let m = mx.max(my);
                let slot = &mut out[tj * nx + ti];
                *slot = (*slot).max(m);
            }
        }
    };

    for dy in 0..cells.1 {
        let y = origin.1 + dy as i32;
        let Ok(cy) = u16::try_from(y) else {
            continue; // 图外（负坐标）：不画，也不参与外溢
        };
        for dx in 0..cells.0 {
            let x = origin.0 + dx as i32;
            let Ok(cx) = u16::try_from(x) else {
                continue;
            };
            let Some(cell) = map.cell(cx, cy) else {
                continue;
            };
            let px = i64::from(dx) * i64::from(CELL_W);
            let py = i64::from(dy) * i64::from(CELL_H);
            // 地表：只铺偶偶格（`compose.rs:117-124`），左上锚点
            if mask.ground && cx & 1 == 0 && cy & 1 == 0 {
                if let Some(i) = 帧号(cell.bk_img) {
                    if let Some((fw, fh)) = cache.size(LibRef::ground(u16::from(cell.tiles)), i) {
                        bump(
                            (px, py, px + i64::from(fw), py + i64::from(fh)),
                            dx,
                            dy,
                            &mut out,
                        );
                    }
                }
            }
            // 中层：逐格左上锚点
            if mask.middle {
                if let Some(i) = 帧号(cell.mid_img) {
                    if let Some((fw, fh)) = cache.size(LibRef::middle(u16::from(cell.sm_tiles)), i) {
                        bump(
                            (px, py, px + i64::from(fw), py + i64::from(fh)),
                            dx,
                            dy,
                            &mut out,
                        );
                    }
                }
            }
            // 表层：**底边**钉在格底 ⇒ 帧向上长
            if mask.front {
                if let Some(i) = 帧号(cell.fr_img) {
                    // 门开帧偏移：**调 render 的那一份**（`miredit_render::门偏移`），
                    // 与 `compose::render` 的表层分支算的是同一个帧号。
                    let i = i + 门偏移(cell, options);
                    if let Some((fw, fh)) = cache.size(LibRef::surface(u16::from(cell.area)), i) {
                        let 底 = py + i64::from(CELL_H);
                        bump(
                            (px, 底 - i64::from(fh), px + i64::from(fw), 底),
                            dx,
                            dy,
                            &mut out,
                        );
                    }
                }
            }
        }
    }

    out
}
// ---------------------------------------------------------------------------
// 四、分块渲染 + 拼接
// ---------------------------------------------------------------------------

/// 一块（窗口内相对格坐标）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Tile {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

/// 把块向四周扩 `margin` 格，并**夹回窗口**（夹回是必须的：窗口外的格
/// 在整图渲染里本来就不画，扩出去会把它们画进来 ⇒ 反而多像素）。
fn expand(t: Tile, margin: u32, win: (u32, u32)) -> Tile {
    let x0 = t.x.saturating_sub(margin);
    let y0 = t.y.saturating_sub(margin);
    let x1 = (t.x + t.w + margin).min(win.0);
    let y1 = (t.y + t.h + margin).min(win.1);
    Tile {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    }
}

/// 按 render crate 的护栏把块边长缩到能过（真的过不了就报错，不硬渲）。
fn fit_tile(base: u32, margin: u32, scale: RenderScale) -> Result<u32, String> {
    let mut t = base.max(1);
    loop {
        let e = t + 2 * margin;
        let 中间宽 = u64::from(e) * u64::from(CELL_W);
        let 中间高 = u64::from(e) * u64::from(CELL_H);
        let 中间字节 = 中间宽 * 中间高 * 4;
        let (tw, th) = scaled_size(e, e, scale);
        let 目标字节 = u64::from(tw) * u64::from(th) * 4;

        // 老护栏（`types.rs:32,35`）。这两个常量在 render crate 里是 `pub(crate)`，
        // 跨 crate 取不到 ⇒ 这里**照抄同值**只为「提前缩块」，真判据仍是
        // `try_render_region_scaled_with` 自己那一条（真超了它会报错，不会静默降级）。
        let ok_side = 中间宽 <= 32_768 && 中间高 <= 32_768 && 中间宽 * 中间高 <= 256_000_000;
        // `One` 档不产生中间图、也不受目标图字节上限约束（`scale.rs:174-176` 直接转调）。
        let ok_bytes = if scale == RenderScale::One {
            true
        } else {
            中间字节 <= INTERMEDIATE_MAX_BYTES && 目标字节 <= SCALED_MAX_BYTES
        };
        if ok_side && ok_bytes {
            return Ok(t);
        }
        if t <= 1 {
            return Err(format!(
                "分块尺寸降到 1 格仍超 render 护栏（扩边 {margin} 格、档位 {}）",
                scale.label()
            ));
        }
        t = (t / 2).max(1);
    }
}

/// 把 `src` 里从 `off` 格开始的一块，拷进 `dst` 的 `at` 格位置（像素偏移按档位整数换算）。
#[allow(clippy::too_many_arguments)]
fn blit_crop(
    dst: &mut [u8],
    dst_w: u32,
    src: &[u8],
    src_w: u32,
    off: (u32, u32),
    at: (u32, u32),
    size: (u32, u32),
    scale: RenderScale,
) -> Result<(), String> {
    let sx = px_of(off.0, CELL_W, scale);
    let sy = px_of(off.1, CELL_H, scale);
    let dx = px_of(at.0, CELL_W, scale);
    let dy = px_of(at.1, CELL_H, scale);
    let w = px_of(size.0, CELL_W, scale);
    let h = px_of(size.1, CELL_H, scale);
    let row = w as usize * 4;

    // 越界一律报错（宁可失败也不写错位的图）
    let need_src = (u64::from(sx) + u64::from(w)) <= u64::from(src_w);
    let need_dst = (u64::from(dx) + u64::from(w)) <= u64::from(dst_w);
    if !need_src || !need_dst {
        return Err(format!(
            "拼接越界：块源偏移 {sx}+{w} 宽 {src_w} / 目标偏移 {dx}+{w} 宽 {dst_w}"
        ));
    }
    for j in 0..h {
        let s = ((u64::from(sy) + u64::from(j)) * u64::from(src_w) + u64::from(sx)) as usize * 4;
        let d = ((u64::from(dy) + u64::from(j)) * u64::from(dst_w) + u64::from(dx)) as usize * 4;
        let (Some(ss), Some(dd)) = (src.get(s..s + row), dst.get_mut(d..d + row)) else {
            return Err("拼接越界：行切片取不到".to_string());
        };
        dd.copy_from_slice(ss);
    }
    Ok(())
}

/// 渲染 `origin` 起 `cells` 格的一块窗口，**内部分块**，返回拼好的整幅 RGBA。
///
/// 这是导出与选区导出的**唯一**渲染入口：两块功能只差 `origin/cells/scale/doors_open`。
///
/// `doors_open` = **导出时的门态**（`false` = 闭门 = 与接线前逐字节相同）。它同时喂给
/// 两条链路，缺一不可（模块文档 §六）：
///
/// 1. [`tile_margins`]（按**真正会画出去的那一帧**的尺寸算扩边）；
/// 2. 每块的 `try_render_region_scaled_with(.., RenderOptions::with_doors_open(doors_open))`。
///
/// 只做②不做① ⇒ 开门态下大尺寸的门开帧跨块时拿不到扩边，拼接与整图不一致；
/// 只做①不做② ⇒ 扩边按开门帧算、画出来还是闭门帧，白扩。
#[allow(clippy::too_many_arguments)] // 8 个参数就是「一次导出请求」的 8 个输入，打包成结构体只会更绕（与本 crate 其它绘制入口同口径）
pub fn render_tiled(
    map: &Map,
    src: &dyn SpriteSource,
    mask: &LayerMask,
    overlay: CollisionOverlay,
    scale: RenderScale,
    origin: (i32, i32),
    cells: (u32, u32),
    options: RenderOptions,
) -> Result<ExportImage, String> {
    if cells.0 == 0 || cells.1 == 0 {
        return Err("导出窗口为 0 格（没有可导出的范围）".to_string());
    }
    let (out_w, out_h) = scaled_size(cells.0, cells.1, scale);
    // 整幅输出的护栏（与 render crate 的单次护栏是两回事，见模块文档 §五）
    if out_w > MAX_EXPORT_SIDE || out_h > MAX_EXPORT_SIDE {
        return Err(format!(
            "导出图太大：{out_w}×{out_h} px 超过单边上限 {MAX_EXPORT_SIDE} px \
             （缩小选区，或换更小的图）"
        ));
    }
    let 整幅字节 = u64::from(out_w) * u64::from(out_h) * 4;
    if 整幅字节 > MAX_EXPORT_BYTES {
        return Err(format!(
            "导出图太大：{out_w}×{out_h} px = {:.1} MB，超过上限 {:.0} MB",
            整幅字节 as f64 / 1_048_576.0,
            MAX_EXPORT_BYTES as f64 / 1_048_576.0
        ));
    }

    let tile = tile_cells_for(scale);
    // 扩边的**真实上限**由 render 护栏反算（不是写死的数，见 [`margin_cap`]）
    let cap = margin_cap(tile, scale);
    let nx = cells.0.div_ceil(tile).max(1);
    let ny = cells.1.div_ceil(tile).max(1);
    let 单块 = nx == 1 && ny == 1;

    // 每块各需要扩多少格（逐块算，见 [`tile_margins`] 的文档）
    let mut margins = tile_margins(map, src, mask, origin, cells, tile, options);
    if 单块 {
        // 单块：扩边区被 [`expand`] 夹回窗口，**多大都无害**（结果与整图一次渲完全相同），
        // 所以不在这里卡上限 —— 否则「3×3 选区 + 图里恰好有个大物件」会被无谓地拒掉。
        for m in margins.iter_mut() {
            *m = (*m).min(cap);
        }
    } else if let Some((i, &bad)) = margins.iter().enumerate().find(|(_, &m)| m > cap) {
        return Err(format!(
            "帧尺寸过大：第 {} 块需要外扩 {bad} 格，超过本档位上限 {cap} 格 \
             （宽 > {} px 或高 > {} px 的帧无法保证分块拼接逐字节一致；\
              缩小导出范围，或先把这些格里的物件挪走）",
            i + 1,
            CELL_W + cap * CELL_W,
            CELL_H + cap * CELL_H
        ));
    }

    let mut buf = vec![0u8; 整幅字节 as usize];
    let mut tiles = 0u32;
    let mut 最大扩边 = 0u32;
    // `options` 由调用方直接递进来（门态 + 光照档位同在一份 `RenderOptions` 里），
    // 逐块传值不会重复构造。为什么不用 `try_render_region_scaled`：那个入口内部写死
    // `RenderOptions::default()` ⇒ 门恒闭 + 恒白天（这正是本模块要收口的边界，见 §六/§七）。

    for tj in 0..ny {
        for ti in 0..nx {
            let tx = ti * tile;
            let ty = tj * tile;
            let t = Tile {
                x: tx,
                y: ty,
                w: tile.min(cells.0 - tx),
                h: tile.min(cells.1 - ty),
            };
            let margin = margins[(tj * nx + ti) as usize];
            最大扩边 = 最大扩边.max(margin);
            let e = expand(t, margin, cells);
            let vp = Viewport::new(origin.0 + e.x as i32, origin.1 + e.y as i32, e.w, e.h);
            let r = try_render_region_scaled_with(map, src, &vp, mask, overlay, scale, options)
                .map_err(|err| {
                    format!(
                        "分块渲染失败（块 {}×{} 格 @({tx},{ty})，扩边 {margin} 格 → \
                         实际渲 {}×{} 格，档位 {}）：{err}",
                        t.w,
                        t.h,
                        e.w,
                        e.h,
                        scale.label()
                    )
                })?;
            // 尺寸自证：render 出的必须正好是「扩边块」在本次档位下的像素尺寸
            let (ew, eh) = scaled_size(e.w, e.h, scale);
            if r.width != ew || r.height != eh {
                return Err(format!(
                    "分块尺寸不符：期望 {ew}×{eh}，实际 {}×{}",
                    r.width, r.height
                ));
            }
            blit_crop(
                &mut buf,
                out_w,
                &r.rgba,
                r.width,
                (t.x - e.x, t.y - e.y),
                (t.x, t.y),
                (t.w, t.h),
                scale,
            )?;
            tiles += 1;
        }
    }

    Ok(ExportImage {
        width: out_w,
        height: out_h,
        rgba: buf,
        tiles,
        margin: 最大扩边,
    })
}

// ---------------------------------------------------------------------------
// 五、PNG 原子写
// ---------------------------------------------------------------------------

/// 把 RGBA8 编成 PNG 字节（**复用 render crate 的编码器**，不自己写）。
///
/// 为什么绕一圈临时文件：`png_io` 的公开 API 只有「写文件」形态
/// （`write_rgba_png` / `PngImage::save_png`），没有「返回 `Vec<u8>`」形态 ——
/// 与 `miredit-web/src/routes/mod.rs:84-86` 的处理完全一致。
/// 临时文件带 pid + 自增序号，并发不会撞名；**无论成败都删**。
fn encode_png_bytes(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!("miredit-gui-{}-{seq}.png", std::process::id()));
    if let Err(e) = write_rgba_png(&tmp, width, height, rgba) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("PNG 编码失败（{width}×{height}）：{e}"));
    }
    let bytes = std::fs::read(&tmp)
        .map_err(|e| format!("PNG 读回失败（{}）：{e}", tmp.display()));
    if std::fs::remove_file(&tmp).is_err() {
        eprintln!("[导出] 临时文件清理失败：{}", tmp.display());
    }
    bytes
}

/// 把一张位图**原子地**写成 PNG（`tmp → 写 → rename`，失败不留半个文件）。
///
/// 真正的写盘是 [`crate::save::save_atomic`]（= `miredit_core::save_atomic`）：
/// 写 `<目标名>.miredit-tmp` → `sync_all` → 读回逐字节核对 → `rename` 原子替换 →
/// 尽力 fsync 父目录。目录不存在 ⇒ 第一阶段就失败，**目标目录里一个文件都不会多**。
pub fn write_png_atomic(target: &Path, img: &ExportImage) -> Result<(), String> {
    let bytes = encode_png_bytes(img.width, img.height, &img.rgba)?;
    save::save_atomic(target, &bytes).map(|_| ())
}

// ---------------------------------------------------------------------------
// 六、两个导出入口
// ---------------------------------------------------------------------------

/// 落盘 + 回执（两个导出共用的尾巴）。
fn finish(
    img: ExportImage,
    target: &Path,
    scale: RenderScale,
    two_stage: bool,
    official_size: bool,
    options: RenderOptions,
) -> Result<ExportReport, String> {
    write_png_atomic(target, &img)?;
    let bytes = std::fs::metadata(target).map(|m| m.len()).unwrap_or(0);
    Ok(ExportReport {
        path: target.to_path_buf(),
        width: img.width,
        height: img.height,
        bytes,
        scale,
        tiles: img.tiles,
        margin: img.margin,
        two_stage,
        official_size,
        doors_open: options.doors_open,
        lighting: options.lighting,
    })
}

/// 取「地图 + 图库」，缺哪个报哪个（中文原因，一个文件都不写）。
fn map_and_source(doc: &DocState) -> Result<(&Map, &dyn SpriteSource), String> {
    let d = doc
        .doc
        .as_ref()
        .ok_or_else(|| "需要先开图：当前没有打开地图（文件 → 打开地图…）".to_string())?;
    let src = doc
        .assets
        .source()
        .ok_or_else(|| "没有图库：图库目录不可用，导不出图（不是空图，是取不到帧）".to_string())?;
    Ok((&d.map, src as &dyn SpriteSource))
}

/// **导出选区**：把 `sel`（闭区间）按 `scale = 1` 渲成 PNG 写到 `target`。
///
/// - `sel == None` ⇒ 报「没有选区」的原因，**不写文件**；
/// - 选区先按地图尺寸裁剪（与 `EditSession::set_selection` 同一条 `clamp_to`），
///   整块在图外 ⇒ 报错；
/// - 层口径：**三层全开、不叠阻挡**（导出的是地图内容本身，不是当前视图状态）
///   —— 这正是 `miredit-web` 的 `/api/tiles` 的缺省口径（`layers` 缺省三层全开、
///   `collision` 缺省 0，见 `tiles.rs:23-24`）⇒ 同一区域、**同一 `doors_open`** 下导出的
///   PNG 与 `/api/tiles?...&scale=1&doors=<0|1>` **逐字节相同**（同一套 `compose::render`
///   + 同一个编码器）；
/// - `options`：**渲染选项**（门态 + 地图光照档位）。调用方必须传**当前视图的那一份**
///   （菜单路径 = `RenderOptions::with_doors_and_lighting(*cx.doors_open, *cx.lighting)`，
///   命令行路径 = [`crate::cli::Cli::export_options`]），否则又回到「画布开着门/切了夜晚、
///   导出是闭门白天」那个不一致（模块文档 §六/§七）。
///   缺省 [`RenderOptions::default()`] = 闭门 + 白天 ⇒ 与接线前逐字节相同。
///
/// 选区块本身走 [`render_tiled`]：3×3 时只有一块，与直接调 render 完全等价；
/// 用户框了一大片（比如 200×200 格 @1× = 9600×6400）时自动分块，不会撞 1× 护栏。
pub fn export_selection(
    doc: &DocState,
    sel: Option<Selection>,
    target: &Path,
    options: RenderOptions,
) -> Result<ExportReport, String> {
    let (map, src) = map_and_source(doc)?;
    let sel = sel.ok_or_else(|| {
        "没有选区：先在画布上用选择工具拖出一个矩形（或给 --select x0,y0,x1,y1）".to_string()
    })?;
    let sel = sel
        .clamp_to(map.width, map.height)
        .ok_or_else(|| {
            format!(
                "选区整块在地图外：({},{})..({},{}) 对不上 {}×{} 的地图",
                sel.x0, sel.y0, sel.x1, sel.y1, map.width, map.height
            )
        })?;
    let img = render_tiled(
        map,
        src,
        &LayerMask::all(),
        CollisionOverlay::none(),
        RenderScale::One,
        (i32::from(sel.x0), i32::from(sel.y0)),
        (u32::from(sel.width()), u32::from(sel.height())),
        options,
    )?;
    finish(img, target, RenderScale::One, false, false, options)
}

/// **第二段：把已渲好的整幅按面积平均缩到 `max_side` 以内**（长宽比精确保持）。
///
/// # 采样归属（这是本模块最容易被误解的一处）
///
/// - **第一段**（决定每个像素取哪一格、哪一帧）**只能**由 `miredit-render` 做 ——
///   即 [`render_tiled`] 里的 `try_render_region_scaled_with`；
/// - **第二段**只是「把一张已经渲好的 RGBA 图整体缩小」，不再碰任何地图语义 ——
///   这里复用 `miredit_render::compare::resize_to`（面积平均 / 确定性 / 不预乘）。
///   ⇒ 本模块**仍然一行滤镜都没写**。
///
/// # 为什么是 `compare::resize_to` 而不是 `resample::盒式_流式`
///
/// `resample` 在 `miredit-render` 里是**私有模块**（`lib.rs:162` `mod resample;`，
/// 两个函数都是 `pub(crate)`）⇒ 从本 crate **物理上调不到**；要用它必须改
/// `miredit-render`（不在本单写域）。
/// 公开可用的面积平均入口只有 `compare::resize_to`（`compare.rs:154`，文档原话
/// 「面积平均缩放（确定性…）」），语义与 `盒式_流式` 同类。
///
/// 内存上它也更省：`盒式_流式` 每行要一份 `sw × 32 B` 的预乘缓冲（8400 宽 = 269 KB，不大），
/// 但 `resample::缩放`（整幅预乘版）要 `47 MP × 32 B ≈ 1.5 GB` —— 那才是必须避开的那个。
/// `resize_to` 的额外内存只有**每个目标像素一个 `[u64; 4]` 累加器**，
/// 峰值 ≈ 源图 + 目标图（`8400×5600×4 = 188 MB` + `2046×1364×4 = 11 MB`）。
///
/// `target = Some((w,h))` 时**直接按官方指定像素尺寸**做第二段面积平均缩放
/// （官方 `PreviewRaster.Create(..., width, height)` 的等价落点）；`None` 才是本仓旧
/// 「保持格比例、缩到长边以内」的兼容链。`cw/ch` 只被兼容链使用。
///
/// 返回 `(新图, 是否真的缩了)`。
fn downscale_to_fit(
    img: ExportImage,
    cw: u32,
    ch: u32,
    max_side: u32,
    target: Option<(u32, u32)>,
) -> Result<(ExportImage, bool), String> {
    let 目标 = match target {
        Some(t) => Some(t),
        None => fit_ratio(img.width, img.height, cw, ch, max_side)?,
    };
    let Some((tw, th)) = 目标 else {
        return Ok((img, false));
    };
    // 官方默认 8400×5600 与 Quarter 原生输出恰好同尺寸：resize_to 只会 clone，
    // 这时回执不能虚报「两段式 yes」（禁假数据）。
    if img.width == tw && img.height == th {
        return Ok((img, false));
    }
    // 交给 render crate 的公开面积平均缩放（不自己写滤镜）
    let src = miredit_render::PngImage {
        width: img.width,
        height: img.height,
        rgba: img.rgba,
    };
    let out = miredit_render::compare::resize_to(&src, tw, th);
    drop(src);
    Ok((
        ExportImage {
            width: out.width,
            height: out.height,
            rgba: out.rgba,
            tiles: img.tiles,
            margin: img.margin,
        },
        true,
    ))
}

/// **导出小地图**：整张地图先按 [`pick_minimap_scale`] 选出的档位渲，再（必要时）
/// 用面积平均缩到 `max_side` 以内。
///
/// `options`：**渲染选项**（门态 + 光照档位）。门偏移与光照遮罩都在 1× 合成阶段
/// 生效、缩放只是纯后处理 ⇒ 缩完的图与「同一组选项的画布」一致（模块文档 §六/§七）。
/// 缺省 [`RenderOptions::default()`] = 闭门 + 白天 ⇒ 与接线前逐字节相同。
///
/// 输出尺寸与长宽比：
/// - 一档装得下 ⇒ 尺寸 = `round(宽*48*f) × round(高*32*f)`，比例与 1× 渲染恒等；
/// - 装不下（如 700×700 @`Quarter` = 8400×5600）⇒ 先出 8400×5600，再按
///   [`fit_ratio`] 缩到 `(a*k, b*k)`，**比例仍与 1× 渲染逐位相同**（判据
///   `w*ch*32 == h*cw*48`）。700×700 + `max_side=2048` ⇒ **2046×1364**
///   （不是 2048×1365 —— 那个比例是 1.500366…，不是 3:2）。
pub fn export_minimap(
    doc: &DocState,
    target: &Path,
    max_side: u32,
    options: RenderOptions,
) -> Result<ExportReport, String> {
    let (map, src) = map_and_source(doc)?;
    let (w, h) = (u32::from(map.width), u32::from(map.height));
    let scale = pick_minimap_scale(w, h, max_side);
    let img = render_tiled(
        map,
        src,
        &LayerMask::all(),
        CollisionOverlay::none(),
        scale,
        (0, 0),
        (w, h),
        options,
    )?;
    let (img, two_stage) = downscale_to_fit(img, w, h, max_side, None)?;
    finish(img, target, scale, two_stage, false, options)
}

/// 官方 `ValidSize` 的等价判据 + 本仓更严的内存护栏（**不静默截断/降级**）。
///
/// @acfbbf8a `MinimapExporter.cs:15-16,21-23`：官方只判 `w>0 && h>0 && w*h <= Array.MaxLength/4`；
/// 本仓 [`MINIMAP_MAX_PIXELS`] 是该 .NET 数字按 `Vec<u8>` / `isize::MAX` 做的等价换算
/// （不是抄数字），同时 [`MAX_EXPORT_SIDE`] / [`MAX_EXPORT_BYTES`] 更严，冲突时**保留更严者**。
/// ⇒ 0 / 负数（CLI 解析成 `u32` 后就是 `0`）/ 超大像素一律给中文原因并拒绝。
pub fn valid_minimap_size(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err(format!(
            "小地图导出尺寸非法：宽和高都必须是正整数（收到 {width}×{height} px，官方 ValidSize 拒绝 0）"
        ));
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MINIMAP_MAX_PIXELS {
        return Err(format!(
            "小地图导出尺寸过大：{width}×{height} px = {pixels} 像素，超过 Rust RGBA8 可分配上限 {MINIMAP_MAX_PIXELS} 像素（官方 ValidSize 的等价换算）"
        ));
    }
    if width > MAX_EXPORT_SIDE || height > MAX_EXPORT_SIDE {
        return Err(format!(
            "小地图导出尺寸过大：{width}×{height} px 超过本仓单边上限 {MAX_EXPORT_SIDE} px（保留更严的内存护栏，不截断、不降级）"
        ));
    }
    let bytes = pixels * 4;
    if bytes > MAX_EXPORT_BYTES {
        return Err(format!(
            "小地图导出尺寸过大：{width}×{height} px 的 RGBA 需要 {:.1} MB，超过本仓 {:.0} MB 上限（保留更严的内存护栏，不截断、不降级）",
            bytes as f64 / 1_048_576.0,
            MAX_EXPORT_BYTES as f64 / 1_048_576.0
        ));
    }
    Ok(())
}

/// 官方 16 级 overview mip → 本仓 4 档的**有据映射**（不是自定值）。
///
/// @acfbbf8a `MinimapExporter.cs:47-48`：
/// `scale = min(width/(W*48.0), height/(H*32.0))`；
/// `level = scale >= 1 ? 0 : clamp(floor(log2(1/scale)), 0, 16)`；
/// 官方 `Assets.cs:119` 的 `1 << level` 把源图缩小 `2^level` 倍。
/// 本仓 [`RenderScale`] 只有 `Quarter(1/4) / Half(1/2) / One(1) / Two(2)`，
/// **没有 16 级 mip** ⇒ 取「最接近且不越过目标尺寸的档位」：
///
/// ```text
/// scale >= 2            → Two      （官方 scale>=1 本可 level=0；本仓 2× 不越过目标）
/// 1 <= scale < 2        → One
/// 0.5 <= scale < 1      → Half     （官方 level=1）
/// 0 < scale < 0.5       → Quarter  （官方 level>=2 统一夹到本仓最小档）
/// ```
///
/// ⚠ **官方用 16 级 overview mip，本仓只有 4 档 ⇒ 有据映射，不是自定值**。
/// 目标像素尺寸最终仍由 [`downscale_to_fit`] 精确落到 `ValidSize` 判过的宽高，
/// 不放大采样核、不改任何 render crate 常量。
pub fn pick_official_scale(
    cells_w: u32,
    cells_h: u32,
    width: u32,
    height: u32,
) -> RenderScale {
    let native_w = f64::from(cells_w) * f64::from(CELL_W);
    let native_h = f64::from(cells_h) * f64::from(CELL_H);
    let scale = (f64::from(width) / native_w).min(f64::from(height) / native_h);
    if scale >= 2.0 {
        RenderScale::Two
    } else if scale >= 1.0 {
        RenderScale::One
    } else if scale >= 0.5 {
        RenderScale::Half
    } else {
        RenderScale::Quarter
    }
}

/// **导出小地图（官方 @acfbbf8a 尺寸口径）**：缺省 `W*12 × H*8`，
/// 三档预设 `W*48/{4,2,1} × H*32/{4,2,1}`，或 `--minimap-width/--minimap-height` 指定像素。
///
/// 流程：`valid_minimap_size` 先拒绝非法尺寸（中文原因、不写文件）→
/// `pick_official_scale` 把官方 16 级 mip 有据映射到本仓 4 档 →
/// 复用既有 [`render_tiled`] 分块护栏 → `compare::resize_to` 精确落到指定宽高。
/// 官方 `PreviewRaster.Create` 从 **overview mip 精灵** 重绘到任意 `width×height`
/// （`MinimapExporter.cs:46-57`）；本仓没有 mip 精灵缓存，故第一段按最接近且不越界的
/// 档位出整幅 RGBA，第二段只做公开的面积平均缩放 —— **有据映射，不是自定值**，
/// 也不改 render crate 的任何常量。门态 + 光照档位整份 `RenderOptions` 透传，
/// 缺省仍是闭门 + 白天。
pub fn export_minimap_sized(
    doc: &DocState,
    target: &Path,
    size: MinimapSize,
    options: RenderOptions,
) -> Result<ExportReport, String> {
    let (map, src) = map_and_source(doc)?;
    let (w, h) = (u32::from(map.width), u32::from(map.height));
    let (out_w, out_h) = size.resolve(w, h)?;
    valid_minimap_size(out_w, out_h)?;
    let scale = pick_official_scale(w, h, out_w, out_h);
    // 目标档位整幅输出的护栏：预设 Large(1:1) 若超 MAX_EXPORT_SIDE / MAX_EXPORT_BYTES，
    // 必须中文报错，不许静默降到 Half（官方预设就是 1:1，降档=偷偷改口径）。
    if !档位输出可行(w, h, scale) {
        return Err(format!(
            "官方预设所需档位 {} 的单次输出过不了本仓分块护栏（地图 {w}×{h} 格、目标 {out_w}×{out_h} px）：请改用 smaller 预设或指定更小尺寸（不静默换档、不截断）",
            scale.label()
        ));
    }
    let img = render_tiled(
        map,
        src,
        &LayerMask::all(),
        CollisionOverlay::none(),
        scale,
        (0, 0),
        (w, h),
        options,
    )?;
    let (img, two_stage) = downscale_to_fit(img, w, h, out_w.min(out_h), Some((out_w, out_h)))?;
    finish(img, target, scale, two_stage, true, options)
}

#[cfg(test)]
mod tests;
