//! 10 个工具按钮的**矢量图标**（任务书 §五，档位 B：矢量、不近似）。
//!
//! ## 数据来源（不猜）
//! 4 个按钮在 `layout/crystalstudio-ui.json` 里带有 `icon.paths[]`（`d/fill/stroke/stroke_thickness`，
//! 坐标系 24×24，显示区见各条 `box`）。这 4 个**逐字照抄** JSON 的 `d` 字符串：
//!
//! | 按钮 | 工具 | JSON 路径 | 显示区 |
//! |---|---|---|---|
//! | 0 | Brush   | `/MainWindow/0/0/ToolPanel[0].icon.paths` | 16×16 |
//! | 2 | Bucket  | `/MainWindow/0/0/ToolPanel/BucketToolButton.icon.paths` | 16×16 |
//! | 8 | Wall    | `/MainWindow/0/0/ToolPanel/WallToolButton.icon.paths` | 18×18 |
//! | 9 | Special | `/MainWindow/0/0/ToolPanel/SpecialBrushToolButton.icon.paths` | 16×16 |
//!
//! 另外 6 个在 JSON 里是 **Segoe MDL2 Assets 字形**（`icon.type = "glyph"`，
//! `glyphs[0].codepoints` = `U+E70F / U+E75C / U+EF20 / U+E8B9 / U+E7C2 / U+E8C8`）。
//! 任务书 §五 明确要求「**不要依赖系统字体**，用等价的简单矢量自绘并在注释里写
//! 「MDL2 码位 X 的等效自绘」」——本文件照此办理，逐个标注码位。
//!
//! > ⚠ 这 6 个是**等效自绘**，几何上与 MDL2 原字形不逐点相同（原字形是字体轮廓，
//! > JSON 里没有 `d` 数据，取不到就无法逐点还原）。汇报 §五 已如实列明。

use std::sync::OnceLock;

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke};

use crate::icon_path::{self, SubPath};

// ───────────────────────── 数据结构 ─────────────────────────

/// 一条图标路径：填充 / 描边配置 + 原始 `d`。
pub struct IconPathDef {
    /// SVG path 的 `d` 字符串（坐标系 24×24）。
    pub d: &'static str,
    /// 是否填充（JSON `fill` 非 null 即 true）。
    pub fill: bool,
    /// 描边宽度（JSON `stroke_thickness`；0 表示无描边）。单位仍是 24×24 坐标系。
    pub stroke: f32,
}

/// 图标来源，用于汇报与自证。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum IconSource {
    /// 取自 `crystalstudio-ui.json` 的 `icon.paths[]`（几何是真值）。
    JsonPaths,
    /// Segoe MDL2 字形的等效自绘（JSON 只给了码位，没有轮廓数据）。
    Mdl2Equivalent(&'static str),
}

/// 一个工具按钮的图标定义。
pub struct ToolIconDef {
    /// 工具名（取自 JSON `tag_value`）。
    pub tool: &'static str,
    /// 工具提示（取自 JSON `tooltip` 首行）。
    pub tip: &'static str,
    /// 显示区边长（JSON `icon.box`，16 或 18），坐标系仍为 24×24。
    pub box_size: f32,
    /// 路径列表。
    pub paths: &'static [IconPathDef],
    /// 来源。
    pub source: IconSource,
}

// ───────────────────────── 4 个真 path 图标（照抄 JSON） ─────────────────────────

/// Brush（`tag_value = "Brush"`）：JSON 给了 2 条路径，第 1 条填充、第 2 条描边 1.5。
const BRUSH_PATHS: &[IconPathDef] = &[
    IconPathDef {
        d: "M 14,3 C 17,0 21,0 22,2 C 23,4 20,8 17,11 L 12,16 L 8,12 Z \
            M 7,13 L 11,17 C 11,21 7,23 2,22 C 4,20 2,17 5,15 Z",
        fill: true,
        stroke: 0.0,
    },
    IconPathDef {
        d: "M 8,11 L 13,16",
        fill: false,
        stroke: 1.5,
    },
];

