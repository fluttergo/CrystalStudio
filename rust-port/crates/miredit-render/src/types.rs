//! 视口 / 图层开关 / 统计 / 输出 / 错误。
//!
//! 这里的每个常量都对应客户端源码里的一个事实，注释里给出出处（`文件:行号`），
//! 改常量前先回去核源码，别凭直觉。

use crate::lighting::MapLighting;

/// 世界格宽（像素）。
///
/// 出处：原作 `Grobal.h:1980` `UNITX`；Godot 侧 `map_view.gd:9-10`
/// （「世界格 = 48×32 px ⇒ 角色/相机/点击一律 48×32」）与 `map_view.gd:91`（`x*48`）。
pub const CELL_W: u32 = 48;

/// 世界格高（像素）。
///
/// 出处：同上 `Grobal.h:1981` `UNITY`；`map_view.gd:91`（`y*32`）。
pub const CELL_H: u32 = 32;

/// 地表瓦片宽（像素）—— 覆盖 **2×2 世界格**。
///
/// 出处：`map_view.gd:11-12`（「地表瓦片 = 96×64 原生图，覆盖 2×2 世界格」）、
/// `map_view.gd:101`（`Vector2i(96, 64)` 的 TileSet 格尺寸）。
pub const TILE_W: u32 = 96;

/// 地表瓦片高（像素）。
///
/// 出处：`map_view.gd:11-12`、`map_view.gd:101`。
pub const TILE_H: u32 = 64;

/// 单边像素上限（防御性护栏，不是语义限制）。
///
/// 目的是让「`cells_w` 手抖写成 `4_000_000`」这类输入**明确报错**而不是先 OOM 再崩。
/// `pub(crate)`：印章渲染（`stamp.rs`）用同一条护栏，不另定一套阈值。
pub(crate) const MAX_SIDE: u64 = 32_768;

/// 总像素上限（防御性护栏，约 1 GiB 的 RGBA 缓冲）。
pub(crate) const MAX_PIXELS: u64 = 256_000_000;

/// **缩放后**输出的字节上限：`64 MiB = 67,108,864 B`。
///
/// 为什么不复用 [`MAX_PIXELS`]：那条是「1× 合成画布」的护栏（256 M 像素 ≈ 1 GiB），
/// 而多倍率渲染的产物是要**整幅一次性交给网页当瓦片贴图**的 —— 64 MiB 才是那个场景
/// 的量级。两条护栏各管一段，互不覆盖。
///
/// 超限时 [`crate::try_render_region_scaled`] 返回 [`RenderError::TooLarge`]，
/// 并且**在分配任何缓冲之前**返回：不硬渲、不静默降级成 1×（那是假数据）。
///
/// ⚠ 本条只管**目标图**。缩放档位还会在内部先合成一张 1× 中间图，那张图另有
/// [`INTERMEDIATE_MAX_BYTES`] 管 —— 两条护栏管的是两个不同的缓冲，缺一条就有洞。
pub const SCALED_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// **1× 中间图**的字节上限：`128 MiB = 134,217,728 B`。
///
/// # 一、它管的是什么（为什么必须有）
///
/// 多倍率渲染的内部流程是「**先按 1× 合成一整幅，再重采样到目标尺寸**」
/// （见 [`crate::scale`] 模块文档 §二）。[`SCALED_MAX_BYTES`] 只卡**目标图**，
/// 而 1× 中间图**在本次改动之前没有任何上界** —— 于是出现了这种请求：
///
/// | 输入 | 目标图（受 64 MiB 约束）| 1× 中间图（老代码无约束）|
/// |---|---|---|
/// | `200×150` 格 `@0.25` | `2400×1200×4 = 11.5 MB` ✅ 放行 | `9600×4800×4 = 184,320,000 B`（**175.8 MiB**）**真分配** |
///
/// 「目标很小、中间图很大」⇒ 老护栏完全放行。网页服务端可被这一条打爆
/// （一条 URL 就能让服务端分配 176 MB 并跑到重采样里）。
///
/// # 二、为什么取 `128 MiB`（**不是**任务书建议的 256 MiB）
///
/// 256 MiB 与验收门直接冲突：`200×150 @0.25` 的中间图是 175.8 MiB **< 256 MiB**
/// ⇒ 256 MiB 会把它放行，门 3「该请求必须报错」就不成立了。
/// 取 128 MiB 后的实际边界（换算依据：`@0.5` 时 `目标像素 = 中间像素 / 4`，
/// 所以不设上限时 `@0.5` 天然能到 `4 × 64 MiB = 256 MiB` 中间图）：
///
/// | 视口 @ 档位 | 1× 中间图 | 目标图 | 128 MiB 下 |
/// |---|---|---|---|
/// | `200×150 @0.25` | 175.8 MiB | 11.5 MB | ❌ **拒绝**（要拦的就是这一发）|
/// | `64×48 @0.5`（最常用）| 18.0 MiB | 4.5 MiB | ✅ |
/// | `128×96 @0.5` | 72.0 MiB | 18.0 MiB | ✅ |
/// | `170×128 @0.5` | 127.5 MiB | 31.9 MiB | ✅（正好顶到上限）|
/// | `64×48 @2` | 18.0 MiB | 72.0 MiB | ✅ 中间图过关，**目标图**被 64 MiB 拦下 |
///
/// 即：把 `@0.5` 可达的中间图从 256 MiB 砍到 128 MiB（目标 ≤ 32 MiB），
/// 同时**不误伤**现网所有在跑的视口（最常用的 `64×48` 只用 18 MiB）。
///
/// # 三、[`RenderScale::One`] **不受本上限约束**
///
/// `One` 档**不进重采样、不产生中间图**：[`crate::try_render_region_scaled`] 直接转调
/// 旧路径 `compose::render`，产物就是目标图本身，它只受 [`SCALED_MAX_BYTES`] 与
/// [`Viewport::pixel_size`] 的老护栏约束。把本上限套到 `One` 上等于凭空砍掉一半可渲范围
/// —— 那是误伤（`scale.rs` 的测试钉住：同一 `200×150` 视口 `@1` 必须正常出图）。
///
/// # 四、超限行为
///
/// 返回 [`RenderError::IntermediateTooLarge`]，**在取帧之前**（即 `compose::render`
/// 之前）返回：不硬渲、不静默降级成 1×（那是假数据），错误文案点明「这是 1× 中间图
/// 超限，不是目标图超限」并给出「缩小视口格数」的建议。
pub const INTERMEDIATE_MAX_BYTES: u64 = 128 * 1024 * 1024;

