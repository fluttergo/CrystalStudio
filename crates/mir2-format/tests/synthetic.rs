//! 合成样本集成测试：**按源码手工构造**九种格式的字节流，验证
//!
//! 1. 每种格式 `read → write` 逐字节相等；
//! 2. **判别互斥**：每种格式只被自己命中（`detect_format` 与其余八种 `detect` 全为假）；
//! 3. 「`52 + 40·w·h`」这个**无实证**的长度形态（仅防御，真实语料 0 个）显式报 `UnsupportedVariant`；
//! 4. 尾随数据 / 未知格式 / 截断 / 写前自洽校验；
//! 5. C# 判别链（只看标题）与本 crate 判别（标题 + 长度自洽）在**已知会错的样本**上的分歧。
//!
//! ⚠ `Wemade2010` / `AntiHack` / `Mir3Wemade` / `Mir3Shanda` / `Heroes34` / `CrystalCs`
//! 六种格式**本机无真实样本**（`GodotClient/maps` 61 个 + `client/热血传奇23周年客户端/Map`
//! 1097 个里 0 个命中），本文件是它们**唯一的**验证来源 —— 全部字节按
//! `CrystalStudio/LegacyMapReader.cs` 的逐字段读法反推构造。

use mir2_format::formats::{crystal_cs, mir3_wemade};
use mir2_format::{
    csharp_40b_variant, detect_format, detect_format_csharp_order, read_any, read_any_tolerant,
    registry, Cell, FormatError, FormatId, MapFormat, RawMap,
};

/// 资产根缺省值：本机真实根（**整个文件里只出现这一处**）。换机器用 `MIREDIT_ASSET_ROOT`。
const 缺省资产根: &str = r"E:\GodM2";