/// Bucket（`tag_value = "Bucket"`，UI 上叫 Fill）：JSON 2 条路径，
/// 第 1 条描边 1.6（Round join），第 2 条填充。
const BUCKET_PATHS: &[IconPathDef] = &[
    IconPathDef {
        d: "M 3,12 L 11,4 L 20,13 L 12,21 Z M 3,12 L 19,12 M 8,7 L 5,4 C 3,2 6,0 8,2 L 13,7",
        fill: false,
        stroke: 1.6,
    },
    IconPathDef {
        d: "M 4,13 L 18,13 L 12,19 Z M 21,14 C 21,14 18.5,17.5 18.5,19 C 18.5,22.3 23.5,22.3 23.5,19 \
            C 23.5,17.5 21,14 21,14 Z",
        fill: true,
        stroke: 0.0,
    },
];

/// Wall（`tag_value = "Wall"`）：JSON 1 条路径，纯描边 1.5（砖墙纹理）。显示区 18×18。
const WALL_PATHS: &[IconPathDef] = &[IconPathDef {
    d: "M 2,5 L 22,5 L 22,20 L 2,20 Z M 2,10 L 22,10 M 2,15 L 22,15 M 8,5 L 8,10 M 16,5 L 16,10 \
        M 5,10 L 5,15 M 13,10 L 13,15 M 20,10 L 20,15 M 8,15 L 8,20 M 16,15 L 16,20",
    fill: false,
    stroke: 1.5,
}];

/// Special Brush（`tag_value = "SpecialBrush"`）：JSON 1 条路径，
/// 填充（魔杖 + 两颗四角星；四角星是**凹多边形**，走耳切三角化）。
const SPECIAL_PATHS: &[IconPathDef] = &[IconPathDef {
    d: "M 3,18 L 16,5 L 19,8 L 6,21 Z M 3,1 L 4,4 L 7,5 L 4,6 L 3,9 L 2,6 L 0,5 L 2,4 Z \
        M 19,12 L 20,15 L 23,16 L 20,17 L 19,20 L 18,17 L 15,16 L 18,15 Z",
    fill: true,
    stroke: 0.0,
}];

// ───────────────────────── 6 个 MDL2 等效自绘 ─────────────────────────
//
// 每个都注明「MDL2 码位 X 的等效自绘」。坐标系一律 24×24，显示区 16×16。

/// **MDL2 码位 U+E70F 的等效自绘**（Pencil / 铅笔）。
/// 参照截图 (60,168)-(75,183) 图标簇：斜向铅笔，笔尖朝左下，笔身两条平行边 + 笔箍线。
const MDL2_E70F_PENCIL: &[IconPathDef] = &[
    IconPathDef {
        d: "M 15,2 L 22,9 L 10,21 L 3,21 L 3,14 Z",
        fill: false,
        stroke: 1.5,
    },
    IconPathDef {
        d: "M 9,8 L 16,15",
        fill: false,
        stroke: 1.5,
    },
];

/// **MDL2 码位 U+E75C 的等效自绘**（Eraser / 橡皮）。
/// 参照截图 (133,133)-(148,148)：斜放的平行四边形，被一条对角线分成两块。
const MDL2_E75C_ERASER: &[IconPathDef] = &[
    IconPathDef {
        d: "M 10,3 L 21,9 L 15,21 L 4,15 Z",
        fill: false,
        stroke: 1.5,
    },
    IconPathDef {
        d: "M 4,15 L 15,9",
        fill: false,
        stroke: 1.5,
    },
];

