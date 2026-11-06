//! 原版 CrystalStudio `.lib` 图库读取器（对齐 `Assets.cs:14-204@901dbe2f` 的 `CrystalLibrary`）。
//!
//! # 一、为什么单独有一条 `.lib` 路径
//!
//! 本 crate 的另一条路径（[`crate::WilSource`]）走 `wil2atlas` 解码 `.wil/.wix`、`.wzl/.wzx`。
//! 官方 C# 编辑器 CrystalStudio 用的是**第三种容器** `.lib`：单文件、内嵌偏移表、
//! 每帧 gzip 压缩的 BGRA 位图。两者**帧头的锚点字段同构**（`w,h,x,y` 四个 `i16`，
//! 见 [`crate::frame`] 模块文档），但容器结构完全不同，所以这里自带一份读取器：
//! 公开仓用原版 `.lib` 就能自洽，不依赖任何私有解码器。
//!
//! # 二、两种磁盘布局（逐字段对齐上游，不猜）
//!
//! ## 2.1 v2 / v3（`Assets.cs:63-66@901dbe2f`）
//!
//! ```text
//! i32 version          // 2 或 3
//! i32 count            // 图片数
//! i32 reserved         // **仅 v3** 有，读掉不校验（Assets.cs:65）
//! i32 offsets[count]   // 每项必须是 0 ..< 文件长度（Assets.cs:66）
//! ```
//!
//! ## 2.2 Sanxiu v5 / v6（`Assets.cs:27-60@901dbe2f`）
//!
//! ```text
//! i32 version          // 5 或 6
//! u8  flag             // 必须 0；非 0 表示加密，上游直接拒绝（Assets.cs:29）
//! i32 headerLength     // 4 ..= min(文件剩余, 128 MiB)（Assets.cs:31）
//! u8  header[headerLength]
//!     i32 imageCount   // 0 ..= 2_000_000，且 imageCount * (v6?18:17) <= headerLength-4
//!     每张图 imageCount 次（v5 = 17 字节，v6 = 18 字节）：
//!         i16 width, i16 height, i16 x, i16 y
//!         i16 unused, i16 unused
//!         u8  shadow
//!         u8  fileType            // **仅 v6**；v5 视为 0
//!         i32 length
//!         if shadow & 0x80 != 0:
//!             i16 unused × 4
//!             i32 maskLength      // 遮罩数据长度，紧跟在该图 length 之后
//! ```
//!
//! 图片数据从 **header 之后**开始，按元数据顺序**首尾相接**（`Assets.cs:40,56-58`）：
//! 第 `i` 张的偏移 = 起点 + Σ(length + maskLength)。`length` 与 `maskLength` 都在
//! 打开时校验过「非负 + 不越过文件末尾」（`Assets.cs:54`），越界即整库打开失败。
//!
//! ## 2.3 帧头（v2/v3，`Assets.cs:86-87@901dbe2f`）
//!
//! ```text
//! i16 w, i16 h, i16 x, i16 y    // 尺寸与锚点
//! i16 unused, i16 unused
//! u8  unused
//! i32 length                    // 紧随其后的 gzip 流长度
//! ```
//!
//! 定长 17 字节，与 WIL 帧头**前 8 字节逐字段同构**（锚点位置相同）。
//!
//! # 三、像素口径：磁盘是 BGRA，对外是 RGBA
//!
//! 上游把解压后的字节直接塞给 `RasterFormats.Bgra32` 位图（`Assets.cs:105-106@901dbe2f`），
//! 每个像素在内存里是 `B,G,R,A`。本 crate 的 [`Frame::rgba`] 契约是**行优先 RGBA8**，
//! 因此这里必须逐像素换位（[`bgra_to_rgba`]）—— 少这一步颜色会整体错（蓝红互换），
//! 且是「看起来像、其实不对」的那种错。
//!
//! gzip 解压后的字节数**必须正好**等于 `w * h * 4`：上游 `ReadExactly(raw)` 之后还要
//! 再读一字节确认 EOF（`Assets.cs:105`），多一字节也算损坏。本实现用
//! [`gunzip_exact`] 一次做完这三件事（长度不足 / 超长 / gzip 损坏都拒绝）。
//!
//! # 四、Sanxiu `fileType`
//!
//! | `fileType` | 数据 | 宽高取值 | 出处 |
//! |---|---|---|---|
//! | `0` | gzip 压缩的 BGRA | header 的 `w/h` | `Assets.cs:160-163@901dbe2f` |
//! | `1` | 一整个图片文件（GIF/PNG…） | **解码出来那张图本身的宽高** | `Assets.cs:157-158@901dbe2f` |
//! | 其它 | —— | 上游抛 `InvalidDataException` | `Assets.cs:159@901dbe2f` |
//!
//! `fileType == 1` 有一处**只在 `ReadHeader` 里**的特例（`Assets.cs:74-82@901dbe2f`）：
//! header 的 `w/h` 非正时，去偏移处读 10 字节，若前 3 字节是 `GIF` 就把
//! 第 6..8、8..10 字节当作小端 `u16` 的宽高。它只读 10 字节、不做完整解码，
//! 所以本实现的 [`CrystalLibrary::read_header`] 照抄该回退，而
//! [`CrystalLibrary::decode`] 对 `fileType == 1` 一律以**解码器给的宽高**为准。
//!
//! # 五、护栏与错误口径（不 panic、不返回假图）
//!
//! | 情况 | 原版行为 | 本实现 |
//! |---|---|---|
//! | 帧号越界 | `return null`（`Assets.cs:70`） | `Ok(None)` |
//! | `w <= 0 \|\| h <= 0` | `return null`（`Assets.cs:88`） | `Ok(None)` |
//! | `w > 8192 \|\| h > 8192` | 抛 `InvalidDataException` | [`AssetError::FrameDecodeFailed`] |
//! | `length < 0` 或越过文件 | 抛 `InvalidDataException` | [`AssetError::FrameDecodeFailed`] |
//! | gzip 损坏 / 解压字节数不符 | 抛异常 | [`AssetError::FrameDecodeFailed`] |
//! | 文件头 / 版本 / 偏移表非法 | 抛 `InvalidDataException` | [`AssetError::LibHeaderInvalid`] |
//!
//! 对外两个层次：`decode` / `read_header` 把一切折成 `Option`（`SpriteSource` 的契约），
//! `decode_checked` / `read_header_checked` 保留「无效帧」与「真损坏」的区别，
//! 供 [`crate::CrystalLibSource`] 记失败、供上层诊断。
//!
//! 本模块**不使用 `unsafe`**，所有切片访问走 `get(..)`，所有整数换位走
//! `from_le_bytes`（上游 `BinaryReader` 是小端，与宿主字节序无关）。
//!
//! # 六、与原版的一处已知差异
//!
//! 上游 `CrystalLibrary` 有 `allowSanxiu` 开关（`Assets.cs:22,27,62`），两条入口各认一半：
//! 普通入口只认 v2/v3，Sanxiu 入口只认 v3/v5/v6（`Assets.cs:62` 会把 v2 拒掉）。
//! 本 crate 的路径表已按格式分好族，不需要再靠版本号分流，故 [`CrystalLibrary::open`]
//! 接受**并集** `v2/v3/v5/v6`，不做 `allowSanxiu` 开关。
//!
//! # 七、存储后端：path-backed，按帧 seek（**不整文件读入**）
//!
//! 上游 `CrystalLibrary` 用 `File.OpenRead` 打开、再用 `Position` 定位到帧头读那一帧
//! （`Assets.cs:85-106@901dbe2f`），内存里从没有过整库字节。本实现逐步同构：
//!
//! | 阶段 | 实际读什么 | 备注 |
//! |---|---|---|
//! | [`CrystalLibrary::open`] | 文件头 + 偏移表（v2/v3，最多 `8 或 12 + 4×count` 字节）／ Sanxiu header（v5/v6，`9 + headerLength` 字节，上限 128 MiB） | 数据区**一字节不读** |
//! | [`CrystalLibrary::read_header`] | 该帧 17 字节帧头（Sanxiu `fileType 1` 再加 10 字节 GIF 探针） | 现开 + `seek` + 读 |
//! | [`CrystalLibrary::decode`] | 该帧 `length` 字节的 gzip 流（或整段内嵌图片） | 现开 + `seek` + 读 |
//!
//! 为什么必须这样：原版真实素材里 `Map/WemadeMir2/Tiles.Lib` 是 595 MB、
//! `Objects.Lib` 128.9 MB，整文件读入叠上 [`crate::CrystalLibSource`] 的 4 槽 LRU
//! 会吃掉 GB 级内存。按帧 seek 之后常驻的只有偏移表（595 MB 那本约几 MB）
//! 与正在解的那一帧像素。
//!
//! **不持有打开的文件句柄**（每次读现开现关）：
//!
//! - `CrystalLibrary` 因此恒为 `Send + Sync`，跨线程共享 `Arc` 不用给游标加锁；
//! - 与上游「每次操作各自 `File.OpenRead`」同构，不会因为 LRU 淘汰漏关句柄；
//! - 代价是每次取帧多一次 `open` 系统调用 —— 相对 gzip 解压可忽略（实测见
//!   `tests/crystal_lib_real.rs` 的耗时打印）。
//!
//! [`CrystalLibrary::from_bytes`] 保留**内存后端**（合成样本 / 调用方已有字节时用），
//! 两条后端共用同一套解析与校验代码（`MetaAccess` + `CrystalLibrary::read_array` /
//! `CrystalLibrary::read_vec`），语义逐字节相同；模块内单测 `两种后端解码结果一致`
//! 与 `open_走path_backed只读元数据区` 钉住这两点。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::error::AssetError;
use crate::frame::Frame;

