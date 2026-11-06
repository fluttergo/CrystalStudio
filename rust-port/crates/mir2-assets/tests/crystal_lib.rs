//! 原版 `.lib` 读取器 / 路径表 / 图源的**合成样本**测试。
//!
//! # 为什么全是合成样本
//!
//! 全部样本由本文件按 `Assets.cs:14-204@901dbe2f` 的字段表逐字节拼出来：
//! 这样每个断言都能指向「上游第几行要求的那个字段」，而不是「反正是从某个文件读出来的」，
//! 公开仓 CI 也不依赖任何本机商业素材。
//!
//! **真实素材验收**在 `tests/crystal_lib_real.rs`（用 `MIREDIT_LIB_ROOT` 指定原版
//! 客户端 `Data` 目录，未设置则 `[skip]`）：那边证明「原版素材能读、能解、像素长度自洽」，
//! 这边证明「字段顺序读对了」，两者不可互相替代。
//!
//! # 覆盖矩阵
//!
//! | 用例 | 覆盖 |
//! |---|---|
//! | `v2两帧_bgra转rgba与锚点` | v2 布局、gzip BGRA→RGBA、锚点、帧号越界 |
//! | `v3跳过保留字段且偏移正确` | v3 额外 `i32`、偏移表 |
//! | `sanxiu_v5_gzip与遮罩长度` | v5 元数据、`shadow & 0x80` + `maskLength` 的偏移推进 |
//! | `sanxiu_v6_文件类型0与1` | v6 每图 18 字节元数据、`fileType` 分派、内嵌 PNG |
//! | `sanxiu_v6_gif尺寸回退` | `Assets.cs:74-82` 的 10 字节 GIF 回退 |
//! | `坏样本_*` | 偏移越界 / length 越界 / gzip 尾随字节 / 尺寸 0 与 8192/8193 / headerLength |
//! | `路径发现_*` | 三族登记、N+1 换算、规则外与超范围、`root/Map`、`Auto` 探测 |
//! | `source_*` | 帧数、失败归集、锚点快路径与取帧同值、LRU |
//!
//! 注：`CrystalLibrary::from_bytes`（内存后端）与 `open`（文件后端）的等价性由
//! `src/crystal_lib.rs` 的单测 `两种后端解码结果一致` 钉住；`open` 必须是 path-backed
//! 由单测 `open_走path_backed只读元数据区` 钉住（若退回整文件读入会当场红）。

use std::path::{Path, PathBuf};

use mir2_assets::{
    AssetError, CrystalAssetFormat, CrystalLibPaths, CrystalLibSource, CrystalLibrary,
    CrystalSkipReason, Family, LibRef, SpriteSource,
};

// ===================== 合成样本构造 =====================

/// gzip 压缩（每帧数据在 `.lib` 里都是 gzip 流，`Assets.cs:105@901dbe2f`）。
fn gzip(data: &[u8]) -> Vec<u8> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;

    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data).expect("写 gzip 流");
    enc.finish().expect("收尾 gzip 流")
}

/// RGBA 字节序 → **BGRA** 字节序（`.lib` 磁盘口径，`RasterFormats.Bgra32`）。
fn to_bgra(rgba: &[u8]) -> Vec<u8> {
    let (pixels, rest) = rgba.as_chunks::<4>();
    assert!(rest.is_empty(), "样本必须是整像素");
    let mut out = Vec::with_capacity(rgba.len());
    for px in pixels {
        out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
    }
    out
}

/// 一帧 v2/v3 的规格。
#[derive(Clone)]
struct LegacyFrame {
    w: i16,
    h: i16,
    x: i16,
    y: i16,
    payload: Vec<u8>,
    /// `Some(n)` 时把帧头的 `length` 写成 `n`（造坏样本用），否则用 `payload.len()`。
    length_override: Option<i32>,
}

/// 造一帧（长度取 `payload.len()`）。
fn frame(w: i16, h: i16, x: i16, y: i16, payload: Vec<u8>) -> LegacyFrame {
    LegacyFrame {
        w,
        h,
        x,
        y,
        payload,
        length_override: None,
    }
}