/// **MDL2 码位 U+EF20 的等效自绘**（Select / 框选）。
/// 参照截图 (24,133)-(38,164)：虚线方框（8 段短划）+ 右上角一个加号。
const MDL2_EF20_SELECT: &[IconPathDef] = &[
    IconPathDef {
        // 虚线方框 (3,7)-(17,21)，四边各两段
        d: "M 3,7 L 7,7 M 11,7 L 17,7 M 17,10 L 17,14 M 17,18 L 17,21 \
            M 14,21 L 9,21 M 6,21 L 3,21 M 3,18 L 3,13 M 3,10 L 3,7",
        fill: false,
        stroke: 1.5,
    },
    // 加号：(19,3)-(19,9) 与 (16,6)-(22,6)
    IconPathDef {
        d: "M 19,3 L 19,9 M 16,6 L 22,6",
        fill: false,
        stroke: 1.5,
    },
];

/// **MDL2 码位 U+E8B9 的等效自绘**（Stamp / 图章）。
/// 参照截图 (60,133)-(75,164)：两层叠放的矩形（后一层向左上偏移 3px）。
const MDL2_E8B9_STAMP: &[IconPathDef] = &[
    IconPathDef {
        d: "M 6,7 L 21,7 L 21,21 L 6,21 Z",
        fill: false,
        stroke: 1.5,
    },
    IconPathDef {
        d: "M 3,4 L 18,4 L 18,7",
        fill: false,
        stroke: 1.5,
    },
    IconPathDef {
        d: "M 6,7 L 3,4",
        fill: false,
        stroke: 1.5,
    },
];

/// **MDL2 码位 U+E7C2 的等效自绘**（Pickup / 拾取）。
/// 参照截图 (100,133)-(107,164)：一个竖长条矩形外框。
const MDL2_E7C2_PICKUP: &[IconPathDef] = &[IconPathDef {
    d: "M 9,4 L 15,4 L 15,20 L 9,20 Z",
    fill: false,
    stroke: 1.5,
}];

/// **MDL2 码位 U+E8C8 的等效自绘**（Clone / 克隆）。
/// 参照截图 (134,133)-(147,164)：两张叠放的"纸"，后者向右下偏移。
const MDL2_E8C8_CLONE: &[IconPathDef] = &[
    IconPathDef {
        d: "M 3,3 L 14,3 L 14,16 L 3,16 Z",
        fill: false,
        stroke: 1.5,
    },
    IconPathDef {
        d: "M 10,8 L 21,8 L 21,21 L 10,21 Z",
        fill: false,
        stroke: 1.5,
    },
];

// ───────────────────────── 10 个工具（顺序 = 截图网格顺序） ─────────────────────────

/// 10 个工具按钮，**顺序与截图网格一致**（4 / 4 / 2 三行）。
/// `tool` 与 `tip` 取自 `crystalstudio-ui.json` → `ToolPanel.children[*].tag_value / tooltip`。
pub const TOOLS: &[ToolIconDef] = &[
    ToolIconDef {
        tool: "Brush",
        tip: "Brush · B",
        box_size: 16.0,
        paths: BRUSH_PATHS,
        source: IconSource::JsonPaths,
    },
    ToolIconDef {
        tool: "Pencil",
        tip: "Pencil · P",
        box_size: 16.0,
        paths: MDL2_E70F_PENCIL,
        source: IconSource::Mdl2Equivalent("U+E70F"),
    },
    ToolIconDef {
        tool: "Bucket",
        tip: "Fill · F",
        box_size: 16.0,
        paths: BUCKET_PATHS,
        source: IconSource::JsonPaths,
    },
    ToolIconDef {
        tool: "Eraser",
        tip: "Eraser · E",
        box_size: 16.0,
        paths: MDL2_E75C_ERASER,
        source: IconSource::Mdl2Equivalent("U+E75C"),
    },
    ToolIconDef {
        tool: "Select",
        tip: "Select · M",
        box_size: 16.0,
        paths: MDL2_EF20_SELECT,
        source: IconSource::Mdl2Equivalent("U+EF20"),
    },
    ToolIconDef {
        tool: "Stamp",
        tip: "Stamp · O",
        box_size: 16.0,
        paths: MDL2_E8B9_STAMP,
        source: IconSource::Mdl2Equivalent("U+E8B9"),
    },
    ToolIconDef {
        tool: "Pickup",
        tip: "Pickup · V",
        box_size: 16.0,
        paths: MDL2_E7C2_PICKUP,
        source: IconSource::Mdl2Equivalent("U+E7C2"),
    },
    ToolIconDef {
        tool: "Clone",
        tip: "Clone · K",
        box_size: 16.0,
        paths: MDL2_E8C8_CLONE,
        source: IconSource::Mdl2Equivalent("U+E8C8"),
    },
    ToolIconDef {
        tool: "Wall",
        tip: "Wall · H",
        box_size: 18.0,
        paths: WALL_PATHS,
        source: IconSource::JsonPaths,
    },
    ToolIconDef {
        tool: "SpecialBrush",
        tip: "Special Brush · T",
        box_size: 16.0,
        paths: SPECIAL_PATHS,
        source: IconSource::JsonPaths,
    },
];

