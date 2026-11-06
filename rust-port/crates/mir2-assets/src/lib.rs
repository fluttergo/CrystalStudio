//! `mir2-assets` —— 传奇 M2 图库解码层（地图编辑器渲染地基）。
//!
//! 本 crate 只做一件事：把「地图格里的图库字段 + 帧号」变成**行优先 RGBA8 像素**，
//! 且与客户端离线烘焙的图集**逐字节一致**。不做 GUI、不做 CLI、不做图集打包。
//!
//! 底层有**两条互不依赖的容器路径**，对外都是 [`SpriteSource`]：
//!
//! | 路径 | 容器 | 解码实现 | 入口 |
//! |---|---|---|---|
//! | WIL/WZL | `.wil/.wix`、`.wzl/.wzx` | vendored `wil2atlas`（只解码） | [`LibPaths`] + [`WilSource`] |
//! | 原版 `.lib` | v2/v3、Sanxiu v5/v6 | **本 crate 自带**（[`crystal_lib`]） | [`CrystalLibPaths`] + [`CrystalLibSource`] |
//!
//! 两条路径的锚点字段同源（`.lib` 帧头与 WIL 帧头**前 8 字节逐字段同构**），语义见
//! [`frame`] 模块文档；像素口径也一致（对外一律 RGBA8，[`Frame::rgba`]）。
//!
//! # 一、库号 → 文件规则（实测 + C++ 实证，非猜测）
//!
//! 客户端 `Classes/common/MShare.cpp:827-873` 逐个 `sprintf` 图库文件名：`i == 0` 用基名常量，
//! `i > 0` 用 `%d` 模板并传 `i + 1`（宏见 `Classes/common/Share.h:36-41`）。
//!
//! 定位键是 [`LibRef`]（**族 + 族内索引**），`index` 就是地图格字段的**原值**：
//!
//! | 族（[`Family`]） | 地图格字段 | `LibRef::index == 0` | `LibRef::index > 0` |
//! |---|---|---|---|
//! | [`Family::Ground`] 地面 | `btTiles` | `Tiles` | `Tiles{index+1}` |
//! | [`Family::Middle`] 中层 | `btsmTiles` | `SmTiles` | `SmTiles{index+1}` |
//! | [`Family::Surface`] 表层 | `btArea` | `Objects` | `Objects{index+1}` |
//!
//! 细节：
//!
//! - **只认基名，不认扩展名**。宏里写 `.wil`，本机 23 周年客户端目录里实际是 `.wzl`；
//!   两种容器都收（`wzl+wzx`、`wil+wix`），按数据文件扩展名分派解码器。
//! - 后缀数字只允许「无后缀」或 `{index+1}` 且 `index > 0`；`{Stem}1` 在 C++ 中**永不生成**，
//!   故按规则外处理并记录（[`LibPaths::skipped`]），不做猜测映射。
//! - **三族索引空间互不相通**：`LibRef::ground(0)`（`Tiles`）与 `LibRef::surface(0)`（`Objects`）
//!   是两个不同的库。这里刻意**不提供**「全局扁平号」—— 那种设计会让跨族误传的值
//!   静默命中另一族的某一页，比报错危险得多。
//! - 取文件用 [`LibPaths::resolve`] / [`LibPaths::resolve_lib`]，取像素用 [`SpriteSource::frame`]。
//!
//! # 二、与客户端图集的验收口径（「编辑器所见 = 客户端所见」的机器证据）
//!
//! 客户端不直接读 `.wzl`，读的是离线烘焙产物
//! `GodotClient/assets/<库>/<库>_atlas_NN.png` + `<库>_atlas_meta.json`
//! （`frames[i]` 含 `idx / w / h / anchor / atlas / region`）。所以本 crate 的正确性判据是：
//!
//! > 用本 crate 解码出的 `Frame.rgba`，与「图集 PNG 里该帧 `region` 裁出来的同尺寸像素」
//! > **逐字节相等**（不是「看起来差不多」，不做模糊比较）。
//!
//! 集成测试 `tests/acceptance.rs` 按此口径跑：`Tiles / Tiles5 / Objects / Objects21`
//! 四个库，每库确定性伪随机抽 50 个「`w > 0` 且 `atlas` 非空」的帧号，逐像素比对；
//! 不一致时打印「库名 / 帧号 / 首个不一致像素坐标 / 两边 RGBA」。
//!
//! 结构性保证还有一层：解码与调色板**完全复用** vendored 的 `vendor/wil2atlas`
//! （`wil_decoder.rs` / `wzl_decoder.rs` / `palette_client.rs`），本 crate 不复制一行解析代码，
//! 见 [`wil_source`] 模块文档。
//!
//! # 三、资源实测（2026-11-04，对开发机客户端根 `<client-root>` 全扫）
//!
//! 下表的 `<client-root>` 由 `MIREDIT_CLIENT_ROOT` 给出（本 crate **不内置任何路径**）：
//!
//! | 客户端目录 | 库文件 | 说明 |
//! |---|---|---|
//! | `<client-root>/data` | 628 对 `.wzl/.wzx` | 23 周年那套；三族齐全 |
//! | `<参考根>/Resources/data` | 21 对 `.wil/.wix`（另有 1 个 `StateItem2.wil` 无 `.wix`，不成对） | 旧版 WIL 容器 |
//! | `移动端UI文件` / `shanpao` | 无 | 不是图库集 |
//!
//! 23 周年那套里三族的实际库号：地面 `0..=21`（22 个）、中层 `0..=35`（36 个）、
//! 表层 `0` + `1..=50` + `511..=517` + `5107`（58 个，其中 `Objects41`、`objects512..518`、
//! `objects5108` 是 64 字节空存根 —— 能登记，但取帧一律 `None`）。
//!
//! # 四、错误与降级口径
//!
//! - 结构性失败（根不存在 / 一个库都没登记 / 单库打开失败）→ [`AssetError`]；
//! - 帧级不可得（库不存在 / 帧号越界 / 空帧）→ [`SpriteSource::frame`] 返回 `None`；
//! - **绝不返回全透明假图**，也**绝不 panic**（含 `u32::MAX` 这类输入）。
//! - 解码期真错误在 `frame()` 里只能折成 `None`（trait 契约如此），但会记进
//!   [`WilSource::failures`] / [`CrystalLibSource::failures`]，不静默吞掉。
//!
//! # 五、原版 `.lib` 路径（自带解码，不依赖 `wil2atlas`）
//!
//! 官方 C# 编辑器 CrystalStudio 读的是第三种容器 `.lib`（单文件 + 内嵌偏移表 +
//! 每帧 gzip 的 BGRA）。这条路径由本 crate 自己实现，逐字段对齐
//! `Assets.cs:14-204@901dbe2f`（读取器）与 `Assets.cs:342-405@901dbe2f`（三族路径表）：
//!
//! | 组件 | 职责 | 对应上游 |
//! |---|---|---|
//! | [`CrystalLibrary`] | 打开一个 `.lib`、读帧头、解一帧 | `CrystalLibrary`（`Assets.cs:14-204`） |
//! | [`CrystalLibPaths`] | 按格式（WemadeMir2 / ShandaMir2 / Flat）登记三族 | `AssetStore.Register`（`Assets.cs:355-376`） |
//! | [`CrystalLibSource`] | 懒打开 + LRU + 失败归集，实现 [`SpriteSource`] | `AssetStore` 的缓存层 |
//! | [`CrystalAssetFormat`] | 目录格式选择（含 `Auto` 探测） | 上游的扁平 ID 表分段 |
//!
//! 口径要点（细节见 [`crystal_lib`] / [`crystal_paths`] 模块文档）：
//!
//! - **path-backed，按帧 seek**：`CrystalLibrary::open` 只读文件头 + 偏移表
//!   （v2/v3）或 Sanxiu header（v5/v6），帧数据在取帧时按偏移现读现丢
//!   （对齐上游 `File.OpenRead` + 按帧 `Position`，`Assets.cs:85-106@901dbe2f`）——
//!   真实素材 `Tiles.Lib` 595 MB 也不会被整文件读进内存；
//! - **磁盘是 BGRA，对外是 RGBA**：`.lib` 的像素字节顺序是 `B,G,R,A`，必须逐像素换位；
//! - 尺寸护栏 `1 ..= 8192`、gzip 解压字节数必须**正好** `w*h*4`（多一字节也算损坏）；
//! - 无效/损坏一律不 panic、不返回假图：无效帧 → `None`，数据损坏 →
//!   [`AssetError::FrameDecodeFailed`]（`decode` 折成 `None`，`decode_checked` 保留原因）；
//! - **未覆盖**：Mir3 的 `WemadeMir3/ShandaMir3`、完整扁平 AssetStore ID 表
//!   （`AniTiles1 = 190`、Sanxiu 的 `1000/2000` 编号表）—— 见 [`crystal_paths`] 模块文档 §四。

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod crystal_lib;
mod crystal_paths;
mod crystal_source;
mod error;
mod frame;
mod lib_paths;
mod wil_source;