/// 按 `Assets.cs:63-66@901dbe2f` 拼一个 v2/v3 `.lib`。
fn build_legacy(version: i32, frames: &[LegacyFrame]) -> Vec<u8> {
    let prefix = 8 + if version == 3 { 4 } else { 0 } + 4 * frames.len();
    let mut body: Vec<u8> = Vec::new();
    let mut offsets: Vec<i32> = Vec::new();
    for f in frames {
        offsets.push((prefix + body.len()) as i32);
        body.extend_from_slice(&f.w.to_le_bytes());
        body.extend_from_slice(&f.h.to_le_bytes());
        body.extend_from_slice(&f.x.to_le_bytes());
        body.extend_from_slice(&f.y.to_le_bytes());
        body.extend_from_slice(&0i16.to_le_bytes()); // 未使用
        body.extend_from_slice(&0i16.to_le_bytes()); // 未使用
        body.push(0); // 未使用
        let len = f.length_override.unwrap_or(f.payload.len() as i32);
        body.extend_from_slice(&len.to_le_bytes());
        body.extend_from_slice(&f.payload);
    }
    let mut out = Vec::with_capacity(prefix + body.len());
    out.extend_from_slice(&version.to_le_bytes());
    out.extend_from_slice(&(frames.len() as i32).to_le_bytes());
    if version == 3 {
        // 非零保留字段：证明解析确实按顺序跳过它（若错位，后面的偏移表必然读歪）。
        out.extend_from_slice(&0x5EED_5EEDi32.to_le_bytes());
    }
    for o in &offsets {
        out.extend_from_slice(&o.to_le_bytes());
    }
    out.extend_from_slice(&body);
    out
}

/// 一张 Sanxiu 图的规格。
#[derive(Clone)]
struct SanxiuImage {
    w: i16,
    h: i16,
    x: i16,
    y: i16,
    /// `0` = gzip BGRA；`1` = 图片文件（v6 才写入该字节）。
    file_type: u8,
    /// 数据区（gzip 流或整张图片）。
    payload: Vec<u8>,
    /// 非空 ⇒ `shadow |= 0x80`，并按 `Assets.cs:50-53` 追加 4×`i16` + `maskLength`。
    mask: Vec<u8>,
    /// `Some(n)` 时把元数据里的 `length` 写成 `n`（造坏样本用）。
    length_override: Option<i32>,
}

/// 造一张 Sanxiu 图（长度取 `payload.len()`）。
fn sanxiu(w: i16, h: i16, x: i16, y: i16, file_type: u8, payload: Vec<u8>) -> SanxiuImage {
    SanxiuImage {
        w,
        h,
        x,
        y,
        file_type,
        payload,
        mask: Vec::new(),
        length_override: None,
    }
}

/// 按 `Assets.cs:27-60@901dbe2f` 拼一个 Sanxiu v5/v6 `.lib`。
fn build_sanxiu(version: i32, images: &[SanxiuImage]) -> Vec<u8> {
    let mut header: Vec<u8> = Vec::new();
    header.extend_from_slice(&(images.len() as i32).to_le_bytes());
    for img in images {
        header.extend_from_slice(&img.w.to_le_bytes());
        header.extend_from_slice(&img.h.to_le_bytes());
        header.extend_from_slice(&img.x.to_le_bytes());
        header.extend_from_slice(&img.y.to_le_bytes());
        header.extend_from_slice(&0i16.to_le_bytes()); // 未使用
        header.extend_from_slice(&0i16.to_le_bytes()); // 未使用
        header.push(if img.mask.is_empty() { 0 } else { 0x80 });
        if version == 6 {
            header.push(img.file_type);
        }
        let len = img.length_override.unwrap_or(img.payload.len() as i32);
        header.extend_from_slice(&len.to_le_bytes());
        if !img.mask.is_empty() {
            header.extend_from_slice(&[0u8; 8]); // 4×i16 未使用
            header.extend_from_slice(&(img.mask.len() as i32).to_le_bytes());
        }
    }
    let mut out = Vec::with_capacity(4 + 1 + 4 + header.len() + 64);
    out.extend_from_slice(&version.to_le_bytes());
    out.push(0); // 未加密
    out.extend_from_slice(&(header.len() as i32).to_le_bytes());
    out.extend_from_slice(&header);
    for img in images {
        out.extend_from_slice(&img.payload);
        out.extend_from_slice(&img.mask);
    }
    out
}

/// 一张 2×2 的 RGBA 样本：四角颜色互不相同，且右下角 alpha 刻意取 4
/// （若实现把 alpha 也一起换位，这条会当场红）。
fn rgba_2x2() -> Vec<u8> {
    vec![
        255, 0, 0, 255, // 左上：红
        0, 255, 0, 255, // 右上：绿
        0, 0, 255, 128, // 左下：半透明蓝
        1, 2, 3, 4,     // 右下：怪值（含 alpha 4）
    ]
}

/// 用 `image` 编一张 PNG（Sanxiu `fileType == 1` 的样本）。
fn png_of(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    use image::ImageEncoder;

    let mut out: Vec<u8> = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(rgba, w, h, image::ExtendedColorType::Rgba8)
        .expect("编码 PNG");
    out
}

