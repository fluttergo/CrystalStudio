//! [`WilSource`]：把 [`LibPaths`] 变成可按帧取像素的图源。
//!
//! # 一、解码复用，不重写
//!
//! 真正的二进制解析在 vendored 的 `vendor/wil2atlas`（`wil_decoder.rs` / `wzl_decoder.rs`）里，
//! 本模块只做三件事：**懒打开 + 小容量 LRU + 错误归集**。
//! 因此本 crate 与 `wil2atlas` 走的是**同一份解码代码、同一份调色板表**，
//! 这是「编辑器所见 = 客户端所见」的结构性保证（另一重保证见集成测试的逐像素比对）。
//!
//! # 二、调色板选择（必须与离线烘焙时一致）
//!
//! | 容器 | 内嵌调色板 | 本模块用的调色板 |
//! |------|-----------|-----------------|
//! | `.wzl`（WZL/WZX） | 无 | [`CLIENT_PALETTE`]（客户端硬编码表） |
//! | `.wil`（WIL/WIX） | 有 | 文件内嵌调色板 |
//!
//! 与上游 `wil2atlas` 主程序（本仓只 vendor 其解码器，未含 `main.rs`）的默认 `--palette file`
//! 同一条路径（上游 `main.rs:147-161`：有内嵌用内嵌，无内嵌回退 client 表）。
//! 本机实测：`GodotClient/assets` 下 `Tiles/Tiles5/Objects/Objects21/Hum/Items/npc`
//! 七个库的 meta 都是 `palette_mismatch: 0`，即两条路径逐项等价。
//!
//! # 三、内存与缓存
//!
//! 每个「已打开」的库都会把整个 `.wzl/.wil` 读进内存（`Objects21.wzl` 82 MB、
//! `Tiles.wzl` 53 MB），所以打开是**懒**的、且用 LRU 限制同时驻留的库数
//! （默认 [`DEFAULT_CACHE_CAPACITY`]）。
//!
//! # 四、失败不静默
//!
//! [`SpriteSource::frame`] 的契约只允许返回 `Some` / `None`，所以「打开失败 / 解码失败」
//! 这类**真错误**也不能往上抛。为了不把它们埋掉，本模块把每次失败记进内部环形列表，
//! 由 [`WilSource::failures`] / [`WilSource::drain_failures`] 取走 —— 调用方可据此
//! 在编辑器状态栏显示「该库损坏」而不是「该帧不存在」。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use wil2atlas::image_lib::ImageLib;
use wil2atlas::palette_client::CLIENT_PALETTE;
use wil2atlas::wil_decoder::{Palette, WilLib};
use wil2atlas::wzl_decoder::WzlLib;

use crate::error::AssetError;
use crate::frame::{Frame, SpriteSource};
use crate::lib_paths::{LibFile, LibPaths, LibRef};

/// 默认同时驻留的图库数。
///
/// 地图渲染最多同时用三张库（地面/中层/表层），留 1 个余量给「刚切换过去的那张」。
/// `Tiles.wzl` 53 MB + `Objects` 30 MB + `Objects21` 82 MB ≈ 165 MB 量级，可接受。
pub const DEFAULT_CACHE_CAPACITY: usize = 4;

/// 失败记录最多保留多少条（超出丢最早的，避免长时间运行的编辑器无限涨）。
const MAX_FAILURES: usize = 64;

/// 一个已打开库的解码器 + 生效调色板。
struct Slot {
    lib: Box<dyn ImageLib>,
    palette: Palette,
}

/// 缓存内部状态（受 `Mutex` 保护，见 [`WilSource::lock`] 的对毒化处理）。
struct Inner {
    slots: HashMap<LibRef, (Arc<Slot>, u64)>,
    tick: u64,
    failures: Vec<String>,
}

