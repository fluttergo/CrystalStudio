//! 运行期文案装配：把「文档状态 + 视口状态 + 鼠标所在格 + 编辑会话」翻成这一帧要画的全部文字。
//!
//! # 为什么要独立一层
//! 界面上带真实数据的文字有 15 处（标题栏、页签、状态栏左/工具/**当前层**/笔刷/**落笔字段**/中/右、
//! 缩放百分比、资源面板分页行、单元格检查器标题 + 六行值）。如果各处自己 `format!`，那么
//! ① 文案模板会散落在绘制代码里（违反「文案唯一来源」）；
//! ② 无法在无窗口测试里核对"空数据时到底显示什么"。
//!
//! 于是：本模块**只做装配**（全部走 [`crate::ui_text`] 的模板），绘制方只管排版；
//! 单元测试直接断言"失败态显示什么"、"没悬停时显示什么"、"有未保存修改时标题带不带 `*`"。
//!
//! # 取值优先级（状态栏左段，自上而下）
//! 1. 地图打开失败 ⇒ `地图打开失败：<原因>`（原因同时已写 stderr）；
//! 2. **保存失败** ⇒ `保存失败：<原因>（原文件未改动）`（用户刚按的 Ctrl+S，必须最显眼）；
//! 3. 资源库装载失败 ⇒ `资源库装载失败：<原因>`；
//! 4. 一切正常 ⇒ `<文件名> · <宽>×<高> 格`；
//! 5. 一张图都没打开 ⇒ `未打开地图`。
//!
//! 为什么不把几条都挤进状态栏：状态栏左段可用宽只有窗口的一半，
//! 多条错误同时存在时**先报最要紧的**（没地图什么都干不了 / 刚存失败最需要处理），
//! 其余几条仍在 stderr 里。

use crate::canvas_view::CanvasView;
use crate::cellinfo::CellLines;
use crate::edit::{brush_lib_text, EditSession, FillRecord, Tool};
use crate::mapdoc::DocState;
use crate::ui_text as tx;