/// v2/v3 帧头定长部分：`w,h,x,y`(8) + 2×`i16`(4) + 1 字节 + `length`(4) = 17 字节。
const LEGACY_HEADER_LEN: usize = 17;
/// 图片数上限（与上游 `Assets.cs:36,64` 一致）。
const MAX_IMAGES: i64 = 2_000_000;
/// 边长上限（与上游 `Assets.cs:83,89` 一致；`8192` 本身合法，`8193` 拒绝）。
const MAX_DIMENSION: i32 = 8192;
/// Sanxiu 头部长度上限：128 MiB（`Assets.cs:31`）。
const MAX_SANXIU_HEADER: i64 = 128 * 1024 * 1024;
/// Sanxiu 头的前缀长度：`version(4) + flag(1) + headerLength(4)`（`Assets.cs:27-31`）。
const SANXIU_PREFIX_LEN: u64 = 9;
/// Sanxiu 元数据的 `shadow` 字节里「后面还跟着遮罩元数据」的标志位（`Assets.cs:49`）。
const MASK_FLAG: u8 = 0x80;
/// GIF 尺寸回退需要读的字节数（`Assets.cs:78`）。
const GIF_HEADER_PROBE: usize = 10;

/// v2/v3 的一帧头（未校验态；校验按上游顺序在各调用点做）。
struct LegacyFrame {
    /// 帧宽（可能 `<= 0`，由调用方判空帧）。
    width: i32,
    /// 帧高（可能 `<= 0`）。
    height: i32,
    /// 锚点 `px`。
    x: i16,
    /// 锚点 `py`。
    y: i16,
    /// 紧随帧头的数据长度（可能为负，由调用方判非法）。
    length: i32,
    /// 数据起始偏移（= 帧头偏移 + 17）。
    ///
    /// 用 `u64` 是为了支持 > 4 GB 的库（真实素材里 `Tiles.Lib` 已有 595 MB，
    /// 更大的客户端库不该在类型上就先炸）。
    data_start: u64,
}

/// Sanxiu v5/v6 的每图元数据（打开时已全部校验过）。
struct ModernImage {
    /// header 里的宽（可能 `<= 0`，`fileType == 1` 时允许）。
    width: i32,
    /// header 里的高（可能 `<= 0`）。
    height: i32,
    /// 锚点 `px`。
    x: i16,
    /// 锚点 `py`。
    y: i16,
    /// 数据长度（`>= 0`，已校验不越过文件）。
    length: u32,
    /// `0` = gzip BGRA；`1` = 内嵌图片文件；其它值解码时报错。
    file_type: u8,
}

/// 两种磁盘布局的公共形态：一张偏移表 +（Sanxiu 才有）每图元数据。
enum Layout {
    /// v2/v3：只有偏移表，数据长度在帧头里。
    Legacy {
        /// 每帧的帧头偏移（已校验 `0 ..< 文件长度`）。
        offsets: Vec<u32>,
    },
    /// Sanxiu v5/v6：偏移表 + 每图元数据（数据首尾相接算出来）。
    Sanxiu {
        /// 每张图的数据偏移（已校验不越过文件）。
        offsets: Vec<u32>,
        /// 每张图的元数据，与 `offsets` 等长。
        images: Vec<ModernImage>,
    },
}

/// 图库的存储后端：**打开时只读元数据区**，数据区按帧读（见模块文档 §七）。
enum Backing {
    /// 内存字节（[`CrystalLibrary::from_bytes`]：合成样本 / 调用方已有字节）。
    Bytes(Vec<u8>),
    /// 磁盘文件：只记长度，**不持句柄、不读数据区**。
    ///
    /// 每次取帧都现开现关（`File::open` + `seek` + 读该帧），与上游
    /// `File.OpenRead` + 按帧 `Position` 定位同构（`Assets.cs:85-106@901dbe2f`）。
    /// 好处：`CrystalLibrary` 恒为 `Send + Sync`（没有共享游标），也不会因为
    /// [`crate::CrystalLibSource`] 的 LRU 淘汰而泄漏文件句柄。
    File {
        /// `open` 时的文件长度（`metadata().len()`），用于「不越过文件末尾」的校验。
        len: u64,
    },
}

/// 打开期读元数据的来源。
enum MetaSource<'a> {
    /// 内存字节。
    Bytes(&'a [u8]),
    /// 磁盘文件句柄（**只在 [`CrystalLibrary::open`] 的栈上存活**，不存进结构体）。
    File {
        /// 已打开的文件。
        file: File,
        /// 出错信息用的路径。
        path: &'a Path,
    },
}

