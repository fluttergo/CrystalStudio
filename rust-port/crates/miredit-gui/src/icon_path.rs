//! 极小的 SVG path `d` → 顶点解析器。
//!
//! **为什么自己写**：任务书 §六 硬约束「不允许任何 SVG/图标/字体加载库」。
//! 本模块只做两件事：
//! 1. 把 `crystalstudio-ui.json` → `icon.paths[].d` 里的路径字符串切成 **折线顶点**
//!    （曲线用**固定细分**近似：三次贝塞尔 16 段、二次贝塞尔 12 段，均 ≥ 任务书要求的 8 段）；
//! 2. 给填充路径做**耳切三角化**，这样凹多边形（例如 `Special Brush` 的四角星）
//!    也能正确填充 —— `Shape::convex_polygon` 只对凸多边形成立，不能直接用在凹形上。
//!
//! 支持的指令：`M m L l H h V v C c Q q Z z`，绝对/相对均可。
//! **不支持**（当前 4 个工具图标都用不到，遇到就跳过并保持不崩）：`S s T t A a`。
//! 这条限制是显式声明的，不是静默忽略 —— 见 `parse` 里的 `_ => {}` 分支注释。

/// 一条子路径：一串顶点 + 是否闭合。
#[derive(Clone, Debug, Default)]
pub struct SubPath {
    /// 顶点序列（坐标系与输入 `d` 相同，即 24×24）。
    pub points: Vec<[f32; 2]>,
    /// `true` 表示以 `Z` 收尾（填充时需要闭合三角化）。
    pub closed: bool,
}

/// 三次贝塞尔的细分段数（≥8，任务书 §五）。
const CUBIC_SEGMENTS: usize = 16;
/// 二次贝塞尔的细分段数（≥8，任务书 §五）。
const QUAD_SEGMENTS: usize = 12;

/// 把 path 字符串切成数字序列与命令序列。
struct Lexer<'a> {
    bytes: &'a [u8],
    i: usize,
}

impl<'a> Lexer<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            bytes: s.as_bytes(),
            i: 0,
        }
    }

    /// 跳过空白与逗号。
    fn skip_sep(&mut self) {
        while self.i < self.bytes.len() {
            let c = self.bytes[self.i] as char;
            if c.is_ascii_whitespace() || c == ',' {
                self.i += 1;
            } else {
                break;
            }
        }
    }

    /// 取下一个命令字母（若无则 None）。
    fn next_cmd(&mut self) -> Option<char> {
        self.skip_sep();
        if self.i < self.bytes.len() {
            let c = self.bytes[self.i] as char;
            if c.is_ascii_alphabetic() {
                self.i += 1;
                return Some(c);
            }
        }
        None
    }

    /// 取下一个数字（若无则 None）。
    fn next_num(&mut self) -> Option<f32> {
        self.skip_sep();
        let start = self.i;
        if self.i < self.bytes.len() && (self.bytes[self.i] == b'-' || self.bytes[self.i] == b'+') {
            self.i += 1;
        }
        let mut seen_dot = false;
        while self.i < self.bytes.len() {
            let c = self.bytes[self.i];
            if c.is_ascii_digit() {
                self.i += 1;
            } else if c == b'.' && !seen_dot {
                seen_dot = true;
                self.i += 1;
            } else if (c == b'e' || c == b'E') && self.i + 1 < self.bytes.len() {
                self.i += 1;
                if self.i < self.bytes.len()
                    && (self.bytes[self.i] == b'-' || self.bytes[self.i] == b'+')
                {
                    self.i += 1;
                }
            } else {
                break;
            }
        }
        if self.i == start {
            return None;
        }
        std::str::from_utf8(&self.bytes[start..self.i])
            .ok()?
            .parse::<f32>()
            .ok()
    }

    /// 是否还有「下一个数字」（用于隐式重复的命令，如 `M 1,2 3,4` 之后的 L）。
    fn has_num(&mut self) -> bool {
        self.skip_sep();
        if self.i >= self.bytes.len() {
            return false;
        }
        let c = self.bytes[self.i];
        c.is_ascii_digit() || c == b'-' || c == b'+' || c == b'.'
    }
}