/// 1×1 透明 GIF89a（43 字节）。
///
/// 逐字节手拼（Pillow 12.3 实测：`format GIF, size (1,1), mode P, 像素 (0,0,0,0)`），
/// 用来覆盖 `fileType == 1` 的「GIF 尺寸回退」与「整张图片解码」两条路径。
const GIF_1X1: [u8; 43] = [
    0x47, 0x49, 0x46, 0x38, 0x39, 0x61, // "GIF89a"
    0x01, 0x00, 0x01, 0x00, // 逻辑屏宽 1、高 1
    0x80, 0x00, 0x00, // 有全局色表（2 色）、背景 0、无比例
    0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, // 全局色表：黑、白
    0x21, 0xF9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, // 图形控制块：透明色索引 0
    0x2C, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, // 图像描述符 1×1
    0x02, 0x02, 0x44, 0x01, 0x00, // LZW 数据
    0x3B, // 结束
];

// ===================== 临时目录 =====================

/// 带 `_{pid}` 的临时根（防并发测试互相踩），`Drop` 时清理。
struct TempRoot(PathBuf);

impl TempRoot {
    /// 造一个空临时根。
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "mir2_assets_crystal_{tag}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        Self(dir)
    }

    /// 根路径。
    fn path(&self) -> &Path {
        &self.0
    }

    /// 在根下写一个文件（自动建父目录），返回绝对路径。
    fn put(&self, rel: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("建父目录");
        }
        std::fs::write(&path, bytes).expect("写样本文件");
        path
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ===================== v2 / v3 =====================

/// **v2 两帧**：一个 2×2（验证 BGRA→RGBA 换位）、一个 1×1；锚点逐帧不同。
#[test]
fn v2两帧_bgra转rgba与锚点() {
    let rgba = rgba_2x2();
    let bytes = build_legacy(
        2,
        &[
            frame(2, 2, 3, -5, gzip(&to_bgra(&rgba))),
            frame(1, 1, -7, 9, gzip(&to_bgra(&[10, 20, 30, 40]))),
        ],
    );
    let lib = CrystalLibrary::from_bytes(Path::new("v2_两帧.lib"), bytes).expect("v2 应能打开");

    assert_eq!(lib.version(), 2);
    assert!(!lib.is_sanxiu());
    assert_eq!(lib.count(), 2);
    assert_eq!(lib.read_header(0), Some((2, 2, 3, -5)));
    assert_eq!(lib.read_header(1), Some((1, 1, -7, 9)));
    assert_eq!(lib.read_header(2), None, "帧号越界必须 None");

    let f0 = lib.decode(0).expect("第 0 帧应能解码");
    assert_eq!((f0.width, f0.height), (2, 2));
    assert_eq!(f0.anchor, (3, -5));
    assert_eq!(f0.rgba, rgba, "像素必须是换位后的 RGBA（不是磁盘上的 BGRA）");
    assert_eq!(f0.rgba.len(), 16);
    assert_eq!(f0.pixel(0, 0), Some([255, 0, 0, 255]), "左上角应为红");
    assert_eq!(f0.pixel(2, 0), None, "x = 2 已越界（宽只有 2）");
    assert_eq!(f0.pixel(1, 1), Some([1, 2, 3, 4]), "右下角");

    let f1 = lib.decode(1).expect("第 1 帧应能解码");
    assert_eq!((f1.width, f1.height), (1, 1));
    assert_eq!(f1.anchor, (-7, 9));
    assert_eq!(f1.rgba, vec![10, 20, 30, 40]);

    assert!(lib.decode(2).is_none(), "帧号越界必须 None");
    assert!(
        matches!(lib.decode_checked(9), Ok(None)),
        "帧号越界属于「无效帧」，不是「数据损坏」"
    );
}

/// **v3**：多出来的 `i32` 保留字段必须被正确跳过，偏移表照样对得上。
#[test]
fn v3跳过保留字段且偏移正确() {
    let a = vec![
        9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
    ];
    let b: Vec<u8> = (0..16u8).map(|i| 100 + i).collect(); // 4×1 = 16 字节
    let bytes = build_legacy(
        3,
        &[
            frame(3, 2, 0, 0, gzip(&to_bgra(&a))),
            frame(4, 1, -1, 1, gzip(&to_bgra(&b))),
        ],
    );
    let lib = CrystalLibrary::from_bytes(Path::new("v3.lib"), bytes).expect("v3 应能打开");
    assert_eq!(lib.version(), 3);
    assert_eq!(lib.count(), 2);

    let f0 = lib.decode(0).expect("第 0 帧");
    assert_eq!((f0.width, f0.height), (3, 2));
    assert_eq!(f0.rgba, a, "v3 的保留字段被正确跳过后像素才对得上");
    let f1 = lib.decode(1).expect("第 1 帧");
    assert_eq!((f1.width, f1.height), (4, 1));
    assert_eq!(f1.anchor, (-1, 1));
    assert_eq!(f1.rgba, b);
}

