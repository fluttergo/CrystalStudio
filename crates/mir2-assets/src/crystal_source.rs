//! [`CrystalLibSource`]：把 [`CrystalLibPaths`] 变成可按帧取像素的图源。
//!
//! # 一、职责
//!
//! 与 [`crate::WilSource`] 同构，只做三件事：**懒打开 + 小容量 LRU + 错误归集**。
//! 真正的二进制解析在 [`crate::CrystalLibrary`]（对齐 `Assets.cs:14-204@901dbe2f`），
//! 路径登记在 [`crate::CrystalLibPaths`]（对齐 `Assets.cs:342-405@901dbe2f`）。
//!
//! # 二、内存与缓存（path-backed：**不再整文件读入**）
//!
//! [`crate::CrystalLibrary::open`] 只解析**元数据区**：v2/v3 的文件头 + 偏移表
//! （`8 或 12 + 4×count` 字节），或 Sanxiu 的 `9 + headerLength` 字节头部；
//! 帧数据在取帧时按偏移 `seek` 现读、读完即丢（对齐上游 `File.OpenRead` +
//! 按帧 `Position`，`Assets.cs:85-106@901dbe2f`）。
//!
//! 所以这里的 LRU 缓存的是**库元数据**（偏移表 / 每图元数据），不是几百 MB 的库字节：
//!
//! - 真实素材 `Map/WemadeMir2/Tiles.Lib` 595 MB、`Objects.Lib` 128.9 MB ——
//!   整文件读入 + 4 槽 LRU 会吃掉 GB 级内存，按帧 seek 后常驻的只有几 MB 偏移表；
//! - 每次 [`SpriteSource::frame`] 的代价 = 一两次 `open`/`seek` + gzip 解压那一帧
//!   （打开系统调用相对 gzip 可忽略）；
//! - 「已打开的库」仍然是**懒**的：第一次取帧才解析元数据，解析失败进失败记录；
//! - 4 个槽位留 1 个余量给「刚切换过去的那张」。
//!
//! # 三、失败不静默
//!
//! [`SpriteSource::frame`] 的契约只允许 `Some` / `None`，所以「库打不开 / 帧头非法 /
//! gzip 损坏 / 内嵌图片解不开」这类**真错误**也传不上去。为了不埋掉它们，
//! 本模块把每次失败记进内部环形列表，由 [`CrystalLibSource::failures`] /
//! [`CrystalLibSource::drain_failures`] 取走 —— 调用方据此在状态栏显示「该库损坏」，
//! 而不是误判成「这一帧不存在」。
//!
//! # 四、锚点快路径
//!
//! [`SpriteSource::anchor`] 被覆盖为「只读帧头」（[`crate::CrystalLibrary::read_header`]），
//! 不解码像素：Sanxiu `fileType == 1` 的 GIF 尺寸回退也只读 10 字节。
//! 光罩（地图光照）每帧要问几百次，只需要 `(px, py)` 两个 `i16`。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::crystal_lib::CrystalLibrary;
use crate::crystal_paths::CrystalLibPaths;
use crate::error::AssetError;
use crate::frame::{Frame, SpriteSource};
use crate::lib_paths::LibRef;

/// 默认同时驻留的 `.lib` 库数。
///
/// 三族各一张 + 1 个余量（与 [`crate::DEFAULT_CACHE_CAPACITY`] 同值，但语义独立：
/// `.lib` 是单文件容器，但缓存的是**元数据**（偏移表 / 每图元数据），
/// 不是整库字节 —— 见模块文档 §二）。
pub const DEFAULT_LIB_CACHE_CAPACITY: usize = 4;

/// 失败记录最多保留多少条（超出丢最早的，避免长时间运行的编辑器无限涨）。
const MAX_FAILURES: usize = 64;

/// 缓存内部状态（受 `Mutex` 保护，见 [`CrystalLibSource::lock`] 的毒化处理）。
struct Inner {
    /// 定位键 →（已打开的库, 最近一次使用的 tick）。
    slots: HashMap<LibRef, (Arc<CrystalLibrary>, u64)>,
    /// 单调递增的 LRU 时钟。
    tick: u64,
    /// 最近的失败记录（环形，最多 [`MAX_FAILURES`] 条）。
    failures: Vec<String>,
}

