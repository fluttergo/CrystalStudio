//! 保存：**原子替换**写回 `.map`（写同目录临时文件 → 校验 → `rename`）。
//!
//! # 〇、本模块现在是一层**薄适配层**（2026-11-05 迁移收口）
//!
//! 「原子保存」的全部实现已搬进 `miredit_core::save`：临时路径命名、`sync_all`、
//! 读回逐字节核对、`rename`、尽力 fsync 父目录 + 如实分类，都在那里。
//! 本文件只做两件事：
//!
//! 1. **保持 public API 逐字不变**（`save_atomic -> Result<PathBuf, String>`、
//!    [`SaveOutcome`] 的四个变体、`save_session(doc, ed)`）—— `app.rs` / `editrun.rs`
//!    只改一处（`&DocState` → `&mut DocState`，见下条）；
//! 2. **保持用户可见文案逐字不变**：内核的 `CoreError::Save` 带**阶段**
//!    （`Temp` / `Sync` / `Verify` / `Rename`），这里按阶段折回原来那四条
//!    [`crate::ui_text`] 模板；目录 fsync 的结果也按原来那三条模板打到 stderr。
//!    于是 CLI 的 `[保存] …` 行与迁移前**一个字符都不差**。
//!
//! 为什么 `save_session` 不直接转调 `miredit_core::save_session`：内核那版要求
//! **会话自己持有文档**（路径从 `session.document().path` 取），而本仓的宿主把
//! `DocState`（路径 + 地图）与会话分开持有。硬套就得把 `DocState` 塞进会话，
//! 那是把宿主的职责搬进内核。所以这里保留"取路径 / 判脏 / 记账"三段宿主逻辑，
//! 只把**真正会出错的那一段**（原子写）交给内核 —— 重复的正是那 130 行。
//!
//! ⚠ **2026-11-05 九格式收口**：出字节从 `d.map.to_bytes()` 改成
//! [`crate::mapdoc::MapDoc::to_bytes`]（= `Document::try_to_bytes`，九格式各自 writer +
//! 未解尾块原样带上）。`map.to_bytes()` 只认 12 / 14 / 36 三档，
//! 会把 Type5 写成 New14、把 67 张带尾块的图截断 —— 那是**假保存**。
//!
//! # 为什么必须原子
//! 直接 `File::create(path)` 会先把原文件截成 0 字节再写。中途断电 / 磁盘满 / 进程被杀
//! 就留下一个半截地图 —— 而地图是用户资产，半截文件与"删了"等价。
//! 本模块的顺序是（实现见 `miredit_core::save`）：
//!
//! ```text
//! ① 写 <目标名>.miredit-tmp（与目标同目录 ⇒ 同一卷 ⇒ rename 是原子操作）
//! ② 把临时文件读回来，长度与内容逐字节核对
//! ③ rename(临时 → 目标)：同名覆盖由 MoveFileExW(MOVEFILE_REPLACE_EXISTING) 保证
//! ④ **尝试** fsync 父目录（目录项持久化）—— Windows 需 FILE_FLAG_BACKUP_SEMANTICS
//! ```
//!
//! **任何一步失败都不会动原文件**：①失败时目标还没被碰过；②失败时删掉临时文件；
//! ③失败时同样删掉临时文件，原文件仍在原处。失败原因一律**带路径与系统报错原文**返回，
//! 由调用方写到状态栏（绝不静默）。
//!
//! # 为什么还要"读回来核对"
//! `std::fs::write` 返回 `Ok` 只代表系统调用成功，不代表内容真的落了盘（缓写、
//! 磁盘满时的短写都可能）。地图文件动辄 5.9 MB，这里多花一次读+memcmp（本地 SSD 上
//! 约 2~5 ms），换的是"存下去的一定是我算出来的那些字节"。
//!
//! # 落盘顺序（审计 E7：断电后"文件内容"与"目录项"是两件事）
//!
//! ①②③ 与旧版一致，只多了 `sync_all()`：没有它，rename 之后断电可能留下
//! "文件名在、内容是 0 或半截"的洞（rename 只保证目录项原子，不保证数据已落盘）。
//!
//! ④ 的结果用 [`DirSync`] **如实分类**：
//! - `Ok(dir)` ⇒ 目录项已刷；
//! - `OpenFailed` / `FlushFailed` ⇒ **打印一行 `[保存] 目录 fsync 不可用 …`**，
//!   明说"文件内容已落盘，目录项持久化未验证"。
//!
//! ⚠ **Windows 上大概率走 `FlushFailed`**：NTFS 不支持对目录句柄 `FlushFileBuffers`
//! （返回 `ERROR_INVALID_HANDLE`）。这不是本模块的 bug，也不是我们假装成功 ——
//! 它会**如实出现在每一次保存的 stderr 里**，汇报里同样写"目录项持久化：已实现 / 未验证"。
//!
//! ④ 失败**不影响保存成败**：rename 已经成功，目标路径上就是新内容；
//! 把"目录项没刷"报成"保存失败"会让用户以为文件没存上（那是假警报）。