pub use crystal_lib::CrystalLibrary;
pub use crystal_paths::{
    CrystalAssetFormat, CrystalLibInfo, CrystalLibPaths, CrystalSkipReason, CrystalSkipped,
};
pub use crystal_source::{CrystalLibSource, DEFAULT_LIB_CACHE_CAPACITY};
pub use error::AssetError;
pub use frame::{Frame, SpriteSource};
pub use lib_paths::{Family, LibFile, LibInfo, LibPaths, LibRef, SkipReason, Skipped};
pub use wil_source::{WilSource, DEFAULT_CACHE_CAPACITY};

use std::path::PathBuf;

/// 客户端根目录的**环境变量名**：图库扫描根（客户端 `data/` 下 `*.wzl+.wzx` 或 `*.wil+.wix`）。
pub const CLIENT_ROOT_ENV: &str = "MIREDIT_CLIENT_ROOT";

/// 客户端**图集根**的环境变量名（只读比对用：`<库>/<库>_atlas_NN.png` + `<库>_atlas_meta.json`）。
///
/// 只有验收 / 测试需要它，正常运行不读图集。
pub const CLIENT_ATLAS_ROOT_ENV: &str = "MIREDIT_CLIENT_ATLAS_ROOT";

/// 另一套客户端库目录的环境变量名（旧版 `.wil/.wix` 容器，例如上游的 `reference_full/Resources`）。
pub const CLIENT_WIL_ROOT_ENV: &str = "MIREDIT_CLIENT_WIL_ROOT";