/// 资产根：`MIREDIT_ASSET_ROOT` → 缺省 [`缺省资产根`]。
///
/// 根目录不存在（公开仓/PR 的机器上没有游戏素材）⇒ `None`，调用方打印 `[skip]` 并返回。
/// 只做 `var_os` + `PathBuf`，不引入任何第三方库。
fn 资产根() -> Option<std::path::PathBuf> {
    let root = std::env::var_os("MIREDIT_ASSET_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(缺省资产根));
    root.is_dir().then_some(root)
}

// ────────────────────────────── 合成样本构造器 ──────────────────────────────

/// 造一块「头 `header_len` 字节全零 + 其后按确定模式填充」的缓冲区。
fn 空白(header_len: usize, cell_len: usize, w: u16, h: u16) -> Vec<u8> {
    let mut b = vec![0u8; header_len + cell_len * w as usize * h as usize];
    for (i, slot) in b.iter_mut().enumerate().skip(header_len) {
        *slot = (i.wrapping_mul(37).wrapping_add(11) & 0xFF) as u8;
    }
    b
}

fn 写宽高(b: &mut [u8], off_w: usize, off_h: usize, w: u16, h: u16) {
    b[off_w..off_w + 2].copy_from_slice(&(w as i16).to_le_bytes());
    b[off_h..off_h + 2].copy_from_slice(&(h as i16).to_le_bytes());
}

/// `Old12`：52 字节头（`wWidth` i16@0、`wHeight` i16@2），12 B/格。
fn 造_old12(w: u16, h: u16) -> Vec<u8> {
    let mut b = 空白(52, 12, w, h);
    写宽高(&mut b, 0, 2, w, h);
    b
}

/// `Shanda14`：52 字节头 + 标题签名 `Bytes[4]=0x0F, [18]=0x0D, [19]=0x0A`，14 B/格。
fn 造_shanda14(w: u16, h: u16, 带签名: bool) -> Vec<u8> {
    let mut b = 空白(52, 14, w, h);
    写宽高(&mut b, 0, 2, w, h);
    b[4] = if 带签名 { 0x0F } else { 0x07 };
    b[18] = 0x0D;
    b[19] = 0x0A;
    b
}

/// `Shanda2012`：同 `Shanda14` 的头，36 B/格。
fn 造_shanda2012(w: u16, h: u16) -> Vec<u8> {
    let mut b = 空白(52, 36, w, h);
    写宽高(&mut b, 0, 2, w, h);
    b[4] = 0x0F;
    b[18] = 0x0D;
    b[19] = 0x0A;
    b
}

/// `Wemade2010`：54 字节头，`w`@21、`xor`@23、`h`@25，15 B/格。
fn 造_wemade2010(w: u16, h: u16, xor: i16) -> Vec<u8> {
    let mut b = 空白(54, 15, w, h);
    b[0] = 0x10;
    b[2] = 0x61;
    b[7] = 0x31;
    b[14] = 0x31;
    b[21..23].copy_from_slice(&((w as i16) ^ xor).to_le_bytes());
    b[23..25].copy_from_slice(&xor.to_le_bytes());
    b[25..27].copy_from_slice(&((h as i16) ^ xor).to_le_bytes());
    b
}

/// `AntiHack`：64 字节头，`w`@31、`xor`@33、`h`@35，12 B/格。
fn 造_antihack(w: u16, h: u16, xor: i16) -> Vec<u8> {
    let mut b = 空白(64, 12, w, h);
    b[0] = 0x15;
    b[4] = 0x32;
    b[6] = 0x41;
    b[19] = 0x31;
    b[31..33].copy_from_slice(&((w as i16) ^ xor).to_le_bytes());
    b[33..35].copy_from_slice(&xor.to_le_bytes());
    b[35..37].copy_from_slice(&((h as i16) ^ xor).to_le_bytes());
    b
}

/// `Mir3Wemade`：28 字节头 + `3*ceil(w/2)*floor(h/2)` 背景块，14 B/格。
fn 造_mir3_wemade(w: u16, h: u16) -> Vec<u8> {
    let off = mir3_wemade::Mir3Wemade::cells_offset(w, h);
    let mut b = vec![0u8; off + 14 * w as usize * h as usize];
    for (i, slot) in b.iter_mut().enumerate().skip(28) {
        *slot = (i.wrapping_mul(29).wrapping_add(5) & 0xFF) as u8;
    }
    b[0] = 0; // 无标题
    b[20..22].copy_from_slice(&1i16.to_le_bytes());
    写宽高(&mut b, 22, 24, w, h);
    b
}

/// `Mir3Shanda`：40 字节头，`w`@16、`h`@18，20 B/格。
fn 造_mir3_shanda(w: u16, h: u16) -> Vec<u8> {
    let mut b = 空白(40, 20, w, h);
    b[0] = 0x0F;
    b[5] = 0x53;
    b[14] = 0x33;
    写宽高(&mut b, 16, 18, w, h);
    b
}

/// `Heroes34`：54 字节头，`w`@21、`h`@25，15 B/格。
fn 造_heroes34(w: u16, h: u16) -> Vec<u8> {
    let mut b = 空白(54, 15, w, h);
    b[0] = 0x0D;
    b[1] = 0x4C;
    b[7] = 0x20;
    b[11] = 0x6D;
    写宽高(&mut b, 21, 25, w, h);
    b
}

/// `CrystalCs`：8 字节头（ver@0、`C#`@2、w@4、h@6），变长格。
fn 造_crystal_cs(ver: i16, w: u16, h: u16, 带前景: bool) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&ver.to_le_bytes());
    b.extend_from_slice(&crystal_cs::MAGIC);
    b.extend_from_slice(&(w as i16).to_le_bytes());
    b.extend_from_slice(&(h as i16).to_le_bytes());
    for i in 0..(w as usize * h as usize) {
        b.extend_from_slice(&(i as i16).to_le_bytes());
        b.extend_from_slice(&(0x1000u32 + i as u32).to_le_bytes());
        b.extend_from_slice(&2i16.to_le_bytes());
        b.extend_from_slice(&(0x2000u16 + i as u16).to_le_bytes());
        if ver > 2 {
            b.push(u8::from(带前景));
            if 带前景 {
                b.extend_from_slice(&7i16.to_le_bytes());
                b.extend_from_slice(&0x3000u16.to_le_bytes());
                if ver > 3 {
                    b.push(0x11);
                    b.push(0x22);
                    if ver > 4 {
                        b.push(0x33);
                    }
                }
            }
        } else {
            b.extend_from_slice(&(if 带前景 { 7i16 } else { 0i16 }).to_le_bytes());
            b.extend_from_slice(&(if 带前景 { 0x3000u16 } else { 0u16 }).to_le_bytes());
        }
        b.push(0x81);
        b.push(0x02);
        if ver < 4 {
            b.push(0x44);
            b.push(0x55);
        }
        b.push(0x08);
        b.push(0x09);
        b.extend_from_slice(&0x1234i16.to_le_bytes());
        b.extend_from_slice(&0x5678i16.to_le_bytes());
        b.push(0x0A);
        b.push(0x0B);
        if ver >= 2 {
            for k in 0..12u8 {
                b.push(0xC0 + k);
            }
        }
    }
    b
}

