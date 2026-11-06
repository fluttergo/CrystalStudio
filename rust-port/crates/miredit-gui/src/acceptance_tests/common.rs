//! 验收用例的**共用夹具**：本机样本路径 / 临时目录 / 逐字节差异 / 偏移转格 / 合成样本。
//!
//! 放在单独一个文件里的理由：门 2~6 全都要用这几个函数。各文件各写一份的话，
//! 「逐字节差异」的口径迟早会漂（一个算长度差、一个不算），而这类漂移只会在
//! 某个门突然变绿/变红时才暴露。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mir2_map::Map;

use crate::mapdoc::{Assets, DocState, MapDoc};

use miredit_core::Selection;

// ───────────────────────── 本机样本 ─────────────────────────

/// 资产根缺省值：本机真实根（**整个模块里只出现这一处**）。换机器用 `MIREDIT_ASSET_ROOT`。
pub(super) const 缺省资产根: &str = r"E:\GodM2";
/// 取证根缺省值：`.X` 的取证来源。换机器用 `MIREDIT_UPSTREAM_ROOT`。
pub(super) const 缺省取证根: &str = r"I:\miredit\_upstream";

/// 资产根：`MIREDIT_ASSET_ROOT` → 缺省 [`缺省资产根`]。
///
/// 根目录不存在（公开仓/PR 的机器上没有游戏素材）⇒ `None`，调用方打印 `[跳过]` 并返回。
/// 只做 `var_os` + `PathBuf`，不引入任何第三方库。
pub(super) fn 资产根() -> Option<PathBuf> {
    let root = std::env::var_os("MIREDIT_ASSET_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(缺省资产根));
    root.is_dir().then_some(root)
}

/// 取证根：`MIREDIT_UPSTREAM_ROOT` → 缺省 [`缺省取证根`]；根不存在 ⇒ `None`。
pub(super) fn 取证根() -> Option<PathBuf> {
    let root = std::env::var_os("MIREDIT_UPSTREAM_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(缺省取证根));
    root.is_dir().then_some(root)
}

/// 真实地图样本（700×700 / 12 B 每格 / `Old12`）：`<资产根>/GodotClient/maps/0.map`。
pub(super) fn 地图路径() -> Option<PathBuf> {
    let p = 资产根()?.join("GodotClient").join("maps").join("0.map");
    p.is_file().then_some(p)
}

/// 真实印章样本（25 格 / 包围盒 `(-1,-2,4,3)`；core 门 2 用的也是它）。
pub(super) fn 印章路径() -> Option<PathBuf> {
    let p = 取证根()?.join("Objects").join("mir").join("mir_00000.X");
    p.is_file().then_some(p)
}

/// 真实地图的头部长度（`Old12`：52 字节头 + 12 字节/格）。
pub(super) const 头长: usize = 52;
/// 真实地图的格长。
pub(super) const 格长: usize = 12;


// ───────────────────────── 小工具 ─────────────────────────

/// 临时目录（`%TEMP%` 下按 tag 分开；每次先清空）。
pub(super) fn 临时目录(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("miredit_gui_accept_{tag}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

/// 把真实地图复制一份到临时目录，返回 `(目录, 副本路径, 原字节)`。
///
/// 真图**只读**：所有改动都发生在副本上（`0.map` 是用户资产，绝不碰）。
pub(super) fn 复制地图(tag: &str) -> Option<(PathBuf, PathBuf, Vec<u8>)> {
    let src = match 地图路径() {
        Some(p) => p,
        None => {
            eprintln!("[跳过] 无资产根 MIREDIT_ASSET_ROOT（或缺 GodotClient/maps/0.map）");
            return None;
        }
    };
    let d = 临时目录(tag);
    let p = d.join("0.map");
    std::fs::copy(&src, &p).expect("复制 0.map 副本");
    let 原 = std::fs::read(&p).expect("读副本");
    Some((d, p, 原))
}

/// 造一份 [`DocState`]（与 `DocState::load` 的产物逐字段相同，只是**不装载图库**）。
///
/// 为什么不直接调 `DocState::load`：那会顺带扫描 117 个资源库（与逐字节比对无关，
/// 且拖慢用例）；验收要的是"地图 + 路径"这两样，图库在这一门里一个字节都不参与。
///
/// ⚠ 2026-11-05 九格式收口后**不再手搓 `MapDoc` 字面量**：`MapDoc` 多了一个私有的
/// `document`（字节层真值），字面量构造已经不可能，而且手搓会绕过
/// `mir2_format::read_any_tolerant` 的判别 —— 那正是本单要验的东西。
/// 现在统一走 [`MapDoc::open`]（与 `DocState::load_map` 同一个 `read_map` → `Document::load`），
/// 解析与错误文案**同一份实现**。
pub(super) fn 文档(p: &Path) -> DocState {
    let mapdoc = MapDoc::open(p).expect("副本应能解析");
    DocState {
        doc: Some(mapdoc),
        assets: Assets::Failed("验收不装载图库".to_string()),
        map_error: None,
        map_gen: 1,
    }
}

/// 逐字节差异：返回 `(偏移, 原值, 新值)`（长度不同时多出来的字节按 `0xFF` 记）。
pub(super) fn 差异清单(旧: &[u8], 新: &[u8]) -> Vec<(usize, u8, u8)> {
    let mut out = Vec::new();
    for i in 0..旧.len().max(新.len()) {
        let a = 旧.get(i).copied();
        let b = 新.get(i).copied();
        if a != b {
            out.push((i, a.unwrap_or(0xFF), b.unwrap_or(0xFF)));
        }
    }
    out
}

/// 字节偏移 → 世界格（`Old12`：52 字节头 + 12 字节/格，**列优先** `x * h + y`）。
pub(super) fn 偏移转格(off: usize, 高: u16) -> Option<(u16, u16)> {
    if off < 头长 {
        return None; // 头部字节：不该有任何差异
    }
    let idx = (off - 头长) / 格长;
    let h = usize::from(高);
    Some(((idx / h) as u16, (idx % h) as u16))
}

/// 一格的 12 字节（`Old12`），用于贴 hex 证据。
pub(super) fn 格字节(字节: &[u8], x: u16, y: u16, 高: u16) -> Vec<u8> {
    let off = 头长 + (usize::from(x) * usize::from(高) + usize::from(y)) * 格长;
    字节[off..off + 格长].to_vec()
}

/// 12 字节 → `0x` 开头的 hex（大端书写 = 人读顺序，与文件里的小端存储无关，只作指纹）。
pub(super) fn hex(b: &[u8]) -> String {
    let mut s = String::from("0x");
    for v in b {
        s.push_str(&format!("{v:02X}"));
    }
    s
}

/// 差异清单 → 「哪些格变了」的集合（头部差异会让它 `panic`：头部绝不该变）。
pub(super) fn 变化格(差异: &[(usize, u8, u8)], 高: u16) -> BTreeSet<(u16, u16)> {
    let mut out = BTreeSet::new();
    for (off, _, _) in 差异 {
        let (x, y) = 偏移转格(*off, 高).unwrap_or_else(|| {
            panic!("头部字节 {off} 变了（头部必须逐字节不变）：{差异:?}");
        });
        out.insert((x, y));
    }
    out
}

/// 在图上找一块 **3×3、9 格地表都有图号**的区域（真实地图的地表是密铺的）。
///
/// `避开` = 要跳过的区域（保证源块与粘贴目标不重叠，读起来更直观）。
pub(super) fn 找地表3x3(map: &Map, 避开: Option<Selection>) -> Option<(u16, u16)> {
    for x in 0..map.width.saturating_sub(2) {
        for y in 0..map.height.saturating_sub(2) {
            let 区域 = Selection::new(x, y, x + 2, y + 2);
            if let Some(a) = 避开 {
                if 区域.x0 <= a.x1 && a.x0 <= 区域.x1 && 区域.y0 <= a.y1 && a.y0 <= 区域.y1 {
                    continue;
                }
            }
            let ok = 区域.iter().all(|(cx, cy)| {
                map.cell(cx, cy)
                    .map(|c| c.bk_img & 0x7FFF != 0)
                    .unwrap_or(false)
            });
            if ok {
                return Some((x, y));
            }
        }
    }
    None
}