use std::path::{Path, PathBuf};

use miredit_core::SaveStage;

use crate::ui_text as tx;

// 原子写与目录 fsync 的实现全在内核；这三个符号**只有一份定义**。
pub use miredit_core::{fsync_parent, temp_path_for, DirSync, TEMP_SUFFIX};

/// 把一次目录 fsync 结果打到 stderr（成功也打一行，方便取证时确认走过这条路）。
///
/// ⚠ **不用 `DirSync::note()`**：内核那套文案是给"网页版/状态栏"用的通用句子，
/// 与本仓既有的三条模板**不是逐字相同**。CLI 的 `[保存] …` 行是验收要 grep 的契约，
/// 所以这里仍按原来的三条模板输出（迁移前后 stderr 逐字节一致）。
fn report_dir_sync(r: &DirSync) {
    match r {
        DirSync::Ok(dir) => eprintln!(
            "{}",
            tx::fill(
                tx::DIAG_SAVE_DIRSYNC_OK,
                &[("path", &dir.display().to_string())]
            )
        ),
        DirSync::OpenFailed { dir, err } => eprintln!(
            "{}",
            tx::fill(
                tx::SAVE_ERR_DIRSYNC_OPEN,
                &[("path", &dir.display().to_string()), ("err", err)]
            )
        ),
        DirSync::FlushFailed { dir, err } => eprintln!(
            "{}",
            tx::fill(
                tx::SAVE_ERR_DIRSYNC,
                &[("path", &dir.display().to_string()), ("err", err)]
            )
        ),
    }
}

/// 内核的保存错误 → **与迁移前逐字相同**的中文原因（按**阶段**选模板）。
///
/// 阶段 → 模板的对应（与迁移前 `save_atomic` 的五条 `return Err(...)` 一一对应）：
///
/// | 内核阶段 | 含义 | 本仓模板 |
/// |---|---|---|
/// | [`SaveStage::Temp`] | 写临时文件 / `write_all` / 读回时 IO 失败 | [`tx::SAVE_ERR_TEMP`] |
/// | [`SaveStage::Sync`] | `sync_all()` 刷盘失败 | [`tx::SAVE_ERR_SYNC`] |
/// | [`SaveStage::Verify`] | 读回内容与写入不等（短写） | 见下 |
/// | [`SaveStage::Rename`] | `rename` 原子替换失败 | [`tx::SAVE_ERR_RENAME`] |
///
/// `Verify` 那一档：内核给的 `detail` 已经是
/// `写入不完整：期望 {want} 字节，读回 {got} 字节`，而本仓模板要的是 `{want}` / `{got}`
/// 两个独立数字。这里**剥掉内核的前缀再套本仓句式**，不重复也不丢数字；
/// 该分支在本地 SSD 上不可达（写→刷→读回之间不会短写），如实记录在此。
fn save_err_text(e: miredit_core::CoreError) -> String {
    match e {
        miredit_core::CoreError::Save {
            stage,
            path,
            tmp,
            detail,
        } => {
            let tmp_s = tmp.display().to_string();
            match stage {
                SaveStage::Temp => {
                    tx::fill(tx::SAVE_ERR_TEMP, &[("path", &tmp_s), ("err", &detail)])
                }
                SaveStage::Sync => {
                    tx::fill(tx::SAVE_ERR_SYNC, &[("path", &tmp_s), ("err", &detail)])
                }
                SaveStage::Verify => {
                    let tail = detail.strip_prefix("写入不完整：").unwrap_or(&detail);
                    format!("临时文件 {tmp_s} 写入不完整（{tail}）")
                }
                SaveStage::Rename => tx::fill(
                    tx::SAVE_ERR_RENAME,
                    &[
                        ("path", &path.display().to_string()),
                        ("tmp", &tmp_s),
                        ("err", &detail),
                    ],
                ),
            }
        }
        // 保存路径上不会有别的错误种类；真出现就如实报全（绝不吞）
        other => other.to_string(),
    }
}

