//! 原子保存：`tmp → write_all → sync_all → 读回逐字节核对 → rename → 尽力 fsync 父目录`。
//!
//! 抽取自 `miredit-gui/src/save.rs:1-330`。**顺序一字不改**，只把"打印到 stderr"换成
//! "把结果交给调用方"（本 crate 是库，不许往 stdout/stderr 写东西；见第四节）。
//!
//! # 一、为什么必须原子
//!
//! 直接 `File::create(path)` 会先把原文件截成 0 字节再写。中途断电 / 磁盘满 / 进程被杀
//! 就留下一个半截地图 —— 而地图是用户资产，半截文件与"删了"等价（`save.rs:3-5`）。
//! 本模块的顺序是：
//!
//! ```text
//! ① 写 <目标名>.miredit-tmp（与目标同目录 ⇒ 同一卷 ⇒ rename 是原子操作）
//! ② 把临时文件读回来，长度与内容逐字节核对
//! ③ rename(临时 → 目标)：同名覆盖由 MoveFileExW(MOVEFILE_REPLACE_EXISTING) 保证
//! ```
//!
//! **任何一步失败都不会动原文件**：①失败时目标还没被碰过；②失败时删掉临时文件；
//! ③失败时同样删掉临时文件，原文件仍在原处。失败原因一律**带路径与系统报错原文**返回。
//!
//! # 二、为什么还要"读回来核对"
//!
//! `std::fs::write` 返回 `Ok` 只代表系统调用成功，不代表内容真的落了盘（缓写、
//! 磁盘满时的短写都可能）。地图文件动辄 5.9 MB，这里多花一次读 + memcmp
//! （本地 SSD 上约 2~5 ms），换的是"存下去的一定是我算出来的那些字节"（`save.rs:18-21`）。
//!
//! # 三、落盘顺序（审计 E7：断电后"文件内容"与"目录项"是两件事）
//!
//! ```text
//! ① File::create(临时文件) → write_all → **sync_all()**   ← 数据先刷到盘
//! ② 读回来逐字节核对
//! ③ rename(临时 → 目标)
//! ④ **尝试** fsync 父目录（目录项持久化）—— Windows 需 FILE_FLAG_BACKUP_SEMANTICS
//! ```
//!
//! 没有 ①' 的 `sync_all()`，rename 之后断电可能留下"文件名在、内容是 0 或半截"的洞
//! （rename 只保证目录项原子，不保证数据已落盘）。
//!
//! ④ 的结果用 [`DirSync`] **如实分类**，并且**不影响保存成败**：rename 已经成功，
//! 目标路径上就是新内容；把"目录项没刷"报成"保存失败"会让用户以为文件没存上（假警报）。
//!
//! ⚠ **Windows 上大概率走 `FlushFailed`**：NTFS 不支持对目录句柄 `FlushFileBuffers`
//! （返回 `ERROR_INVALID_HANDLE`，`std` 侧常见形态是 `os error 5`）。这不是本模块的 bug，
//! 也不是我们假装成功 —— 它会**如实出现在** [`SavedFile::dir_sync`] 里，
//! 由调用方决定怎么显示（[`DirSync::note`] 给了一句能直接上屏的中文）。
//!
//! # 四、为什么不在这里 `eprintln!`
//!
//! GUI 版把每一步都打到 stderr（`save.rs:152-177` 的 `report_dir_sync`）。本 crate 是
//! **无 UI 的共享层**：网页版要把这句话回给浏览器、PC 版要写状态栏，两边去向不同 ⇒
//! 库只**返回**分类结果，由调用方报告。这样"报告"这件事不会因为没人接 stderr 而丢失。

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{CoreError, SaveStage};
use crate::session::EditSession;

/// 临时文件后缀（ASCII，属于文件系统约定而非界面文案）。来源：`save.rs:53-54`。
pub const TEMP_SUFFIX: &str = ".miredit-tmp";

/// Windows：打开**目录**句柄必须带 `FILE_FLAG_BACKUP_SEMANTICS`（0x0200_0000）。
/// 来源：`save.rs:56-61`。
///
/// 不带它时 `CreateFileW` 对目录一律返回"拒绝访问"（系统不允许把目录当普通文件打开），
/// 于是永远拿不到目录句柄、也就永远刷不了目录项。
#[cfg(windows)]
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;