/// 要渲染的世界格窗口。
///
/// 语义：以 `origin_x/origin_y`（**世界格**坐标，可为负）为左上角，铺 `cells_w × cells_h`
/// 个世界格。输出图像素尺寸 = `cells_w * 48 × cells_h * 32`（`scale == 1.0` 时）。
///
/// `scale` **本单只支持 `1.0`**：不做半吊子缩放（见 [`crate::try_render_region`] 的错误路径）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    /// 左上角世界格 X（可为负 —— 图外格不画，也不计入缺帧统计）。
    pub origin_x: i32,
    /// 左上角世界格 Y（可为负）。
    pub origin_y: i32,
    /// 横向世界格数。
    pub cells_w: u32,
    /// 纵向世界格数。
    pub cells_h: u32,
    /// 缩放（本单只支持 `1.0`）。
    pub scale: f32,
}

impl Viewport {
    /// 构造 `scale = 1.0` 的视口（最常用形态）。
    pub const fn new(origin_x: i32, origin_y: i32, cells_w: u32, cells_h: u32) -> Self {
        Viewport {
            origin_x,
            origin_y,
            cells_w,
            cells_h,
            scale: 1.0,
        }
    }

    /// 按 `scale == 1.0` 计算输出像素尺寸；缩放不为 1 或尺寸越界时返回 `None`。
    ///
    /// 独立于渲染主流程，便于 GUI 在「还没开始渲染」时先问尺寸。
    pub fn pixel_size(&self) -> Option<(u32, u32)> {
        if self.scale != 1.0 || self.cells_w == 0 || self.cells_h == 0 {
            return None;
        }
        let w = self.cells_w as u64 * CELL_W as u64;
        let h = self.cells_h as u64 * CELL_H as u64;
        if w > MAX_SIDE || h > MAX_SIDE || w * h > MAX_PIXELS {
            return None;
        }
        Some((w as u32, h as u32))
    }
}

/// 多倍率渲染的倍率档位：`0.25 / 0.5 / 1 / 2`。
///
/// **与 [`Viewport::scale`] 是两件事**：旧入口（[`crate::try_render_region`] /
/// [`crate::try_render_region_with`] / [`crate::render_region`]）仍然**只接受
/// `Viewport::scale == 1.0`**，行为一个字节都不变；多倍率走新入口
/// [`crate::try_render_region_scaled`]，倍率由本枚举**显式**给出。
///
/// 为什么不用 `f32`：`1.0000001` / `0.4999999` 这类「差一点点」的浮点值会让分档变成
/// 隐式猜测（到底算 1× 还是 0.5×？），而档位是**有限四个**的业务事实 ⇒ 用枚举钉死。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RenderScale {
    /// `0.25×`：4×4 源像素块 → 1 目标像素（面积加权平均，不丢细节）。
    Quarter,
    /// `0.5×`：2×2 源像素块 → 1 目标像素（面积加权平均，不丢细节）。
    Half,
    /// `1×`：原始像素尺寸（**与旧路径逐字节相同**，见 `try_render_region_scaled`）。
    One,
    /// `2×`：1 源像素 → 2×2 目标像素（**最近邻**，像素画不许糊）。
    Two,
}

impl RenderScale {
    /// 四个档位（按倍率升序），供 GUI / 网页直接生成下拉框。
    pub const ALL: [RenderScale; 4] = [
        RenderScale::Quarter,
        RenderScale::Half,
        RenderScale::One,
        RenderScale::Two,
    ];

    /// 倍率：`0.25 / 0.5 / 1.0 / 2.0`。
    ///
    /// 四个值在 `f32` 里都**能精确表示**（2 的负整数次幂的整数倍），
    /// 所以 `f32 → f64` 不引入误差，目标尺寸可复算（见 [`RenderScale::pixel_size`]）。
    pub const fn factor(self) -> f32 {
        match self {
            RenderScale::Quarter => 0.25,
            RenderScale::Half => 0.5,
            RenderScale::One => 1.0,
            RenderScale::Two => 2.0,
        }
    }

