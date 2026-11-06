//! 验收门 5：**保存原子性**（正常保存 + 失败时原文件不变）。
//!
//! 跑法：`cargo test -p miredit-core --test save_atomic -- --nocapture --test-threads=1`
//!
//! 判据（任务书 §六.5）：
//! 1. **正常保存** ⇒ 读回核对通过（磁盘内容 == `to_bytes()`）、脏标记清零、临时文件不残留；
//! 2. **目标是非空目录** ⇒ 保存失败（停在「原子替换」阶段）且原目标（目录 + 里面的文件）
//!    一字节不变、临时文件被清掉。
//!
//! ⚠ 所有写操作都发生在 `%TEMP%` 下的临时目录里；**真图只被读取**（末尾再读一次比对）。

use std::path::{Path, PathBuf};

use miredit_core::{
    apply, save_atomic, save_session, temp_path_for, Command, CoreError, EditSession, LibRef,
    SaveOutcome, SaveStage,
};

/// 资产根缺省值：本机真实根（**整个文件里只出现这一处**）。换机器用 `MIREDIT_ASSET_ROOT`。
const 缺省资产根: &str = r"E:\GodM2";

/// 资产根：`MIREDIT_ASSET_ROOT` → 缺省 [`缺省资产根`]。
///
/// 根目录不存在（公开仓/PR 的机器上没有游戏素材）⇒ `None`，调用方打印 `[skip]` 并返回。
/// 只做 `var_os` + `PathBuf`，不引入任何第三方库。
fn 资产根() -> Option<PathBuf> {
    let root = std::env::var_os("MIREDIT_ASSET_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(缺省资产根));
    root.is_dir().then_some(root)
}

/// 真值地图 `0.map`（23 周年客户端 / Old12 / 700×700）。
///
/// **缺资产根或文件 ⇒ `None`**（公开仓/PR 的机器上没有游戏素材），调用方打印 `[skip]` 并返回。
fn 真图路径() -> Option<PathBuf> {
    let root = 资产根()?;
    let p = root
        .join("client")
        .join("热血传奇23周年客户端")
        .join("Map")
        .join("0.map");
    p.is_file().then_some(p)
}

/// 读真图字节；缺资产 ⇒ `None`（**不再 panic**：没有素材不算代码错）。
fn 读真图() -> Option<Vec<u8>> {
    let p = 真图路径()?;
    match std::fs::read(&p) {
        Ok(b) => Some(b),
        Err(e) => {
            eprintln!("[skip] 真值地图读不到：{} ⇒ {e}", p.display());
            None
        }
    }
}
/// 取文件 `mtime`；**取不到就失败**，绝不允许静默跳过（审查报告 `2026-11-05` 第 7 轮 · 缺陷 7）。
///
/// # 为什么必须 `panic!` 而不是 `if let (Some, Some)`
///
/// 「没改动就不许重写文件」这条判据的**唯一**证据就是 `mtime` 不变。原实现是
/// `if let (Some(a), Some(b)) = (写前, …modified().ok())`：任一 `modified()` 取不到，
/// 整条断言**不执行**、用例照样绿 —— 这是"条件恒真"的假绿形态，等于判据静默消失。
/// 现在改成取不到就 `panic!`：宁可红，也不许悄悄把判据丢掉。
///
/// # 平台跳过只能**显式**
///
/// 若日后确认某平台的 `modified()` 确实不可靠，**只能**二选一，且必须让人看得见：
/// 1. 在该用例上加 `#[cfg_attr(<平台>, ignore = "<原因>")]`（`cargo test` 会打印
///    `ignored` 与该原因）；
/// 2. 在本函数里 `eprintln!("本平台跳过：<原因>")` 后返回 `None`，调用处显式处理 `None`
///    （**仍然会打印**，不是静默）。
///
/// **禁止**改回 `if let (Some, Some)` —— 那就是本缺陷本身。
fn 取mtime(p: &Path, 时机: &str) -> std::time::SystemTime {
    match std::fs::metadata(p).and_then(|m| m.modified()) {
        Ok(t) => t,
        Err(e) => panic!(
            "取不到 mtime（{时机}）：{} ⇒ {e}\n\
             这条判据是「没改动就不许重写文件」的唯一证据，取不到就**必须失败**，不许静默跳过。\n\
             若本平台 mtime 确实不可靠，请在该用例上加 `#[cfg_attr(<平台>, ignore = \"<原因>\")]`，\
             不要改回 `if let (Some, Some)`。",
            p.display()
        ),
    }
}

/// 临时目录（`%TEMP%` 下，按 `tag` + **进程 ID** 分开；每次先清空）。
///
/// # 为什么目录名必须带 PID（⚠ 别改回去）
///
/// 两个 `cargo test` 进程并发跑同一个测试目标时，**固定目录名**会让两边互相
/// `remove_dir_all`、并抢同一个临时文件，产生**假红**（实测症状：
/// 「写入不完整：期望 5880052 字节，读回 480052」）。
/// 加上 [`std::process::id`] 之后两个进程各用各的目录，并发互不干扰；
/// `tag` 仍然保留，便于在 `%TEMP%` 里按用例名排查。
fn 临时目录(tag: &str) -> PathBuf {
    let pid = std::process::id();
    let d = std::env::temp_dir().join(format!("miredit_core_accept_{tag}_{pid}"));
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
    let 期望名 = format!("miredit_core_accept_{tag}_{本进程}");
    assert_eq!(
        实际名,
        期望名.as_str(),
        "临时目录名必须逐字等于「miredit_core_accept_<tag>_<本进程 PID>」，否则并发隔离失效：{}",
        d.display()
    );
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

/// **门 5 之一：正常保存** —— 写→刷→读回核对→rename 全过，磁盘内容与内存字节逐字节相等。
#[test]
fn 门5之一_正常保存读回核对通过() {
    let Some(原) = 读真图() else {
        eprintln!("[skip] 无资产根 MIREDIT_ASSET_ROOT（或缺 Map/0.map）—— 本用例 skip（不 panic）");
        return;
    };
    let d = 临时目录("save_ok");
    let p = d.join("0.map");
    std::fs::write(&p, &原).expect("先把真图拷一份到临时目录");

    let doc = miredit_core::Document::load(&p).expect("副本应能解析");
    let mut ed = EditSession::new(doc);
    miredit_core::set_brush_from_palette(&mut ed, LibRef::ground(0), 9);
    assert_eq!(
        apply(&mut ed, Command::Paint { x: 0, y: 0 }).unwrap().changed,
        4
    );
    assert!(ed.dirty(), "改过就该脏");
    let 期望 = ed.map().to_bytes();
    assert_ne!(期望, 原, "改完必须与原文件不同（否则下面的核对是假的）");

    // 走命令（网页版同一条路径）
    let out = apply(&mut ed, Command::Save).expect("保存应成功");
    assert_eq!(out.changed, 0, "保存不改地图");
    assert!(!out.dirty, "保存成功后必须不脏");
    let note = out.note.expect("保存必须有回执");
    assert!(note.contains("已写入"), "{note}");

    // ① 读回核对：磁盘内容 == 内存字节
    let 盘上 = std::fs::read(&p).expect("读回");
    assert_eq!(盘上, 期望, "磁盘内容必须与 to_bytes() 逐字节相等");
    assert_eq!(盘上.len(), 原.len(), "长度不变");
    assert_ne!(盘上, 原, "磁盘上确实是新内容");
    // ② 脏标记 / 保存点
    assert!(!ed.dirty());
    assert_eq!(ed.save_error(), None);
    assert_eq!(ed.saved_pos(), ed.pos());
    // ③ 临时文件不残留
    assert!(!temp_path_for(&p).exists(), "临时文件必须已被 rename 掉");
    // ④ 会话里记的保存结果
    assert!(matches!(ed.last_save(), Some(Ok(()))));

    // ⑤ 目录 fsync 的如实分类（Windows 上大概率 FlushFailed，属已知平台限制，不算失败）
    let 直写 = save_atomic(&p, &期望).expect("再写一次也必须成功");
    eprintln!(
        "SAVE_OK path={} bytes={} dir_sync={:?} note={}",
        p.display(),
        期望.len(),
        直写.dir_sync,
        note
    );
    assert_eq!(
        直写.dir_sync.dir(),
        d.as_path(),
        "报出的目录必须是目标的父目录"
    );
    assert!(!直写.dir_sync.note().is_empty(), "目录 fsync 必须有话说");

    // 真图一个字节都没动
    assert_eq!(读真图(), Some(原.clone()), "真图只许读，不许写");
    let _ = std::fs::remove_dir_all(&d);
}

/// **门 5 之二：目标是非空目录** ⇒ 失败且原目标不变、临时文件被清掉。
///
/// 为什么用"非空目录"当失败源：`MoveFileExW`（`std::fs::rename` 在 Windows 上的实现）
/// **不能**把文件覆盖到目录上，必然返回 `ERROR_ACCESS_DENIED` —— 与只读位/账户策略无关，
/// 是**确定性**失败（`save.rs:401-417` 的同一手法）。
#[test]
fn 门5之二_目标是非空目录时失败且原目标不变() {
    let d = 临时目录("save_dirfail");
    // 目标路径 = 非空目录
    let p = d.join("c.map");
    std::fs::create_dir_all(&p).expect("把目标路径做成目录");
    let 标记 = p.join("keep.txt");
    let 原 = b"ORIGINAL-BYTES".to_vec();
    std::fs::write(&标记, &原).expect("往目录里放个标记文件");

    // ① 直接走 save_atomic
    let err = save_atomic(&p, b"NEW-CONTENT").expect_err("目录挡着时 rename 必须失败");
    match &err {
        CoreError::Save { stage, path, tmp, .. } => {
            assert_eq!(*stage, SaveStage::Rename, "必须停在替换阶段：{err}");
            assert_eq!(path, &p);
            assert!(
                tmp.to_string_lossy().ends_with(miredit_core::TEMP_SUFFIX),
                "报错里要有临时文件路径：{err}"
            );
        }
        other => panic!("应报 Save，实际 {other:?}"),
    }
    assert!(p.is_dir(), "目标目录必须还在（不许被先删后写毁掉）");
    assert_eq!(
        std::fs::read(&标记).expect("标记文件还在"),
        原,
        "目标里的内容必须一字节不动"
    );
    assert!(!temp_path_for(&p).exists(), "替换失败后临时文件必须清掉");

    // ② 走命令（`Command::Save` 在失败时必须 `Err`，且会话记下原因）
    let 小图 = {
        // 造一张 2×2 Old12 图当"原文件"，再用一个同名目录占住它的路径 ——
        // 这里换个做法：目标仍是那个目录，会话的 path 指向它。
        let mut bytes = vec![0u8; 52 + 12 * 4];
        bytes[0..2].copy_from_slice(&2u16.to_le_bytes());
        bytes[2..4].copy_from_slice(&2u16.to_le_bytes());
        bytes
    };
    let 会话图 = d.join("d.map");
    std::fs::write(&会话图, &小图).expect("先写一张合法小图");
    let doc = miredit_core::Document::load(&会话图).expect("小图应能解析");
    let mut ed = EditSession::new(doc);
    let _ = ed.set_brush_from_palette(LibRef::ground(0), 0);
    // 2×2 图：尺寸 3 的笔刷在 (0,0) 覆盖全部 4 格
    assert_eq!(apply(&mut ed, Command::Paint { x: 0, y: 0 }).unwrap().changed, 4);
    // 把目标换成非空目录（模拟"目标不可替换"）
    std::fs::remove_file(&会话图).unwrap();
    std::fs::create_dir_all(&会话图).unwrap();
    std::fs::write(会话图.join("keep2.txt"), b"KEEP").unwrap();

    let err = apply(&mut ed, Command::Save).expect_err("目标不可替换时必须 Err");
    assert!(matches!(err, CoreError::Save { .. }), "{err}");
    assert!(ed.dirty(), "保存失败后必须还是脏的（不许假装存上了）");
    assert!(ed.save_error().is_some(), "失败原因必须留下来给人看");
    assert_eq!(
        std::fs::read(会话图.join("keep2.txt")).unwrap(),
        b"KEEP",
        "目标目录里的内容一字节不动"
    );
    assert!(!temp_path_for(&会话图).exists(), "临时文件必须清掉");

    eprintln!(
        "SAVE_FAIL stage={:?} 目标仍是目录={} 标记未变={} 临时残留={} 会话仍脏={}",
        SaveStage::Rename,
        p.is_dir(),
        std::fs::read(&标记).unwrap() == 原,
        temp_path_for(&p).exists(),
        ed.dirty(),
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// **门 5 之三（补充）**：写临时文件阶段就失败 ⇒ 原文件一字节不动、停在「写临时文件」阶段。
#[test]
fn 门5之三_写临时文件失败时原文件不变() {
    let d = 临时目录("save_tempfail");
    let p = d.join("b.map");
    let 原 = b"ORIGINAL-BYTES".to_vec();
    std::fs::write(&p, &原).expect("先写原文件");
    // 占住临时路径（目录不能当文件写）
    std::fs::create_dir_all(temp_path_for(&p)).expect("把临时路径做成目录");

    let err = save_atomic(&p, b"NEW").expect_err("临时路径不可写时必须失败");
    match &err {
        CoreError::Save { stage, .. } => {
            assert_eq!(*stage, SaveStage::Temp, "必须停在写临时文件阶段：{err}");
        }
        other => panic!("应报 Save，实际 {other:?}"),
    }
    assert_eq!(std::fs::read(&p).unwrap(), 原, "原文件必须一字节不动");
    let _ = std::fs::remove_dir_all(&d);
}

/// **门 5 之四（补充）**：没有修改时 `Save` 一个字节都不写，且如实回执"未写盘"。
#[test]
fn 门5之四_没有修改时不写盘() {
    let d = 临时目录("save_clean");
    let p = d.join("e.map");
    let mut bytes = vec![0u8; 52 + 12 * 4];
    bytes[0..2].copy_from_slice(&2u16.to_le_bytes());
    bytes[2..4].copy_from_slice(&2u16.to_le_bytes());
    std::fs::write(&p, &bytes).unwrap();
    // 写前时间戳：**取不到就失败**，不许静默跳过（审查报告 2026-11-05 第 7 轮 · 缺陷 7）。
    let 写前 = 取mtime(&p, "写盘前");

    let doc = miredit_core::Document::load(&p).unwrap();
    let mut ed = EditSession::new(doc);
    assert!(!ed.dirty());
    let out = apply(&mut ed, Command::Save).unwrap();
    assert_eq!(out.changed, 0);
    assert!(!out.dirty);
    assert_eq!(out.note.as_deref(), Some("没有未保存的修改，未写盘"));
    // 内容没变、临时文件也没出现
    assert_eq!(std::fs::read(&p).unwrap(), bytes);
    assert!(!temp_path_for(&p).exists());
    // mtime 判据：取不到就红（`取mtime` 内 panic），**不是** `if let` 静默跳过。
    let 写后 = 取mtime(&p, "Save 之后");
    assert_eq!(写前, 写后, "没改动就不该重写文件（mtime 不许变）");
    let _ = std::fs::remove_dir_all(&d);
}

/// `save_session` 在 `Clean` 时返回 [`SaveOutcome::Clean`]（不写盘）。
#[test]
fn save_session_干净时返回clean() {
    let d = 临时目录("save_session_clean");
    let p = d.join("f.map");
    let mut bytes = vec![0u8; 52 + 12 * 4];
    bytes[0..2].copy_from_slice(&2u16.to_le_bytes());
    bytes[2..4].copy_from_slice(&2u16.to_le_bytes());
    std::fs::write(&p, &bytes).unwrap();
    let doc = miredit_core::Document::load(&p).unwrap();
    let mut ed = EditSession::new(doc);
    let out = save_session(&mut ed).expect("干净时不该报错");
    assert_eq!(out, SaveOutcome::Clean);
    assert!(!out.wrote());
    assert_eq!(out.note(), "没有未保存的修改，未写盘");
    // 路径型的健全性检查：Clean 分支里临时文件从未被创建
    assert!(!temp_path_for(&p).exists());
    let _ = std::fs::remove_dir_all(&d);
}