/// 目标路径对应的临时文件路径（**同目录**，这样 `rename` 不跨卷、才是原子的）。
/// 来源：`save.rs:63-70`。
pub fn temp_path_for(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "map".to_string());
    dir_of(path).join(format!("{name}{TEMP_SUFFIX}"))
}

/// 目标文件所在的目录（空父路径回落到 `.`，与 [`temp_path_for`] 同口径）。来源：`save.rs:72-78`。
fn dir_of(path: &Path) -> PathBuf {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// 父目录 fsync 的结果（**如实分类，不假装成功也不假装失败**）。来源：`save.rs:80-113`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DirSync {
    /// 目录项已刷到盘。
    Ok(PathBuf),
    /// 打不开目录句柄（Windows 上通常是缺 `FILE_FLAG_BACKUP_SEMANTICS` 或权限不足）。
    OpenFailed {
        /// 被打开的目录。
        dir: PathBuf,
        /// 系统报错原文。
        err: String,
    },
    /// 打开了但刷不动（Windows/NTFS 对目录句柄的 `FlushFileBuffers` 通常直接失败）。
    FlushFailed {
        /// 被刷的目录。
        dir: PathBuf,
        /// 系统报错原文。
        err: String,
    },
}

impl DirSync {
    /// 目录项是否真的刷成功了。来源：`save.rs:102-105`。
    pub fn is_ok(&self) -> bool {
        matches!(self, DirSync::Ok(_))
    }

    /// 涉及的目录（三种结果都有）。来源：`save.rs:107-113`。
    pub fn dir(&self) -> &Path {
        match self {
            DirSync::Ok(d) | DirSync::OpenFailed { dir: d, .. } | DirSync::FlushFailed { dir: d, .. } => d,
        }
    }

    /// 一句能直接上屏的中文（成功也有一句，方便取证时确认走过这条路）。
    ///
    /// 文案与 `miredit-gui/src/ui_text.rs` 的 `DIAG_SAVE_DIRSYNC_OK` / `SAVE_ERR_DIRSYNC_OPEN` /
    /// `SAVE_ERR_DIRSYNC` 三个模板同义。**失败也只报告，不改保存结论**。
    pub fn note(&self) -> String {
        match self {
            DirSync::Ok(dir) => format!("[保存] 目录项已刷到盘：{}", dir.display()),
            DirSync::OpenFailed { dir, err } => format!(
                "[保存] 目录 fsync 不可用（打不开目录句柄）：{} ⇒ {err}；文件内容已落盘，目录项持久化未验证",
                dir.display()
            ),
            DirSync::FlushFailed { dir, err } => format!(
                "[保存] 目录 fsync 不可用（目录句柄刷不动，Windows/NTFS 已知限制）：{} ⇒ {err}；文件内容已落盘，目录项持久化未验证",
                dir.display()
            ),
        }
    }
}

/// 打开目录句柄（Windows 走 `FILE_FLAG_BACKUP_SEMANTICS`，其它平台就是普通打开）。
/// 来源：`save.rs:115-129`。
fn open_dir(dir: &Path) -> std::io::Result<File> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(dir)
    }
    #[cfg(not(windows))]
    {
        File::open(dir)
    }
}

/// **尝试**把 `path` 的父目录刷到盘（目录项持久化）；结果如实分类，从不 panic。
/// 来源：`save.rs:131-150`。
pub fn fsync_parent(path: &Path) -> DirSync {
    let dir = dir_of(path);
    let f = match open_dir(&dir) {
        Ok(f) => f,
        Err(e) => {
            return DirSync::OpenFailed {
                dir,
                err: e.to_string(),
            }
        }
    };
    match f.sync_all() {
        Ok(()) => DirSync::Ok(dir),
        Err(e) => DirSync::FlushFailed {
            dir,
            err: e.to_string(),
        },
    }
}