    /// 规范写法：`"0.25"` / `"0.5"` / `"1"` / `"2"`。
    ///
    /// 保证回环：`RenderScale::from_param(s.label()) == Some(s)`（有测试钉住）。
    pub const fn label(self) -> &'static str {
        match self {
            RenderScale::Quarter => "0.25",
            RenderScale::Half => "0.5",
            RenderScale::One => "1",
            RenderScale::Two => "2",
        }
    }

    /// 是否**缩小**档（`0.25` / `0.5`）。
    ///
    /// 缩小走面积加权（`resample::FilterClass::Box`），放大走 `Nearest`；
    /// 裁决写在 [`crate::scale`] 模块文档里（为什么缩小不能用 Nearest）。
    pub const fn is_downscale(self) -> bool {
        matches!(self, RenderScale::Quarter | RenderScale::Half)
    }

    /// 把网页 / 命令行传来的倍率字符串解析成档位；**只认四个档位**，其余一律 `None`。
    ///
    /// 接受的写法（首尾空白忽略，**大小写敏感**）：
    ///
    /// | 档位 | 可写 |
    /// |---|---|
    /// | `0.25` | `0.25` / `.25` / `025` / `quarter` |
    /// | `0.5` | `0.5` / `.5` / `050` / `half` |
    /// | `1` | `1` / `1.0` / `100` / `one` |
    /// | `2` | `2` / `2.0` / `200` / `two` |
    ///
    /// **不做模糊匹配**：`"2x"`、`"0.3"`、`"1.5"` 一律 `None`。把 `0.3` 静默吸到 `0.25`
    /// 就是假数据 —— 调用方该拿到 `None` 去回 400。
    pub fn from_param(s: &str) -> Option<Self> {
        match s.trim() {
            "0.25" | ".25" | "025" | "quarter" => Some(RenderScale::Quarter),
            "0.5" | ".5" | "050" | "half" => Some(RenderScale::Half),
            "1" | "1.0" | "100" | "one" => Some(RenderScale::One),
            "2" | "2.0" | "200" | "two" => Some(RenderScale::Two),
            _ => None,
        }
    }

    /// 该档位下的目标像素尺寸：`round(cells_w*48*factor) × round(cells_h*32*factor)`。
    ///
    /// `cells_w` / `cells_h` 为 0，或结果装不进 `u32` 时返回 `None`。
    /// **本条只管尺寸**，两条字节护栏都由 [`crate::try_render_region_scaled`] 在渲染前判：
    ///
    /// - 目标图超 [`SCALED_MAX_BYTES`] ⇒ [`RenderError::TooLarge`]；
    /// - 1× 中间图超 [`INTERMEDIATE_MAX_BYTES`] ⇒ [`RenderError::IntermediateTooLarge`]
    ///   （只有缩放档会触发；`One` 不产生中间图）。
    pub fn pixel_size(self, cells_w: u32, cells_h: u32) -> Option<(u32, u32)> {
        if cells_w == 0 || cells_h == 0 {
            return None;
        }
        let (w, h) = 目标像素(cells_w, cells_h, self);
        if w > u64::from(u32::MAX) || h > u64::from(u32::MAX) {
            return None;
        }
        Some((w as u32, h as u32))
    }
}

/// 目标像素尺寸（`u64`，不 cast）—— [`RenderScale::pixel_size`] 与缩放渲染**共用同一处算式**。
///
/// 口径：`round(cells * 世界格 * factor)`（四舍五入）。四个档位的 factor 分别是
/// `1/4`、`1/2`、`1`、`2`，而 `CELL_W = 48` / `CELL_H = 32` 都能被 4 整除
/// ⇒ 乘积本来就是整数，取整是**恒等变换**（保留 `round` 是为了「公式即文档」，
/// 也防将来加非整除档位时口径悄悄变掉）。全程 `f64`：`factor()` 的 `f32` 值精确，
/// 转 `f64` 无误差，结果可复算。
pub(crate) fn 目标像素(cells_w: u32, cells_h: u32, scale: RenderScale) -> (u64, u64) {
    let f = f64::from(scale.factor());
    let w = (f64::from(cells_w) * f64::from(CELL_W) * f).round();
    let h = (f64::from(cells_h) * f64::from(CELL_H) * f).round();
    // 下限 1 px 是**防御性**的（`cells > 0` 时四档都 ≥ 1），不是业务口径
    (w.max(1.0) as u64, h.max(1.0) as u64)
}

/// 三层绘制开关（默认全开）。
///
/// 关掉的层**不画也不计数**（统计里的 `missing_*` 只统计「开着但取不到帧」的情形）——
/// 否则关层会凭空产出缺失计数，误导排查。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerMask {
    /// 地表层（`wBkImg` / `btTiles`，96×64，只铺偶偶格）。
    pub ground: bool,
    /// 中层（`wMidImg` / `btsmTiles`，逐格 48×32 左上角锚定）。
    pub middle: bool,
    /// 表层（`wFrImg` / `btArea`，逐格变尺寸，**底边**钉在格底）。
    pub front: bool,
}