/// 九种格式各来一份代表性合成样本（`CrystalCs` 取 ver=2，其余取固定宽高）。
fn 九份样本() -> Vec<(FormatId, Vec<u8>, &'static str)> {
    vec![
        (
            FormatId::CrystalCs,
            造_crystal_cs(2, 3, 2, true),
            "无真实样本，仅合成验证",
        ),
        (
            FormatId::Mir3Wemade,
            造_mir3_wemade(4, 4),
            "无真实样本，仅合成验证",
        ),
        (
            FormatId::Mir3Shanda,
            造_mir3_shanda(4, 3),
            "无真实样本，仅合成验证",
        ),
        (
            FormatId::AntiHack,
            造_antihack(5, 4, 0x0303),
            "无真实样本，仅合成验证",
        ),
        (
            FormatId::Wemade2010,
            造_wemade2010(4, 3, 0x0F0F),
            "无真实样本，仅合成验证",
        ),
        (
            FormatId::Shanda2012,
            造_shanda2012(3, 2),
            "有真实样本（195 个）",
        ),
        (
            FormatId::Shanda14,
            造_shanda14(4, 3, true),
            "有真实样本（107 个）",
        ),
        (
            FormatId::Heroes34,
            造_heroes34(3, 5),
            "无真实样本，仅合成验证",
        ),
        (FormatId::Old12, 造_old12(4, 3), "有真实样本（856 个）"),
    ]
}

// ────────────────────────────── ① 九种格式逐字节往返 ──────────────────────────────

#[test]
fn 九种格式合成样本逐字节往返() {
    for (id, raw, 备注) in 九份样本() {
        let map = read_any(&raw).unwrap_or_else(|e| panic!("{id:?}（{备注}）应解析成功：{e}"));
        assert_eq!(map.format, id, "{id:?} 的 format 字段不对");
        let out = mir2_format::format_by_id(id)
            .write(&map)
            .unwrap_or_else(|e| panic!("{id:?}（{备注}）应写回成功：{e}"));
        assert_eq!(out.len(), raw.len(), "{id:?} 写回长度不一致");
        assert!(out == raw, "{id:?}（{备注}）往返不是逐字节相等");
    }
}

// ────────────────────────────── ② 判别互斥 ──────────────────────────────

#[test]
fn 判别互斥_每种格式只命中自己() {
    for (id, raw, 备注) in 九份样本() {
        // 注册表里的九种格式，除自己外都不得自认
        for f in registry() {
            if f.id() == id {
                assert!(f.detect(&raw), "{} 应自认自己的样本（{备注}）", f.name());
            } else {
                assert!(
                    !f.detect(&raw),
                    "{} 误判了 {id:?} 的样本（{备注}）",
                    f.name()
                );
            }
        }
        // 统一入口只能判出唯一答案
        let hit =
            detect_format(&raw).unwrap_or_else(|| panic!("{id:?} 的样本应能被 detect_format 认出"));
        assert_eq!(
            hit.id(),
            id,
            "detect_format 把 {id:?} 判成了 {}",
            hit.name()
        );
    }
}

#[test]
fn 判别互斥_九种格式两两不重叠() {
    let 样本 = 九份样本();
    for (id_a, raw_a, _) in &样本 {
        for (id_b, _, _) in &样本 {
            if id_a == id_b {
                continue;
            }
            assert_ne!(
                detect_format(raw_a).map(|f| f.id()),
                Some(*id_b),
                "{id_a:?} 的样本被判成了 {id_b:?}"
            );
        }
    }
}