/// 原子写回；成功返回实际用过的临时文件路径（便于诊断/取证）。
///
/// 失败时**原文件保持原样**，错误串已用 [`crate::ui_text`] 的模板装好
/// （含路径与系统报错原文），可直接上状态栏。
///
/// 实现全部委托 `miredit_core::save_atomic`（写 → `sync_all` → 读回核对 → rename →
/// **尝试** fsync 父目录）；最后一步的结果按原来的三条模板打到 stderr
/// （[`report_dir_sync`]），**不影响本函数的返回值**。
pub fn save_atomic(path: &Path, bytes: &[u8]) -> Result<PathBuf, String> {
    match miredit_core::save_atomic(path, bytes) {
        Ok(saved) => {
            report_dir_sync(&saved.dir_sync);
            Ok(saved.tmp)
        }
        Err(err) => Err(save_err_text(err)),
    }
}

/// 一次「保存会话」的结果（GUI 与无窗驱动器共用同一套判定与文案）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveOutcome {
    /// 没打开地图，无从保存。
    NoMap,
    /// 没有未保存的修改 ⇒ **一个字节都没写**。
    Clean,
    /// 写成功：目标路径 + 临时文件 + 字节数。
    Written {
        /// 写回的目标路径。
        path: PathBuf,
        /// 被 rename 掉的临时文件路径（诊断用，正常情况下已不存在）。
        tmp: PathBuf,
        /// 写了多少字节。
        bytes: usize,
    },
    /// 写失败（原文件保持原样），内含已装好模板的原因。
    Failed(String),
}

impl SaveOutcome {
    /// 是否真的写了盘。
    pub fn wrote(&self) -> bool {
        matches!(self, SaveOutcome::Written { .. })
    }
}

/// 把内存里的地图写回它自己的路径（**只在有修改时真写**），并把结果记进编辑会话。
///
/// GUI 的 `Ctrl+S` 与无窗驱动器（`--save`）走的都是这一个函数 —— 两条路径不各写一套。
///
/// # 为什么参数是 `&mut DocState`（2026-11-05 九格式收口）
///
/// 出字节的唯一出口是 [`crate::mapdoc::MapDoc::to_bytes`]，它要把宿主编辑视图与
/// `Document::view` 用 `std::mem::swap` 对调（**O(1)，不拷格**）才能让内核看到"当前编辑结果"。
/// 对调需要 `&mut`，所以这里收 `&mut DocState`。
///
/// ⚠ **绝不再用 `d.map.to_bytes()`**：那只认 12 / 14 / 36 三档，
/// 会把 Type5 写成 New14、把 67 张带尾块的图截断（用户以为存上了，磁盘上却是另一种格式）。
pub fn save_session(
    doc: &mut crate::mapdoc::DocState,
    ed: &mut crate::edit::EditSession,
) -> SaveOutcome {
    let Some(d) = doc.doc.as_mut() else {
        eprintln!("{}", tx::DIAG_EDIT_SAVE_NOMAP);
        return SaveOutcome::NoMap;
    };
    if !ed.dirty() {
        eprintln!("{}", tx::DIAG_EDIT_SAVE_SKIP);
        return SaveOutcome::Clean;
    }
    let path = d.path.clone();
    // 出字节失败**绝不回退**到"原字节"或 `map.to_bytes()`（假保存比报错更糟）
    let bytes = match d.to_bytes() {
        Ok(b) => b,
        Err(err) => {
            eprintln!("{}", tx::fill(tx::DIAG_EDIT_SAVE_FAIL, &[("err", &err)]));
            ed.note_save_err(err.clone());
            return SaveOutcome::Failed(err);
        }
    };
    match save_atomic(&path, &bytes) {
        Ok(tmp) => {
            ed.mark_saved();
            ed.note_save_ok();
            eprintln!(
                "{}",
                tx::fill(
                    tx::DIAG_EDIT_SAVE_OK,
                    &[
                        ("path", &path.display().to_string()),
                        ("bytes", &bytes.len().to_string()),
                        ("tmp", &tmp.display().to_string()),
                    ]
                )
            );
            SaveOutcome::Written {
                path,
                tmp,
                bytes: bytes.len(),
            }
        }
        Err(err) => {
            eprintln!("{}", tx::fill(tx::DIAG_EDIT_SAVE_FAIL, &[("err", &err)]));
            ed.note_save_err(err.clone());
            SaveOutcome::Failed(err)
        }
    }
}