// ───────────────────────── 预解析几何（只做一次） ─────────────────────────

/// 一条子路径的最终几何（顶点 + 三角化结果）。
struct SubGeom {
    points: Vec<[f32; 2]>,
    closed: bool,
    /// 耳切三角化结果（顶点索引）；仅填充路径需要。
    tris: Vec<[usize; 3]>,
}

/// 一条路径的最终几何。
struct PathGeom {
    fill: bool,
    stroke: f32,
    subs: Vec<SubGeom>,
}

/// 一个工具图标的最终几何。
struct ToolGeom {
    box_size: f32,
    paths: Vec<PathGeom>,
}

/// 全量预解析结果（进程内只构造一次，避免每帧重复解析 path 字符串）。
static GEOM: OnceLock<Vec<ToolGeom>> = OnceLock::new();

fn geometry() -> &'static Vec<ToolGeom> {
    GEOM.get_or_init(|| {
        TOOLS
            .iter()
            .map(|t| ToolGeom {
                box_size: t.box_size,
                paths: t
                    .paths
                    .iter()
                    .map(|p| {
                        let subs: Vec<SubGeom> = icon_path::parse(p.d)
                            .into_iter()
                            .map(|sp: SubPath| {
                                let tris = if p.fill {
                                    icon_path::triangulate(&sp.points)
                                } else {
                                    Vec::new()
                                };
                                SubGeom {
                                    points: sp.points,
                                    closed: sp.closed,
                                    tris,
                                }
                            })
                            .collect();
                        PathGeom {
                            fill: p.fill,
                            stroke: p.stroke,
                            subs,
                        }
                    })
                    .collect(),
            })
            .collect()
    })
}

// ───────────────────────── 绘制 ─────────────────────────

/// 描边细分：<=0 表示不描边。
/// 24×24 坐标系下的线宽，缩放到显示区后仍保持视觉粗细（截图里约 1.0~1.1 显示像素）。
#[inline]
fn scaled_stroke(stroke24: f32, scale: f32) -> f32 {
    (stroke24 * scale).max(0.7)
}

/// 在 `box` 区域内绘制第 `idx` 个工具图标。
///
/// - `box`：图标的显示区（截图里是 16×16 或 18×18，位于 32×32 按钮正中）；
/// - `color`：图标颜色（选中态用 `theme::ACCENT_DARK`，普通态用 `theme::TEXT`）。
pub fn paint_tool_icon(painter: &Painter, idx: usize, box_rect: Rect, color: Color32) {
    for shape in icon_shapes(idx, box_rect, color) {
        painter.add(shape);
    }
}