/// 打开期的元数据读取器：只需要「总长度」与「按区间读」两件事。
///
/// 有这个抽象，`parse` / `parse_legacy` / `parse_sanxiu` 就能**一套代码**同时服务
/// [`CrystalLibrary::from_bytes`]（内存）与 [`CrystalLibrary::open`]（按需读盘），
/// 不会出现「两条解析路径各写一遍又慢慢漂移」的经典毛病。
struct MetaAccess<'a> {
    /// 数据来源。
    source: MetaSource<'a>,
    /// 总长度（`Bytes` = 切片长度；`File` = 文件长度）。
    len: u64,
}

impl MetaAccess<'_> {
    /// 总长度。
    fn len(&self) -> u64 {
        self.len
    }

    /// 读 `[start, start + n)`；越过末尾 → `Ok(None)`，IO 失败 → `Err`。
    fn read_range(&mut self, start: u64, n: usize) -> Result<Option<Vec<u8>>, AssetError> {
        if !fits(self.len, start, n) {
            return Ok(None);
        }
        match &mut self.source {
            MetaSource::Bytes(bytes) => Ok(byte_range(bytes, start, n).map(<[u8]>::to_vec)),
            MetaSource::File { file, path } => {
                let mut buf = vec![0u8; n];
                file.seek(SeekFrom::Start(start))
                    .map_err(|e| AssetError::io(*path, e))?;
                match read_exact_or_eof(file, &mut buf) {
                    Ok(true) => Ok(Some(buf)),
                    Ok(false) => Ok(None),
                    Err(e) => Err(AssetError::io(*path, e)),
                }
            }
        }
    }

    /// 读一个小端 `i32`；不足 4 字节 → `Ok(None)`。
    fn read_i32(&mut self, start: u64) -> Result<Option<i32>, AssetError> {
        Ok(self
            .read_range(start, 4)?
            .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]])))
    }

    /// 读一个 `u8`；不足 1 字节 → `Ok(None)`。
    fn read_u8(&mut self, start: u64) -> Result<Option<u8>, AssetError> {
        Ok(self.read_range(start, 1)?.map(|b| b[0]))
    }
}

/// 原版 CrystalStudio `.lib` 图库（只读，**path-backed**）。
///
/// 打开时只解析文件头与偏移表/元数据，帧数据在 [`CrystalLibrary::read_header`] /
/// [`CrystalLibrary::decode`] 时按帧 `seek` 读取（模块文档 §七），所以 595 MB 的
/// `Tiles.Lib` 也不会被整文件读进内存。
///
/// ```no_run
/// use mir2_assets::CrystalLibrary;
/// use std::path::Path;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let lib = CrystalLibrary::open(Path::new(r"D:\Data\Map\WemadeMir2\Tiles.lib"))?;
/// println!("版本 {}，共 {} 帧", lib.version(), lib.count());
/// if let Some(h) = lib.read_header(0) {
///     println!("第 0 帧 {}×{}，锚点 ({}, {})", h.0, h.1, h.2, h.3);
/// }
/// # Ok(())
/// # }
/// ```
pub struct CrystalLibrary {
    /// 文件路径（错误信息、诊断、以及 `Backing::File` 每次读取时打开哪份文件）。
    path: PathBuf,
    /// 存储后端（内存字节 / 磁盘文件长度）。
    backing: Backing,
    /// 版本号（`2` / `3` / `5` / `6`）。
    version: u32,
    /// 图片数（= 偏移表长度，已校验 `<= 2_000_000`）。
    count: u32,
    /// 布局。
    layout: Layout,
}

impl CrystalLibrary {
    /// 打开一个 `.lib` 文件（v2/v3 或 Sanxiu v5/v6 任一）。
    ///
    /// **只读元数据区**：文件头 + 偏移表（v2/v3）或 Sanxiu header（v5/v6），
    /// 数据区一个字节都不读。帧数据在 [`CrystalLibrary::read_header`] /
    /// [`CrystalLibrary::decode`] 时按帧 `seek` 现读（模块文档 §七）。
    ///
    /// 错误：文件读不了 → [`AssetError::Io`]；版本/头部/偏移表非法 →
    /// [`AssetError::LibHeaderInvalid`]（含路径与具体原因）。
    pub fn open(path: impl AsRef<Path>) -> Result<Self, AssetError> {
        let path = path.as_ref();
        let file = File::open(path).map_err(|e| AssetError::io(path, e))?;
        let len = file.metadata().map_err(|e| AssetError::io(path, e))?.len();
        let (version, count, layout) = parse(
            path,
            &mut MetaAccess {
                source: MetaSource::File { file, path },
                len,
            },
        )?;
        Ok(Self {
            path: path.to_path_buf(),
            backing: Backing::File { len },
            version,
            count,
            layout,
        })
    }

    /// 从内存字节构造（与 [`CrystalLibrary::open`] 同一套解析与校验）。
    ///
    /// `path` 只用于错误信息；调用方负责保证它与字节同源。
    /// 存在这个入口是为了让坏样本测试不必落盘、也为了让上层能把
    /// 「从内存里的字节解码」接进来而不必先写文件。
    ///
    /// 与 [`CrystalLibrary::open`] 的唯一差别是内部后端：这里整份字节都在内存里，
    /// `open` 只留路径与长度。
    pub fn from_bytes(path: &Path, bytes: Vec<u8>) -> Result<Self, AssetError> {
        let len = bytes.len() as u64;
        let (version, count, layout) = parse(
            path,
            &mut MetaAccess {
                source: MetaSource::Bytes(&bytes),
                len,
            },
        )?;
        Ok(Self {
            path: path.to_path_buf(),
            backing: Backing::Bytes(bytes),
            version,
            count,
            layout,
        })
    }

    /// 文件路径。
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 版本号（`2` / `3` / `5` / `6`）。
    pub fn version(&self) -> u32 {
        self.version
    }

    /// 版本号是否属于 Sanxiu（v5/v6）。
    pub fn is_sanxiu(&self) -> bool {
        matches!(self.layout, Layout::Sanxiu { .. })
    }

    /// 图片数（= 上游 `Count => offsets.Length`，`Assets.cs:17@901dbe2f`）。
    ///
    /// 这是**索引表长度**，含空帧/无效帧；不等于「能解码出像素的帧数」。
    pub fn count(&self) -> u32 {
        self.count
    }

    /// 读某帧的尺寸与锚点：`(width, height, x, y)`；不可得返回 `None`。
    ///
    /// 语义与 [`CrystalLibrary::read_header_checked`] 相同，只是把「无效帧」与
    /// 「真损坏」都折成 `None`（上游 `ReadHeader` 的对外形态）。
    pub fn read_header(&self, index: u32) -> Option<(u32, u32, i16, i16)> {
        self.read_header_checked(index).ok().flatten()
    }