/// WIL/WZL 图源：按 [`LibRef`]（族 + 族内索引）懒打开库文件并解码单帧。
///
/// ```no_run
/// use mir2_assets::{LibPaths, LibRef, SpriteSource, WilSource};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // 客户端根由环境变量给出：值形如 `<client-root>`（客户端 data/ 的上一级）
/// let root = mir2_assets::client_root().expect("先设置 MIREDIT_CLIENT_ROOT");
/// let paths = LibPaths::discover(&root)?;
/// let src = WilSource::open(&paths)?;
/// let tiles = LibRef::ground(0); // 地面 0 号 = Tiles
/// if let Some(f) = src.frame(tiles, 0) {
///     println!("{}x{} {} 字节", f.width, f.height, f.rgba.len());
/// }
/// # Ok(())
/// # }
/// ```
pub struct WilSource {
    paths: LibPaths,
    capacity: usize,
    inner: Mutex<Inner>,
}

impl WilSource {
    /// 用默认缓存容量打开。
    ///
    /// 登记表为空（`available() == 0`）时返回 [`AssetError::NoLibraries`]：
    /// 空表属于配置错误，不该等到第一次取帧时才发现。
    pub fn open(paths: &LibPaths) -> Result<Self, AssetError> {
        Self::with_cache_capacity(paths, DEFAULT_CACHE_CAPACITY)
    }