impl LayerMask {
    /// 三层全开（默认口径）。
    pub const fn all() -> Self {
        LayerMask {
            ground: true,
            middle: true,
            front: true,
        }
    }

    /// 三层全关（调试「画布是否真透明」用）。
    pub const fn none() -> Self {
        LayerMask {
            ground: false,
            middle: false,
            front: false,
        }
    }
}

impl Default for LayerMask {
    fn default() -> Self {
        Self::all()
    }
}

/// 渲染选项（地图层）：**开启门** + **地图光照** 两个开关
/// （对应编辑器菜单「视图 → 开启门」（`MainWindow.axaml:5` 的 `DoorsOpenToggle`）与
/// 「视图 → 地图光照」（`MapLighting.cs:3` 的 `MapLighting` 枚举））。
///
/// # 一、默认 = 现状（**逐字节不变**）
///
/// [`Default`] 是 `doors_open: false` + `lighting: MapLighting::Day`：前景层帧号仍只有
/// `(fr_img & 0x7FFF) - 1` 这一项，且**白天档连遮罩都不画**（官方 `MapViewport.cs:89`
/// 的短路口径），与引入本结构之前的输出**逐字节相同**。[`crate::try_render_region`] /
/// [`crate::try_render_region_with`] / [`crate::try_render_region_scaled`] 三个旧入口的
/// **签名一个都没改**，内部一律按本默认值转调 —— 调用方（`miredit-gui` / `miredit-web`）
/// 不改一行也照旧。
///
/// # 二、`doors_open == true` 改什么：**只改前景层的帧号**
///
/// ```text
/// 帧号 = (fr_img & 0x7FFF) - 1                       // 闭门帧（0-based，本仓口径）
/// if doors_open && (door_index & 0x7F) != 0
///     帧号 += door_offset & 0x7F                     // 开门帧偏移
/// ```
///
/// 出处：官方**客户端** `client/reference_full/Classes/scenes/GameScene.cpp:3328-3388`
/// （`:3367`/`:3375` 新旧两档各一份判据、`:3370`/`:3378` 加偏移、`:3381` 再 `-1`、
/// `:3388` `GetObjsEx(btArea, fridx, …)`）。客户端里偏移与 `-1` 的先后无关
/// （都是常数平移），所以「先 `-1` 再加偏移」与原文等价。
///
/// 三条边界（都由 [`crate::compose`] 保证，测试钉住）：
///
/// 1. **只有前景层有门**：地表 / 中层一个像素都不受影响（门帧与闭门帧同属 `Objects` 库，
///    `GameScene.cpp:3341,3388`）；
/// 2. **`door_index & 0x7F == 0` 的格不受影响** —— 没有门的格即便 `door_offset` 非 0 也不加；
/// 3. **不加阻挡、不改数据**：编辑器该项也只改「画哪一帧」（`MapPreview.cs:6-10`），
///    可走性另有 `TMap.cpp:339` 的 `0x8000` 判据（见 [`CollisionOverlay`]），两者不许混。
///
/// # 三、为什么是 `& 0x7F` 掩码（**不能整字节相加**）
///
/// `door_offset` 的高位 `0x80` 是「**运行期已开门**」标志位，**不属于帧偏移**；低 7 位才是
/// 静态帧偏移。全量扫描 `client/热血传奇23周年客户端/Map/` 的 **1097** 张 `.map`
/// （本次独立复扫，与取证报告 `deliverables/取证-地图光照与开启门-2026-11-05.md` §2.4⑥
/// 数字一致）：
///
/// | 统计项 | 数 |
/// |---|---:|
/// | `door_offset & 0x80` **已置位**的格 | **1,764**（分布在 **43** 张图）|
/// | 其中同时有前景图（`fr_img & 0x7FFF != 0`） | **1,251** |
/// | 其中同时有前景图 **且** 门索引非 0（**这就是会画错帧的格数**） | **1,229** |
/// | 上述 1,229 格的库号 `area` 分布 | `0`：**1,173** ／ `255`：47 ／ `70`：5 ／ `132`：2 ／ `150`：2 |
///
/// `area == 0` 就是 `Objects` 库（**真实存在**，9,999 帧）⇒ 那 1,173 格**会真的画出来**，
/// 不是「画不出的空引用」。这类格若按整字节相加，帧号会变成 `+128+low` ⇒ 画出完全无关的帧。
///
/// # 四、为什么跟客户端、不跟官方编辑器
///
/// 官方 C# 编辑器的同一处实现是
/// `image + (doorsOpen && (cell.Door & 0x7F) != 0 ? cell.DoorOffset : 0)`
/// （`CrystalStudio/MapPreview.cs:6-10`）—— 它**没有掩 `0x80`**。按 §三 的实测数据，
/// 那是编辑器在该数据上的**偏差**（会画错上面 1,229 格里的每一格），不是我们自定的口径。
/// 本仓一律以**客户端字节语义**为准（与 [`CollisionOverlay`] 那节「判定按 `TMap.cpp:339`、
/// 只借 C# 的视觉样式」是同一条纪律）。
///
/// # 五、缓存键（调用方注意）
///
/// 门开合**改变的是帧号**，所以凡是「按视口缓存渲染结果」的调用方，必须把
/// `doors_open` 并进缓存键（官方编辑器就是这么做的：`MapViewport.cs:91-99` 的 setter
/// 里 `frontRows.Clear()`，并把 `(DoorsOpen ? 16 : 0)` 并进 `RenderLayerSettings` 第 5 位，
/// `:85`）。本库自身**不缓存跨次结果**（`FrameCache` 只在一次渲染内有效），无需额外处理。
///
/// # 六、`lighting`：地图光照（2026-11-05 新增）
///
/// 三档互斥（官方 `MapLighting.cs:3` 的 `enum MapLighting { DawnDusk, Day, Night }`），
/// **默认 [`MapLighting::Day`]**（`MapViewport.cs:88`），语义是
/// 「**全屏黑色遮罩（alpha = 255 − 环境光）+ 逐格用光罩贴图减 alpha**」，**不是逐格改色**：
///
/// | 档 | 环境光 | 遮罩 alpha | 效果 |
/// |---|---:|---:|---|
/// | [`MapLighting::DawnDusk`] | 50 | 205 | 压暗 80% |
/// | [`MapLighting::Day`] | 255 | 0 | **完全不画遮罩**（= 关） |
/// | [`MapLighting::Night`] | 0 | 255 | 全黑 |
///
/// 逐格光源只从**物件层（表层）**取，条件是该层可见 + `cell.light ∈ 1..=9` + 该格有前景图；
/// 强度取自官方光罩 PNG 的预乘 R 通道，合成式 `alpha = max(0, alpha - intensity)`。
/// 全部出处、11 项尺寸表、`index` / `index+1` 的错位规则见 [`crate::lighting`] 的模块文档。
///
/// 三条**不做**（都是无据自造，明确禁止）：
///
/// 1. 不自己生成衰减曲线 —— 只解码官方 `assets/lights/*.png`；
/// 2. 不微调 `205 / 0 / 255` —— 官方字面量 + 注释双证自洽；
/// 3. 不给三档加过渡动画 —— 官方是三个互斥 `CheckBox`（`MainWindow.axaml.cs:626-633`）。
///
/// 缓存键：与 `doors_open` 同理，**`lighting` 也必须并进调用方的视口缓存键**
/// （官方把 `Lighting` 放进 `darknessKey`，`Presentation.cs:65`）。
///
/// # 例子
///
/// ```
/// use miredit_render::{RenderOptions, Viewport, LayerMask, CollisionOverlay};
/// use miredit_render::try_render_region_with_options;
/// use mir2_map::Map;
/// use mir2_assets::{Frame, LibRef, SpriteSource};
///
/// struct 空源;
/// impl SpriteSource for 空源 {
///     fn frame(&self, _lib: LibRef, _index: u32) -> Option<Frame> { None }
/// }
///
/// // 1×1 格：前景图号 1000、门索引 1、门偏移 0x80|6 = 134
/// let mut bytes = vec![0u8; 52 + 12];
/// bytes[0..2].copy_from_slice(&1u16.to_le_bytes());
/// bytes[2..4].copy_from_slice(&1u16.to_le_bytes());
/// bytes[52 + 4..52 + 6].copy_from_slice(&1000u16.to_le_bytes());
/// bytes[52 + 6] = 1;    // door_index
/// bytes[52 + 7] = 134;  // door_offset = 0x86
/// let map = Map::parse(&bytes)?;
///
/// let vp = Viewport::new(0, 0, 1, 1);
/// let 闭 = try_render_region_with_options(
///     &map, &空源, &vp, &LayerMask::all(), CollisionOverlay::none(), RenderOptions::closed())?;
/// let 开 = try_render_region_with_options(
///     &map, &空源, &vp, &LayerMask::all(), CollisionOverlay::none(),
///     RenderOptions::all_doors_open())?;
/// // 两种口径都取不到帧（空源），但缺帧口径一致：图号非 0 且取不到 ⇒ 计 1 次缺帧
/// assert_eq!(闭.stats.missing_front, 1);
/// assert_eq!(开.stats.missing_front, 1);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RenderOptions {
    /// 是否按「门全开」出图（编辑器菜单「视图 → 开启门」的勾选态）。
    ///
    /// `false`（**默认**）= 闭门：帧号只取 `(fr_img & 0x7FFF) - 1`，与改动前逐字节相同；
    /// `true` = 门全开：对 `door_index & 0x7F != 0` 的格再加 `door_offset & 0x7F`。
    /// 详见 [`RenderOptions`] 的类型文档。
    pub doors_open: bool,
    /// 地图光照档位（编辑器菜单「视图 → 地图光照」的勾选态）。
    ///
    /// [`MapLighting::Day`]（**默认**）= 不画遮罩 ⇒ 与改动前逐字节相同；
    /// [`MapLighting::DawnDusk`] / [`MapLighting::Night`] = 全屏遮罩 + 逐格光罩。
    /// 详见 [`RenderOptions`] 的类型文档 §六 与 [`crate::lighting`]。
    pub lighting: MapLighting,
}