    /// [`CrystalLibrary::read_header`] 的结构化版本。
    ///
    /// - `Ok(None)`：该帧**本身无效**（帧号越界 / `w <= 0` / `h <= 0` / 超 8192 /
    ///   Sanxiu header 宽高非正且不是可回退的 GIF）—— 上游对这些返回 `null`；
    /// - `Err(_)`：**数据损坏**（帧头截断 / `length` 越界 / GIF 回退需要 10 字节却不够）。
    pub fn read_header_checked(
        &self,
        index: u32,
    ) -> Result<Option<(u32, u32, i16, i16)>, AssetError> {
        match &self.layout {
            Layout::Legacy { offsets } => {
                let Some(off) = offsets.get(index as usize).copied() else {
                    return Ok(None);
                };
                let Some(f) = self.legacy_frame(index, off)? else {
                    return Ok(None);
                };
                // 顺序与上游一致：先判空帧（return null），再判越界（throw）。
                if f.width <= 0 || f.height <= 0 {
                    return Ok(None);
                }
                if f.width > MAX_DIMENSION || f.height > MAX_DIMENSION {
                    return Err(frame_err(
                        &self.path,
                        index,
                        format!(
                            "帧尺寸 {}×{} 超过上限 {MAX_DIMENSION}（`Assets.cs:89@901dbe2f`）",
                            f.width, f.height
                        ),
                    ));
                }
                let remaining = self.len().saturating_sub(f.data_start);
                if f.length < 0 || i64::from(f.length) > remaining as i64 {
                    return Err(frame_err(
                        &self.path,
                        index,
                        format!(
                            "帧数据长度 {} 非法（帧头之后仅剩 {remaining} 字节，`Assets.cs:89@901dbe2f`）",
                            f.length
                        ),
                    ));
                }
                Ok(Some((f.width as u32, f.height as u32, f.x, f.y)))
            }
            Layout::Sanxiu { offsets, images } => {
                let Some(img) = images.get(index as usize) else {
                    return Ok(None);
                };
                let Some(&off) = offsets.get(index as usize) else {
                    return Ok(None);
                };
                let mut w = img.width;
                let mut h = img.height;
                // GIF 尺寸回退：只在 fileType == 1 且 header 宽高非正、length >= 10 时尝试。
                if img.file_type == 1 && (w <= 0 || h <= 0) && img.length as usize >= GIF_HEADER_PROBE
                {
                    if let Some((gw, gh)) = self.gif_dims(index, off)? {
                        w = gw;
                        h = gh;
                    }
                }
                if w > 0 && w <= MAX_DIMENSION && h > 0 && h <= MAX_DIMENSION {
                    Ok(Some((w as u32, h as u32, img.x, img.y)))
                } else {
                    Ok(None)
                }
            }
        }
    }

    /// 解码某帧为行优先 RGBA8；不可得返回 `None`。
    ///
    /// 语义与 [`CrystalLibrary::decode_checked`] 相同，把「无效帧」与「真损坏」都折成 `None`
    /// （[`crate::SpriteSource::frame`] 的契约）。
    pub fn decode(&self, index: u32) -> Option<Frame> {
        self.decode_checked(index).ok().flatten()
    }

    /// [`CrystalLibrary::decode`] 的结构化版本（`Ok(None)` = 无效帧，`Err` = 真损坏）。
    ///
    /// 三个 `Ok(None)` 出口与上游逐条对齐：
    ///
    /// 1. 帧号越界（`Assets.cs:70,95@901dbe2f`）；
    /// 2. v2/v3 的 `w <= 0 || h <= 0`（`Assets.cs:103`）；
    /// 3. Sanxiu 的 `ReadHeader(index) == null`（`Assets.cs:154`）或 `length == 0`
    ///    （`ReadModernBytes` 返回 null，`Assets.cs:145`）或数据越界（同处）。
    pub fn decode_checked(&self, index: u32) -> Result<Option<Frame>, AssetError> {
        match &self.layout {
            Layout::Legacy { offsets } => {
                let Some(off) = offsets.get(index as usize).copied() else {
                    return Ok(None);
                };
                let Some(f) = self.legacy_frame(index, off)? else {
                    return Ok(None);
                };
                if f.width <= 0 || f.height <= 0 {
                    return Ok(None);
                }
                if f.width > MAX_DIMENSION || f.height > MAX_DIMENSION {
                    return Err(frame_err(
                        &self.path,
                        index,
                        format!(
                            "帧尺寸 {}×{} 超过上限 {MAX_DIMENSION}（`Assets.cs:104@901dbe2f`）",
                            f.width, f.height
                        ),
                    ));
                }
                let remaining = self.len().saturating_sub(f.data_start);
                if f.length < 0 || i64::from(f.length) > remaining as i64 {
                    return Err(frame_err(
                        &self.path,
                        index,
                        format!(
                            "帧数据长度 {} 非法（帧头之后仅剩 {remaining} 字节，`Assets.cs:104@901dbe2f`）",
                            f.length
                        ),
                    ));
                }
                let Some(expected) = expected_len(f.width, f.height) else {
                    return Ok(None);
                };
                let Some(data) = self.read_vec(f.data_start, f.length as usize)? else {
                    // 上面已校验过区间，这里只是防御（不 panic、不越界）。
                    return Err(frame_err(&self.path, index, "帧数据区间越界"));
                };
                let raw = gunzip_exact(&self.path, index, &data, expected)?;
                Ok(Some(Frame {
                    width: f.width as u32,
                    height: f.height as u32,
                    rgba: bgra_to_rgba(&raw),
                    anchor: (f.x, f.y),
                }))
            }
            Layout::Sanxiu { offsets, images } => {
                let Some(img) = images.get(index as usize) else {
                    return Ok(None);
                };
                let Some(&off) = offsets.get(index as usize) else {
                    return Ok(None);
                };
                // 上游先 `ReadHeader(index) == null` 就返回 null（`Assets.cs:154`）。
                if self.read_header_checked(index)?.is_none() {
                    return Ok(None);
                }
                if img.length == 0 {
                    return Ok(None);
                }
                let Some(data) = self.read_vec(u64::from(off), img.length as usize)? else {
                    return Ok(None);
                };
                match img.file_type {
                    1 => decode_embedded_image(&self.path, index, &data, (img.x, img.y)),
                    0 => {
                        let Some(expected) = expected_len(img.width, img.height) else {
                            return Ok(None);
                        };
                        let raw = gunzip_exact(&self.path, index, &data, expected)?;
                        Ok(Some(Frame {
                            width: img.width as u32,
                            height: img.height as u32,
                            rgba: bgra_to_rgba(&raw),
                            anchor: (img.x, img.y),
                        }))
                    }
                    other => Err(frame_err(
                        &self.path,
                        index,
                        format!(
                            "不支持的 Sanxiu 图片类型 {other}（只认 0 = gzip BGRA / 1 = 图片文件，`Assets.cs:159@901dbe2f`）"
                        ),
                    )),
                }
            }
        }
    }

    /// 库的字节长度（`Bytes` = 切片长度；`File` = 打开时记下的文件长度）。
    fn len(&self) -> u64 {
        match &self.backing {
            Backing::Bytes(bytes) => bytes.len() as u64,
            Backing::File { len } => *len,
        }
    }