// ===================== Sanxiu v5 / v6 =====================

/// **Sanxiu v5**：gzip BGRA + `shadow & 0x80` 的遮罩数据（第二张图的偏移必须跳过它）。
#[test]
fn sanxiu_v5_gzip与遮罩长度() {
    let rgba0 = rgba_2x2();
    let rgba1 = vec![7, 8, 9, 10, 11, 12, 13, 14]; // 1×2
    let img0 = sanxiu(2, 2, 5, -6, 0, gzip(&to_bgra(&rgba0)));
    let mut img1 = sanxiu(1, 2, -2, 3, 0, gzip(&to_bgra(&rgba1)));
    img1.mask = vec![0xAA, 0xBB, 0xCC]; // 遮罩数据区（长度计入 maskLength）
    let bytes = build_sanxiu(5, &[img0, img1]);

    let lib = CrystalLibrary::from_bytes(Path::new("v5.lib"), bytes).expect("v5 应能打开");
    assert_eq!(lib.version(), 5);
    assert!(lib.is_sanxiu());
    assert_eq!(lib.count(), 2);

    let f0 = lib.decode(0).expect("第 0 张");
    assert_eq!((f0.width, f0.height), (2, 2));
    assert_eq!(f0.anchor, (5, -6));
    assert_eq!(f0.rgba, rgba0);

    let f1 = lib.decode(1).expect("第 1 张（偏移必须跳过前一图的遮罩数据）");
    assert_eq!((f1.width, f1.height), (1, 2));
    assert_eq!(f1.anchor, (-2, 3));
    assert_eq!(f1.rgba, rgba1);
}

/// **Sanxiu v6**：`fileType 0` 走 gzip，`fileType 1` 走内嵌图片（这里是小 PNG）。
///
/// `fileType 1` 的宽高**取解码出来的图片本身**（`Assets.cs:157-158@901dbe2f`），
/// 所以这里故意让 header 写 9×9、PNG 实际 3×2：`read_header` 报 9×9（header 优先），
/// `decode` 报 3×2（图片优先），两者各自对齐上游。
#[test]
fn sanxiu_v6_文件类型0与1() {
    let rgba0 = rgba_2x2();
    let png_rgba = vec![
        1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, // 第一行
        10, 11, 12, 255, 13, 14, 15, 255, 16, 17, 18, 255, // 第二行
    ];
    let png = png_of(3, 2, &png_rgba);
    let bytes = build_sanxiu(
        6,
        &[
            sanxiu(2, 2, 0, 0, 0, gzip(&to_bgra(&rgba0))),
            sanxiu(9, 9, 4, -8, 1, png),
        ],
    );

    let lib = CrystalLibrary::from_bytes(Path::new("v6.lib"), bytes).expect("v6 应能打开");
    assert_eq!(lib.version(), 6);
    assert_eq!(lib.count(), 2);

    let f0 = lib.decode(0).expect("fileType 0");
    assert_eq!((f0.width, f0.height), (2, 2));
    assert_eq!(f0.rgba, rgba0);

    assert_eq!(
        lib.read_header(1),
        Some((9, 9, 4, -8)),
        "header 里写了 9×9，`ReadHeader` 就该报 9×9"
    );
    let f1 = lib.decode(1).expect("fileType 1");
    assert_eq!(
        (f1.width, f1.height),
        (3, 2),
        "`fileType 1` 的帧宽高取解码图本身，不看 header"
    );
    assert_eq!(f1.anchor, (4, -8), "锚点仍取 header 的 x/y");
    assert_eq!(f1.rgba, png_rgba, "PNG 像素应原样成为 RGBA8");
}

/// **Sanxiu v6 的 GIF 尺寸回退**（`Assets.cs:74-82@901dbe2f`）：
/// header 宽高为 0 时读 10 字节，若前 3 字节是 `GIF` 就用 `[6..8]`/`[8..10]` 当宽高。
#[test]
fn sanxiu_v6_gif尺寸回退() {
    let bytes = build_sanxiu(6, &[sanxiu(0, 0, 2, -3, 1, GIF_1X1.to_vec())]);
    let lib = CrystalLibrary::from_bytes(Path::new("v6_gif.lib"), bytes).expect("应能打开");
    assert_eq!(
        lib.read_header(0),
        Some((1, 1, 2, -3)),
        "GIF 回退应给出 1×1 与 header 的锚点"
    );
    let f = lib.decode(0).expect("1×1 GIF 应能解码");
    assert_eq!((f.width, f.height), (1, 1));
    assert_eq!(f.anchor, (2, -3));
    assert_eq!(f.rgba, vec![0, 0, 0, 0], "样本 GIF 的像素是透明黑");
}