// ────────────────────── ③ 「52 + 40·w·h」长度形态（无实证，仅防御） ──────────────────────

#[test]
fn csharp_四十字节变体显式报不支持() {
    let w = 3u16;
    let h = 2u16;
    let mut raw = vec![0u8; 52 + 40 * w as usize * h as usize];
    写宽高(&mut raw, 0, 2, w, h);
    raw[4] = 0x0F;
    raw[18] = 0x0D;
    raw[19] = 0x0A;

    assert_eq!(csharp_40b_variant(&raw), Some((w, h)));
    assert!(
        detect_format(&raw).is_none(),
        "40 B/格变体不该被任何格式认领"
    );
    match read_any(&raw) {
        Err(FormatError::UnsupportedVariant { format, detail }) => {
            assert_eq!(format, FormatId::Shanda2012);
            assert!(detail.contains("40"), "错误说明里应写明 40B：{detail}");
        }
        other => panic!("应报 UnsupportedVariant，实际 {other:?}"),
    }
}

// ────────────────────────────── ④ 尾随 / 未知 / 截断 / 写前校验 ──────────────────────────────

#[test]
fn 尾随数据只走宽松入口且原样保留() {
    let mut raw = 造_old12(4, 3);
    raw.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);

    // 严格入口必须拒绝（长度不自洽 = 不是 9 种格式里的任何一种合法长度）
    assert!(
        detect_format(&raw).is_none(),
        "严格判别不该认领长度多一截的文件"
    );
    match read_any(&raw) {
        Err(FormatError::LengthMismatch {
            format,
            width,
            height,
            len,
            expected,
        }) => {
            assert_eq!(format, FormatId::Old12);
            assert_eq!((width, height, len, expected), (4, 3, 200, 196));
        }
        other => panic!("应报 LengthMismatch，实际 {other:?}"),
    }

    // 宽松入口能读，且尾随字节原样保留、写回逐字节相等
    let map = read_any_tolerant(&raw).expect("宽松入口应能读");
    assert_eq!(map.format, FormatId::Old12);
    assert_eq!(map.trailer, vec![0xDE, 0xAD, 0xBE, 0xEF]);
    assert_eq!(
        mir2_format::format_by_id(map.format).write(&map).unwrap(),
        raw
    );
}

#[test]
fn 未知格式与截断() {
    assert!(matches!(
        read_any(&[]),
        Err(FormatError::UnknownFormat { len: 0 })
    ));
    assert!(matches!(
        read_any(&[1u8; 10]),
        Err(FormatError::UnknownFormat { len: 10 })
    ));

    // 头部声明 4×3 的 Old12，但只给了一半格数据 ⇒ Truncated
    let mut raw = 造_old12(4, 3);
    raw.truncate(raw.len() - 1);
    match read_any(&raw) {
        Err(FormatError::Truncated { format, .. }) => assert_eq!(format, FormatId::Old12),
        other => panic!("应报 Truncated，实际 {other:?}"),
    }
}

#[test]
fn 写前自洽校验() {
    let raw = 造_old12(4, 3);
    let map = read_any(&raw).unwrap();

    // 格数不对
    let mut bad = map.clone();
    bad.cells.pop();
    assert!(matches!(
        mir2_format::format_by_id(map.format).write(&bad),
        Err(FormatError::Inconsistent { .. })
    ));

    // 头部长度不对
    let mut bad = map.clone();
    bad.header_raw.push(0);
    assert!(matches!(
        mir2_format::format_by_id(map.format).write(&bad),
        Err(FormatError::Inconsistent { .. })
    ));

    // 目标格式与 RawMap.format 不符
    let mut bad = map.clone();
    bad.format = FormatId::Shanda14;
    assert!(matches!(
        mir2_format::format_by_id(FormatId::Old12).write(&bad),
        Err(FormatError::Inconsistent { .. })
    ));

    // 36 B/格格式的尾区长度不对
    let raw36 = 造_shanda2012(3, 2);
    let mut m36 = read_any(&raw36).unwrap();
    m36.cells[0].tail.pop();
    assert!(matches!(
        mir2_format::format_by_id(FormatId::Shanda2012).write(&m36),
        Err(FormatError::Inconsistent { .. })
    ));
}