    /// 读 `[start, start + N)` 这 **17 字节帧头 / 10 字节 GIF 探针**；
    /// 越过文件末尾 → `Ok(None)`，IO 失败 → `Err`。
    ///
    /// `Bytes` 走内存切片，`File` 走「现开 + seek + 读满」（不持句柄，见模块文档 §七）。
    /// 两条后端语义完全一样：同一批坏样本测试对两者都成立。
    fn read_array<const N: usize>(&self, start: u64) -> Result<Option<[u8; N]>, AssetError> {
        let mut buf = [0u8; N];
        match &self.backing {
            Backing::Bytes(bytes) => match byte_range(bytes, start, N) {
                Some(src) => {
                    buf.copy_from_slice(src);
                    Ok(Some(buf))
                }
                None => Ok(None),
            },
            Backing::File { len } => {
                if !fits(*len, start, N) {
                    return Ok(None);
                }
                let mut file = File::open(&self.path).map_err(|e| AssetError::io(&self.path, e))?;
                file.seek(SeekFrom::Start(start))
                    .map_err(|e| AssetError::io(&self.path, e))?;
                match read_exact_or_eof(&mut file, &mut buf) {
                    Ok(true) => Ok(Some(buf)),
                    Ok(false) => Ok(None),
                    Err(e) => Err(AssetError::io(&self.path, e)),
                }
            }
        }
    }

    /// 读 `[start, start + n)` 这 **变长帧数据**（gzip 流或整段内嵌图片）；
    /// 越过文件末尾 → `Ok(None)`。
    ///
    /// 先用文件长度挡一次再分配缓冲：伪造的超大 `length` 骗不走内存，也不会
    /// 先 `vec![0; 4 GB]` 再去发现越界。
    fn read_vec(&self, start: u64, n: usize) -> Result<Option<Vec<u8>>, AssetError> {
        match &self.backing {
            Backing::Bytes(bytes) => Ok(byte_range(bytes, start, n).map(<[u8]>::to_vec)),
            Backing::File { len } => {
                if !fits(*len, start, n) {
                    return Ok(None);
                }
                let mut buf = vec![0u8; n];
                let mut file = File::open(&self.path).map_err(|e| AssetError::io(&self.path, e))?;
                file.seek(SeekFrom::Start(start))
                    .map_err(|e| AssetError::io(&self.path, e))?;
                match read_exact_or_eof(&mut file, &mut buf) {
                    Ok(true) => Ok(Some(buf)),
                    Ok(false) => Ok(None),
                    Err(e) => Err(AssetError::io(&self.path, e)),
                }
            }
        }
    }

    /// 读 v2/v3 的帧头；偏移越界/帧头截断 → `Err`，帧号越界 → `Ok(None)`。
    fn legacy_frame(&self, index: u32, offset: u32) -> Result<Option<LegacyFrame>, AssetError> {
        let start = u64::from(offset);
        let Some(b) = self.read_array::<LEGACY_HEADER_LEN>(start)? else {
            return Err(frame_err(
                &self.path,
                index,
                format!(
                    "帧头被截断：偏移 {start} 之后不足 {LEGACY_HEADER_LEN} 字节（文件 {} 字节）",
                    self.len()
                ),
            ));
        };
        Ok(Some(LegacyFrame {
            width: i32::from(i16::from_le_bytes([b[0], b[1]])),
            height: i32::from(i16::from_le_bytes([b[2], b[3]])),
            x: i16::from_le_bytes([b[4], b[5]]),
            y: i16::from_le_bytes([b[6], b[7]]),
            // b[8..12]：两个未使用的 i16；b[12]：1 个未使用字节。
            length: i32::from_le_bytes([b[13], b[14], b[15], b[16]]),
            data_start: start + LEGACY_HEADER_LEN as u64,
        }))
    }

    /// `fileType == 1` 的 GIF 尺寸回退（`Assets.cs:74-82@901dbe2f`）。
    ///
    /// 上游只读 10 字节：前 3 字节是 `GIF` 时，`[6..8]`、`[8..10]` 小端 `u16` 即宽高。
    /// 读不到 10 字节 → 上游抛异常，这里给结构化错误。
    fn gif_dims(
        &self,
        index: u32,
        offset: u32,
    ) -> Result<Option<(i32, i32)>, AssetError> {
        let start = u64::from(offset);
        let Some(b) = self.read_array::<GIF_HEADER_PROBE>(start)? else {
            return Err(frame_err(
                &self.path,
                index,
                format!(
                    "GIF 尺寸回退需要 {GIF_HEADER_PROBE} 字节，但偏移 {start} 之后不足（文件 {} 字节）",
                    self.len()
                ),
            ));
        };
        if &b[..3] != b"GIF" {
            return Ok(None);
        }
        Ok(Some((
            i32::from(u16::from_le_bytes([b[6], b[7]])),
            i32::from(u16::from_le_bytes([b[8], b[9]])),
        )))
    }
}

/// 手写 `Debug`：不打印整库字节，只报路径/版本/帧数/布局/后端与长度。
impl std::fmt::Debug for CrystalLibrary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (backend, len) = match &self.backing {
            Backing::Bytes(bytes) => ("bytes", bytes.len() as u64),
            Backing::File { len } => ("file", *len),
        };
        f.debug_struct("CrystalLibrary")
            .field("path", &self.path)
            .field("version", &self.version)
            .field("count", &self.count)
            .field("sanxiu", &self.is_sanxiu())
            .field("backend", &backend)
            .field("len", &len)
            .finish()
    }
}

/// 解析文件头，返回 `(版本号, 图片数, 布局)`。
///
/// 只碰元数据区：`meta` 决定这份元数据是从内存切片来（[`CrystalLibrary::from_bytes`]）
/// 还是现读磁盘（[`CrystalLibrary::open`]），解析逻辑只有这一份。
fn parse(path: &Path, meta: &mut MetaAccess<'_>) -> Result<(u32, u32, Layout), AssetError> {
    let version = meta
        .read_i32(0)?
        .ok_or_else(|| header_err(path, "文件不足 4 字节，读不到版本号"))?;
    match version {
        5 | 6 => {
            let (count, layout) = parse_sanxiu(path, meta, version)?;
            Ok((version as u32, count, layout))
        }
        2 | 3 => {
            let (count, layout) = parse_legacy(path, meta, version)?;
            Ok((version as u32, count, layout))
        }
        other => Err(header_err(
            path,
            format!("不支持的版本号 {other}（只认 v2/v3 与 Sanxiu v5/v6，`Assets.cs:27,62-64@901dbe2f`）"),
        )),
    }
}