/// **另存为**：把当前内存里的地图原子写到**另一个路径**，成功后把文档路径改成新路径。
///
/// # 与 [`save_session`] 的三处不同（2026-11-05）
///
/// | | [`save_session`] | 本函数 |
/// |---|---|---|
/// | 目标路径 | 文档自己的 `path` | 调用方给的 `target` |
/// | 脏标记 | 不脏就**一个字节都不写**（`Clean`）| **照写**：另存为的语义就是"把当前内容落到新文件"，没有修改也要有一份 |
/// | 成功后 | 只清脏标记 | 清脏标记 **+ 把文档路径/文件名改成新路径**（标题、页签、状态栏跟着变）|
///
/// # 复用同一套原子写（一行都不另写）
///
/// 真正的写盘是 [`save_atomic`]（= `miredit_core::save_atomic`）：
/// 写 `<目标名>.miredit-tmp` → `sync_all` → 读回逐字节核对 → `rename` 原子替换 →
/// **尽力** fsync 父目录并如实分类。失败时**原文件、原路径、脏标记全都不动**。
///
/// # 两条前置判据（先说清楚，别等写完 5.9 MB 临时文件再删）
///
/// 1. 目标路径为空 ⇒ [`tx::SAVE_AS_ERR_EMPTY`]；
/// 2. 目标路径是一个**已存在的目录** ⇒ [`tx::SAVE_AS_ERR_IS_DIR_TMPL`]
///    （`MoveFileExW` 把文件覆盖到目录上必然失败，但那时已经白写一份临时文件）。
///
/// 两条都**不碰磁盘、不改文档路径**，只回一条带原因的回执。
pub fn save_as_session(
    doc: &mut crate::mapdoc::DocState,
    ed: &mut crate::edit::EditSession,
    target: &Path,
) -> SaveOutcome {
    let Some(d) = doc.doc.as_mut() else {
        eprintln!("{}", tx::DIAG_EDIT_SAVE_NOMAP);
        return SaveOutcome::NoMap;
    };
    if target.as_os_str().is_empty() {
        return save_as_failed(ed, tx::SAVE_AS_ERR_EMPTY.to_owned());
    }
    if target.is_dir() {
        return save_as_failed(
            ed,
            tx::fill(
                tx::SAVE_AS_ERR_IS_DIR_TMPL,
                &[("path", &target.display().to_string())],
            ),
        );
    }
    let old = d.path.clone();
    // 出字节失败**绝不回退**到"原字节"或 `map.to_bytes()`（假保存比报错更糟）
    let bytes = match d.to_bytes() {
        Ok(b) => b,
        Err(err) => return save_as_failed(ed, err),
    };
    match save_atomic(target, &bytes) {
        Ok(tmp) => {
            // ① 文档出处改到新路径（宿主 path/file_name + 内核 Document 同名字段，三处一起）
            d.retarget(target);
            // ② 会话绑的字节层真值换成**刚写出去的那份**：库号字节还原 / 地表闸门 /
            //    选区裁剪读的都是它，留在旧文件上就是两套真值。
            if !ed.bind_document(target, &bytes) {
                eprintln!(
                    "{}",
                    tx::fill(
                        tx::DIAG_EDIT_BIND_FAIL,
                        &[("path", &target.display().to_string())]
                    )
                );
            }
            // ③ 清脏标记 + 记一次保存成功
            ed.mark_saved();
            ed.note_save_ok();
            eprintln!(
                "{}",
                tx::fill(
                    tx::DIAG_SAVE_AS_OK,
                    &[
                        ("path", &target.display().to_string()),
                        ("bytes", &bytes.len().to_string()),
                        ("tmp", &tmp.display().to_string()),
                        ("old", &old.display().to_string()),
                    ]
                )
            );
            SaveOutcome::Written {
                path: target.to_path_buf(),
                tmp,
                bytes: bytes.len(),
            }
        }
        Err(err) => save_as_failed(ed, err),
    }
}