impl RenderOptions {
    /// 闭门口径（= [`Default`]）：帧号不加任何门偏移、光照为白天，与改动前**逐字节相同**。
    pub const fn closed() -> Self {
        RenderOptions {
            doors_open: false,
            lighting: MapLighting::Day,
        }
    }

    /// 门全开口径：`door_index & 0x7F != 0` 的前景格加 `door_offset & 0x7F`。
    pub const fn all_doors_open() -> Self {
        RenderOptions {
            doors_open: true,
            lighting: MapLighting::Day,
        }
    }

    /// 按布尔值构造（GUI 直接把复选框状态传进来用这个）。
    pub const fn with_doors_open(open: bool) -> Self {
        RenderOptions {
            doors_open: open,
            lighting: MapLighting::Day,
        }
    }

    /// 只改光照档位（其余取默认：闭门）。GUI 点「视图 → 地图光照」的三档时用它。
    pub const fn with_lighting(lighting: MapLighting) -> Self {
        RenderOptions {
            doors_open: false,
            lighting,
        }
    }

    /// 两个开关一起给（GUI 把「开启门」与「地图光照」两处状态一起传进来）。
    pub const fn with_doors_and_lighting(open: bool, lighting: MapLighting) -> Self {
        RenderOptions {
            doors_open: open,
            lighting,
        }
    }
}