/// 这一帧要画的全部动态文字。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocText {
    /// 窗口标题 / 自绘标题栏文字（有未保存修改时带 ` *`）。
    pub title: String,
    /// 页签标题（地图文件名；有未保存修改时带 ` *`）。
    pub tab: String,
    /// 工具选项面板里那颗胶囊的文字：`7×7` / `填充` / `1×1`。
    ///
    /// **本版新增**（2026-11-04 审计 P0-5）：上一版这里写死 `3×3`（`ui_text::PILL_TEXT`），
    /// 改笔刷尺寸时胶囊不动，等于在骗人。
    pub pill: String,
    /// 当前**笔刷尺寸**（3..=31 的奇数）——滑杆滑块的位置要用它（见
    /// [`crate::dock_left::thumb_rect`]）。胶囊文字已经含尺寸，但那是**给人看的字符串**，
    /// 几何计算不该去解析它。
    pub brush_size: usize,
    /// 状态栏左段。
    pub status_left: String,
    /// 状态栏中段：当前工具（`工具：笔刷`）。
    pub status_tool: String,
    /// 状态栏中段：**当前编辑层**（`当前层：物体`）。
    pub status_layer: String,
    /// 状态栏中段：当前笔刷内容（`笔刷：物体 图号 901 · 资源库 4`）。
    pub status_brush: String,
    /// 状态栏中段：落笔字段（`落笔 fr_img + btArea`）或**族不匹配拒绝原因**。
    pub status_hint: String,
    /// 上面那句是不是"拒绝"（决定它用警示色还是次要色）。
    pub status_hint_warn: bool,
    /// 状态栏中段（鼠标所在格）。
    pub status_coord: String,
    /// 状态栏右段（资源库数量）。
    pub status_lib: String,
    /// 状态栏：**磁盘格式**（九格式人读名，`miredit_core::Document::format_label`）。
    ///
    /// **本版新增**（2026-11-05 九格式收口）：打开一张 Type5 时，左段的 `{w}×{h} 格`
    /// 说的是**视图**口径（三档投影），磁盘上其实既不是 12 也不是 14/36 —— 格式必须单列。
    /// 没打开地图时是**空串**（状态栏整段不画：左段已经写着「未打开地图」，再挂一句
    /// 「格式：—」是纯噪音）。
    pub status_format: String,
    /// **常驻黄条**：`miredit_core::Document::edit_warning` 的**原文**（`None` = 没有要提醒的事）。
    ///
    /// **本版新增**：两种来源（可能同时成立，用「；」连接）——
    /// ① 未解尾块（实测那 67 张真实文件都是 8040 字节）；
    /// ② 格式未承诺编辑语义（`caps().editable == false`，**逐格式**带自己的 `editable_caveat`；Old12 自 2026-11-05 起也是 `true`）。
    ///
    /// ⚠ **不是 toast、不自动消失**（与网页版同一做法）：它描述的是"这张图本身的性质"，
    /// 用户画一整天它都在。绘制见 [`crate::banner`]。
    pub banner: Option<String>,
    /// 地表图层是否**不可落笔**（`Document::ground_paint_supported() == false`）。
    ///
    /// 九种里只有 Type5（`Mir3Wemade`）为 `true`；为真时 ① 地表图层行挂
    /// [`tx::GROUND_LOCKED_MARK`] 标记 ② 点画布前就被拦下（[`crate::app`]）。
    pub ground_locked: bool,
    /// 状态栏：**当前选中的印章**（`印章：<名称>`）；`None` = 没选，整段不画。
    ///
    /// **本版新增**（资源面板「分类」页签）：选中一枚印章后要在状态栏留一句可见的回执，
    /// 否则"点了没反应"看起来像坏了。没选时**不写占位**（`印章：无` 那种话没有信息量）。
    /// 由 `MireditApp::update` 在 [`build`] 之后填（[`build`] 本身不碰印章状态，
    /// 免得 30 多个既有测试调用点全改签名）。
    pub status_stamp: Option<String>,
    /// 状态栏：上一次填充的格数（`填充 1234 格`）；`None` = 没有可显示的填充回执。
    ///
    /// **本版新增**：官方把填充结果写进状态栏（`MapViewport.cs:537`），本仓放在
    /// `status.hint` 之后、坐标段之前（见 [`crate::statusbar::dyn_labels`]）。
    pub status_fill: Option<String>,
    /// 状态栏：**上一次选区 / 印章操作的回执**（`放置印章「mir_00000」8 格` /
    /// `剪切 3×3 格（清空 9 格）` / 拒绝原因）。
    ///
    /// **本版新增**（2026-11-05）：这些操作的回执原先只能挤进 `status.hint`
    /// （那一段是"这一笔会改哪些字节"），挤进去就等于把落笔字段顶掉。
    /// 单列一段之后，落笔字段始终在，回执也始终在。
    ///
    /// 由 [`crate::app`] 在 [`build`] 之后填（与 `status_stamp` 同一条纪律：
    /// `build` 不碰"最近一次操作"这种跨帧状态）。
    pub status_op: Option<String>,
    /// `status_op` 那一段是不是**拒绝**（决定用警示粉还是正文色）。
    ///
    /// 为什么要有它：成功回执被染成警示色就是假警报（用户会以为操作失败了）。
    pub status_op_warn: bool,
    /// 状态栏：**选区 / 剪贴板**（`选区 3×3（200,200..202,202）· 剪贴板 3×3`）。
    ///
    /// **本版新增**：这是**纯 UI 状态**（内核保证选区/剪贴板不置脏、不进撤销栈），
    /// 所以它每帧从会话现算（[`crate::selection_ops::status_text`]），`None` = 整段不画。
    pub status_sel: Option<String>,
    /// 生成 [`Self::status_sel`] 的那条模板（三条互斥，`DynLabel` 要求模板登记在
    /// [`crate::ui_text::ALL`] 里）。与 `status_sel` **同生同灭**（要么都 `Some`、要么都 `None`）。
    pub status_sel_tmpl: Option<&'static str>,
    /// 缩放浮层百分比。
    pub zoom: String,
    /// 资源面板分页行（资源库数量）。
    pub page: String,
    /// 单元格检查器标题行。
    pub inspector_head: String,
    /// 单元格检查器六行值。
    pub inspector_values: [String; 6],
}