/// 把一个图标编译成待绘制的 `Shape` 列表（与 Painter 解耦，便于单元测试校验网格不越界）。
pub fn icon_shapes(idx: usize, box_rect: Rect, color: Color32) -> Vec<Shape> {
    let mut out: Vec<Shape> = Vec::new();
    let Some(def) = TOOLS.get(idx) else {
        return out;
    };
    let Some(geom) = geometry().get(idx) else {
        return out;
    };
    // 数据自检：JSON 的 box 与几何表必须一致，不一致说明常量表被改坏了
    debug_assert!((def.box_size - geom.box_size).abs() < f32::EPSILON);

    let scale = box_rect.width() / 24.0;
    let to_screen = |p: [f32; 2]| -> Pos2 {
        Pos2::new(
            box_rect.left() + p[0] * scale,
            box_rect.top() + p[1] * scale,
        )
    };

    for path in &geom.paths {
        for sub in &path.subs {
            let pts: Vec<Pos2> = sub.points.iter().map(|p| to_screen(*p)).collect();
            if pts.len() < 2 {
                continue;
            }
            // ① 填充：直接构造 `Mesh`（不走 `Shape::convex_polygon`）
            //
            // ⚠ 实测定下的教训（不要改回 convex_polygon）：
            // epaint 的 `fill_closed_path`（tessellator.rs:765）是**扇形填充**，
            // 并且会给每个顶点按「斜接法线」外扩一段羽化量（`pos ± 0.5*feathering*normal`）。
            // 当喂进去的是退化三角形（面积≈0，例如曲线细分后相邻点几乎重合）时，
            // 法线会算成极大的值，三角面直接飞出面板 —— 本机实测 Brush 图标的
            // 细分网格顶点跑到 (-50.47,-127.66)-(57.14,147.66)，而显示区只有 16×16，
            // 屏幕上表现为一道横跨整个左停靠区的巨大黑色 X。
            // 改用 `Mesh` 后顶点原样进网格、不做任何法线外扩，几何 100% 可控；
            // 代价是没有羽化抗锯齿（图标边缘略硬），这是可接受的交换。
            if path.fill && !sub.tris.is_empty() {
                let mut mesh = egui::Mesh::default();
                for t in &sub.tris {
                    let base = mesh.vertices.len() as u32;
                    mesh.colored_vertex(pts[t[0]], color);
                    mesh.colored_vertex(pts[t[1]], color);
                    mesh.colored_vertex(pts[t[2]], color);
                    mesh.add_triangle(base, base + 1, base + 2);
                }
                out.push(Shape::mesh(mesh));
            }
            // ② 描边：逐段线段 + 拐点补小圆（等价于 round join，避免尖角裂缝）
            if path.stroke > 0.0 {
                let w = scaled_stroke(path.stroke, scale);
                let stroke = Stroke::new(w, color);
                let n = pts.len();
                let segs = if sub.closed { n } else { n - 1 };
                for i in 0..segs {
                    out.push(Shape::line_segment([pts[i], pts[(i + 1) % n]], stroke));
                }
                if sub.closed || n > 2 {
                    for p in &pts {
                        out.push(Shape::circle_filled(*p, w * 0.5, color));
                    }
                }
            }
        }
    }
    out
}