/// 解析 Sanxiu v5/v6 头部（`Assets.cs:27-60@901dbe2f`）。
///
/// 只读 `9 + headerLength` 字节的头部（`headerLength` 上限 128 MiB，
/// 真实素材远小于此），图片数据区不碰。
fn parse_sanxiu(
    path: &Path,
    meta: &mut MetaAccess<'_>,
    version: i32,
) -> Result<(u32, Layout), AssetError> {
    let flag = meta
        .read_u8(4)?
        .ok_or_else(|| header_err(path, "Sanxiu 头缺少加密标志字节"))?;
    if flag != 0 {
        return Err(header_err(
            path,
            format!("只支持未加密的 Sanxiu 图库：加密标志字节为 {flag}（`Assets.cs:29@901dbe2f`）"),
        ));
    }
    let header_len = meta
        .read_i32(5)?
        .ok_or_else(|| header_err(path, "Sanxiu 头缺少 headerLength 字段"))?;
    let remaining = meta.len().saturating_sub(SANXIU_PREFIX_LEN);
    if header_len < 4
        || i64::from(header_len) > remaining as i64
        || i64::from(header_len) > MAX_SANXIU_HEADER
    {
        return Err(header_err(
            path,
            format!(
                "Sanxiu headerLength = {header_len} 非法（此处剩余 {remaining} 字节，合法区间 4 ..= min(剩余, 128 MiB)，`Assets.cs:31@901dbe2f`）"
            ),
        ));
    }
    let header_len = header_len as usize;
    let header = meta
        .read_range(SANXIU_PREFIX_LEN, header_len)?
        .ok_or_else(|| header_err(path, "Sanxiu 头部被截断"))?;

    let mut m = Cursor::new(&header);
    let image_count = m
        .read_i32()
        .ok_or_else(|| header_err(path, "Sanxiu 头部不足 4 字节，读不到图片数"))?;
    let per = if version == 6 { 18i64 } else { 17i64 };
    if image_count < 0
        || i64::from(image_count) > MAX_IMAGES
        || i64::from(image_count) * per > header_len as i64 - 4
    {
        return Err(header_err(
            path,
            format!(
                "Sanxiu 图片数 {image_count} 非法（头部 {header_len} 字节，每图至少 {per} 字节，`Assets.cs:36@901dbe2f`）"
            ),
        ));
    }
    let count = image_count as u32;
    let total = meta.len();
    let mut offsets: Vec<u32> = Vec::with_capacity(count as usize);
    let mut images: Vec<ModernImage> = Vec::with_capacity(count as usize);
    // 图片数据从 header 之后开始（`Assets.cs:40,56-58`）：前缀 9 字节 + headerLength。
    let mut offset = SANXIU_PREFIX_LEN as i64 + header_len as i64;
    for i in 0..count {
        let width = m.i16_or(|| meta_err(path, i))?;
        let height = m.i16_or(|| meta_err(path, i))?;
        let x = m.i16_or(|| meta_err(path, i))?;
        let y = m.i16_or(|| meta_err(path, i))?;
        m.take_or(2, || meta_err(path, i))?;
        m.take_or(2, || meta_err(path, i))?;
        let shadow = m.u8_or(|| meta_err(path, i))?;
        let file_type = if version == 6 {
            m.u8_or(|| meta_err(path, i))?
        } else {
            0
        };
        let length = m.i32_or(|| meta_err(path, i))?;
        let mut mask_len: i32 = 0;
        if shadow & MASK_FLAG != 0 {
            m.take_or(8, || meta_err(path, i))?;
            mask_len = m.i32_or(|| meta_err(path, i))?;
        }
        let span = i64::from(length) + i64::from(mask_len);
        if length < 0
            || mask_len < 0
            || offset > i64::from(i32::MAX)
            || offset + span > total as i64
        {
            return Err(header_err(
                path,
                format!(
                    "Sanxiu 第 {i} 张图的偏移/长度越界（offset = {offset}，length = {length}，maskLength = {mask_len}，文件 {total} 字节，`Assets.cs:54@901dbe2f`）"
                ),
            ));
        }
        offsets.push(offset as u32);
        images.push(ModernImage {
            width: i32::from(width),
            height: i32::from(height),
            x,
            y,
            length: length as u32,
            file_type,
        });
        offset += span;
    }
    Ok((count, Layout::Sanxiu { offsets, images }))
}

/// 解析 v2/v3 头部（`Assets.cs:63-66@901dbe2f`）。
///
/// 只读 `8 或 12 + 4×count` 字节的偏移表（`count ≤ 2_000_000` ⇒ 最多 8 MB），
/// 帧数据与帧头一个字节都不读。
fn parse_legacy(
    path: &Path,
    meta: &mut MetaAccess<'_>,
    version: i32,
) -> Result<(u32, Layout), AssetError> {
    let count = meta
        .read_i32(4)?
        .ok_or_else(|| header_err(path, "v2/v3 头缺少图片数字段"))?;
    let total = meta.len();
    let base = if version == 3 { 12i64 } else { 8i64 };
    if count < 0 || i64::from(count) > MAX_IMAGES || base + i64::from(count) * 4 > total as i64 {
        return Err(header_err(
            path,
            format!(
                "v{version} 图片数 {count} 非法（文件 {total} 字节，要求 {base} + 4×count ≤ 文件长度，`Assets.cs:64@901dbe2f`）"
            ),
        ));
    }
    if version == 3 {
        // 上游读掉一个未使用的 i32（`Assets.cs:65@901dbe2f`），不做任何校验。
        meta.read_i32(8)?
            .ok_or_else(|| header_err(path, "v3 头缺少保留字段（i32）"))?;
    }
    let count = count as u32;
    let table = meta
        .read_range(base as u64, count as usize * 4)?
        .ok_or_else(|| {
            header_err(
                path,
                format!("v{version} 偏移表被截断（要求 {base} + 4×{count} 字节）"),
            )
        })?;
    let (chunks, rest) = table.as_chunks::<4>();
    debug_assert!(rest.is_empty(), "偏移表长度必须是 4 的整数倍");
    let mut offsets: Vec<u32> = Vec::with_capacity(chunks.len());
    for (i, raw) in chunks.iter().enumerate() {
        let off = i32::from_le_bytes(*raw);
        if off < 0 || off as i64 >= total as i64 {
            return Err(header_err(
                path,
                format!(
                    "v{version} 第 {i} 项偏移 {off} 越界（文件 {total} 字节，要求 0 ..< 长度，`Assets.cs:66@901dbe2f`）"
                ),
            ));
        }
        offsets.push(off as u32);
    }
    Ok((count, Layout::Legacy { offsets }))
}

/// `[start, start + n)` 是否整段落在 `total` 之内（溢出/越界一律 `false`）。
fn fits(total: u64, start: u64, n: usize) -> bool {
    match start.checked_add(n as u64) {
        Some(end) => end <= total,
        None => false,
    }
}

/// `bytes[start .. start + n]`；越界（含 `start` 超过 `usize`）→ `None`。
///
/// 内存后端的所有区间读取都过这一道，切片访问永不越界（不 panic）。
fn byte_range(bytes: &[u8], start: u64, n: usize) -> Option<&[u8]> {
    let start = usize::try_from(start).ok()?;
    bytes.get(start..start.checked_add(n)?)
}