/// 另存为失败的统一出口：**一条 stderr 诊断 + 记进会话 + 返回 `Failed`**。
///
/// 三条前置判据与写盘失败都走它，所以"失败时状态栏显示什么"只有一处实现。
fn save_as_failed(ed: &mut crate::edit::EditSession, err: String) -> SaveOutcome {
    eprintln!("{}", tx::fill(tx::DIAG_SAVE_AS_FAIL, &[("err", &err)]));
    ed.note_save_err(err.clone());
    SaveOutcome::Failed(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn 临时目录(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("miredit_gui_save_{tag}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("建临时目录");
        d
    }

    /// 临时文件必须与目标同目录（跨卷的话 rename 不再是原子操作）。
    #[test]
    fn 临时文件与目标同目录() {
        // 目标路径只用来算「临时文件放哪」，不落盘；仍要**唯一且可移植**：
        // 用 `%TEMP%` + 进程号，别写死某块盘上的固定路径（公开仓/PR 的机器上没有 `I:`）。
        let 本进程 = std::process::id();
        assert_ne!(本进程, 0, "进程号不可能是 0，但 std::process::id() 返回了 0");
        let dir = std::env::temp_dir().join(format!("miredit_gui_same_dir_{本进程}"));
        let p = dir.join("edit-0.map");
        let t = temp_path_for(&p);
        assert_eq!(t.parent(), p.parent());
        assert_eq!(
            t.file_name().unwrap().to_string_lossy(),
            "edit-0.map.miredit-tmp"
        );
        assert!(
            t.file_name().unwrap().to_string_lossy().ends_with(TEMP_SUFFIX),
            "临时文件名必须以 {TEMP_SUFFIX} 收尾"
        );
    }

    /// **原子写成功**：内容与输入逐字节相等、临时文件不残留。
    #[test]
    fn 原子写成功且无残留() {
        let d = 临时目录("ok");
        let p = d.join("a.map");
        std::fs::write(&p, b"OLD").unwrap();
        let data: Vec<u8> = (0..=255u8).cycle().take(4096).collect();
        let tmp = save_atomic(&p, &data).expect("应能写入");
        assert_eq!(std::fs::read(&p).unwrap(), data, "目标内容应与输入一致");
        assert!(!tmp.exists(), "临时文件必须已被 rename 掉，不许残留");
        assert!(!temp_path_for(&p).exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **写临时文件失败 ⇒ 原文件一字节不动**（用「临时路径被一个目录占着」来模拟）。
    #[test]
    fn 临时文件写失败时原文件不变() {
        let d = 临时目录("tempfail");
        let p = d.join("b.map");
        let orig = b"ORIGINAL-BYTES".to_vec();
        std::fs::write(&p, &orig).unwrap();
        // 占住临时路径（目录不能当文件写）
        std::fs::create_dir_all(temp_path_for(&p)).unwrap();

        let err = save_atomic(&p, b"NEW").unwrap_err();
        assert!(
            err.contains("miredit-tmp"),
            "报错要说清是哪个临时路径失败：{err}"
        );
        assert_eq!(std::fs::read(&p).unwrap(), orig, "原文件必须一字节不动");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 目标目录不存在 ⇒ 如实报错，且不会"顺手建目录"（建到别处更难查）。
    #[test]
    fn 目标目录不存在时如实报错() {
        // 不写死盘符：`%TEMP%` 下的唯一名字（带上进程号）照样"不存在"，且可移植到公开仓/PR 的机器。
        let 本进程 = std::process::id();
        let dir = std::env::temp_dir().join(format!("__no_such_dir_miredit_{本进程}__"));
        assert!(!dir.exists(), "前提：这个目录本来就不存在：{}", dir.display());
        let p = dir.join("x.map");
        let err = save_atomic(&p, b"X").unwrap_err();
        assert!(err.contains("x.map"), "报错里要有路径：{err}");
        assert!(!p.exists());
        assert!(!dir.exists(), "失败后也不许顺手把目录建出来：{}", dir.display());
    }

    /// **替换阶段失败 ⇒ 原目标仍在、内容未变、临时文件被清掉**。
    ///
    /// ## 为什么改成"目标路径被目录占着"（审计 E8，2026-11-04）
    /// 旧版用"目标文件只读"逼 `rename` 失败，然后 `match` 两个分支**都断言成功**：
    /// 在只读位挡不住 replace 的机器上走 `Ok` 分支，用例照样绿 —— 等于什么都不验证。
    ///
    /// 现在用**确定性**的失败来源：目标路径是一个**非空目录**。`MoveFileExW`
    /// （`std::fs::rename` 在 Windows 上的实现）**不能**把文件覆盖到目录上，
    /// 必然返回 `ERROR_ACCESS_DENIED`，与本机文件系统/账户策略无关。
    ///
    /// 断言的是**具体阶段**（三选一里唯一能解释得通的）：
    /// - 错误串必须来自 [`tx::SAVE_ERR_RENAME`]（含"原子替换"这个词），**不是**
    ///   [`tx::SAVE_ERR_TEMP`]（"写临时文件"）或 [`tx::SAVE_ERR_SHORT`]（"写入不完整"）——
    ///   所以"把阶段报错为另一阶段"的变异会让本用例红；
    /// - 错误串里同时带**目标路径**与**临时文件路径**（只有 rename 分支才带 tmp）；
    /// - 目标目录与里面的标记文件**一字节没变**（没被"先删后写"毁掉）；
    /// - 临时文件**已被清理**（把 `remove_file(&tmp)` 那行删掉 ⇒ 本用例红）。
    #[test]
    fn 替换失败时原目标不变且停在替换阶段() {
        let d = 临时目录("renamefail");
        // 目标路径 = 非空目录（rename 无法把文件覆盖到目录上）
        let p = d.join("c.map");
        std::fs::create_dir_all(&p).unwrap();
        let marker = p.join("keep.txt");
        let orig = b"ORIGINAL-BYTES".to_vec();
        std::fs::write(&marker, &orig).unwrap();

        let err = save_atomic(&p, b"NEW-CONTENT").expect_err("目录挡着时 rename 必须失败");

        // ① 阶段必须是「原子替换」，不是写临时文件/短写
        assert!(
            err.contains("原子替换"),
            "报错必须来自替换阶段（SAVE_ERR_RENAME）：{err}"
        );
        assert!(
            !err.contains("写临时文件") && !err.contains("写入不完整"),
            "不许把阶段报成写临时文件/短写：{err}"
        );
        // ② 三个真实路径都带齐（目标 + 临时）
        assert!(err.contains("c.map"), "报错里要有目标路径：{err}");
        assert!(
            err.contains("miredit-tmp"),
            "报错里要有临时文件路径（只有替换阶段才带它）：{err}"
        );
        // ③ 原目标一字节没变（目录还在、标记文件原样）
        assert!(p.is_dir(), "目标目录必须还在（不许被先删后写毁掉）");
        assert_eq!(
            std::fs::read(&marker).unwrap(),
            orig,
            "目标里的内容必须一字节不动"
        );
        // ④ 临时文件被清掉（不许把 5.9 MB 的残渣留在用户目录里）
        assert!(
            !temp_path_for(&p).exists(),
            "替换失败后临时文件必须清掉"
        );

        std::fs::remove_dir_all(&d).ok();
    }

    /// 只读目标 + 只读位挡得住 replace 的那种系统：**同样**要停在替换阶段。
    ///
    /// 与上一条互补：这条覆盖"目标是个普通文件"的真实形态。本机只读位未必挡得住
    /// `MoveFileExW`，所以它**允许**走成功分支 —— 但走成功分支时**不假装验证了失败路径**
    /// （打了 `[跳过]` 标记就 `return`，旧版在这里的 `assert_eq!(read, NEW-CONTENT)` 就是
    /// 那条"两分支都绿"的空转断言）。
    #[test]
    fn 只读目标失败路径不假装通过() {
        let d = 临时目录("rokill");
        let p = d.join("c.map");
        let orig = b"READ-ONLY-ORIGINAL".to_vec();
        std::fs::write(&p, &orig).unwrap();
        let mut perm = std::fs::metadata(&p).unwrap().permissions();
        perm.set_readonly(true);
        std::fs::set_permissions(&p, perm).unwrap();

        match save_atomic(&p, b"NEW-CONTENT") {
            Err(err) => {
                assert!(err.contains("原子替换"), "报错必须来自替换阶段：{err}");
                assert_eq!(std::fs::read(&p).unwrap(), orig, "原文件必须一字节不动");
                assert!(!temp_path_for(&p).exists(), "临时文件必须清掉");
                eprintln!("[通过] 只读目标下 replace 失败：{err}");
            }
            Ok(_) => {
                // **不制造"通过"的假象**：本机这个系统调用不受只读位约束，
                // 失败路径由 `替换失败时原目标不变且停在替换阶段` 确定性证明。
                eprintln!("[跳过] 本机只读位挡不住 replace，失败路径见 替换失败时原目标不变且停在替换阶段");
            }
        }
        let mut perm2 = std::fs::metadata(&p).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perm2.set_readonly(false);
        let _ = std::fs::set_permissions(&p, perm2);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **目录项 fsync 如实分类，且不影响保存成败**（审计 E7）。
    ///
    /// 判据三条：
    /// ① 目录 fsync 无论成不成，`save_atomic` 都必须返回 `Ok`（rename 已成功 ⇒ 文件是新的）；
    /// ② 结果里报出的目录必须**就是**目标文件的父目录（不能报别处，那会让人查错地方）；
    /// ③ 失败必须带**系统报错原文**（空串等于没报）。
    #[test]
    fn 目录项fsync如实分类且不影响保存() {
        let d = 临时目录("dirsync");
        let p = d.join("e.map");
        std::fs::write(&p, b"OLD").unwrap();
        let data: Vec<u8> = (0..=255u8).cycle().take(1024).collect();
        save_atomic(&p, &data).expect("目录 fsync 的结果不该让保存失败");
        assert_eq!(std::fs::read(&p).unwrap(), data, "目标内容应已被替换");

        let r = fsync_parent(&p);
        eprintln!("[目录fsync] {r:?}");
        assert_eq!(r.dir(), d.as_path(), "报出的目录必须是目标的父目录");
        match &r {
            DirSync::Ok(dir) => {
                assert_eq!(dir, &d);
                assert!(r.is_ok());
            }
            DirSync::OpenFailed { err, .. } | DirSync::FlushFailed { err, .. } => {
                assert!(!err.is_empty(), "失败必须带系统报错原文：{r:?}");
                assert!(!r.is_ok());
            }
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 目录不存在 ⇒**确定性**地报 `OpenFailed`（不是 `Ok`、不 panic、不静默）。
    #[test]
    fn 目录不存在时报打不开() {
        // 同上：`%TEMP%` 下的唯一名字，不写死盘符。
        let 本进程 = std::process::id();
        let dir = std::env::temp_dir().join(format!("__no_such_dir_miredit_dirsync_{本进程}__"));
        assert!(!dir.exists(), "前提：这个目录本来就不存在：{}", dir.display());
        let p = dir.join("x.map");
        let r = fsync_parent(&p);
        eprintln!("[目录fsync] {r:?}");
        assert!(matches!(r, DirSync::OpenFailed { .. }), "{r:?}");
        assert!(!r.is_ok());
        assert_eq!(r.dir(), dir.as_path());
    }

    // ───────────────────────── 另存为（2026-11-05） ─────────────────────────

    /// 造一张 `w×h` 的 12B 地图文件（列优先；每格 `bk_img = 下标 + 1`）。
    fn 造图(tag: &str, w: u16, h: u16) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("miredit_gui_saveas_{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let p = dir.join(format!("{tag}.map"));
        let cell = 12usize;
        let mut bytes = vec![0u8; mir2_map::HEADER_LEN + cell * (w as usize * h as usize)];
        bytes[0..2].copy_from_slice(&w.to_le_bytes());
        bytes[2..4].copy_from_slice(&h.to_le_bytes());
        for i in 0..(w as usize * h as usize) {
            let base = mir2_map::HEADER_LEN + i * cell;
            bytes[base..base + 2].copy_from_slice(&((i as u16) + 1).to_le_bytes());
        }
        std::fs::write(&p, &bytes).expect("写临时地图");
        p
    }

    /// 打开一张图并**改一笔**（= 另存为的典型前置状态：脏的）。
    fn 打开并改一笔(p: &Path) -> (crate::mapdoc::DocState, crate::edit::EditSession) {
        let (doc, err) = crate::mapdoc::DocState::load_map(p);
        let doc = doc.expect("应能打开");
        let mut st = crate::mapdoc::DocState {
            doc: Some(doc),
            assets: crate::mapdoc::Assets::Failed("单元测试不扫资源库".to_owned()),
            map_error: err,
            map_gen: 1,
        };
        let mut ed = crate::edit::EditSession::default();
        if let Some(d) = st.doc.as_ref() {
            assert!(ed.bind_document(&d.path, d.orig_bytes()), "绑定真文档");
        }
        ed.select(0);
        let rep = crate::edit::apply_script(
            &mut st,
            &mut ed,
            &[crate::edit::EditCmd::Pickup(0, 0), crate::edit::EditCmd::Brush(1, 1)],
        );
        assert!(rep.edited_cells > 0, "改一笔必须真改到格");
        assert!(ed.dirty(), "改过就必须是脏的");
        (st, ed)
    }

    /// **另存为成功**：新文件在、长度不变、**原文件一字节不动**、文档路径/文件名换成新路径、
    /// 脏标记清掉、没有临时文件残留。
    ///
    /// 这条是验收口径的机器版（① 新文件存在且大小相同 ② 原文件未被修改 ③ 文档改认新路径）。
    #[test]
    fn 另存为写到新路径且原文件不动() {
        let a = 造图("ok", 6, 6);
        let orig = std::fs::read(&a).expect("读原图");
        let (mut st, mut ed) = 打开并改一笔(&a);
        let b = a.parent().expect("父目录").join("b.map");

        let out = save_as_session(&mut st, &mut ed, &b);
        let SaveOutcome::Written { path, tmp, bytes } = out else {
            panic!("另存为应成功，实际：{out:?}");
        };
        assert_eq!(path, b, "回执里的路径必须是新路径");
        assert!(!tmp.exists(), "临时文件必须已被 rename 掉");
        assert!(!temp_path_for(&b).exists(), "不许留下临时文件残渣");

        let new = std::fs::read(&b).expect("新文件必须真的在盘上");
        assert_eq!(new.len(), orig.len(), "另存为不许改文件长度");
        assert_eq!(new.len(), bytes, "回执里的字节数要与磁盘一致");
        assert_ne!(new, orig, "改过一笔 ⇒ 新文件内容必须与原文件不同");
        assert_eq!(std::fs::read(&a).expect("读原图"), orig, "原文件必须一字节不动");

        let d = st.doc.as_ref().expect("文档还在");
        assert_eq!(d.path, b, "文档路径必须改成新路径");
        assert_eq!(d.file_name, "b.map", "标题/页签/状态栏读的是 file_name");
        let title = st.title();
        assert!(title.contains("b.map"), "标题里要是新文件名：{title}");
        assert!(!ed.dirty(), "另存为成功后脏标记必须清");
        assert!(ed.save_error().is_none(), "成功时不许留失败原因");

        let _ = std::fs::remove_dir_all(a.parent().unwrap());
    }

    /// **另存为失败（目标是非空目录）**：报错 + 原文件不动 + 文档路径不动 + 目标没被创建/破坏 +
    /// 临时文件不残留 + 脏标记保持。
    ///
    /// 失败来源是**确定性**的（目录挡着，`MoveFileExW` 不可能把文件覆盖到目录上），
    /// 与本机文件系统/账户策略无关。
    #[test]
    fn 另存为目标是目录时失败且原文件不动() {
        let a = 造图("isdir", 4, 4);
        let orig = std::fs::read(&a).expect("读原图");
        let (mut st, mut ed) = 打开并改一笔(&a);
        let target = a.parent().expect("父目录").join("b.map");
        std::fs::create_dir_all(&target).expect("把目标占成目录");
        let marker = target.join("keep.txt");
        std::fs::write(&marker, b"KEEP").expect("写标记文件");

        let out = save_as_session(&mut st, &mut ed, &target);
        let SaveOutcome::Failed(err) = out else {
            panic!("目标是目录时必须失败，实际：{out:?}");
        };
        assert!(err.contains("目录"), "报错要说清是「目录」挡着：{err}");
        assert!(err.contains("b.map"), "报错里要有目标路径：{err}");

        // ① 原文件一字节不动
        assert_eq!(std::fs::read(&a).expect("读原图"), orig, "原文件必须一字节不动");
        // ② 文档路径不动（失败不许改认路径）
        let d = st.doc.as_ref().expect("文档还在");
        assert_eq!(d.path, a, "失败时文档路径不许改");
        assert_eq!(d.file_name, "isdir.map");
        // ③ 目标仍是那个目录、里面的东西没被动、没有多出文件
        assert!(target.is_dir(), "目标目录必须还在");
        assert_eq!(std::fs::read(&marker).expect("读标记"), b"KEEP");
        assert!(!temp_path_for(&target).exists(), "失败后不许留下临时文件");
        // ④ 脏标记保持（改动还在内存里，用户还有机会换个路径再存）
        assert!(ed.dirty(), "失败不许清脏标记");
        assert!(ed.save_error().is_some(), "失败原因要记进会话（状态栏要显示）");

        let _ = std::fs::remove_dir_all(a.parent().unwrap());
    }

    /// **另存为空路径被拒**：一条带原因的回执，磁盘与文档路径都不动。
    #[test]
    fn 另存为空路径被拒() {
        let a = 造图("empty", 4, 4);
        let orig = std::fs::read(&a).expect("读原图");
        let (mut st, mut ed) = 打开并改一笔(&a);

        let out = save_as_session(&mut st, &mut ed, Path::new(""));
        assert_eq!(out, SaveOutcome::Failed(tx::SAVE_AS_ERR_EMPTY.to_owned()));
        assert_eq!(std::fs::read(&a).expect("读原图"), orig, "一个字节都不许写");
        assert_eq!(st.doc.as_ref().expect("文档还在").path, a);
        assert!(ed.dirty(), "失败不许清脏标记");

        let _ = std::fs::remove_dir_all(a.parent().unwrap());
    }

    /// **没有修改也照写**（与 [`save_session`] 的 `Clean` 语义不同）：
    /// 另存为的语义就是"把当前内容落到新文件"，没改过也要有一份。
    ///
    /// 顺带钉住**字节保真**：没改过 ⇒ 新文件与原文件逐字节相同。
    #[test]
    fn 没有修改也照写且逐字节相同() {
        let a = 造图("clean", 5, 4);
        let orig = std::fs::read(&a).expect("读原图");
        let (doc, err) = crate::mapdoc::DocState::load_map(&a);
        let mut st = crate::mapdoc::DocState {
            doc,
            assets: crate::mapdoc::Assets::Failed("单元测试不扫资源库".to_owned()),
            map_error: err,
            map_gen: 1,
        };
        let mut ed = crate::edit::EditSession::default();
        if let Some(d) = st.doc.as_ref() {
            assert!(ed.bind_document(&d.path, d.orig_bytes()));
        }
        assert!(!ed.dirty(), "刚打开时不该是脏的");

        let b = a.parent().expect("父目录").join("b.map");
        let out = save_as_session(&mut st, &mut ed, &b);
        assert!(out.wrote(), "没有修改也必须真写：{out:?}");
        assert_eq!(
            std::fs::read(&b).expect("新文件必须真的在盘上"),
            orig,
            "没改过 ⇒ 新文件应与原文件逐字节相同"
        );
        assert_eq!(std::fs::read(&a).expect("读原图"), orig, "原文件必须一字节不动");

        let _ = std::fs::remove_dir_all(a.parent().unwrap());
    }
}