/// 工具选项胶囊里该写什么 —— **照官方 `MainWindow.axaml.cs:932` 的三分支**。
///
/// ```csharp
/// BrushSizeLabel.Text = (Viewport.Tool == EditorTool.Bucket ? LocalText.T("Fill")
///     : (Viewport.Tool == EditorTool.Pencil ? ("1×1") : ($"{Viewport.BrushSize}×{Viewport.BrushSize}")));
/// ```
///
/// 本仓对应：填充 ⇒ [`tx::TOOL_BUCKET`]（官方复用的就是 `Fill` 这条文案）；
/// 铅笔本单未接线（[`Tool::Unwired`]），故不会走到 `1×1` 那一支 —— 分支留着，
/// 文案 [`tx::PILL_PENCIL`] 也在清单里（将来接铅笔时直接用）。
/// 其余工具 ⇒ `N×N`。
pub fn pill_text(tool: Tool, size: usize) -> String {
    match tool {
        Tool::Bucket => tx::TOOL_BUCKET.to_owned(),
        _ => tx::fill(tx::PILL_SIZE_TMPL, &[("n", &size.to_string())]),
    }
}

/// 状态栏里"填充结果"那一段的文字（`None` = 不显示）。
///
/// 图号层 ⇒ `填充 N 格`；阻挡层 ⇒ `应用低位阻挡 N 格`（官方 `:537` 对这两种是两句不同的话）。
pub fn fill_text(rec: Option<FillRecord>) -> Option<String> {
    let rec = rec?;
    Some(if rec.layer.is_collision() {
        tx::fill(
            tx::STATUS_FILL_COLLISION_TMPL,
            &[("layer", rec.layer.name()), ("n", &rec.cells.to_string())],
        )
    } else {
        tx::fill(tx::STATUS_FILL_TMPL, &[("n", &rec.cells.to_string())])
    })
}