/// 历史常量，**值恒为空串**（=「没有内建缺省根」）。
///
/// 公开仓不内置任何构建机绝对路径。取根目录请用 [`client_root`]（读 `MIREDIT_CLIENT_ROOT`）；
/// 保留这个名字只为不打断既有调用方的编译 —— 判空请用 `is_empty()`，**不要**拿去拼路径
/// （`Path::new("").join("data")` 会变成相对 cwd 的 `data`，那是猜，不是缺省）。
pub const DEFAULT_CLIENT_ROOT: &str = "";

/// 见 [`DEFAULT_CLIENT_ROOT`]：恒为空串，真正的入口是 [`client_atlas_root`]。
pub const DEFAULT_CLIENT_ATLAS_ROOT: &str = "";

/// 见 [`DEFAULT_CLIENT_ROOT`]：恒为空串，真正的入口是 [`client_wil_root`]。
pub const DEFAULT_CLIENT_WIL_ROOT: &str = "";

/// 读一个「目录型」环境变量：**未设 / 空串 / 不是目录 ⇒ `None`**（不猜、不回落）。
fn env_dir(key: &str) -> Option<PathBuf> {
    let raw = std::env::var_os(key)?;
    if raw.is_empty() {
        return None;
    }
    let p = PathBuf::from(raw);
    p.is_dir().then_some(p)
}

/// 客户端根目录：读 `MIREDIT_CLIENT_ROOT`；**未设或不是目录 ⇒ `None`**。
///
/// 为什么是函数而不是常量：常量只能写死一条路径，而公开仓不该内置任何开发机路径。
/// 返回值已保证 `is_dir()`，可以直接交给 [`LibPaths::discover`]。
pub fn client_root() -> Option<PathBuf> {
    env_dir(CLIENT_ROOT_ENV)
}

/// 客户端图集根：读 `MIREDIT_CLIENT_ATLAS_ROOT`；**未设或不是目录 ⇒ `None`**。
///
/// 仅测试 / 验收（图集逐字节比对）需要；生产路径不读图集。
pub fn client_atlas_root() -> Option<PathBuf> {
    env_dir(CLIENT_ATLAS_ROOT_ENV)
}

/// 另一套客户端库目录（旧 `.wil/.wix`）：读 `MIREDIT_CLIENT_WIL_ROOT`；
/// **未设或不是目录 ⇒ `None`**。
pub fn client_wil_root() -> Option<PathBuf> {
    env_dir(CLIENT_WIL_ROOT_ENV)
}

/// 扫描 [`client_root`] 的便捷入口。
///
/// 环境变量未设置 ⇒ [`AssetError::RootUnset`]（**不回落**到任何内建路径）；
/// 其余失败语义与 [`LibPaths::discover`] 完全一致。
pub fn discover_default() -> Result<LibPaths, AssetError> {
    match client_root() {
        Some(root) => LibPaths::discover(&root),
        None => Err(AssetError::RootUnset {
            var: CLIENT_ROOT_ENV,
        }),
    }
}