#[inline]
fn lerp(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

/// 解析一条 `d` 字符串，返回子路径列表。
pub fn parse(d: &str) -> Vec<SubPath> {
    let mut out: Vec<SubPath> = Vec::new();
    let mut cur: Vec<[f32; 2]> = Vec::new();
    // 当前点 / 子路径起点
    let mut p = [0.0f32, 0.0f32];
    let mut start = [0.0f32, 0.0f32];

    let flush = |out: &mut Vec<SubPath>, cur: &mut Vec<[f32; 2]>, closed: bool| {
        if cur.len() >= 2 {
            out.push(SubPath {
                points: std::mem::take(cur),
                closed,
            });
        } else {
            cur.clear();
        }
    };

    let mut lx = Lexer::new(d);
    while let Some(cmd) = lx.next_cmd() {
        let rel = cmd.is_ascii_lowercase();
        match cmd.to_ascii_uppercase() {
            'M' => {
                // 新的子路径
                flush(&mut out, &mut cur, false);
                let (Some(x), Some(y)) = (lx.next_num(), lx.next_num()) else {
                    continue;
                };
                p = if rel { [p[0] + x, p[1] + y] } else { [x, y] };
                start = p;
                cur.push(p);
                // M 之后紧跟的坐标对当作 L 处理（SVG 规范）
                while lx.has_num() {
                    let (Some(x), Some(y)) = (lx.next_num(), lx.next_num()) else {
                        break;
                    };
                    p = if rel { [p[0] + x, p[1] + y] } else { [x, y] };
                    cur.push(p);
                }
            }
            'L' => {
                while lx.has_num() {
                    let (Some(x), Some(y)) = (lx.next_num(), lx.next_num()) else {
                        break;
                    };
                    p = if rel { [p[0] + x, p[1] + y] } else { [x, y] };
                    cur.push(p);
                }
            }
            'H' => {
                while lx.has_num() {
                    let Some(x) = lx.next_num() else { break };
                    p = [if rel { p[0] + x } else { x }, p[1]];
                    cur.push(p);
                }
            }
            'V' => {
                while lx.has_num() {
                    let Some(y) = lx.next_num() else { break };
                    p = [p[0], if rel { p[1] + y } else { y }];
                    cur.push(p);
                }
            }
            'C' => {
                while lx.has_num() {
                    let nums: Vec<f32> = (0..6).filter_map(|_| lx.next_num()).collect();
                    if nums.len() < 6 {
                        break;
                    }
                    let c1 = if rel {
                        [p[0] + nums[0], p[1] + nums[1]]
                    } else {
                        [nums[0], nums[1]]
                    };
                    let c2 = if rel {
                        [p[0] + nums[2], p[1] + nums[3]]
                    } else {
                        [nums[2], nums[3]]
                    };
                    let e = if rel {
                        [p[0] + nums[4], p[1] + nums[5]]
                    } else {
                        [nums[4], nums[5]]
                    };
                    push_cubic(&mut cur, p, c1, c2, e);
                    p = e;
                }
            }
            'Q' => {
                while lx.has_num() {
                    let nums: Vec<f32> = (0..4).filter_map(|_| lx.next_num()).collect();
                    if nums.len() < 4 {
                        break;
                    }
                    let c1 = if rel {
                        [p[0] + nums[0], p[1] + nums[1]]
                    } else {
                        [nums[0], nums[1]]
                    };
                    let e = if rel {
                        [p[0] + nums[2], p[1] + nums[3]]
                    } else {
                        [nums[2], nums[3]]
                    };
                    push_quad(&mut cur, p, c1, e);
                    p = e;
                }
            }
            'Z' => {
                flush(&mut out, &mut cur, true);
                p = start;
            }
            // 不支持 S/T/A：当前 4 个带 path 的工具图标都不使用它们。
            // 这里显式忽略（不静默造点），并在 `SubPath` 文档里写明限制。
            _ => {
                // 跳过该命令的参数，避免死循环
                while lx.has_num() {
                    lx.next_num();
                }
            }
        }
    }
    flush(&mut out, &mut cur, false);
    out
}

/// 三次贝塞尔固定细分（`CUBIC_SEGMENTS` 段），末点精确等于终点。
fn push_cubic(cur: &mut Vec<[f32; 2]>, p0: [f32; 2], c1: [f32; 2], c2: [f32; 2], p3: [f32; 2]) {
    for i in 1..=CUBIC_SEGMENTS {
        let t = i as f32 / CUBIC_SEGMENTS as f32;
        let a = lerp(p0, c1, t);
        let b = lerp(c1, c2, t);
        let c = lerp(c2, p3, t);
        let ab = lerp(a, b, t);
        let bc = lerp(b, c, t);
        let pt = if i == CUBIC_SEGMENTS {
            p3
        } else {
            lerp(ab, bc, t)
        };
        cur.push(pt);
    }
}

/// 二次贝塞尔固定细分（`QUAD_SEGMENTS` 段），末点精确等于终点。
fn push_quad(cur: &mut Vec<[f32; 2]>, p0: [f32; 2], c1: [f32; 2], p2: [f32; 2]) {
    for i in 1..=QUAD_SEGMENTS {
        let t = i as f32 / QUAD_SEGMENTS as f32;
        let a = lerp(p0, c1, t);
        let b = lerp(c1, p2, t);
        let pt = if i == QUAD_SEGMENTS {
            p2
        } else {
            lerp(a, b, t)
        };
        cur.push(pt);
    }
}

/// 多边形有向面积的两倍（> 0 为逆时针，y 轴向下的屏幕坐标系里视觉上为"顺时针"）。
fn area2(pts: &[[f32; 2]]) -> f32 {
    let n = pts.len();
    let mut s = 0.0;
    for i in 0..n {
        let a = pts[i];
        let b = pts[(i + 1) % n];
        s += a[0] * b[1] - b[0] * a[1];
    }
    s
}

#[inline]
fn cross(o: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

#[inline]
fn point_in_tri(p: [f32; 2], a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> bool {
    let d1 = cross(a, b, p);
    let d2 = cross(b, c, p);
    let d3 = cross(c, a, p);
    let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_neg && has_pos)
}

/// 耳切三角化：把任意简单多边形拆成三角形列表（索引指向输入的 `pts`）。
///
/// 返回的每个三角形顶点按多边形原始方向排列；调用方按需翻转绕向即可。
pub fn triangulate(pts: &[[f32; 2]]) -> Vec<[usize; 3]> {
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }
    // 统一成正面积方向，最后不用再翻（填充时 egui 不依赖绕向）
    let mut idx: Vec<usize> = (0..n).collect();
    if area2(pts) < 0.0 {
        idx.reverse();
    }
    let mut tris: Vec<[usize; 3]> = Vec::with_capacity(n.saturating_sub(2));
    let mut guard = 0usize;
    while idx.len() > 3 {
        guard += 1;
        // 理论上不会死循环；保险起见给个上限并退出
        if guard > n * n + 16 {
            break;
        }
        let m = idx.len();
        let mut clipped = false;
        for i in 0..m {
            let i0 = idx[(i + m - 1) % m];
            let i1 = idx[i];
            let i2 = idx[(i + 1) % m];
            let (a, b, c) = (pts[i0], pts[i1], pts[i2]);
            // 凸顶点判定
            if cross(a, b, c) <= 1e-6 {
                continue;
            }
            // 凸顶点内不得含其它顶点
            let mut inside = false;
            for &j in &idx {
                if j == i0 || j == i1 || j == i2 {
                    continue;
                }
                if point_in_tri(pts[j], a, b, c) {
                    inside = true;
                    break;
                }
            }
            if inside {
                continue;
            }
            tris.push([i0, i1, i2]);
            idx.remove(i);
            clipped = true;
            break;
        }
        if !clipped {
            // 退化多边形（共线/自交）：直接按扇形兜底，避免整块图标消失
            break;
        }
    }
    if idx.len() == 3 {
        tris.push([idx[0], idx[1], idx[2]]);
    } else if idx.len() > 3 {
        for i in 1..idx.len() - 1 {
            tris.push([idx[0], idx[i], idx[i + 1]]);
        }
    }
    tris
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 解析矩形四段线() {
        let sp = parse("M 2,5 L 22,5 L 22,20 L 2,20 Z");
        assert_eq!(sp.len(), 1);
        assert_eq!(sp[0].points.len(), 4);
        assert!(sp[0].closed);
    }

    #[test]
    fn 三次贝塞尔按16段细分() {
        // 起点 (0,0) → 终点 (24,0)，中间两个控制点
        let sp = parse("M 0,0 C 6,12 18,12 24,0");
        assert_eq!(sp.len(), 1);
        // 起点 1 个 + 16 段细分点
        assert_eq!(sp[0].points.len(), 1 + CUBIC_SEGMENTS);
        assert!((sp[0].points.last().unwrap()[0] - 24.0).abs() < 1e-4);
        assert!(sp[0].points.last().unwrap()[1].abs() < 1e-4);
    }

    #[test]
    fn 二次贝塞尔按12段细分() {
        let sp = parse("M 0,0 Q 12,24 24,0");
        assert_eq!(sp[0].points.len(), 1 + QUAD_SEGMENTS);
    }

    #[test]
    fn 相对指令可解析() {
        let a = parse("M 10,10 l 5,0 l 0,5 z");
        let b = parse("M 10,10 L 15,10 L 15,15 Z");
        assert_eq!(a[0].points.len(), b[0].points.len());
        for (x, y) in a[0].points.iter().zip(b[0].points.iter()) {
            assert!((x[0] - y[0]).abs() < 1e-4 && (x[1] - y[1]).abs() < 1e-4);
        }
    }

    #[test]
    fn 凹多边形三角数等于n减2() {
        // 四角星（8 顶点、凹）
        let pts = vec![
            [3.0, 1.0],
            [4.0, 4.0],
            [7.0, 5.0],
            [4.0, 6.0],
            [3.0, 9.0],
            [2.0, 6.0],
            [0.0, 5.0],
            [2.0, 4.0],
        ];
        let tris = triangulate(&pts);
        assert_eq!(tris.len(), pts.len() - 2);
    }
}