/// 低/高位阻挡标记的显示口径（编辑器「碰撞层」可视化）。
///
/// # 语义（**以客户端为准**，不看编辑器自己的位编码）
///
/// - **低阻挡** = `wBkImg & 0x8000`（背景层带 `0x8000` 位）；
/// - **高位阻挡** = `wFrImg & 0x8000`（前景层带 `0x8000` 位）。
///
/// 出处：客户端 `client/reference_full/Classes/map/TMap.cpp:339`
/// （`CanWalk` 里 `((wBkImg & 0x8000) + (wFrImg & 0x8000)) == 0` 才可走）；
/// Rust 侧同口径见 `mir2-map/src/cell.rs:204-212` 的 `Cell::is_walkable`。
///
/// ⚠ **与 C# 编辑器的差异**：`CrystalStudio` 把「低/高阻挡」编成 `BackImage` 的
/// `0x20000000` / `0x40000000` 两个**自有位**（`MapViewport.cs:23`），那是编辑器内部的
/// 待写回状态，**不是客户端语义**。本库只借它的**视觉样式**，判定一律走 `0x8000`。
///
/// # 视觉（借 C# 的样式）
///
/// 整格 48×32 铺一层半透明纯色：低阻挡 `RGB(240,85,105)`、高位阻挡 `RGB(65,165,245)`，
/// 默认 alpha `90/255`（`CollisionMarkers.cs:6-7` 的 `Color.FromArgb(90, …)`）；
/// 同格两者都有 ⇒ **左半格**（24×32）低阻挡、**右半格**高阻挡（`CollisionMarkers.cs:19-23`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CollisionOverlay {
    /// 是否叠加**低阻挡**标记（`wBkImg & 0x8000`）。
    pub low: bool,
    /// 是否叠加**高位阻挡**标记（`wFrImg & 0x8000`）。
    pub high: bool,
    /// 标记透明度：`0` = 一个像素都不写（见 [`CollisionOverlay::none`] 的说明），
    /// `255` = 实心。默认 [`CollisionOverlay::DEFAULT_ALPHA`]。
    pub alpha: u8,
}

impl CollisionOverlay {
    /// 默认标记透明度 `90`。
    ///
    /// 出处：`CrystalStudio/CollisionMarkers.cs:6-7`
    /// （`Color.FromArgb(90, 240, 85, 105)` / `Color.FromArgb(90, 65, 165, 245)`，首参即 alpha）。
    pub const DEFAULT_ALPHA: u8 = 90;

    /// 两个通道都关（= 旧行为：默认不画阻挡）。
    ///
    /// 这是 [`Default`] 的取值，也是旧函数 [`crate::try_render_region`] 内部转调时用的口径
    /// —— 「默认不画阻挡」这条兼容性靠它保证。
    pub const fn none() -> Self {
        CollisionOverlay {
            low: false,
            high: false,
            alpha: Self::DEFAULT_ALPHA,
        }
    }

    /// 只开低阻挡通道。
    pub const fn low_only() -> Self {
        CollisionOverlay {
            low: true,
            high: false,
            alpha: Self::DEFAULT_ALPHA,
        }
    }

    /// 只开高位阻挡通道。
    pub const fn high_only() -> Self {
        CollisionOverlay {
            low: false,
            high: true,
            alpha: Self::DEFAULT_ALPHA,
        }
    }

    /// 两个通道都开。
    pub const fn all() -> Self {
        CollisionOverlay {
            low: true,
            high: true,
            alpha: Self::DEFAULT_ALPHA,
        }
    }

    /// 是否有通道开着（两个都关 ⇒ 整个叠加环节直接跳过，输出与旧路径逐字节相同）。
    pub const fn any(self) -> bool {
        self.low || self.high
    }
}

impl Default for CollisionOverlay {
    /// 默认 **两个通道都关**（向后兼容：默认渲染不出现任何标记）。
    fn default() -> Self {
        Self::none()
    }
}