/// 每个工具按钮的显示区尺寸（供布局层使用）。
pub fn box_size_of(idx: usize) -> f32 {
    TOOLS.get(idx).map(|t| t.box_size).unwrap_or(16.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 工具数量为10() {
        assert_eq!(TOOLS.len(), 10);
    }

    #[test]
    fn 四个真路径_六个等效自绘() {
        let json = TOOLS
            .iter()
            .filter(|t| matches!(t.source, IconSource::JsonPaths))
            .count();
        let mdl2 = TOOLS
            .iter()
            .filter(|t| matches!(t.source, IconSource::Mdl2Equivalent(_)))
            .count();
        assert_eq!(json, 4, "JSON 里有 path 的工具应恰好 4 个");
        assert_eq!(mdl2, 6, "MDL2 字形工具应恰好 6 个");
    }

    #[test]
    fn 所有图标都能解析出顶点() {
        for (i, g) in geometry().iter().enumerate() {
            assert!(!g.paths.is_empty(), "第 {i} 个图标无路径");
            let total: usize = g
                .paths
                .iter()
                .map(|p| p.subs.iter().map(|s| s.points.len()).sum::<usize>())
                .sum();
            assert!(total >= 2, "第 {i} 个图标顶点数为 {total}，解析失败");
        }
    }

    #[test]
    fn 填充路径都有三角化结果() {
        for (i, g) in geometry().iter().enumerate() {
            for (j, p) in g.paths.iter().enumerate() {
                if p.fill {
                    for (k, s) in p.subs.iter().enumerate() {
                        assert!(
                            !s.tris.is_empty(),
                            "图标 {i} 路径 {j} 子路径 {k} 填充但三角化为空"
                        );
                    }
                }
            }
        }
    }

    /// 所有顶点必须落在 24×24 图标坐标系内（否则会画出面板外）。
    #[test]
    fn 所有顶点都在坐标系内() {
        for (i, g) in geometry().iter().enumerate() {
            let mut lo = (f32::MAX, f32::MAX);
            let mut hi = (f32::MIN, f32::MIN);
            for p in &g.paths {
                for s in &p.subs {
                    for q in &s.points {
                        lo = (lo.0.min(q[0]), lo.1.min(q[1]));
                        hi = (hi.0.max(q[0]), hi.1.max(q[1]));
                    }
                }
            }
            assert!(
                lo.0 >= -0.01 && lo.1 >= -0.01 && hi.0 <= 24.01 && hi.1 <= 24.01,
                "图标 {i}（{}）顶点越界: ({:.2},{:.2})-({:.2},{:.2})",
                TOOLS[i].tool,
                lo.0,
                lo.1,
                hi.0,
                hi.1
            );
        }
    }

    /// **网格不越界**：把图标真正跑一遍 epaint 的细分器，检查生成网格的顶点是否都在
    /// 图标显示区（+1px 羽化余量）内。
    ///
    /// 这条断言是「10 个工具按钮只在自己 32×32 按钮里画」的硬保证 ——
    /// 曾经出现过 `Shape::convex_polygon` 被喂进退化三角形后，
    /// 细分器吐出跨整个面板的巨大三角面的事故，用这条测试能当场抓到。
    #[test]
    fn 细分后的网格不越界() {
        let ctx = egui::Context::default();
        // 先跑一帧空 frame，把字体纹理装上（`Context::tessellate` 内部需要 fonts 已加载）
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        for (i, tool) in TOOLS.iter().enumerate() {
            let side = tool.box_size;
            let box_rect = Rect::from_min_size(Pos2::new(0.0, 0.0), egui::vec2(side, side));
            let shapes = icon_shapes(i, box_rect, Color32::BLACK);
            let clipped: Vec<egui::epaint::ClippedShape> = shapes
                .into_iter()
                .map(|s| egui::epaint::ClippedShape {
                    clip_rect: Rect::EVERYTHING,
                    shape: s,
                })
                .collect();
            let prims = ctx.tessellate(clipped, 1.0);
            let mut lo = (f32::MAX, f32::MAX);
            let mut hi = (f32::MIN, f32::MIN);
            for p in &prims {
                if let egui::epaint::Primitive::Mesh(m) = &p.primitive {
                    for v in &m.vertices {
                        lo = (lo.0.min(v.pos.x), lo.1.min(v.pos.y));
                        hi = (hi.0.max(v.pos.x), hi.1.max(v.pos.y));
                    }
                }
            }
            eprintln!(
                "图标 {i:2} {:<14} mesh bbox=({:.2},{:.2})-({:.2},{:.2})",
                TOOLS[i].tool, lo.0, lo.1, hi.0, hi.1
            );
            assert!(
                lo.0 >= -1.5 && lo.1 >= -1.5 && hi.0 <= side + 1.5 && hi.1 <= side + 1.5,
                "图标 {i}（{}）细分后网格越界: ({:.2},{:.2})-({:.2},{:.2})，显示区 {side}×{side}",
                TOOLS[i].tool,
                lo.0,
                lo.1,
                hi.0,
                hi.1
            );
        }
    }
}