/// 原版 `.lib` 图源：按 [`LibRef`]（族 + 族内索引）懒打开 `.lib` 并解码单帧。
///
/// ```no_run
/// use mir2_assets::{CrystalAssetFormat, CrystalLibPaths, CrystalLibSource, LibRef, SpriteSource};
/// use std::path::Path;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let paths = CrystalLibPaths::discover(Path::new(r"D:\Data"), CrystalAssetFormat::Auto)?;
/// let src = CrystalLibSource::open(&paths)?;
/// let tiles = LibRef::ground(0); // 地面 0 号 = Tiles.lib
/// if let Some(f) = src.frame(tiles, 0) {
///     println!("{}×{} {} 字节", f.width, f.height, f.rgba.len());
/// }
/// # Ok(())
/// # }
/// ```
pub struct CrystalLibSource {
    /// 登记表（只读，克隆一份自用）。
    paths: CrystalLibPaths,
    /// 同时驻留的库数上限。
    capacity: usize,
    /// 缓存与失败记录。
    inner: Mutex<Inner>,
}

impl CrystalLibSource {
    /// 用默认缓存容量打开。
    ///
    /// 登记表为空（`available() == 0`）时返回 [`AssetError::NoLibraries`]：
    /// 空表属于配置错误，不该等到第一次取帧时才发现。
    pub fn open(paths: &CrystalLibPaths) -> Result<Self, AssetError> {
        Self::with_cache_capacity(paths, DEFAULT_LIB_CACHE_CAPACITY)
    }

    /// 指定同时驻留的库数（内部按 `max(1)` 收敛，0 视为 1）。
    pub fn with_cache_capacity(
        paths: &CrystalLibPaths,
        capacity: usize,
    ) -> Result<Self, AssetError> {
        if paths.available() == 0 {
            return Err(AssetError::NoLibraries {
                root: paths.root().to_path_buf(),
            });
        }
        Ok(Self {
            paths: paths.clone(),
            capacity: capacity.max(1),
            inner: Mutex::new(Inner {
                slots: HashMap::new(),
                tick: 0,
                failures: Vec::new(),
            }),
        })
    }

    /// 底层登记表。
    pub fn paths(&self) -> &CrystalLibPaths {
        &self.paths
    }

    /// 同时驻留库数上限。
    pub fn cache_capacity(&self) -> usize {
        self.capacity
    }

    /// 当前驻留的库数。
    pub fn cached(&self) -> usize {
        self.lock().slots.len()
    }

    /// 清空缓存（丢掉已解析的库元数据；磁盘上的库文件不受影响）。
    pub fn clear_cache(&self) {
        self.lock().slots.clear();
    }

    /// 最近的失败记录（按发生顺序，最多 [`MAX_FAILURES`] 条）。
    pub fn failures(&self) -> Vec<String> {
        self.lock().failures.clone()
    }

    /// 取走失败记录并清空。
    pub fn drain_failures(&self) -> Vec<String> {
        std::mem::take(&mut self.lock().failures)
    }

    /// 取帧 —— 与 [`SpriteSource::frame`] 等价，省掉调用点引入 trait。
    pub fn frame_of(&self, lib: LibRef, index: u32) -> Option<Frame> {
        self.lookup_frame(lib, index)
    }

    /// 该库的帧总数；库未登记 / 打不开 → `None`。
    ///
    /// 帧数在打开时就从头部解析好了（[`CrystalLibrary::count`] = 偏移表长度），
    /// 所以这里只走「懒打开 + LRU」路径，不重新数、不解码任何一帧。
    /// 返回的是**索引表长度**（含空帧/无效帧），与「能出像素的帧数」不是一回事。
    pub fn frame_count(&self, lib: LibRef) -> Option<u32> {
        self.slot(lib).map(|slot| slot.count())
    }

    /// 帧查找的实际实现（三个 `None` 出口：库不可得 / 帧号越界 / 该帧无效）。
    fn lookup_frame(&self, lib: LibRef, index: u32) -> Option<Frame> {
        let slot = self.slot(lib)?;
        match slot.decode_checked(index) {
            Ok(frame) => frame,
            Err(e) => {
                self.note_failure(format!("{lib} 第 {index} 帧解码失败：{e}"));
                None
            }
        }
    }

    /// 只取锚点偏移：**只读帧头，不解码像素**。
    ///
    /// 与 [`CrystalLibSource::lookup_frame`] 走同一条「懒打开 + LRU」路径，
    /// 也做同样的两道 `None` 判定，差别只在跳过像素解码。
    fn anchor_of(&self, lib: LibRef, index: u32) -> Option<(i16, i16)> {
        let slot = self.slot(lib)?;
        match slot.read_header_checked(index) {
            Ok(header) => header.map(|(_, _, x, y)| (x, y)),
            Err(e) => {
                self.note_failure(format!("{lib} 第 {index} 帧读帧头失败：{e}"));
                None
            }
        }
    }