/// 一次成功保存的产物（临时文件路径 + 目录 fsync 分类）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedFile {
    /// 被 `rename` 掉的临时文件路径（诊断用，正常情况下已不存在）。
    pub tmp: PathBuf,
    /// 父目录 fsync 的如实分类（**失败不影响"保存成功"这件事**，见模块文档第三节）。
    pub dir_sync: DirSync,
}

/// 原子写回；成功返回 [`SavedFile`]。
///
/// 失败时**原文件保持原样**，错误已带齐「阶段 + 目标路径 + 临时路径 + 系统报错原文」。
/// 落盘顺序见模块文档第三节：写 → `sync_all` → 读回核对 → rename → **尝试** fsync 父目录。
/// 最后一步的结果放在 [`SavedFile::dir_sync`] 里，**不影响本函数的返回值**。
/// 来源：`save.rs:179-260`。
pub fn save_atomic(path: &Path, bytes: &[u8]) -> Result<SavedFile, CoreError> {
    let tmp = temp_path_for(path);
    let 失败 = |stage: SaveStage, detail: String| CoreError::Save {
        stage,
        path: path.to_path_buf(),
        tmp: tmp.clone(),
        detail,
    };

    // ① 写临时文件（同目录）并**刷盘**：没有 sync_all 的话，rename 成功后断电可能
    //    留下"文件名在、内容没落盘"的洞（rename 只保证目录项原子，不保证数据落盘）。
    let mut f = match File::create(&tmp) {
        Ok(f) => f,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(失败(SaveStage::Temp, e.to_string()));
        }
    };
    if let Err(e) = f.write_all(bytes) {
        drop(f);
        let _ = std::fs::remove_file(&tmp);
        return Err(失败(SaveStage::Temp, e.to_string()));
    }
    if let Err(e) = f.sync_all() {
        drop(f);
        let _ = std::fs::remove_file(&tmp);
        // 刷不动就**不进** rename 阶段：宁可原文件一个字节不动，也不留下"看似成功"的半截
        return Err(失败(SaveStage::Sync, e.to_string()));
    }
    drop(f);

    // ② 读回来逐字节核对（防短写/缓写）
    match std::fs::read(&tmp) {
        Ok(back) => {
            if back.len() != bytes.len() || back != bytes {
                let _ = std::fs::remove_file(&tmp);
                return Err(失败(
                    SaveStage::Verify,
                    format!("写入不完整：期望 {} 字节，读回 {} 字节", bytes.len(), back.len()),
                ));
            }
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(失败(SaveStage::Temp, e.to_string()));
        }
    }

    // ③ 原子替换
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(失败(SaveStage::Rename, e.to_string()));
    }

    // ④ 目录项持久化：**尽力而为 + 如实报告**（失败不改变"保存成功"这件事）
    Ok(SavedFile {
        tmp,
        dir_sync: fsync_parent(path),
    })
}

/// 一次「保存会话」的结果（GUI 的 `Ctrl+S` 与网页版的 `Command::Save` 共用同一套判定）。
/// 来源：`save.rs:262-287`。
///
/// ⚠ 与 GUI 的差别（有意）：GUI 的 `SaveOutcome` 有 `NoMap` 与 `Failed(String)` 两个变体，
/// 本 crate **都没有**——
/// - `NoMap` 不可达：会话必持有文档（见 [`EditSession`]）；
/// - `Failed` 改成 `Err(CoreError)`：失败要带**阶段**（[`SaveStage`]），塞进一个字符串
///   就把"停在哪一步"丢了（而阶段正是定位问题的第一手信息）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveOutcome {
    /// 没有未保存的修改 ⇒ **一个字节都没写**。
    Clean,
    /// 写成功：目标路径 + 临时文件 + 字节数 + 目录 fsync 分类。
    Written {
        /// 写回的目标路径。
        path: PathBuf,
        /// 被 `rename` 掉的临时文件路径（诊断用，正常情况下已不存在）。
        tmp: PathBuf,
        /// 写了多少字节。
        bytes: usize,
        /// 父目录 fsync 的如实分类（失败不改结论）。
        dir_sync: DirSync,
    },
}

impl SaveOutcome {
    /// 是否真的写了盘。来源：`save.rs:282-287`。
    pub fn wrote(&self) -> bool {
        matches!(self, SaveOutcome::Written { .. })
    }