// ===================== 坏样本（必须 None / 结构化错误，不许 panic） =====================

/// v2 的偏移表越界 → 打开就失败（上游 `Assets.cs:66` 抛异常）。
#[test]
fn 坏样本_偏移越界() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&2i32.to_le_bytes());
    bytes.extend_from_slice(&1i32.to_le_bytes());
    bytes.extend_from_slice(&0x7FFF_FFF0i32.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 32]);
    let err = CrystalLibrary::from_bytes(Path::new("坏偏移.lib"), bytes).unwrap_err();
    assert!(matches!(err, AssetError::LibHeaderInvalid { .. }), "{err:?}");
    assert!(err.to_string().contains("偏移"), "{err}");
}

/// 帧头 `length` 越过文件末尾 → `decode` 为 `None`，`decode_checked` 给原因。
#[test]
fn 坏样本_帧长度越界() {
    let payload = gzip(&to_bgra(&rgba_2x2()));
    let mut f = frame(2, 2, 0, 0, payload);
    f.length_override = Some(1_000_000);
    let lib = CrystalLibrary::from_bytes(Path::new("坏长度.lib"), build_legacy(2, &[f]))
        .expect("打开时只校验偏移，长度在取帧时校验");
    assert!(lib.decode(0).is_none());
    match lib.decode_checked(0) {
        Err(AssetError::FrameDecodeFailed { index, reason, .. }) => {
            assert_eq!(index, 0);
            assert!(reason.contains("长度"), "{reason}");
        }
        other => panic!("期望 FrameDecodeFailed，实际 {other:?}"),
    }
    assert!(lib.read_header(0).is_none(), "`ReadHeader` 也做同一道长度校验");
}

/// gzip 解压后**多一字节**也算损坏（上游 `gzip.ReadByte() != -1` 即抛，`Assets.cs:105`）。
#[test]
fn 坏样本_gzip尾随字节() {
    // 3×1 需要 12 字节，这里压 13 字节（多出的一字节必须被发现）
    let payload = gzip(&[0u8; 13]);
    let lib = CrystalLibrary::from_bytes(Path::new("尾随.lib"), build_legacy(2, &[frame(3, 1, 0, 0, payload)]))
        .expect("打开应成功");
    assert!(lib.decode(0).is_none(), "多一字节必须拒绝，不许静默截断");
    match lib.decode_checked(0) {
        Err(AssetError::FrameDecodeFailed { reason, .. }) => {
            assert!(reason.contains("13 字节"), "{reason}");
            assert!(reason.contains("12 字节"), "{reason}");
        }
        other => panic!("期望 FrameDecodeFailed，实际 {other:?}"),
    }
}

/// 尺寸：`w == 0` 是「空帧」（`Ok(None)`），`w == 8193` 是「损坏」（`Err`），
/// `w == 8192` 合法（`ReadHeader` 只拒 `> 8192`）。
#[test]
fn 坏样本_尺寸零与超限与边界() {
    let bytes = build_legacy(
        2,
        &[
            frame(0, 4, 1, 1, Vec::new()),
            frame(8193, 1, 1, 1, Vec::new()),
            frame(8192, 1, 2, 2, Vec::new()),
        ],
    );
    let lib = CrystalLibrary::from_bytes(Path::new("尺寸.lib"), bytes).expect("打开应成功");

    assert_eq!(lib.read_header(0), None);
    assert!(matches!(lib.decode_checked(0), Ok(None)), "w=0 是空帧，不是损坏");

    assert_eq!(lib.read_header(1), None);
    match lib.read_header_checked(1) {
        Err(AssetError::FrameDecodeFailed { reason, .. }) => {
            assert!(reason.contains("8193"), "{reason}");
            assert!(reason.contains("8192"), "{reason}");
        }
        other => panic!("期望 FrameDecodeFailed（8193 超限），实际 {other:?}"),
    }

    assert_eq!(
        lib.read_header(2),
        Some((8192, 1, 2, 2)),
        "8192 是合法上界（上游只拒 > 8192）"
    );
}