// ────────────────────────────── ⑤ C# 判别链 vs 本 crate 判别 ──────────────────────────────

/// 已知冲突 1：14 B/格 但标题首字节是 `0x07`（真实样本 `GodotClient/maps/10.map` 就是这种）。
/// C# 只看标题 ⇒ 落到兜底 `LoadMapType0`（12 B/格），读成花图；本 crate 按长度判出 14 B/格。
#[test]
fn 冲突一_十四字节格但标题首字节为零七() {
    let raw = 造_shanda14(4, 3, false);
    assert_eq!(
        detect_format_csharp_order(&raw).map(|f| f.id()),
        Some(FormatId::Old12),
        "纯 C# 判别链会把它判成兜底的 Old12"
    );
    assert_eq!(
        detect_format(&raw).map(|f| f.id()),
        Some(FormatId::Shanda14),
        "本 crate 按长度自洽判出 Shanda14"
    );
}

/// 已知冲突 2：12 B/格 但标题恰好命中 14 B/格 签名（真实样本 `hell107.map` 就是这种）。
/// C# 只看标题 ⇒ 按 14 B/格读，越界读穿；本 crate 按长度判出 12 B/格。
#[test]
fn 冲突二_十二字节格但标题命中十四字节签名() {
    let raw = 造_old12(4, 3);
    let mut raw = raw;
    raw[4] = 0x0F;
    raw[18] = 0x0D;
    raw[19] = 0x0A;
    let csharp = detect_format_csharp_order(&raw)
        .map(|f| f.id())
        .expect("纯 C# 判别链一定有兜底");
    assert!(
        matches!(csharp, FormatId::Shanda2012 | FormatId::Shanda14),
        "纯 C# 判别链会按标题把它判成 14/36 字节格那两支之一，实际 {csharp:?}"
    );
    assert_eq!(
        detect_format(&raw).map(|f| f.id()),
        Some(FormatId::Old12),
        "本 crate 按长度自洽判出 Old12"
    );
}

// ────────────────────────────── ⑥ CrystalCs 全版本往返 ──────────────────────────────

#[test]
fn crystal_cs_全版本全分支往返() {
    for ver in 1..=5i16 {
        for 带前景 in [false, true] {
            let raw = 造_crystal_cs(ver, 3, 2, 带前景);
            let map = read_any(&raw)
                .unwrap_or_else(|e| panic!("ver={ver} 带前景={带前景} 应解析成功：{e}"));
            assert_eq!(map.format, FormatId::CrystalCs);
            assert_eq!(map.cells.len(), 6);
            assert_eq!(
                crystal_cs::CrystalCs.write(&map).unwrap_or_else(|e| {
                    panic!("ver={ver} 带前景={带前景} 应写回成功：{e}")
                }),
                raw,
                "ver={ver} 带前景={带前景} 往返不一致"
            );
            // 格长必须落在源码推出的区间内
            let per = (raw.len() - crystal_cs::HEADER_LEN) / 6;
            assert!(
                per >= crystal_cs::min_cell_len(ver),
                "ver={ver} 带前景={带前景} 的实测格长 {per} 小于源码给出的最小格长 {}",
                crystal_cs::min_cell_len(ver)
            );
        }
    }
}

// ────────────────────────────── ⑦ 列优先下标 ──────────────────────────────