    /// 取（必要时打开）某个库。
    fn slot(&self, lib: LibRef) -> Option<Arc<CrystalLibrary>> {
        if let Some(slot) = self.touch(lib) {
            return Some(slot);
        }
        let path = self.paths.resolve(lib)?.to_path_buf();
        match CrystalLibrary::open(&path) {
            Ok(lib_file) => {
                let arc = Arc::new(lib_file);
                self.store(lib, Arc::clone(&arc));
                Some(arc)
            }
            Err(e) => {
                self.note_failure(format!("{lib} → {}：{e}", path.display()));
                None
            }
        }
    }

    /// 命中缓存时刷新 LRU 时间戳并返回。
    fn touch(&self, lib: LibRef) -> Option<Arc<CrystalLibrary>> {
        let mut inner = self.lock();
        inner.tick += 1;
        let tick = inner.tick;
        let entry = inner.slots.get_mut(&lib)?;
        entry.1 = tick;
        Some(Arc::clone(&entry.0))
    }

    /// 放入缓存并按需淘汰最久未用的库。
    fn store(&self, lib: LibRef, slot: Arc<CrystalLibrary>) {
        let mut inner = self.lock();
        inner.tick += 1;
        let tick = inner.tick;
        inner.slots.insert(lib, (slot, tick));
        while inner.slots.len() > self.capacity {
            let victim = inner
                .slots
                .iter()
                .min_by_key(|(_, (_, t))| *t)
                .map(|(k, _)| *k);
            match victim {
                Some(k) => {
                    inner.slots.remove(&k);
                }
                None => break,
            }
        }
    }

    /// 记录一条失败（环形，最多 [`MAX_FAILURES`] 条）。
    fn note_failure(&self, msg: String) {
        let mut inner = self.lock();
        if inner.failures.len() >= MAX_FAILURES {
            inner.failures.remove(0);
        }
        inner.failures.push(msg);
    }

    /// 加锁；互斥量若被毒化（持锁线程 panic 过）也继续用内部状态，**不 panic**。
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl SpriteSource for CrystalLibSource {
    fn frame(&self, lib: LibRef, index: u32) -> Option<Frame> {
        self.lookup_frame(lib, index)
    }

    /// 覆盖默认实现：走「只读帧头」的 [`CrystalLibSource::anchor_of`]，不解码像素。
    fn anchor(&self, lib: LibRef, index: u32) -> Option<(i16, i16)> {
        self.anchor_of(lib, index)
    }
}

/// 手写 `Debug`：库元数据可能很大，只报路径/格式/登记数/容量/已驻留。
impl std::fmt::Debug for CrystalLibSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CrystalLibSource")
            .field("root", &self.paths.root())
            .field("format", &self.paths.resolved_format())
            .field("available", &self.paths.available())
            .field("capacity", &self.capacity)
            .field("cached", &self.lock().slots.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crystal_paths::CrystalAssetFormat;

    /// 空登记表打开即报错（不等到第一次取帧才发现配置错）。
    #[test]
    fn 空登记表打开即报错() {
        let dir = std::env::temp_dir().join(format!("mir2_assets_lib_empty_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let paths = CrystalLibPaths::discover(&dir, CrystalAssetFormat::Auto).expect("空目录应返回空表");
        assert!(paths.is_empty());
        let err = CrystalLibSource::open(&paths).unwrap_err();
        assert!(matches!(err, AssetError::NoLibraries { .. }), "{err:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 缓存容量为零时收敛为一（与 [`crate::WilSource`] 同口径）。
    #[test]
    fn 缓存容量为零时收敛为一() {
        let dir =
            std::env::temp_dir().join(format!("mir2_assets_lib_cap_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let sub = dir.join("WemadeMir2");
        std::fs::create_dir_all(&sub).expect("建临时目录");
        std::fs::write(sub.join("Tiles.lib"), vec![0u8; 8]).expect("造空壳 .lib");

        let paths = CrystalLibPaths::discover(&dir, CrystalAssetFormat::WemadeMir2)
            .expect("扫描应成功");
        let src = CrystalLibSource::with_cache_capacity(&paths, 0).expect("应能打开");
        assert_eq!(src.cache_capacity(), 1);
        assert_eq!(src.cached(), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