/// Sanxiu 头部：`headerLength` 越界 / 加密标志非 0 / 图片数越界 → 打开即失败。
#[test]
fn 坏样本_sanxiu头部() {
    // headerLength = -1
    let mut b = Vec::new();
    b.extend_from_slice(&5i32.to_le_bytes());
    b.push(0);
    b.extend_from_slice(&(-1i32).to_le_bytes());
    b.extend_from_slice(&[0u8; 16]);
    let err = CrystalLibrary::from_bytes(Path::new("h_len_负.lib"), b).unwrap_err();
    assert!(err.to_string().contains("headerLength"), "{err}");

    // headerLength 超过剩余字节
    let mut b = Vec::new();
    b.extend_from_slice(&5i32.to_le_bytes());
    b.push(0);
    b.extend_from_slice(&(1_000i32).to_le_bytes());
    b.extend_from_slice(&[0u8; 16]);
    let err = CrystalLibrary::from_bytes(Path::new("h_len_越界.lib"), b).unwrap_err();
    assert!(err.to_string().contains("headerLength"), "{err}");

    // 加密标志非 0
    let mut b = Vec::new();
    b.extend_from_slice(&6i32.to_le_bytes());
    b.push(1);
    b.extend_from_slice(&4i32.to_le_bytes());
    b.extend_from_slice(&0i32.to_le_bytes());
    let err = CrystalLibrary::from_bytes(Path::new("加密.lib"), b).unwrap_err();
    assert!(err.to_string().contains("未加密"), "{err}");

    // 图片数越界（header 只有 4 字节，装不下 1 张图）
    let mut b = Vec::new();
    b.extend_from_slice(&6i32.to_le_bytes());
    b.push(0);
    b.extend_from_slice(&4i32.to_le_bytes());
    b.extend_from_slice(&2_000_001i32.to_le_bytes());
    let err = CrystalLibrary::from_bytes(Path::new("图数.lib"), b).unwrap_err();
    assert!(err.to_string().contains("图片数"), "{err}");
}

/// Sanxiu 某图的 `length` 越过文件末尾 → 打开即失败（`Assets.cs:54`）。
#[test]
fn 坏样本_sanxiu长度越界() {
    let mut img = sanxiu(1, 1, 0, 0, 0, gzip(&to_bgra(&[1, 2, 3, 4])));
    img.length_override = Some(9_999);
    let err = CrystalLibrary::from_bytes(Path::new("v6_坏长度.lib"), build_sanxiu(6, &[img]))
        .unwrap_err();
    assert!(matches!(err, AssetError::LibHeaderInvalid { .. }), "{err:?}");
    assert!(err.to_string().contains("越界"), "{err}");
}

// ===================== 路径发现 =====================

/// **WemadeMir2**：三族基名 + `Objects2` 编号；`Tiles1` 属规则外，必须记进 `skipped`。
#[test]
fn 路径发现_wemade三族与规则外文件() {
    let root = TempRoot::new("paths_wemade");
    let tiles = root.put("WemadeMir2/Tiles.lib", &[0u8]);
    let smtiles = root.put("WemadeMir2/Smtiles.lib", &[0u8]);
    let objects = root.put("WemadeMir2/Objects.lib", &[0u8]);
    let objects2 = root.put("WemadeMir2/Objects2.lib", &[0u8]);
    let tiles1 = root.put("WemadeMir2/Tiles1.lib", &[0u8]);

    let paths = CrystalLibPaths::discover(root.path(), CrystalAssetFormat::WemadeMir2)
        .expect("扫描应成功");
    assert_eq!(paths.available(), 4);
    assert_eq!(paths.resolved_format(), Some(CrystalAssetFormat::WemadeMir2));
    assert_eq!(paths.indices(Family::Ground), vec![0]);
    assert_eq!(paths.indices(Family::Surface), vec![0, 1]);

    assert_eq!(paths.resolve(LibRef::ground(0)), Some(tiles.as_path()));
    assert_eq!(paths.resolve(LibRef::middle(0)), Some(smtiles.as_path()));
    assert_eq!(paths.resolve(LibRef::surface(0)), Some(objects.as_path()));
    assert_eq!(
        paths.resolve(LibRef::surface(1)),
        Some(objects2.as_path()),
        "Objects2 ⇒ surface(1)（N+1 规则）"
    );
    assert_eq!(paths.resolve(LibRef::surface(2)), None, "未登记就是 None，不回落");
    assert_eq!(paths.resolve(LibRef::middle(1)), None);

    assert_eq!(paths.find_stem("objects2"), Some(LibRef::surface(1)));
    assert_eq!(paths.find_stem("TILES"), Some(LibRef::ground(0)));

    let skipped: Vec<&Path> = paths.skipped().iter().map(|s| s.path.as_path()).collect();
    assert_eq!(skipped, vec![tiles1.as_path()], "Tiles1 必须被记录，不许静默丢");
    assert_eq!(paths.skipped()[0].reason, CrystalSkipReason::OutOfRule);
    assert!(paths.duplicates().is_empty());
}