    /// 一句能上屏的中文（成功 / 跳过都有话说，绝不静默）。来源：`save.rs:292-330`。
    pub fn note(&self) -> String {
        match self {
            SaveOutcome::Clean => "没有未保存的修改，未写盘".to_string(),
            SaveOutcome::Written {
                path,
                bytes,
                dir_sync,
                ..
            } => format!(
                "[保存] 已写入 {}（{bytes} 字节）· {}",
                path.display(),
                dir_sync.note()
            ),
        }
    }
}

/// 把内存里的地图写回它自己的路径（**只在有修改时真写**），并把结果记进编辑会话。
/// 来源：`save.rs:289-330`。
///
/// 脏标记与保存点是两件事：写盘成功才 `mark_saved()`（把保存点挪到当前位置）——
/// 写失败时保存点**不动**，于是"还是脏的"这个事实得以保留（`save.rs:324-328`）；
/// 同时把原因记进 `last_save`，调用方随时能取出来显示。
pub fn save_session(session: &mut EditSession) -> Result<SaveOutcome, CoreError> {
    if !session.dirty() {
        return Ok(SaveOutcome::Clean);
    }
    let path = session.document().path.clone();
    // 出字节走 `try_to_bytes`：视图与字节层形状不符 / writer 拒值 ⇒ `Err(CoreError::Map)`，
    // **绝不静默回退到原字节**（回退＝用户以为存上了，实际磁盘上还是旧内容）。
    let bytes = session.document().try_to_bytes()?;
    match save_atomic(&path, &bytes) {
        Ok(saved) => {
            session.mark_saved();
            session.note_save_ok();
            Ok(SaveOutcome::Written {
                path,
                tmp: saved.tmp,
                bytes: bytes.len(),
                dir_sync: saved.dir_sync,
            })
        }
        Err(err) => {
            session.note_save_err(err.to_string());
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 临时目录（`%TEMP%` 下，按 `tag` + **进程 ID** 分开；每次先清空）。
    ///
    /// # 为什么目录名必须带 PID（⚠ 别改回去）
    ///
    /// 两个 `cargo test` 进程并发跑同一个 lib 测试二进制时，**固定目录名**会让两边互相
    /// `remove_dir_all`、并抢同一个临时文件，产生**假红**（实测症状：另一个进程刚
    /// `create_dir_all`，这边就把整棵目录删掉，于是「建临时目录」或写盘随机失败）。
    /// 加上 [`std::process::id`] 之后两个进程各用各的目录，并发互不干扰；
    /// `tag` 仍然保留，便于在 `%TEMP%` 里按用例名排查。
    fn 临时目录(tag: &str) -> PathBuf {
        let pid = std::process::id();
        let d = std::env::temp_dir().join(format!("miredit_core_save_{tag}_{pid}"));
        // 自测（审查报告 2026-11-05 第 7 轮 · 缺陷 8）：光比"后缀是 _{pid}"是**自证** ——
        // `d` 就是上一行用同一个 `pid` 拼出来的，把 `std::process::id()` 换成常量 0 也照样绿。
        // 改成三条真判据：
        //   ① 期望串里的 pid 由**再一次独立调用**取回，不复用上面的 `pid`（两处不同源才叫交叉验证）；
        //   ② 与**实际目录名逐字相等**比较（不是只比后缀）；
        //   ③ 额外钉死"进程号不可能是 0"。
        // ⇒ 把 `let pid = std::process::id();` 改成 `let pid = 0;`，本断言必红（已做变异实验）。
        let 本进程 = std::process::id();
        assert_ne!(本进程, 0, "进程号不可能是 0，但 std::process::id() 返回了 0");
        let 实际名 = d.file_name().and_then(|s| s.to_str()).unwrap_or("<非 UTF-8>");
        let 期望名 = format!("miredit_core_save_{tag}_{本进程}");
        assert_eq!(
            实际名,
            期望名.as_str(),
            "临时目录名必须逐字等于「miredit_core_save_<tag>_<本进程 PID>」，否则并发隔离失效：{}",
            d.display()
        );
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("建临时目录");
        d
    }

    /// 临时文件必须与目标同目录（跨卷的话 rename 不再是原子操作）。来源：`save.rs:344-357`。
    #[test]
    fn 临时文件与目标同目录() {
        // 目标路径只用来算「临时文件放哪」，不落盘；但仍要**唯一且可移植**：
        // 用 `%TEMP%` + 进程号，别写死某块盘上的固定路径（公开仓/PR 的机器上没有 `I:`）。
        let 本进程 = std::process::id();
        assert_ne!(本进程, 0, "进程号不可能是 0，但 std::process::id() 返回了 0");
        let dir = std::env::temp_dir().join(format!("miredit_core_same_dir_{本进程}"));
        let p = dir.join("edit-0.map");
        let t = temp_path_for(&p);
        assert_eq!(t.parent(), p.parent());
        assert_eq!(
            t.file_name().unwrap().to_string_lossy(),
            "edit-0.map.miredit-tmp"
        );
        assert!(t.file_name().unwrap().to_string_lossy().ends_with(TEMP_SUFFIX));
    }

    /// **原子写成功**：内容与输入逐字节相等、临时文件不残留。来源：`save.rs:360-371`。
    #[test]
    fn 原子写成功且无残留() {
        let d = 临时目录("ok");
        let p = d.join("a.map");
        std::fs::write(&p, b"OLD").unwrap();
        let data: Vec<u8> = (0..=255u8).cycle().take(4096).collect();
        let saved = save_atomic(&p, &data).expect("应能写入");
        assert_eq!(std::fs::read(&p).unwrap(), data, "目标内容应与输入一致");
        assert!(!saved.tmp.exists(), "临时文件必须已被 rename 掉，不许残留");
        assert!(!temp_path_for(&p).exists());
        // 目录 fsync 的结果必须指向目标的父目录（不论成不成）
        assert_eq!(saved.dir_sync.dir(), d.as_path());
        eprintln!("[目录fsync] {:?}", saved.dir_sync);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **写临时文件失败 ⇒ 原文件一字节不动**（用「临时路径被一个目录占着」来模拟）。
    /// 来源：`save.rs:373-390`。
    #[test]
    fn 临时文件写失败时原文件不变() {
        let d = 临时目录("tempfail");
        let p = d.join("b.map");
        let orig = b"ORIGINAL-BYTES".to_vec();
        std::fs::write(&p, &orig).unwrap();
        std::fs::create_dir_all(temp_path_for(&p)).unwrap();

        let err = save_atomic(&p, b"NEW").unwrap_err();
        match &err {
            CoreError::Save { stage, tmp, .. } => {
                assert_eq!(*stage, SaveStage::Temp, "阶段必须是写临时文件：{err}");
                assert!(tmp.to_string_lossy().contains("miredit-tmp"));
            }
            other => panic!("应报 Save，实际 {other:?}"),
        }
        assert_eq!(std::fs::read(&p).unwrap(), orig, "原文件必须一字节不动");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **替换阶段失败 ⇒ 原目标仍在、内容未变、临时文件被清掉**。
    ///
    /// 用**确定性**的失败来源：目标路径是一个**非空目录**。`MoveFileExW`
    /// （`std::fs::rename` 在 Windows 上的实现）**不能**把文件覆盖到目录上，
    /// 必然返回 `ERROR_ACCESS_DENIED`，与本机文件系统/账户策略无关。来源：`save.rs:401-459`。
    #[test]
    fn 替换失败时原目标不变且停在替换阶段() {
        let d = 临时目录("renamefail");
        let p = d.join("c.map");
        std::fs::create_dir_all(&p).unwrap();
        let marker = p.join("keep.txt");
        let orig = b"ORIGINAL-BYTES".to_vec();
        std::fs::write(&marker, &orig).unwrap();

        let err = save_atomic(&p, b"NEW-CONTENT").expect_err("目录挡着时 rename 必须失败");

        match &err {
            CoreError::Save { stage, path, tmp, .. } => {
                assert_eq!(*stage, SaveStage::Rename, "必须停在替换阶段：{err}");
                assert_eq!(path, &p);
                assert!(
                    tmp.to_string_lossy().contains("miredit-tmp"),
                    "报错里要有临时文件路径（只有替换阶段才带它）：{err}"
                );
            }
            other => panic!("应报 Save，实际 {other:?}"),
        }
        // 原目标一字节没变（目录还在、标记文件原样）
        assert!(p.is_dir(), "目标目录必须还在（不许被先删后写毁掉）");
        assert_eq!(
            std::fs::read(&marker).unwrap(),
            orig,
            "目标里的内容必须一字节不动"
        );
        // 临时文件被清掉（不许把 5.9 MB 的残渣留在用户目录里）
        assert!(!temp_path_for(&p).exists(), "替换失败后临时文件必须清掉");
        std::fs::remove_dir_all(&d).ok();
    }

    /// 目标目录不存在 ⇒ 如实报错（阶段是写临时文件），且不会"顺手建目录"。
    /// 来源：`save.rs:393-399`。
    #[test]
    fn 目标目录不存在时如实报错() {
        // 不写死盘符：`%TEMP%` 下的唯一名字（带上进程号）照样"不存在"，且可移植到公开仓/PR 的机器。
        let 本进程 = std::process::id();
        let 目录 = std::env::temp_dir().join(format!("__no_such_dir_miredit_core_{本进程}__"));
        assert!(!目录.exists(), "前提：这个目录本来就不存在：{}", 目录.display());
        let p = 目录.join("x.map");
        let err = save_atomic(&p, b"X").unwrap_err();
        assert!(err.to_string().contains("x.map"), "报错里要有路径：{err}");
        assert!(!p.exists());
        assert!(!目录.exists(), "失败后也不许顺手把目录建出来：{}", 目录.display());
    }

    /// **目录项 fsync 如实分类，且不影响保存成败**（审计 E7）。来源：`save.rs:497-526`。
    ///
    /// 判据三条：① 无论成不成 `save_atomic` 都 `Ok`；② 报出的目录**就是**目标的父目录；
    /// ③ 失败必须带**系统报错原文**（空串等于没报），且 [`DirSync::note`] 有话说。
    #[test]
    fn 目录项fsync如实分类且不影响保存() {
        let d = 临时目录("dirsync");
        let p = d.join("e.map");
        std::fs::write(&p, b"OLD").unwrap();
        let data: Vec<u8> = (0..=255u8).cycle().take(1024).collect();
        let saved = save_atomic(&p, &data).expect("目录 fsync 的结果不该让保存失败");
        assert_eq!(std::fs::read(&p).unwrap(), data, "目标内容应已被替换");
        assert_eq!(saved.dir_sync.dir(), d.as_path(), "报出的目录必须是目标的父目录");
        eprintln!("[目录fsync] {:?} ⇒ {}", saved.dir_sync, saved.dir_sync.note());
        assert!(!saved.dir_sync.note().is_empty(), "note 不许为空");
        match &saved.dir_sync {
            DirSync::Ok(dir) => {
                assert_eq!(dir, &d);
                assert!(saved.dir_sync.is_ok());
            }
            DirSync::OpenFailed { err, .. } | DirSync::FlushFailed { err, .. } => {
                assert!(!err.is_empty(), "失败必须带系统报错原文：{err}");
                assert!(!saved.dir_sync.is_ok());
            }
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 目录不存在 ⇒**确定性**地报 `OpenFailed`（不是 `Ok`、不 panic、不静默）。
    /// 来源：`save.rs:529-537`。
    #[test]
    fn 目录不存在时报打不开() {
        // 同上：`%TEMP%` 下的唯一名字，不写死盘符。
        let 本进程 = std::process::id();
        let 目录 = std::env::temp_dir().join(format!("__no_such_dir_miredit_core_dirsync_{本进程}__"));
        assert!(!目录.exists(), "前提：这个目录本来就不存在：{}", 目录.display());
        let p = 目录.join("x.map");
        let r = fsync_parent(&p);
        eprintln!("[目录fsync] {r:?}");
        assert!(matches!(r, DirSync::OpenFailed { .. }), "{r:?}");
        assert!(!r.is_ok());
        assert_eq!(r.dir(), 目录.as_path());
    }
}