/// 一次渲染的统计（用于「如实报缺口」而不是靠眼睛猜）。
///
/// 全部是**实际发生的事**，不是估算：
/// - `missing_*` 只统计「该层有图号、但帧源返回 `None`」的格数；
/// - **图号为 0（该层本来就没图）不计入 missing** —— 空地不是缺帧，混进来会让计数失去意义；
/// - 窗口越出地图范围的格不画、也不计入任何计数；
/// - `cells_low_blocked` / `cells_high_blocked` 只统计**真的画出了标记**的格数
///   （通道关着、或 `alpha == 0` ⇒ 一律 0）；它们**不计入** `cells_drawn`
///   —— `cells_drawn` 的口径始终是「地表/中层/表层三层」的落笔，阻挡标记是独立通道。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderStats {
    /// 至少有一层**真的往画布写了至少 1 个像素**的世界格数（同一格画了多层只算 1）。
    ///
    /// 口径是「实际产生了绘制」，**不是「扫过了这一格」**。以下情形一律不计：
    /// 该层图号为 0（本来就没图）、帧取不到（缺帧 —— 那种格计入 `missing_*`）、
    /// 0×0 空帧、帧与画布无交集（整帧落在画布外）、帧落在画布内的像素全透明。
    pub cells_drawn: u32,
    /// 地表层：有图号但帧不可得（库缺 / 帧号越界 / 空帧）的格数。
    pub missing_ground: u32,
    /// 中层：同上。
    pub missing_middle: u32,
    /// 表层：同上。
    pub missing_front: u32,
    /// 真正向图库发起的**成功解码**次数（帧缓存未命中且取到帧才 +1；命中缓存的重复使用不重复计）。
    pub frames_decoded: u32,
    /// 本次渲染**真的画出低阻挡标记**的世界格数（`wBkImg & 0x8000` 且该通道开着且 `alpha > 0`）。
    pub cells_low_blocked: u32,
    /// 同上，高位阻挡（`wFrImg & 0x8000`）。
    pub cells_high_blocked: u32,
}

impl core::fmt::Display for RenderStats {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "cells_drawn={} missing_ground={} missing_middle={} missing_front={} frames_decoded={} \
             cells_low_blocked={} cells_high_blocked={}",
            self.cells_drawn,
            self.missing_ground,
            self.missing_middle,
            self.missing_front,
            self.frames_decoded,
            self.cells_low_blocked,
            self.cells_high_blocked
        )
    }
}

/// 一次渲染的产物：行优先 RGBA8 + 尺寸 + 统计 + 倍率。
///
/// 底色是**全透明黑** `(0,0,0,0)`，不是任何灰/白背景 —— 上层（GUI 或比对脚本）自己决定
/// 叠在什么底色上看。这一点必须保持：客户端截图有它自己的清屏色，本库不该替它选。
///
/// # 为什么标 `#[non_exhaustive]`
///
/// 本单在结构体上**新增了 [`Rendered::scale`] 字段**。加字段对「只读字段」的调用方
/// （`miredit-gui` / `miredit-web` / 三个集成测试，全仓仅 `compose.rs` 一处构造）零影响，
/// 但 `#[non_exhaustive]` 顺手把「以后再加字段也不破外部代码」这条护栏补上：
/// 外部 crate 从此不能再用结构体字面量构造 / 穷尽解构它（现在也没有人这么做），
/// 构造权收归本 crate。
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Rendered {
    /// 像素宽。
    pub width: u32,
    /// 像素高。
    pub height: u32,
    /// 行优先 RGBA8，长度 = `width * height * 4`。
    pub rgba: Vec<u8>,
    /// 本次渲染统计。
    pub stats: RenderStats,
    /// 本张图出自哪个倍率档位。
    ///
    /// 旧入口（[`crate::try_render_region`] / [`crate::try_render_region_with`] /
    /// [`crate::render_region`]）出的图**恒为 [`RenderScale::One`]**；只有
    /// [`crate::try_render_region_scaled`] 会给出其它档位。
    /// 上层据此判断「这张位图要不要按倍率再换算坐标」，不用自己记调用过哪个入口。
    pub scale: RenderScale,
}

impl Rendered {
    /// 取某像素 RGBA；越界返回 `None`。
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        let s = self.rgba.get(i..i + 4)?;
        Some([s[0], s[1], s[2], s[3]])
    }

    /// 导出为 PNG（RGBA8）。文件不存在则报错，不做隐式建目录。
    pub fn save_png(&self, path: &std::path::Path) -> std::io::Result<()> {
        crate::png_io::write_rgba_png(path, self.width, self.height, &self.rgba)
    }
}