/// **ShandaMir2**：三族都有编号，上限 10 / 10 / 31；`Auto` 在没有 WemadeMir2 时应落到它。
#[test]
fn 路径发现_shanda编号上限与探测顺序() {
    let root = TempRoot::new("paths_shanda");
    let tiles = root.put("ShandaMir2/Tiles.lib", &[0u8]);
    let tiles10 = root.put("ShandaMir2/Tiles10.lib", &[0u8]);
    let smtiles = root.put("ShandaMir2/SmTiles.lib", &[0u8]);
    let objects = root.put("ShandaMir2/Objects.lib", &[0u8]);
    let objects31 = root.put("ShandaMir2/Objects31.lib", &[0u8]);
    let tiles11 = root.put("ShandaMir2/Tiles11.lib", &[0u8]);
    let ani = root.put("ShandaMir2/AniTiles1.lib", &[0u8]);

    let paths =
        CrystalLibPaths::discover(root.path(), CrystalAssetFormat::Auto).expect("扫描应成功");
    assert_eq!(
        paths.resolved_format(),
        Some(CrystalAssetFormat::ShandaMir2),
        "没有 WemadeMir2 时 Auto 应落到 ShandaMir2"
    );
    assert_eq!(paths.resolve(LibRef::ground(0)), Some(tiles.as_path()));
    assert_eq!(
        paths.resolve(LibRef::ground(9)),
        Some(tiles10.as_path()),
        "Tiles10 ⇒ ground(9)"
    );
    assert_eq!(paths.resolve(LibRef::middle(0)), Some(smtiles.as_path()));
    assert_eq!(paths.resolve(LibRef::surface(0)), Some(objects.as_path()));
    assert_eq!(
        paths.resolve(LibRef::surface(30)),
        Some(objects31.as_path()),
        "Objects31 ⇒ surface(30)"
    );
    assert_eq!(paths.resolve(LibRef::surface(31)), None);

    let reasons: Vec<(PathBuf, CrystalSkipReason)> = paths
        .skipped()
        .iter()
        .map(|s| (s.path.clone(), s.reason))
        .collect();
    assert!(
        reasons.contains(&(tiles11.clone(), CrystalSkipReason::OutOfRange)),
        "Tiles11 超出上限 10：{reasons:?}"
    );
    assert!(
        reasons.contains(&(ani.clone(), CrystalSkipReason::NotInThreeFamilies)),
        "AniTiles1 是上游扁平 ID 190，不属于三族：{reasons:?}"
    );
}

/// **Flat**：库直接平铺在 `root/Map/` 下（`Assets.cs:360` 的 `Map` 目录优先规则）。
#[test]
fn 路径发现_平铺与map子目录() {
    let root = TempRoot::new("paths_flat");
    let tiles = root.put("Map/Tiles.lib", &[0u8]);
    let smtiles = root.put("Map/SmTiles.lib", &[0u8]);
    let objects = root.put("Map/Objects.lib", &[0u8]);
    let objects3 = root.put("Map/Objects3.lib", &[0u8]);
    // 根下再放一套同名文件：`root/Map` 存在时必须只看 `Map`，不许两套混着登记。
    root.put("Tiles.lib", &[0u8]);

    let paths = CrystalLibPaths::discover(root.path(), CrystalAssetFormat::Auto)
        .expect("扫描应成功");
    assert_eq!(paths.map_dir(), root.path().join("Map"));
    assert_eq!(paths.scan_dir(), root.path().join("Map"));
    assert_eq!(paths.resolved_format(), Some(CrystalAssetFormat::Flat));
    assert_eq!(paths.available(), 4);
    assert_eq!(paths.resolve(LibRef::ground(0)), Some(tiles.as_path()));
    assert_eq!(paths.resolve(LibRef::middle(0)), Some(smtiles.as_path()));
    assert_eq!(paths.resolve(LibRef::surface(0)), Some(objects.as_path()));
    assert_eq!(paths.resolve(LibRef::surface(2)), Some(objects3.as_path()));
}