/// 尽量读满 `buf`；**填满前就 EOF** → `Ok(false)`（等价于「这段区间越过文件末尾」）。
///
/// 用循环而不是 `read_exact`：后者把「越界」也报成 `io::Error`，
/// 会与真正的 IO 故障（盘掉线、权限）混成同一个错误，分不清该不该重试。
fn read_exact_or_eof(file: &mut File, buf: &mut [u8]) -> std::io::Result<bool> {
    let mut filled = 0usize;
    while filled < buf.len() {
        match file.read(&mut buf[filled..]) {
            Ok(0) => return Ok(false),
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(true)
}

/// `w * h * 4`（`usize`，溢出即 `None`）。
fn expected_len(w: i32, h: i32) -> Option<usize> {
    if w <= 0 || h <= 0 {
        return None;
    }
    (w as usize).checked_mul(h as usize)?.checked_mul(4)
}

/// gzip 解压并要求**恰好** `expected` 字节（不足 / 超长 / 坏流都算损坏）。
///
/// 上游等价做法：`ReadExactly(raw)` + 再读一字节必须 EOF（`Assets.cs:105,162@901dbe2f`）。
/// 这里用 `take(expected + 1)` + `read_to_end`：既不预分配 `expected` 大小的缓冲
/// （避免被伪造的超大头骗走内存），也天然把「多一字节」变成可见事实。
fn gunzip_exact(
    path: &Path,
    index: u32,
    data: &[u8],
    expected: usize,
) -> Result<Vec<u8>, AssetError> {
    let mut out: Vec<u8> = Vec::new();
    let mut reader = flate2::read::GzDecoder::new(data).take(expected as u64 + 1);
    reader.read_to_end(&mut out).map_err(|e| {
        frame_err(
            path,
            index,
            format!("gzip 解压失败（预期 {expected} 字节）：{e}"),
        )
    })?;
    if out.len() != expected {
        return Err(frame_err(
            path,
            index,
            format!(
                "解压后 {} 字节，应为 w×h×4 = {expected} 字节（多一字节或少一字节都算损坏，`Assets.cs:105@901dbe2f`）",
                out.len()
            ),
        ));
    }
    Ok(out)
}

/// **BGRA → RGBA** 逐像素换位（行优先；上游位图格式是 `RasterFormats.Bgra32`）。
///
/// 调用方已保证 `bgra.len() == w*h*4`，故 `as_chunks::<4>()` 的余数恒为空。
fn bgra_to_rgba(bgra: &[u8]) -> Vec<u8> {
    let (pixels, rest) = bgra.as_chunks::<4>();
    debug_assert!(rest.is_empty(), "像素字节数必须是 4 的整数倍");
    let mut rgba = Vec::with_capacity(bgra.len());
    for px in pixels {
        rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
    }
    rgba
}

/// `fileType == 1`：把整段数据当图片文件解码为 RGBA8（宽高取解码结果，锚点取 header）。
fn decode_embedded_image(
    path: &Path,
    index: u32,
    data: &[u8],
    anchor: (i16, i16),
) -> Result<Option<Frame>, AssetError> {
    let image = image::load_from_memory(data).map_err(|e| {
        frame_err(
            path,
            index,
            format!("内嵌图片解码失败（只启用 png/gif 解码器）：{e}"),
        )
    })?;
    let rgba = image.to_rgba8();
    let (w, h) = rgba.dimensions();
    if w == 0 || h == 0 {
        return Err(frame_err(
            path,
            index,
            format!("内嵌图片尺寸 {w}×{h} 非法"),
        ));
    }
    Ok(Some(Frame {
        width: w,
        height: h,
        rgba: rgba.into_raw(),
        anchor,
    }))
}

/// 极简小端游标：所有读操作越界返回 `None`，**不 panic**。
struct Cursor<'a> {
    /// 底层字节。
    buf: &'a [u8],
    /// 当前位置。
    pos: usize,
}

impl<'a> Cursor<'a> {
    /// 从字节切片开头建立游标。
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// 前进 `n` 字节并返回该段；不足返回 `None`（游标不动）。
    fn take_or<F>(&mut self, n: usize, err: F) -> Result<&'a [u8], AssetError>
    where
        F: FnOnce() -> AssetError,
    {
        let Some(end) = self.pos.checked_add(n) else {
            return Err(err());
        };
        let Some(slice) = self.buf.get(self.pos..end) else {
            return Err(err());
        };
        self.pos = end;
        Ok(slice)
    }

    /// 同 [`Cursor::take_or`]，但不足时返回 `Option`（供解析文件头最外层用）。
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let slice = self.buf.get(self.pos..end)?;
        self.pos = end;
        Some(slice)
    }

    /// 读小端 `i32`。
    fn read_i32(&mut self) -> Option<i32> {
        self.take(4)
            .map(|s| i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }

    /// 读 `u8`，失败时用给定闭包造错。
    fn u8_or<F>(&mut self, err: F) -> Result<u8, AssetError>
    where
        F: FnOnce() -> AssetError,
    {
        self.take_or(1, err).map(|s| s[0])
    }

    /// 读小端 `i16`，失败时用给定闭包造错。
    fn i16_or<F>(&mut self, err: F) -> Result<i16, AssetError>
    where
        F: FnOnce() -> AssetError,
    {
        self.take_or(2, err)
            .map(|s| i16::from_le_bytes([s[0], s[1]]))
    }

    /// 读小端 `i32`，失败时用给定闭包造错。
    fn i32_or<F>(&mut self, err: F) -> Result<i32, AssetError>
    where
        F: FnOnce() -> AssetError,
    {
        self.take_or(4, err)
            .map(|s| i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
}

/// 文件头类错误。
fn header_err(path: &Path, reason: impl Into<String>) -> AssetError {
    AssetError::LibHeaderInvalid {
        path: path.to_path_buf(),
        reason: reason.into(),
    }
}

/// 单帧类错误。
fn frame_err(path: &Path, index: u32, reason: impl Into<String>) -> AssetError {
    AssetError::FrameDecodeFailed {
        path: path.to_path_buf(),
        index,
        reason: reason.into(),
    }
}

/// Sanxiu 元数据截断错误。
fn meta_err(path: &Path, index: u32) -> AssetError {
    header_err(path, format!("Sanxiu 元数据在第 {index} 张图处被截断"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 版本号字段本身就读不全时必须报错（而不是被当成 v0 之类的假版本）。
    #[test]
    fn 不足四字节报头部错误() {
        let err = CrystalLibrary::from_bytes(Path::new("x.lib"), vec![0, 0]).unwrap_err();
        assert!(matches!(err, AssetError::LibHeaderInvalid { .. }), "{err:?}");
        assert!(err.to_string().contains("不足 4 字节"), "{err}");
    }

    /// 未知版本号必须明确拒绝，并说清只认哪几个版本。
    #[test]
    fn 未知版本号被拒绝() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&7i32.to_le_bytes());
        bytes.extend_from_slice(&0i32.to_le_bytes());
        let err = CrystalLibrary::from_bytes(Path::new("x.lib"), bytes).unwrap_err();
        assert!(err.to_string().contains("不支持的版本号 7"), "{err}");
        assert!(err.to_string().contains("v5/v6"), "{err}");
    }

    /// BGRA → RGBA 必须逐像素换位且保留 alpha。
    #[test]
    fn bgra换位为rgba() {
        let bgra = [10u8, 20, 30, 255, 1, 2, 3, 128];
        assert_eq!(bgra_to_rgba(&bgra), vec![30, 20, 10, 255, 3, 2, 1, 128]);
    }

    /// 游标越界必须返回 `None` 且不移动位置。
    #[test]
    fn 游标越界返回none() {
        let mut c = Cursor::new(&[1, 0, 0]);
        assert_eq!(c.take(1), Some(&[1u8][..]));
        assert_eq!(c.read_i32(), None, "只剩 2 字节，读不了 i32");
        assert_eq!(
            c.take(2),
            Some(&[0u8, 0][..]),
            "失败的读取不应移动游标（否则这 2 字节就读不到了）"
        );
        assert_eq!(c.take(1), None);
    }

    /// **结构钉**：`open` 必须是 path-backed —— 595 MB 的库打开后，内存里不许躺着整库字节。
    ///
    /// 造法：`set_len` 把文件拉到 595,097,241 字节（= 真实 `Tiles.Lib` 的长度），
    /// 但只写文件头 + 偏移表 + 帧 0 的帧头，数据区是 NTFS/exFAT 的 VDL 空洞
    /// （读出来是 0，**不占磁盘、也不占内存**）。于是：
    ///
    /// - 断言 `Backing::File` ⇒ 一旦有人把 `open` 改回 `std::fs::read`，这条当场红，
    ///   与耗时无关（不靠墙钟，不做性能断言）；
    /// - 断言帧 0 的帧头能按偏移读出来 ⇒ 偏移表真的被用上了，不是「读了头部但偏移没用」。
    #[test]
    fn open_走path_backed只读元数据区() {
        use std::io::Write;

        // 与真实素材 `Map\WemadeMir2\Tiles.Lib` 同长。
        const BIG: u64 = 595_097_241;
        let dir = std::env::temp_dir().join(format!(
            "mir2_assets_lib_backing_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let path = dir.join("Big.lib");
        {
            let mut f = std::fs::File::create(&path).expect("建大库");
            // v2：version + count + offsets[2]（头 8 字节 + 偏移表 8 字节 ⇒ 数据区从 16 开始）。
            f.write_all(&2i32.to_le_bytes()).expect("version");
            f.write_all(&2i32.to_le_bytes()).expect("count");
            f.write_all(&16i32.to_le_bytes()).expect("offset0 = 16");
            let far = (BIG - LEGACY_HEADER_LEN as u64) as i32;
            f.write_all(&far.to_le_bytes()).expect("offset1 = 末尾前 17 字节");
            // 帧 0 的帧头：1×1、锚点 (7, -8)、length 0（数据区留空壳，本用例不 decode）。
            f.write_all(&1i16.to_le_bytes()).expect("w");
            f.write_all(&1i16.to_le_bytes()).expect("h");
            f.write_all(&7i16.to_le_bytes()).expect("x");
            f.write_all(&(-8i16).to_le_bytes()).expect("y");
            f.write_all(&[0u8; 5]).expect("未使用 5 字节");
            f.write_all(&0i32.to_le_bytes()).expect("length");
            f.set_len(BIG).expect("把文件拉到 595 MB（空洞，不落盘）");
        }

        let started = std::time::Instant::now();
        let lib = CrystalLibrary::open(&path).expect("大库必须能打开");
        let elapsed = started.elapsed();

        match &lib.backing {
            Backing::File { len } => assert_eq!(*len, BIG),
            Backing::Bytes(_) => panic!(
                "`open` 必须走 path-backed：整文件读入会为 595 MB 的库吃掉 595 MB 内存"
            ),
        }
        assert_eq!(lib.version(), 2);
        assert_eq!(lib.count(), 2);
        assert_eq!(lib.len(), BIG);
        assert_eq!(
            lib.read_header(0),
            Some((1, 1, 7, -8)),
            "帧 0 的帧头必须能按偏移 16 读出来"
        );
        assert_eq!(lib.read_header(1), None, "第二帧落在空洞里（w = h = 0）⇒ 空帧");
        assert_eq!(
            lib.decode(0),
            None,
            "length = 0 的解不出 w×h×4 字节 ⇒ 必须 None（不返回假图）"
        );
        println!("595 MB 稀疏库：open + 两次 read_header + 一次 decode 用时 {elapsed:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **等价钉**：同一份字节，`from_bytes`（内存后端）与 `open`（文件后端）结果必须逐项相同。
    ///
    /// 这条挡的是「重构时只改了一条后端」：两条后端共用 `parse` / `read_array` / `read_vec`，
    /// 任何分歧都会在这里显形（版本、帧数、帧头、像素、锚点、越界帧）。
    #[test]
    fn 两种后端解码结果一致() {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;

        // 2×1 的一帧：磁盘是 BGRA，解出来应是 RGBA。
        let bgra = [7u8, 8, 9, 10, 11, 12, 13, 14];
        let mut payload: Vec<u8> = Vec::new();
        {
            let mut enc = GzEncoder::new(&mut payload, Compression::default());
            enc.write_all(&bgra).expect("压 gzip");
            enc.finish().expect("收尾 gzip");
        }
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(&2i32.to_le_bytes()); // version
        bytes.extend_from_slice(&1i32.to_le_bytes()); // count
        bytes.extend_from_slice(&12i32.to_le_bytes()); // offsets[0]
        bytes.extend_from_slice(&2i16.to_le_bytes()); // w
        bytes.extend_from_slice(&1i16.to_le_bytes()); // h
        bytes.extend_from_slice(&3i16.to_le_bytes()); // x
        bytes.extend_from_slice(&(-5i16).to_le_bytes()); // y
        bytes.extend_from_slice(&[0u8; 5]); // 未使用
        bytes.extend_from_slice(&(payload.len() as i32).to_le_bytes());
        bytes.extend_from_slice(&payload);

        let dir = std::env::temp_dir().join(format!(
            "mir2_assets_lib_equiv_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let path = dir.join("Equiv.lib");
        std::fs::write(&path, &bytes).expect("写样本");

        let mem = CrystalLibrary::from_bytes(&path, bytes).expect("内存后端应能打开");
        let disk = CrystalLibrary::open(&path).expect("文件后端应能打开");

        assert!(matches!(mem.backing, Backing::Bytes(_)), "from_bytes 用内存后端");
        assert!(
            matches!(&disk.backing, Backing::File { .. }),
            "open 用文件后端"
        );
        assert_eq!(mem.version(), disk.version());
        assert_eq!(mem.is_sanxiu(), disk.is_sanxiu());
        assert_eq!(mem.count(), disk.count());
        assert_eq!(mem.len(), disk.len());
        assert_eq!(mem.read_header(0), disk.read_header(0));
        assert_eq!(mem.read_header(1), disk.read_header(1), "越界帧两边都 None");
        assert_eq!(
            mem.decode_checked(0).ok(),
            disk.decode_checked(0).ok(),
            "结构化出口两边必须一致"
        );
        assert_eq!(
            mem.decode(0),
            Some(Frame {
                width: 2,
                height: 1,
                rgba: vec![9, 8, 7, 10, 13, 12, 11, 14],
                anchor: (3, -5),
            }),
            "BGRA → RGBA 且锚点取帧头"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