    /// 指定同时驻留的库数（内部按 `max(1)` 收敛，0 视为 1）。
    pub fn with_cache_capacity(paths: &LibPaths, capacity: usize) -> Result<Self, AssetError> {
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
    pub fn paths(&self) -> &LibPaths {
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

    /// 清空缓存（释放已读入的库字节）。
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
    /// # 为什么不是「重新数一遍」
    /// 帧数在解码器打开时就已经解析好了（`ImageLib::frame_count` 返回的是索引表长度：
    /// WZL 是 `.wzx` 头 `0x2C` 处的 `count`，WIL 是 `.wix` 的偏移表长度），
    /// 所以这里**只走已有的库打开路径**（[`WilSource::slot`] 的懒打开 + LRU 缓存），
    /// 不重新读文件、不解码任何一帧。
    ///
    /// # 为什么可以拿它跟图集 meta 对账
    /// 客户端图集 `meta.total_frames` 与这里是**同一个数**（都是索引表长度，
    /// 含空帧/无效帧）。单元测试 `四库帧数与图集meta一致` 逐个比对四个库。
    ///
    /// # 例
    /// ```no_run
    /// use mir2_assets::{LibPaths, LibRef, WilSource};
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let root = mir2_assets::client_root().expect("先设置 MIREDIT_CLIENT_ROOT");
    /// let paths = LibPaths::discover(&root)?;
    /// let src = WilSource::open(&paths)?;
    /// println!("Tiles 共 {:?} 帧", src.frame_count(LibRef::ground(0)));
    /// # Ok(())
    /// # }
    /// ```
    pub fn frame_count(&self, lib: LibRef) -> Option<u32> {
        let slot = self.slot(lib)?;
        // 解码器已对 `count > 10_000_000` 做过上限校验，转 u32 只是为了对外用窄类型；
        // 极端情况下（理论上不可达）转换失败就如实返回 None，不截断成一个假数字。
        u32::try_from(slot.lib.frame_count()).ok()
    }

    /// 帧查找的实际实现（三个 `None` 出口：库不可得 / 帧号越界 / 空帧）。
    fn lookup_frame(&self, lib: LibRef, index: u32) -> Option<Frame> {
        let slot = self.slot(lib)?;
        let idx = index as usize;
        // 帧号越界 → frame_info 返回 None；空帧/尺寸越界 → valid == false。
        let info = slot.lib.frame_info(idx)?;
        if !info.valid {
            return None;
        }
        match slot.lib.decode_frame(idx, &slot.palette) {
            Ok(f) => Some(Frame {
                width: f.width,
                height: f.height,
                rgba: f.rgba,
                // 锚点就在 `DecodedFrame.info` 里（`wil_decoder.rs:78-85` 的 `info: FrameInfo`），
                // 零额外解码成本。WIL 与 WZL 两种容器都经 `ImageLib::decode_frame` 走这里
                // ⇒ 一处搬运即两种容器同时覆盖（WZL 的 px/py 在块头 8..12，
                // `wzl_decoder.rs:267-268`；WIL 在帧头 4..8，`wil_decoder.rs:208-209`）。
                anchor: (f.info.px, f.info.py),
            }),
            Err(e) => {
                self.note_failure(format!("{lib} 第 {index} 帧解码失败：{e:#}"));
                None
            }
        }
    }

    /// 只取锚点偏移：**只读帧头，不解码像素**。
    ///
    /// 与 [`WilSource::lookup_frame`] 走同一条「库懒打开 + LRU」路径，也做同样的两道
    /// `None` 判定（帧号越界 → `frame_info` 返回 `None`；空帧/尺寸越界 → `valid == false`），
    /// 差别只在**跳过 `decode_frame`**：帧头里已经有 `px/py`，没必要为了一对数解一张 RGBA。
    ///
    /// 光罩（地图光照）每帧渲染要问几百次，走这里与走 [`SpriteSource::frame`] 的
    /// **输出完全相同**（同一帧的同一字段），只是省掉像素解码。
    fn anchor_of(&self, lib: LibRef, index: u32) -> Option<(i16, i16)> {
        let slot = self.slot(lib)?;
        let info = slot.lib.frame_info(index as usize)?;
        if !info.valid {
            return None;
        }
        Some((info.px, info.py))
    }

    /// 取（必要时打开）某个库的解码器。
    fn slot(&self, lib: LibRef) -> Option<Arc<Slot>> {
        if let Some(slot) = self.touch(lib) {
            return Some(slot);
        }
        let file = self.paths.resolve_lib(lib)?.clone();
        match open_slot(&file) {
            Ok(slot) => {
                let arc = Arc::new(slot);
                self.store(lib, Arc::clone(&arc));
                Some(arc)
            }
            Err(e) => {
                self.note_failure(format!("{lib} → {}：{e}", file.data.display()));
                None
            }
        }
    }

    /// 命中缓存时刷新 LRU 时间戳并返回。
    fn touch(&self, lib: LibRef) -> Option<Arc<Slot>> {
        let mut inner = self.lock();
        inner.tick += 1;
        let tick = inner.tick;
        let entry = inner.slots.get_mut(&lib)?;
        entry.1 = tick;
        Some(Arc::clone(&entry.0))
    }

    /// 放入缓存并按需淘汰最久未用的库。
    fn store(&self, lib: LibRef, slot: Arc<Slot>) {
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

impl SpriteSource for WilSource {
    fn frame(&self, lib: LibRef, index: u32) -> Option<Frame> {
        self.lookup_frame(lib, index)
    }

    /// 覆盖默认实现：走「只读帧头」的 [`WilSource::anchor_of`]，不解码像素。
    ///
    /// 为什么值得覆盖：默认实现每次都要解一整帧 RGBA（`Objects21` 里 96×64 帧就是 24 KB），
    /// 而光罩定位一帧画面里要问几百次、且只需要 `(px, py)` 两个 i16。
    fn anchor(&self, lib: LibRef, index: u32) -> Option<(i16, i16)> {
        self.anchor_of(lib, index)
    }
}

/// 手写 `Debug`：解码器本身没有 `Debug`，这里只报「扫描根 / 登记数 / 容量 / 已驻留」。
impl std::fmt::Debug for WilSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WilSource")
            .field("root", &self.paths.root())
            .field("available", &self.paths.available())
            .field("capacity", &self.capacity)
            .field("cached", &self.lock().slots.len())
            .finish()
    }
}

/// 打开一个库：按数据文件扩展名分派到 WZL 或 WIL 解码器，并确定生效调色板。
fn open_slot(file: &LibFile) -> Result<Slot, AssetError> {
    let data = std::fs::read(&file.data).map_err(|e| AssetError::io(&file.data, e))?;
    let index = std::fs::read(&file.index).map_err(|e| AssetError::io(&file.index, e))?;
    let ext = file
        .data
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match ext.as_str() {
        "wzl" => {
            let lib = WzlLib::open(data, &index).map_err(|e| AssetError::OpenLibFailed {
                path: file.data.clone(),
                reason: format!("{e:#}"),
            })?;
            // WZL 容器没有内嵌调色板（wzl_decoder.rs:79-80 恒 None），必须回退客户端表。
            Ok(Slot {
                lib: Box::new(lib),
                palette: CLIENT_PALETTE,
            })
        }
        "wil" => {
            let lib = WilLib::open(data, &index).map_err(|e| AssetError::OpenLibFailed {
                path: file.data.clone(),
                reason: format!("{e:#}"),
            })?;
            // 有内嵌调色板就用它（与 wil2atlas 默认 --palette file 一致）。
            let palette = lib.file_palette.unwrap_or(CLIENT_PALETTE);
            Ok(Slot {
                lib: Box::new(lib),
                palette,
            })
        }
        other => Err(AssetError::OpenLibFailed {
            path: file.data.clone(),
            reason: format!("不支持的数据文件扩展名 `.{}`（只认 .wzl / .wil）", other),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 空登记表打开即报错() {
        let dir = std::env::temp_dir().join("mir2_assets_no_lib_probe");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let paths = LibPaths::discover(&dir).expect("空目录应返回空表");
        let err = WilSource::open(&paths).unwrap_err();
        assert!(matches!(err, AssetError::NoLibraries { .. }), "{err:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 缓存容量为零时收敛为一() {
        let dir = std::env::temp_dir().join("mir2_assets_cap_probe");
        let _ = std::fs::remove_dir_all(&dir);
        let sub = dir.join("data");
        std::fs::create_dir_all(&sub).expect("建临时目录");
        std::fs::File::create(sub.join("Tiles.wzl")).expect("造数据文件");
        std::fs::File::create(sub.join("Tiles.wzx")).expect("造索引文件");

        let paths = LibPaths::discover(&dir).expect("扫描应成功");
        let src = WilSource::with_cache_capacity(&paths, 0).expect("应能打开");
        assert_eq!(src.cache_capacity(), 1);
        assert_eq!(src.cached(), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 空壳库取帧返回none且失败被记录() {
        let dir = std::env::temp_dir().join("mir2_assets_stub_probe");
        let _ = std::fs::remove_dir_all(&dir);
        let sub = dir.join("data");
        std::fs::create_dir_all(&sub).expect("建临时目录");
        // 64 字节的 .wzl + 48 字节的 .wzx = 与客户端 objects41 同型的空存根
        std::fs::write(sub.join("Tiles.wzl"), vec![0u8; 64]).expect("造空壳数据文件");
        std::fs::write(sub.join("Tiles.wzx"), vec![0u8; 48]).expect("造空壳索引文件");

        let paths = LibPaths::discover(&dir).expect("扫描应成功");
        let src = WilSource::open(&paths).expect("应能打开");
        let tiles = LibRef::ground(0);
        assert!(src.frame(tiles, 0).is_none(), "空壳库必须返回 None");
        assert!(!src.failures().is_empty(), "打开失败必须留痕，不许静默");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 从图集 meta 里抠出 `total_frames` 的值。
    ///
    /// 不引 `serde_json`（本 crate 的运行期依赖里没有它，为一条测试加依赖不值当）：
    /// meta 是**上游图集烘焙工具 `wil2atlas` 写出来的定格式文件**，`"total_frames": <整数>`
    /// 出现在顶层，字面查找足够可靠；抠不到就返回 `None`，由调用方当"meta 不可用"处理。
    fn meta_total_frames(path: &std::path::Path) -> Option<u64> {
        let text = std::fs::read_to_string(path).ok()?;
        let at = text.find("\"total_frames\"")?;
        let rest = &text[at + "\"total_frames\"".len()..];
        let colon = rest.find(':')?;
        let digits: String = rest[colon + 1..]
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        digits.parse().ok()
    }

    /// **帧数查询（任务书 §三 唯一允许的扩展）**：四个库的 `frame_count` 必须
    /// ① `> 0`；② 与客户端图集 meta 的 `total_frames` 逐值相等。
    ///
    /// 为什么拿 meta 对账：`total_frames` 是上游 `wil2atlas` 用**同一个解码器**数出来的
    /// （索引表长度），两条独立路径得到同一个数，才说明 `frame_count` 真的复用了
    /// 打开时解析好的 count、没有另数一遍。
    /// meta 缺失（换机器/未烘焙）时**跳过并打印**，不让本机资源差异变成红灯。
    #[test]
    fn 四库帧数与图集meta一致() {
        let Some(root) = crate::client_root() else {
            eprintln!("[跳过] 未设置 {}（图库扫描根）", crate::CLIENT_ROOT_ENV);
            return;
        };
        let paths = match LibPaths::discover(&root) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[跳过] 本机没有图库根目录 {}：{e}", root.display());
                return;
            }
        };
        let src = match WilSource::open(&paths) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[跳过] 本机图库打不开：{e}");
                return;
            }
        };

        // (文件基名, 定位键)：N+1 规则 ⇒ Tiles5 = ground(4)、Objects21 = surface(20)
        let cases = [
            ("Tiles", LibRef::ground(0)),
            ("Tiles5", LibRef::ground(4)),
            ("Objects", LibRef::surface(0)),
            ("Objects21", LibRef::surface(20)),
        ];
        let atlas_root = crate::client_atlas_root();

        let mut checked = 0;
        for (stem, lib) in cases {
            let n = src.frame_count(lib);
            assert!(
                matches!(n, Some(v) if v > 0),
                "{stem}（{lib}）的帧数应为 Some(>0)，实际 {n:?}"
            );
            let n = n.expect("上面已断言非 None");
            assert_eq!(
                paths.find_stem(stem),
                Some(lib),
                "{stem} 的定位键反查不一致（族/索引写错了）"
            );

            let Some(atlas_root) = atlas_root.as_deref() else {
                eprintln!(
                    "[跳过] 未设置 {}（图集根），本库只验证 frame_count",
                    crate::CLIENT_ATLAS_ROOT_ENV
                );
                continue;
            };
            let meta = atlas_root.join(stem).join(format!("{stem}_atlas_meta.json"));
            match meta_total_frames(&meta) {
                Some(total) => {
                    assert_eq!(
                        u64::from(n),
                        total,
                        "{stem}：frame_count = {n} 与 meta total_frames = {total} 不符（{}）",
                        meta.display()
                    );
                    eprintln!("[帧数] {stem:<10} frame_count = {n:<6} == meta.total_frames");
                    checked += 1;
                }
                None => eprintln!("[跳过] 没有可用的图集 meta：{}", meta.display()),
            }
        }
        if checked == 0 {
            eprintln!("[跳过] 四个库的 meta 一个都没读到，只验证了 frame_count > 0");
        }
    }

    /// 未登记的库（族/索引不存在）帧数必须是 `None`，**不许**回落到别的族或给出 0。
    #[test]
    fn 未登记库帧数为none() {
        let Some(root) = crate::client_root() else {
            eprintln!("[跳过] 未设置 {}（图库扫描根）", crate::CLIENT_ROOT_ENV);
            return;
        };
        let paths = match LibPaths::discover(&root) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[跳过] 本机没有图库根目录 {}：{e}", root.display());
                return;
            }
        };
        let src = match WilSource::open(&paths) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[跳过] 本机图库打不开：{e}");
                return;
            }
        };

        // 地面族实测只有 0..=21；表层族没有 60 号。两者都必须落空。
        for lib in [LibRef::ground(u16::MAX), LibRef::surface(60)] {
            assert_eq!(
                src.frame_count(lib),
                None,
                "{lib} 未登记，frame_count 必须 None（0 会被读成「该库 0 帧」）"
            );
        }
        // 已登记的库照旧给数（对照，证明上面的 None 不是"永远 None"）
        if paths.resolve_lib(LibRef::ground(0)).is_some() {
            assert!(src.frame_count(LibRef::ground(0)).is_some());
        }
    }

    /// **锚点快路径不是第二条真相**：`anchor_of` 必须与「取帧再读 `frame.anchor`」逐帧同值。
    ///
    /// 两条实现走的是不同代码：`anchor_of` 只读帧头（`ImageLib::frame_info`），
    /// `lookup_frame` 完整解码（`decode_frame`）。若哪天有人在其中一条里把 `px/py` 搬错位置
    /// （WZL 块头前多 4 字节，最容易错位），这条会当场红。
    #[test]
    fn 锚点快路径与取帧结果同值() {
        let Some(root) = crate::client_root() else {
            eprintln!("[跳过] 未设置 {}（图库扫描根）", crate::CLIENT_ROOT_ENV);
            return;
        };
        let paths = match LibPaths::discover(&root) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[跳过] 本机没有图库根目录 {}：{e}", root.display());
                return;
            }
        };
        let src = match WilSource::open(&paths) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[跳过] 本机图库打不开：{e}");
                return;
            }
        };

        // (基名, 定位键, 抽样帧号)：覆盖 `.wzl` 的两个库 + 有内嵌调色板的 `.wil` 路径不保证本机有，
        // 故只用客户端 23 周年那套 `.wzl`（`Objects` 里锚点非零的帧很多，见下断言）。
        let cases = [
            ("Tiles", LibRef::ground(0), vec![0u32, 1, 2, 7, 31]),
            ("Objects", LibRef::surface(0), vec![0, 1, 608, 2723, 5180]),
            ("Objects21", LibRef::surface(20), vec![0, 3, 99, 500]),
        ];

        let mut 比对 = 0usize;
        let mut 非零 = 0usize;
        for (stem, lib, frames) in cases {
            if paths.resolve_lib(lib).is_none() {
                eprintln!("[跳过] 本机未登记 {lib}（{stem}）");
                continue;
            }
            for idx in frames {
                let Some(frame) = src.frame(lib, idx) else {
                    continue; // 该帧号本机为空帧/越界 —— 与锚点无关
                };
                let fast = src.anchor_of(lib, idx);
                assert_eq!(
                    fast,
                    Some(frame.anchor),
                    "{stem} 帧 {idx}：`anchor_of` = {fast:?} 与 `frame().anchor` = {:?} 不一致",
                    frame.anchor
                );
                // trait 的默认/覆写路径也必须同值
                assert_eq!(SpriteSource::anchor(&src, lib, idx), Some(frame.anchor));
                比对 += 1;
                if frame.anchor != (0, 0) {
                    非零 += 1;
                }
            }
        }
        if 比对 == 0 {
            eprintln!("[跳过] 本机没有可抽样的帧");
            return;
        }
        eprintln!("[锚点] 比对 {比对} 帧，其中非零锚点 {非零} 帧");
        // 客户端 `.wzl` 实测大量帧锚点非零（`Objects#2723 = (-51,-113)` 等）；
        // 若一个非零都没抽到，说明搬运把 px/py 丢成 0 了，这条用例就等于没验。
        assert!(非零 > 0, "抽样 {比对} 帧全为 (0,0) —— 锚点搬运可疑");
    }

    /// 取不到时必须是 `None`，**不许**用 `(0, 0)` 冒充「这一帧的锚点是零」。
    ///
    /// 三种 `None` 出口与 [`WilSource::lookup_frame`] 完全同口径：库不可得 / 帧号越界 / 空帧。
    #[test]
    fn 取不到锚点时返回none而不是零() {
        let dir = std::env::temp_dir().join("mir2_assets_anchor_none_probe");
        let _ = std::fs::remove_dir_all(&dir);
        let sub = dir.join("data");
        std::fs::create_dir_all(&sub).expect("建临时目录");
        // 空壳库（64 字节 .wzl + 48 字节 .wzx，与客户端 objects41 同型）⇒ 打不开 ⇒ 库不可得
        std::fs::write(sub.join("Tiles.wzl"), vec![0u8; 64]).expect("造空壳数据文件");
        std::fs::write(sub.join("Tiles.wzx"), vec![0u8; 48]).expect("造空壳索引文件");

        let paths = LibPaths::discover(&dir).expect("扫描应成功");
        let src = WilSource::open(&paths).expect("应能打开");
        let tiles = LibRef::ground(0);
        assert_eq!(src.anchor_of(tiles, 0), None, "空壳库必须 None");
        assert_eq!(src.anchor(tiles, 0), None, "trait 路径同样必须 None");
        assert_eq!(src.anchor_of(LibRef::middle(65_535), 0), None, "未登记库必须 None");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