/// 三种格式都没有库时必须返回**空表**（不是 panic、不是错误）。
#[test]
fn 路径发现_全空返回空表() {
    let root = TempRoot::new("paths_empty");
    root.put("WemadeMir2/Hum.lib", &[0u8]); // 有目录但没有三族库
    let paths = CrystalLibPaths::discover(root.path(), CrystalAssetFormat::Auto)
        .expect("空表也是 Ok");
    assert!(paths.is_empty());
    assert_eq!(paths.available(), 0);
    assert_eq!(paths.resolved_format(), None);
    assert_eq!(paths.format(), CrystalAssetFormat::Auto);
    assert_eq!(paths.resolve(LibRef::ground(0)), None);
    assert_eq!(
        paths.skipped()[0].reason,
        CrystalSkipReason::NotInThreeFamilies
    );
}

// ===================== 图源 =====================

/// 造一个含 `Tiles.lib`（v2 两帧）+ `Objects.lib`（v2 一帧）的 WemadeMir2 目录。
fn wemade_two_libs(root: &TempRoot) -> (PathBuf, PathBuf) {
    let tiles = root.put(
        "WemadeMir2/Tiles.lib",
        &build_legacy(
            2,
            &[
                frame(2, 2, 3, -5, gzip(&to_bgra(&rgba_2x2()))),
                frame(1, 1, -7, 9, gzip(&to_bgra(&[10, 20, 30, 40]))),
            ],
        ),
    );
    let objects = root.put(
        "WemadeMir2/Objects.lib",
        &build_legacy(2, &[frame(1, 1, 0, 0, gzip(&to_bgra(&[1, 2, 3, 4])))]),
    );
    (tiles, objects)
}

/// 图源的帧数 / 取帧 / 锚点快路径必须彼此一致，且锚点**不**被默认成 `(0, 0)`。
#[test]
fn source_锚点与取帧同值且帧数正确() {
    let root = TempRoot::new("source_anchor");
    wemade_two_libs(&root);
    let paths = CrystalLibPaths::discover(root.path(), CrystalAssetFormat::WemadeMir2)
        .expect("扫描应成功");
    let src = CrystalLibSource::open(&paths).expect("应能打开");

    assert_eq!(src.frame_count(LibRef::ground(0)), Some(2));
    assert_eq!(src.frame_count(LibRef::surface(0)), Some(1));
    assert_eq!(src.frame_count(LibRef::surface(7)), None, "未登记库必须是 None");

    for (lib, index) in [(LibRef::ground(0), 0u32), (LibRef::ground(0), 1)] {
        let f = src.frame(lib, index).expect("应能取到帧");
        assert_eq!(src.frame_of(lib, index), Some(f.clone()));
        assert_eq!(
            SpriteSource::anchor(&src, lib, index),
            Some(f.anchor),
            "{lib} 第 {index} 帧：锚点快路径必须与 frame().anchor 同值"
        );
        assert_eq!(src.anchor(lib, index), Some(f.anchor));
    }
    assert_ne!(
        SpriteSource::anchor(&src, LibRef::ground(0), 0),
        Some((0, 0)),
        "样本锚点是 (3,-5)，若快路径返回 (0,0) 说明它没真读帧头"
    );

    // 越界 / 未登记：都是 None
    assert!(src.frame(LibRef::ground(0), 2).is_none());
    assert!(src.anchor(LibRef::ground(0), 99).is_none());
    assert!(src.frame(LibRef::middle(0), 0).is_none());

    // 懒打开 + LRU
    src.clear_cache();
    assert_eq!(src.cached(), 0);
    assert!(src.frame(LibRef::ground(0), 0).is_some());
    assert_eq!(src.cached(), 1);
    assert!(src.failures().is_empty(), "正常路径不该留失败记录：{:?}", src.failures());
}

/// 打不开的库（版本号非法）：取帧一律 `None`，但**失败必须留痕**且可被取走。
#[test]
fn source_坏库失败被归集() {
    let root = TempRoot::new("source_failure");
    wemade_two_libs(&root);
    root.put("WemadeMir2/Objects2.lib", &[7, 0, 0, 0, 1, 0, 0, 0]);

    let paths = CrystalLibPaths::discover(root.path(), CrystalAssetFormat::WemadeMir2)
        .expect("扫描应成功");
    let src = CrystalLibSource::open(&paths).expect("登记表非空即可打开");
    let bad = LibRef::surface(1);
    assert!(src.frame(bad, 0).is_none(), "坏库不许返回假图");
    assert_eq!(src.frame_count(bad), None);
    assert!(src.anchor(bad, 0).is_none());

    let failures = src.failures();
    assert!(!failures.is_empty(), "打开失败必须留痕，不许静默");
    assert!(
        failures.iter().any(|f| f.contains("Objects2.lib")),
        "失败记录要点名文件：{failures:?}"
    );
    assert!(!src.drain_failures().is_empty());
    assert!(src.failures().is_empty(), "drain 应清空");
}