/// 装配一帧的全部动态文字。
///
/// - `hover`：鼠标所在世界格（`None` = 不在图上）；
/// - `lines`：该格的六行信息（由调用方缓存 —— 它要问资源库要帧，不能每帧重算）；
/// - `ed`：编辑会话（工具 / 笔刷 / 脏标记 / 最近一次保存结果）。
pub fn build(
    doc: &DocState,
    view: &CanvasView,
    hover: Option<(u16, u16)>,
    lines: &CellLines,
    ed: &EditSession,
) -> DocText {
    let lib_n = doc.assets.lib_count().to_string();
    let dirty = ed.dirty();

    // ① 状态栏左段：见模块文档的优先级
    let status_left = if let Some(err) = doc.map_error.as_deref() {
        tx::fill(tx::STATUS_MAP_FAIL_TMPL, &[("err", err)])
    } else if let Some(err) = ed.save_error() {
        tx::fill(tx::STATUS_SAVE_FAIL_TMPL, &[("err", err)])
    } else if let Some(err) = doc.assets.error() {
        tx::fill(tx::STATUS_LIB_FAIL_TMPL, &[("err", err)])
    } else if let Some(d) = doc.doc.as_ref() {
        tx::fill(
            tx::STATUS_LEFT_TMPL,
            &[
                ("name", &d.file_name),
                ("w", &d.map.width.to_string()),
                ("h", &d.map.height.to_string()),
            ],
        )
    } else {
        tx::STATUS_NO_MAP.to_owned()
    };

    // ② 状态栏中段：工具 + **当前编辑层** + 笔刷 + 落笔字段（没取过笔刷就如实说没取）
    let status_tool = tx::fill(tx::STATUS_TOOL_TMPL, &[("tool", ed.tool().name())]);
    let layer = ed.layer();
    let status_layer = tx::fill(tx::STATUS_LAYER_TMPL, &[("layer", layer.name())]);
    let fmt = doc.doc.as_ref().map(|d| d.map.format);
    let status_brush = match ed.brush() {
        Some(b) => tx::fill(
            tx::STATUS_BRUSH_TMPL,
            &[
                ("fam", b.family_name()),
                ("img", &b.sprite().to_string()),
                // 资源面板选的笔刷带真实库号（12B 图也显示它），吸管在 12B 图上取的是 `—`
                ("lib", &brush_lib_text(fmt, &b)),
            ],
        ),
        None => tx::STATUS_BRUSH_NONE.to_owned(),
    };
    // ③ 状态栏中段：落笔字段（三档口径分开说 —— 12B 档没有库号字节，不假装写过；
    //    阻挡层单独一句：它改的是 bit15，不是图号，与档位无关）
    let (status_hint, status_hint_warn) = match ed.note() {
        // 族不匹配 / 图库笔刷落到阻挡层：原因由 `edit` 侧装好（含两边层名），这里只负责显眼
        Some(note) => (note.to_owned(), true),
        None => (
            match fmt {
                None => tx::STATUS_HINT_NO_MAP.to_owned(),
                // 阻挡层：**先于档位判定** —— 12/14/36 三档的 `0x8000` 语义完全一样，
                // 说"本图 12B 档无库号字节"在这里是答非所问。
                Some(_) if layer.is_collision() => tx::fill(
                    tx::STATUS_WRITE_COLLISION_TMPL,
                    &[("img", layer.img_field()), ("layer", layer.name())],
                ),
                Some(f) if f.has_lib_bytes() => tx::fill(
                    tx::STATUS_WRITE_TMPL,
                    &[
                        ("img", layer.img_field()),
                        ("lib", layer.lib_field().unwrap_or(tx::VAL_UNKNOWN)),
                    ],
                ),
                Some(_) => tx::fill(tx::STATUS_WRITE_12B_TMPL, &[("img", layer.img_field())]),
            },
            false,
        ),
    };

    // ④ 状态栏中段：没悬停就是 `—`（不写一个假的 0,0）
    let status_coord = match hover {
        Some((x, y)) => tx::fill(
            tx::STATUS_COORD_TMPL,
            &[("x", &x.to_string()), ("y", &y.to_string())],
        ),
        None => tx::VAL_UNKNOWN.to_owned(),
    };

    // ⑤ 标题 / 页签：有未保存修改时加 ` *`
    let (title, tab) = match doc.doc.as_ref() {
        Some(d) => {
            let (tmpl, tab_tmpl) = if dirty {
                (tx::TITLE_DIRTY_TMPL, tx::TAB_TITLE_DIRTY_TMPL)
            } else {
                (tx::TITLE_TMPL, tx::TAB_TITLE_TMPL)
            };
            (
                tx::fill(
                    tmpl,
                    &[
                        ("name", &d.file_name),
                        ("w", &d.map.width.to_string()),
                        ("h", &d.map.height.to_string()),
                    ],
                ),
                tx::fill(tab_tmpl, &[("name", &d.file_name)]),
            )
        }
        None => (tx::WINDOW_TITLE_EMPTY.to_owned(), tx::VAL_UNKNOWN.to_owned()),
    };

    // ⑥ **磁盘格式 / 常驻黄条 / 地表不可落笔**（2026-11-05 九格式收口）
    //
    //    三者全部来自 `miredit_core::Document` 的真值，**宿主一个字符都不加工**：
    //    - `format_label()` 是 `mir2-format` 的判别结果（不是从格长反推的档位名）；
    //    - `edit_warning()` 是 core 拼好的中文原文（未解尾块 / 未承诺编辑语义）；
    //    - `ground_paint_supported()` 是 `caps()` 的能力位（九种里只有 Type5 为假）。
    //
    //    没打开地图时：格式段**整段不存在**（空串 ⇒ 状态栏不画那一段；左段已经写着
    //    「未打开地图」，再挂一句「格式：—」是纯噪音），黄条不存在，地表不挂标记
    //    （**没有地图就没有能力**，不许凭空说"可以落笔"）。
    let (status_format, banner, ground_locked) = match doc.doc.as_ref() {
        Some(d) => (
            tx::fill(tx::STATUS_FORMAT_TMPL, &[("fmt", d.format_label())]),
            d.edit_warning(),
            !d.ground_paint_supported(),
        ),
        None => (String::new(), None, false),
    };

    DocText {
        title,
        tab,
        pill: pill_text(ed.tool(), ed.brush_size()),
        brush_size: ed.brush_size(),
        status_left,
        status_tool,
        status_layer,
        status_brush,
        status_hint,
        status_hint_warn,
        status_coord,
        status_lib: tx::fill(tx::STATUS_LIB_TMPL, &[("n", &lib_n)]),
        status_format,
        banner,
        ground_locked,
        // 印章那一段由调用方在 build 之后填（见 DocText::status_stamp）
        status_stamp: None,
        status_fill: fill_text(ed.last_fill()),
        // 「上一次操作回执」同样由调用方填（`app` 在成功/拒绝时写入，撤销时清掉）
        status_op: None,
        status_op_warn: false,
        // 选区/剪贴板是**纯 UI 状态**，每帧现算（不置脏、不进撤销栈）
        status_sel: crate::selection_ops::status_text(ed.selection(), ed.clipboard()),
        status_sel_tmpl: crate::selection_ops::status_tmpl(ed.selection(), ed.clipboard()),
        zoom: tx::fill(
            tx::ZOOM_PCT_TMPL,
            &[("pct", &view.zoom_percent().to_string())],
        ),
        page: tx::fill(tx::PAGE_LABEL_TMPL, &[("n", &lib_n)]),
        inspector_head: lines.head.clone(),
        inspector_values: lines.values.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::{EditSession, TOOL_BRUSH_IDX, TOOL_PICKUP_IDX, TOOL_ERASER_IDX};
    use crate::mapdoc::{Assets, MapDoc};
    use mir2_map::Map;
    use std::path::Path;

    /// 2×2 `Old12` 合成图的**字节**（构造 `MapDoc` 要的是字节，不是 `Map`）。
    fn 小图字节() -> Vec<u8> {
        let mut bytes = vec![0u8; 52 + 12 * 4];
        bytes[0..2].copy_from_slice(&2u16.to_le_bytes());
        bytes[2..4].copy_from_slice(&2u16.to_le_bytes());
        bytes
    }

    fn 小图() -> Map {
        Map::parse(&小图字节()).expect("合成图应能解析")
    }

    fn 文档(assets: Assets) -> DocState {
        let mut d = 文档字节(小图字节());
        d.assets = assets;
        d
    }

    /// 用给定字节造一份 `DocState`（图库固定为失败态：本模块一个字节都不看图库）。
    ///
    /// ⚠ 2026-11-05：`MapDoc` 多了一个私有的 `document`（字节层真值），字面量构造不再可能，
    /// 统一走 `MapDoc::from_bytes`（与生产路径同一份 `read_any_tolerant` 判别）。
    fn 文档字节(bytes: Vec<u8>) -> DocState {
        let mapdoc =
            MapDoc::from_bytes(Path::new(r"E:\x\0.map"), bytes).expect("合成图应能解析");
        DocState {
            doc: Some(mapdoc),
            assets: Assets::Failed("x".to_string()),
            map_error: None,
            map_gen: 1,
        }
    }

    /// 空会话（画笔未接线内容、未取笔刷、不脏）。
    fn 空会话() -> EditSession {
        EditSession::default()
    }

    /// 资源库失败 ⇒ 状态栏左段报资源库失败，右段如实 0（不是 117）。
    #[test]
    fn 图库失败如实报零() {
        let d = 文档(Assets::Failed("根目录不存在".to_string()));
        let t = build(&d, &CanvasView::default(), None, &CellLines::unknown(), &空会话());
        assert_eq!(t.status_left, "资源库装载失败：根目录不存在");
        assert_eq!(t.status_lib, "资源库 0 个");
        assert_eq!(t.page, "资源库 0 个");
    }

    /// 本机资源库（未设 `MIREDIT_CLIENT_ROOT` 或装不上就返回 `None`，让相关测试跳过大字报路径 —— 换机器不该变红）。
    fn 就绪图库() -> Option<Assets> {
        let root = mir2_assets::client_root()?;
        let paths = mir2_assets::LibPaths::discover(&root).ok()?;
        let src = mir2_assets::WilSource::open(&paths).ok()?;
        Some(Assets::Ready {
            paths,
            src: Box::new(src),
        })
    }

    /// 正常态：左段是真实文件名与尺寸，坐标没悬停时是 `—`。
    #[test]
    fn 正常态文案() {
        let Some(assets) = 就绪图库() else {
            eprintln!(
                "[跳过] 未设置 {}（图库扫描根）或本机装不上资源库",
                mir2_assets::CLIENT_ROOT_ENV
            );
            return;
        };
        let d = 文档(assets);
        let t = build(&d, &CanvasView::default(), None, &CellLines::unknown(), &空会话());
        assert_eq!(t.status_left, "0.map · 2×2 格");
        assert!(t.status_lib.starts_with("资源库 "), "{}", t.status_lib);
        assert!(
            !t.status_lib.contains(" 0 "),
            "真实库数不该是 0：{}",
            t.status_lib
        );
        assert_eq!(t.status_coord, "—");
        assert_eq!(t.tab, "0.map");
        assert_eq!(t.title, "0.map • 2 × 2 格 — Crystal Studio");
        assert_eq!(t.zoom, "100%");
        assert_eq!(t.page, t.status_lib);
        assert_eq!(t.inspector_head, "—");
    }

    /// 地图失败优先于图库失败（没地图什么都干不了）。
    #[test]
    fn 地图失败优先报() {
        let mut d = 文档(Assets::Failed("图库也挂了".to_string()));
        d.doc = None;
        d.map_error = Some("长度不符".to_string());
        let t = build(&d, &CanvasView::default(), None, &CellLines::unknown(), &空会话());
        assert_eq!(t.status_left, "地图打开失败：长度不符");
        assert_eq!(t.tab, "—");
        assert_eq!(t.title, "Crystal Studio");
    }

    /// 悬停到某格：状态栏中段的坐标必须是真实坐标（不在图上就是 `—`）。
    #[test]
    fn 悬停坐标() {
        let d = 文档(Assets::Failed("x".to_string()));
        let t = build(
            &d,
            &CanvasView::default(),
            Some((380, 310)),
            &CellLines::unknown(),
            &空会话(),
        );
        assert_eq!(t.status_coord, "单元格 380,310");
        let t = build(&d, &CanvasView::default(), None, &CellLines::unknown(), &空会话());
        assert_eq!(t.status_coord, "—");
    }

    /// 缩放百分比随视口变（0.25 → 25%、4.0 → 400%）。
    #[test]
    fn 缩放百分比() {
        let d = 文档(Assets::Failed("x".to_string()));
        let mut v = CanvasView::default();
        v.scale = 0.25;
        assert_eq!(
            build(&d, &v, None, &CellLines::unknown(), &空会话()).zoom,
            "25%"
        );
        v.scale = 4.0;
        assert_eq!(
            build(&d, &v, None, &CellLines::unknown(), &空会话()).zoom,
            "400%"
        );
    }

    /// **脏标记进标题**：有未保存修改时标题/页签都要带 `*`，保存后自动去掉。
    #[test]
    fn 未保存修改时标题带星号() {
        let d = 文档(Assets::Failed("x".to_string()));
        let mut ed = 空会话();
        assert_eq!(
            build(&d, &CanvasView::default(), None, &CellLines::unknown(), &ed).title,
            "0.map • 2 × 2 格 — Crystal Studio"
        );
        assert_eq!(
            build(&d, &CanvasView::default(), None, &CellLines::unknown(), &ed).tab,
            "0.map"
        );

        // 造一次"改过"的状态：直接改脏标记的底层（步号前进）
        let mut map = 小图();
        map.cells[0].bk_img = 7;
        ed.pick_at(&map, 0, 0);
        ed.apply_once(&mut map, 1, 1);
        assert!(ed.dirty());
        let t = build(&d, &CanvasView::default(), None, &CellLines::unknown(), &ed);
        assert_eq!(t.title, "0.map * • 2 × 2 格 — Crystal Studio");
        assert_eq!(t.tab, "0.map *");

        ed.mark_saved();
        assert_eq!(
            build(&d, &CanvasView::default(), None, &CellLines::unknown(), &ed).title,
            "0.map • 2 × 2 格 — Crystal Studio"
        );
    }

    /// **笔刷内容上状态栏**：没取过笔刷如实说没取；取过之后是「地表 图号 N · 库 M」。
    #[test]
    fn 笔刷内容文案() {
        let d = 文档(Assets::Failed("x".to_string()));
        let mut ed = 空会话();
        ed.select(TOOL_BRUSH_IDX);
        let t = build(&d, &CanvasView::default(), None, &CellLines::unknown(), &ed);
        assert_eq!(t.status_tool, "工具：笔刷");
        assert_eq!(t.status_brush, "笔刷：未取（先用吸管）");

        // 取一格：图号 901、库号 4（14/36 档才有库号字节）
        let mut map = 小图();
        map.cells[0].bk_img = 0x8000 | 901;
        map.cells[0].tiles = 4;
        let mut ed2 = 空会话();
        ed2.select(TOOL_PICKUP_IDX);
        ed2.pick_at(&map, 0, 0);
        // 上面这张 2×2 合成图是 Old12 档 ⇒ 库号如实 `—`
        let t = build(&d, &CanvasView::default(), None, &CellLines::unknown(), &ed2);
        assert_eq!(t.status_tool, "工具：吸管");
        assert_eq!(t.status_brush, "笔刷：地表图层 图号 901 · 资源库 —");

        // 换成 14 档的文档：库号要出来
        //
        // ⚠ 整份文档都换成 14B 字节（不再"只手改 `map` 字段"）：`MapDoc` 的 `map` 与
        //    `document.view` 是同一批格的两份拷贝，只手改 `map` 会让
        //    `format_label` / `ground_paint_supported` 仍按 Old12 报 —— 那是假状态。
        let mut bytes = vec![0u8; 52 + 14 * 4];
        bytes[0..2].copy_from_slice(&2u16.to_le_bytes());
        bytes[2..4].copy_from_slice(&2u16.to_le_bytes());
        bytes[52] = 0x85; // 图号 901 低字节
        bytes[53] = 0x03;
        bytes[64] = 4; // btTiles
        let d14 = 文档字节(bytes);
        let mut ed3 = 空会话();
        ed3.pick_at(&d14.doc.as_ref().unwrap().map, 0, 0);
        let t = build(&d14, &CanvasView::default(), None, &CellLines::unknown(), &ed3);
        assert_eq!(t.status_brush, "笔刷：地表图层 图号 901 · 资源库 4");

        ed.select(TOOL_ERASER_IDX);
        assert_eq!(
            build(&d, &CanvasView::default(), None, &CellLines::unknown(), &ed).status_tool,
            "工具：橡皮擦"
        );
    }

    /// **阻挡层的落笔位要写在状态栏上**（审计 P0-9：画完不知道能不能走人）。
    ///
    /// 三个断言：
    /// 1. `当前层` 显示「低位阻挡」；
    /// 2. `落笔` 显示 `bk_img.0x8000（低位阻挡）· 置位=不可走`（**位名 + 方向**）；
    /// 3. 换成高位阻挡时，字段名换成 `fr_img`、层名换成「高位阻挡」，而**档位无关**
    ///    （12/14/36 三档的 `0x8000` 语义一样，不许冒出一句"本图 12B 档无库号字节"）。
    #[test]
    fn 阻挡层落笔位文案() {
        use crate::edit::EditLayer;
        let d = 文档(Assets::Failed("x".to_string()));
        for (layer, img) in [
            (EditLayer::LowCollision, tx::LAYER_IMG_FIELDS[0]),
            (EditLayer::HighCollision, tx::LAYER_IMG_FIELDS[2]),
        ] {
            let mut ed = 空会话();
            ed.set_layer(layer);
            let t = build(&d, &CanvasView::default(), None, &CellLines::unknown(), &ed);
            assert_eq!(
                t.status_layer,
                tx::fill(tx::STATUS_LAYER_TMPL, &[("layer", layer.name())])
            );
            assert_eq!(
                t.status_hint,
                tx::fill(
                    tx::STATUS_WRITE_COLLISION_TMPL,
                    &[("img", img), ("layer", layer.name())]
                ),
                "{layer:?} 的落笔位文案不对"
            );
            assert!(!t.status_hint_warn, "正常态不是拒绝，不该用警示色");
            // 不许出现"12B 档无库号字节"那类答非所问的档位说明
            assert!(
                !t.status_hint.contains("库号"),
                "阻挡层与档位无关，不该提库号：{}",
                t.status_hint
            );
        }
    }

    /// 保存失败优先于图库失败（刚按的 Ctrl+S 失败，用户最需要看到）。
    #[test]
    fn 保存失败优先显示() {
        let d = 文档(Assets::Failed("图库也挂了".to_string()));
        let mut ed = 空会话();
        ed.note_save_err("磁盘满".to_string());
        let t = build(&d, &CanvasView::default(), None, &CellLines::unknown(), &ed);
        assert_eq!(t.status_left, "保存失败：磁盘满（原文件未改动）");
        // 但图库失败仍然在 stderr/Assets::error 里拿得到，不算丢
        assert_eq!(d.assets.error(), Some("图库也挂了"));
    }

    /// **胶囊显示真实尺寸**（上一版写死 `3×3`）：改尺寸胶囊必须跟着变，填充工具显示「填充」。
    #[test]
    fn 胶囊显示真实尺寸() {
        let d = 文档(Assets::Failed("x".to_string()));
        let mut ed = 空会话();
        // 缺省：画笔 + 尺寸 3 ⇒ `3×3`
        assert_eq!(ed.tool(), Tool::Brush);
        let t = build(&d, &CanvasView::default(), None, &CellLines::unknown(), &ed);
        assert_eq!(t.pill, "3×3");
        assert_eq!(t.brush_size, 3);

        // 改尺寸（不经地图）⇒ 胶囊跟着变，且**没有产生任何步骤**
        for (size, want) in [(7usize, "7×7"), (15, "15×15"), (31, "31×31")] {
            ed.set_brush_size(size);
            let t = build(&d, &CanvasView::default(), None, &CellLines::unknown(), &ed);
            assert_eq!(t.pill, want, "尺寸 {size} 的胶囊文字");
            assert_eq!(t.brush_size, size);
        }
        assert_eq!(ed.undo_len(), 0, "改尺寸不该产生撤销步");
        assert!(!ed.dirty(), "改尺寸不该把文档弄脏");

        // 填充工具 ⇒ 官方复用 `Fill` 这条文案（`MainWindow.axaml.cs:932`）
        ed.select(crate::edit::TOOL_BUCKET_IDX);
        let t = build(&d, &CanvasView::default(), None, &CellLines::unknown(), &ed);
        assert_eq!(t.pill, tx::TOOL_BUCKET);
        assert_eq!(t.status_tool, tx::fill(tx::STATUS_TOOL_TMPL, &[("tool", tx::TOOL_BUCKET)]));
    }

    /// **填充回执进状态栏**：图号层说「填充 N 格」，阻挡层说「应用低位阻挡 N 格」；
    /// 没填过时整段不存在（不是一句空字符串）。
    #[test]
    fn 填充回执进状态栏() {
        let d = 文档(Assets::Failed("x".to_string()));
        let mut ed = 空会话();
        let t = build(&d, &CanvasView::default(), None, &CellLines::unknown(), &ed);
        assert_eq!(t.status_fill, None, "没填过就没有这一段");

        // 造一次真填充：4×4 全同号 ⇒ 地表 step=2 填 4 个偶偶格
        let mut map = mir2_map::Map::parse(&{
            let mut b = vec![0u8; mir2_map::HEADER_LEN + 12 * 16];
            b[0..2].copy_from_slice(&4u16.to_le_bytes());
            b[2..4].copy_from_slice(&4u16.to_le_bytes());
            b
        })
        .expect("4×4 合成图应能解析");
        for c in map.cells.iter_mut() {
            c.bk_img = 5;
        }
        map.cells[0].bk_img = 1;
        ed.set_layer(crate::edit::EditLayer::Ground);
        ed.pick_at(&map, 0, 0);
        map.cells[0].bk_img = 5;
        let out = ed.fill(&mut map, 1, 1);
        assert_eq!(out, crate::edit::FillOutcome::Filled(4));
        let t = build(&d, &CanvasView::default(), None, &CellLines::unknown(), &ed);
        assert_eq!(t.status_fill.as_deref(), Some("填充 4 格"));

        // 阻挡层：官方是另一句话（`MapViewport.cs:537`）
        let mut ed2 = 空会话();
        ed2.set_layer(crate::edit::EditLayer::LowCollision);
        let mut map2 = 小图();
        assert!(matches!(
            ed2.fill(&mut map2, 0, 0),
            crate::edit::FillOutcome::Filled(4)
        ));
        let t2 = build(&d, &CanvasView::default(), None, &CellLines::unknown(), &ed2);
        assert_eq!(t2.status_fill.as_deref(), Some("应用低位阻挡 4 格"));

        // 撤销之后回执必须消失（它说的是"刚才填了多少"，撤销完就不成立了）
        assert_eq!(ed2.undo(&mut map2), 4);
        let t3 = build(&d, &CanvasView::default(), None, &CellLines::unknown(), &ed2);
        assert_eq!(t3.status_fill, None, "撤销后不许还挂着填充回执");
    }
}