#[test]
fn 九种格式都是列优先() {
    // 对每种定长格式：把 (x=2,y=1) 的格首两字节改成 0xAA 0xAA，
    // 若实现是行优先（idx = y*w+x），它会落到 (x=1,y=2) 去。
    let 样本: Vec<(FormatId, Vec<u8>, usize, usize, usize)> = vec![
        // (格式, 原始字节, 头部长度, 宽, 高)
        (FormatId::Old12, 造_old12(4, 3), 52, 4, 3),
        (FormatId::Shanda14, 造_shanda14(4, 3, true), 52, 4, 3),
        (FormatId::Shanda2012, 造_shanda2012(4, 3), 52, 4, 3),
        (FormatId::Wemade2010, 造_wemade2010(4, 3, 0), 54, 4, 3),
        (FormatId::AntiHack, 造_antihack(4, 3, 0), 64, 4, 3),
        (FormatId::Mir3Shanda, 造_mir3_shanda(4, 3), 40, 4, 3),
        (FormatId::Heroes34, 造_heroes34(4, 3), 54, 4, 3),
        (
            FormatId::Mir3Wemade,
            造_mir3_wemade(4, 6),
            mir3_wemade::Mir3Wemade::cells_offset(4, 6),
            4,
            6,
        ),
    ];
    for (id, mut raw, head, w, h) in 样本 {
        let cell = match id {
            FormatId::Old12 => 12,
            FormatId::Shanda14 => 14,
            FormatId::Shanda2012 => 36,
            FormatId::Wemade2010 | FormatId::Heroes34 => 15,
            FormatId::AntiHack => 12,
            FormatId::Mir3Shanda => 20,
            FormatId::Mir3Wemade => 14,
            _ => unreachable!("本表只放定长格式"),
        };
        let idx = 2 * h + 1; // (x=2, y=1) 的**列优先**下标 = x*h + y
        let 行优先idx = 2 * w + 1; // (x=1, y=2) 的**行优先**下标 = y*w + x
        assert_ne!(idx, 行优先idx, "前提：(2,1) 与 (1,2) 的下标必须不同");
        raw[head + idx * cell] = 0xAA;
        raw[head + idx * cell + 1] = 0xAA;

        let map = read_any(&raw).unwrap_or_else(|e| panic!("{id:?} 应能读：{e}"));
        // Mir3 系列的格内偏移 0 是 flag（1 字节）；其余格式是背景图号的低 2 字节。
        let mir3 = matches!(id, FormatId::Mir3Wemade | FormatId::Mir3Shanda);
        let 取值 = |c: &Cell| -> u16 {
            if mir3 {
                u16::from(c.flag)
            } else {
                (c.bk_img & 0xFFFF) as u16
            }
        };
        let 期望: u16 = if mir3 { 0x00AA } else { 0xAAAA };
        let c = map
            .cell(2, 1)
            .unwrap_or_else(|| panic!("{id:?} (2,1) 应在图内"));
        assert_eq!(
            取值(c),
            期望,
            "{id:?} 的格首字节应落在 (2,1)（列优先 idx = x*h+y = {idx}）"
        );

        let c2 = map
            .cell(1, 2)
            .unwrap_or_else(|| panic!("{id:?} (1,2) 应在图内"));
        assert_ne!(取值(c2), 期望, "{id:?} 疑似行优先");

        assert_eq!(
            mir2_format::format_by_id(id).write(&map).unwrap(),
            raw,
            "{id:?} 改动后仍应逐字节往返"
        );
    }
}

// ────────────────────────────── ⑧ Cell 构造助手 ──────────────────────────────
#[test]
fn cell_zeroed_尾区长度正确() {
    assert_eq!(Cell::zeroed(0).tail.len(), 0);
    assert_eq!(Cell::zeroed(22).tail, vec![0u8; 22]);
}

#[test]
fn rawmap_越界取格返回none() {
    let map: RawMap = read_any(&造_old12(2, 3)).unwrap();
    assert!(map.cell(0, 0).is_some());
    assert!(map.cell(2, 0).is_none());
    assert!(map.cell(0, 3).is_none());
}

// ─────────────────────── ⑨ 父代理 2026-11-04 指定的 3 条判别回归 ───────────────────────