/// 渲染前置校验失败的原因。
///
/// 只覆盖「**能明确说清**」的几类：不支持的缩放、空窗口、尺寸越界。
/// 缺帧**不是错误**（那是数据事实，计入 [`RenderStats`]）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    /// 本单只支持 `scale == 1.0`；其它值一律拒绝，不做半吊子缩放。
    UnsupportedScale {
        /// 传入的 `scale`（用 `f32::to_bits` 之外的整型表达，便于 `Eq` 与日志）。
        scale_milli: i64,
    },
    /// `cells_w` 或 `cells_h` 为 0：没有可画的格。
    ZeroCells,
    /// 像素尺寸越界（防御性护栏，避免手抖输入直接 OOM）。
    DimensionOverflow {
        /// 期望像素宽。
        width: u64,
        /// 期望像素高。
        height: u64,
    },
    /// 印章缩放的非法取值（`NaN` / `±inf` / `≤ 0`）。
    ///
    /// 与 [`RenderError::UnsupportedScale`] **不是一回事**：地图渲染只接受 `1.0`，
    /// 而印章缩略图**允许任意正比例**（面板要 46×46，网页要 96×96）。
    /// 这里拒绝的只是「物理上无意义」的比例。
    InvalidStampScale {
        /// 传入的 `scale`（十进制毫值；`NaN` 饱和成 `0`，`±inf` 饱和成 `i64::MAX`）。
        scale_milli: i64,
    },
    /// 印章缩略图的**目标框**非法：宽或高为 0，或框比两侧留白还小（没有可画区域）。
    InvalidStampBox {
        /// 目标框宽（像素）。
        box_w: u32,
        /// 目标框高（像素）。
        box_h: u32,
    },
    /// **缩放后**的输出缓冲超过字节上限 [`SCALED_MAX_BYTES`]（64 MiB）。
    ///
    /// 由 [`crate::try_render_region_scaled`] 在**分配任何缓冲之前**返回：
    /// 既不硬渲（那会先 OOM 或把网页拖死），也**不静默降级成 1×**（那是假数据 ——
    /// 调用方要 2× 却拿到 1×，前端会按 2× 的尺寸去贴图）。
    ///
    /// ⚠ 与 [`RenderError::IntermediateTooLarge`] 是**两个不同的缓冲**：本条说「你要的
    /// 那张图太大」，那条说「我内部那张 1× 中间图太大」。
    TooLarge {
        /// 目标尺寸所需的字节数（`w * h * 4`）。
        want: u64,
        /// 上限（恒等于 [`SCALED_MAX_BYTES`]）。
        limit: u64,
    },
    /// **1× 中间图**超过字节上限 [`INTERMEDIATE_MAX_BYTES`]（128 MiB）。
    ///
    /// 只在**缩放档**（`0.25` / `0.5` / `2`）出现：它们内部要先合成一整幅 1× 再重采样
    /// ⇒ 目标图可能很小、中间图却很大（`200×150 @0.25`：目标 11.5 MB / 中间图 175.8 MiB）。
    /// [`RenderScale::One`] **不产生中间图**，永远走不到这个变体。
    ///
    /// 由 [`crate::try_render_region_scaled`] 在**取帧之前**返回（不硬渲、不静默降级）。
    IntermediateTooLarge {
        /// 1× 中间图所需的字节数（`原宽 * 原高 * 4`）。
        want: u64,
        /// 上限（恒等于 [`INTERMEDIATE_MAX_BYTES`]）。
        limit: u64,
    },
}

impl core::fmt::Display for RenderError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            RenderError::UnsupportedScale { scale_milli } => write!(
                f,
                "只支持 scale == 1.0，收到 scale_milli = {scale_milli}（= {}）",
                *scale_milli as f64 / 1000.0
            ),
            RenderError::ZeroCells => f.write_str("cells_w / cells_h 不能为 0"),
            RenderError::DimensionOverflow { width, height } => write!(
                f,
                "输出尺寸 {width}×{height} 超出护栏（单边 ≤ {MAX_SIDE}，总像素 ≤ {MAX_PIXELS}）"
            ),
            RenderError::InvalidStampScale { scale_milli } => write!(
                f,
                "印章缩放比例必须是有限正数，收到 scale_milli = {scale_milli}（= {}）",
                *scale_milli as f64 / 1000.0
            ),
            RenderError::InvalidStampBox { box_w, box_h } => write!(
                f,
                "印章缩略图目标框非法：{box_w}×{box_h}（宽高必须 > 0，且减去两侧留白后仍要有可画区域）"
            ),
            RenderError::TooLarge { want, limit } => write!(
                f,
                "缩放输出需要 {want} 字节（{:.1} MiB），超过上限 {limit} 字节（{:.0} MiB）：\
                 请缩小视口格数或降低倍率（不硬渲，也不静默降级成 1×）",
                *want as f64 / (1024.0 * 1024.0),
                *limit as f64 / (1024.0 * 1024.0)
            ),
            RenderError::IntermediateTooLarge { want, limit } => write!(
                f,
                "1× 中间图需要 {want} 字节（{:.1} MiB），超过中间图上限 {limit} 字节（{:.0} MiB）：\
                 注意这是「1× 中间图」超限，不是目标图超限（目标图另有 {:.0} MiB 上限，\
                 本例的目标图远小于此）；缩放档要先按 1× 合成再重采样，中间图与目标图是两个缓冲。\
                 建议缩小视口格数（cells_w × cells_h），或改用 1× 档（1× 不产生中间图）\
                 （不硬渲，也不静默降级成 1×）",
                *want as f64 / (1024.0 * 1024.0),
                *limit as f64 / (1024.0 * 1024.0),
                SCALED_MAX_BYTES as f64 / (1024.0 * 1024.0)
            ),
        }
    }
}

impl std::error::Error for RenderError {}