/// **回归 1**：真实 14 B/格 但 `Bytes[4] != 0x0F` 的图必须判成 `Shanda14`，不许判成 `Old12`。
///
/// 样本：`GodotClient/maps/10.map`（挂在资产根下）—— 110×118、181772 字节
/// （`52 + 14*110*118 = 181772` 精确成立），标题首字节实测 **`0x07`** 而不是 `0x0F`。
/// C# 只看标题 ⇒ 落到兜底 `LoadMapType0` 按 12 B/格读 ⇒ 读成花图。
/// 同批还有 14 个（`14.map` `B[4]=0x0D`、`20.map` `0xFF`、`33.map`/`4.map` `0x03`、
/// `42.map`/`58.map`/`64.map` `0x0B`、其余 `0x07`），`需要的资料` 下另有同名 15 个。
#[test]
fn 回归一_真实十四字节格但标题不是零f() {
    let Some(root) = 资产根() else {
        eprintln!("[skip] 无资产根 MIREDIT_ASSET_ROOT");
        return;
    };
    let p = root.join("GodotClient").join("maps").join("10.map");
    if !p.is_file() {
        eprintln!("[skip] 缺文件 {}", p.display());
        return;
    }
    let raw = std::fs::read(&p).expect("应能读该真实样本");
    assert_eq!(raw.len(), 181_772, "样本长度应实测 181772");
    assert_eq!(
        raw[4], 0x07,
        "该样本 B[4] 实测应为 0x07（不是 Shanda14 签名要的 0x0F）"
    );
    assert!(
        !mir2_format::formats::shanda14::Shanda14.header_signature(&raw),
        "前提：它确实不满足 C# 的 Shanda14 标题签名"
    );

    let hit = detect_format(&raw).expect("必须能判出格式");
    assert_eq!(
        hit.id(),
        FormatId::Shanda14,
        "必须判成 Shanda14，实际判成 {}",
        hit.name()
    );
    let map = read_any(&raw).expect("应能解析");
    assert_eq!((map.width, map.height), (110, 118));
    assert_eq!(map.cells.len(), 110 * 118);
    assert_eq!(
        mir2_format::format_by_id(map.format).write(&map).unwrap(),
        raw
    );
}

/// **回归 2**：12 B/格 家族里，有 `AntiHack` 签名 ⇒ `AntiHack`；无签名 ⇒ `Old12`。
///
/// 两者格长都是 12，但头部长度不同（`AntiHack` 64 / `Old12` 52），
/// 所以 `64 + 12·w·h` 与 `52 + 12·w·h` 在同一组 `w/h` 下不可能同时成立 ⇒ 长度即可分家。
#[test]
fn 回归二_十二字节家族按签名区分antihack与old12() {
    let ah = 造_antihack(4, 3, 0);
    assert_eq!(ah.len(), 64 + 12 * 12);
    assert!(mir2_format::formats::antihack::AntiHack.header_signature(&ah));
    assert_eq!(
        detect_format(&ah).map(|f| f.id()),
        Some(FormatId::AntiHack),
        "有 AntiHack 签名就该判成 AntiHack"
    );

    let o12 = 造_old12(4, 3);
    assert_eq!(o12.len(), 52 + 12 * 12);
    assert!(!mir2_format::formats::antihack::AntiHack.header_signature(&o12));
    assert_eq!(
        detect_format(&o12).map(|f| f.id()),
        Some(FormatId::Old12),
        "无 AntiHack 签名就该判成 Old12"
    );
}

/// **回归 3**：头部声明 `w×h` 但长度既不整除 12 也不整除 14 ⇒ 必须 `Err`，不许落到任何格式。
#[test]
fn 回归三_长度与格数对账不上必须报错() {
    // 4×3 = 12 格：12 B/格要 196 字节、14 B/格要 220 字节；给 190 字节（截断）
    let mut 截断 = 造_old12(4, 3);
    截断.truncate(190);
    assert!(detect_format(&截断).is_none(), "严格判别不该认领");
    match read_any(&截断) {
        Err(FormatError::Truncated { format, len, need }) => {
            assert_eq!((format, len, need), (FormatId::Old12, 190, 196));
        }
        other => panic!("应报 Truncated，实际 {other:?}"),
    }
    assert!(
        read_any_tolerant(&截断).is_err(),
        "宽松入口也不该救活截断文件"
    );

    // 给 205 字节（多一截但既不整除 12 也不整除 14：205-52 = 153）
    let mut 多一截 = 造_old12(4, 3);
    多一截.resize(205, 0);
    assert!(detect_format(&多一截).is_none(), "严格判别不该认领");
    assert!(
        matches!(read_any(&多一截), Err(FormatError::LengthMismatch { .. })),
        "严格入口必须报 LengthMismatch"
    );
    // 宽松入口能读，且字节精确往返
    let m = read_any_tolerant(&多一截).expect("宽松入口应能读");
    assert_eq!(m.trailer.len(), 205 - 196);
    assert_eq!(
        mir2_format::format_by_id(m.format).write(&m).unwrap(),
        多一截
    );
}
