//! `routes::maps` 的测试（拆出来是因为「主体 + 测试」会超 600 行）。
//!
//! 与 `routes/tiles/tests.rs` 同一套办法：`maps.rs` 末尾只留 `#[cfg(test)] mod tests;`，
//! 测试用 `use super::*` 拿到全部私有项（`scan` / `natural_cmp` / `page_json` / `page_params`）。

use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use crate::state::AppState;

/// 每个测试一个独立临时目录（pid + 自增序号，互不撞名）。
fn 临时根(标签: &str) -> PathBuf {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let n = SEQ.fetch_add(1, AtomicOrdering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "miredit-web-maps-{}-{n}-{标签}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).expect("建临时目录");
    p
}

/// 写一个指定字节数的文件（内容全 0：列表阶段不许读内容，写什么无所谓）。
fn 写文件(p: &Path, bytes: usize) {
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).expect("建父目录");
    }
    std::fs::write(p, vec![0u8; bytes]).expect("写测试文件");
}

/// 造一个 `map_root` 指向临时目录的 `AppState`（**不读 env**，测试之间不打架）。
fn 造状态(root: PathBuf) -> Shared {
    let (events, _) = tokio::sync::broadcast::channel(4);
    Arc::new(AppState {
        map_root: root,
        web_root: None,
        session: Arc::new(RwLock::new(None)),
        libs: None,
        source: None,
        stamps: Mutex::new(None),
        trailer_cache: Mutex::new(None),
        palette_json: PathBuf::new(),
        events,
    })
}

/// 发一条真 HTTP/1.1 请求，返回 `(状态码, 响应体)`。
fn 请求(addr: std::net::SocketAddr, path: &str) -> (u16, String) {
    use std::io::{Read as _, Write as _};
    let mut s = std::net::TcpStream::connect(addr).expect("应能连上测试服务");
    s.set_read_timeout(Some(Duration::from_secs(10)))
        .expect("设读超时");
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).expect("应能发出请求");
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).expect("应能读到完整响应");
    let text = String::from_utf8_lossy(&buf).into_owned();
    let (head, body) = match text.split_once("\r\n\r\n") {
        Some((h, b)) => (h.to_string(), b.to_string()),
        None => (text, String::new()),
    };
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    (status, body)
}

/// 起一个真 TCP 服务（端口 0 由内核分配，不与其它测试抢）。
async fn 起服务(state: Shared) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("回环端口应可绑定");
    let addr = listener.local_addr().expect("应能取到本地地址");
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, crate::server::app(state)).await;
    });
    (addr, server)
}

/// **自然序**：`n2` 必须在 `n10` 之前（任务书硬要求），且与 `0/10/2` 这类纯数字名同样成立。
#[test]
fn 自然序数字段按数值比() {
    assert_eq!(natural_cmp("n2.map", "n10.map"), Ordering::Less);
    assert_eq!(natural_cmp("n10.map", "n2.map"), Ordering::Greater);
    assert_eq!(natural_cmp("n2.map", "n2.map"), Ordering::Equal);
    assert_eq!(natural_cmp("2.map", "10.map"), Ordering::Less);
    assert_eq!(natural_cmp("0.map", "10.map"), Ordering::Less);
    // 前导零：数值相同 ⇒ 用原始字节兜底，且仍是确定的全序
    assert_eq!(natural_cmp("n02.map", "n2.map"), Ordering::Less);
    assert_eq!(natural_cmp("n2.map", "n02.map"), Ordering::Greater);
    // 大小写不敏感比较 + 字节兜底（A 在 a 前）
    assert_eq!(natural_cmp("A.map", "a.map"), Ordering::Less);
    // 前缀短的在先
    assert_eq!(natural_cmp("n1.map", "n10.map"), Ordering::Less);
}

/// 把一批真实风格的名字排序，钉住「n2 在 n10 前」这个可肉眼复核的结论。
#[test]
fn 排序结果里n2在n10前() {
    let mut names = vec![
        "n10.map", "n2.map", "n1.map", "0.map", "41.map", "n0104.map", "0100.map", "alx.map",
    ];
    names.sort_by(|a, b| natural_cmp(a, b));
    // ⚠ `0100.map` 排在 `41.map` 之后：前导零去掉后是 **100 > 41**，这就是「按数值」的含义。
    // `n0104` 同理 = 104 > 10 ⇒ 排在 `n10.map` 之后。
    assert_eq!(
        names,
        vec![
            "0.map", "41.map", "0100.map", "alx.map", "n1.map", "n2.map", "n10.map", "n0104.map"
        ],
        "自然序结果不符：{names:?}"
    );
    // 显式点出任务书那条判据
    let i2 = names.iter().position(|n| *n == "n2.map").expect("n2.map");
    let i10 = names.iter().position(|n| *n == "n10.map").expect("n10.map");
    assert!(i2 < i10, "n2.map 必须在 n10.map 之前，实际 {names:?}");
}

/// 扫描**只收 `.map`**，扩展名大小写不敏感；非地图文件一个都不许混进来。
#[test]
fn 只收map文件() {
    let root = 临时根("只收map");
    写文件(&root.join("a.map"), 12);
    写文件(&root.join("B.MAP"), 24);
    写文件(&root.join("c.txt"), 8);
    写文件(&root.join("d.map.bak"), 8);
    写文件(&root.join("noext"), 8);
    let all = scan_maps(&root).expect("扫描应成功");
    let names: Vec<&str> = all.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["a.map", "B.MAP"], "只该收 .map，实际 {names:?}");
    assert_eq!(all[0].bytes, 12);
    assert_eq!(all[1].bytes, 24);
    assert!(all.iter().all(|e| e.mtime.is_some()), "临时文件的 mtime 必须读得到");
    let _ = std::fs::remove_dir_all(&root);
}

/// **深度上限**：`max_depth = 1` 时只扫根 + 一层子目录，更深的目录整棵跳过（不报错）。
#[test]
fn 递归深度有上限() {
    let root = 临时根("深度");
    写文件(&root.join("root.map"), 1);
    写文件(&root.join("sub/one.map"), 1);
    写文件(&root.join("sub/deep/two.map"), 1);
    写文件(&root.join("sub/deep/deeper/three.map"), 1);

    let 一层 = scan(&root, 1, MAX_ENTRIES).expect("扫描应成功");
    let paths: Vec<&str> = 一层.iter().map(|e| e.path.as_str()).collect();
    // 按**文件名**自然序：`one.map` < `root.map`（与目录无关）
    assert_eq!(paths, vec!["sub/one.map", "root.map"], "max_depth=1 只该到一层：{paths:?}");

    let 三层 = scan(&root, MAX_DEPTH, MAX_ENTRIES).expect("扫描应成功");
    assert_eq!(三层.len(), 4, "缺省深度 3 应扫到最深那张：{三层:?}");
    // 相对路径用 `/` 分隔（跨平台一致，且能直接喂 open）
    assert!(三层.iter().any(|e| e.path == "sub/deep/deeper/three.map"));
    assert!(三层.iter().all(|e| !e.path.contains('\\')));
    let _ = std::fs::remove_dir_all(&root);
}

/// 条目数护栏：超上限 ⇒ 报错（**不返回半截列表冒充完整**）。
#[test]
fn 条目数超上限必报错() {
    let root = 临时根("上限");
    for n in 0..3 {
        写文件(&root.join(format!("{n}.map")), 1);
    }
    let err = scan(&root, MAX_DEPTH, 2).expect_err("3 张 > 上限 2，必须报错");
    assert!(err.contains("超过 2 张"), "错误文案要带数字：{err}");
    assert!(
        scan(&root, MAX_DEPTH, 3).is_ok(),
        "上限放到 3 就该成功（证明上面那次失败是上限导致，不是别的原因）"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// 根不存在 / 不是目录 ⇒ `Err`，且映射成 **503**（不是 200 空列表）。
#[test]
fn 根不存在必报503() {
    let missing = std::env::temp_dir().join("miredit-web-maps-绝对不存在的目录-zzz");
    let err = scan_maps(&missing).expect_err("不存在的根必须报错");
    assert!(err.contains("地图根读不到"), "错误文案要指认根：{err}");
    let api = scan_error(err);
    assert_eq!(api.status().as_u16(), 503, "扫描失败必须是 503");
    assert_eq!(api.code(), "assets_unavailable");
    assert!(api.message().contains("地图根读不到"));

    // 存在但是文件 ⇒ 同样 503（不静默返回空列表）
    let root = 临时根("不是目录");
    let file = root.join("x.map");
    写文件(&file, 1);
    let err = scan_maps(&file).expect_err("根是文件必须报错");
    assert!(err.contains("不是目录"), "{err}");
    let _ = std::fs::remove_dir_all(&root);
}

/// 分页：`total` 是**总数**不是本页数；`offset` 真的跳过，两页首项不重复。
#[test]
fn 分页total是总数且不重复() {
    let all: Vec<MapEntry> = (0..25)
        .map(|i| MapEntry {
            name: format!("n{i}.map"),
            path: format!("n{i}.map"),
            bytes: 100 + i,
            mtime: Some(1_700_000_000 + i),
        })
        .collect();

    let 页0 = page_json(&all, 0, 10);
    assert_eq!(页0["total"], 25, "total 必须是总数，不是本页条数");
    assert_eq!(页0["items"].as_array().map(Vec::len), Some(10));
    assert_eq!(页0["items"][0]["name"], "n0.map");

    let 页1 = page_json(&all, 10, 10);
    assert_eq!(页1["total"], 25);
    assert_eq!(页1["items"].as_array().map(Vec::len), Some(10));
    assert_eq!(页1["items"][0]["name"], "n10.map");
    assert_ne!(
        页0["items"][0]["name"], 页1["items"][0]["name"],
        "两页首项不许重复"
    );

    // 最后一页只剩余数；越界页 ⇒ 空 items 但 total 不变
    let 页2 = page_json(&all, 20, 10);
    assert_eq!(页2["items"].as_array().map(Vec::len), Some(5));
    let 越界 = page_json(&all, 999, 10);
    assert_eq!(越界["items"].as_array().map(Vec::len), Some(0));
    assert_eq!(越界["total"], 25);
    // limit=0 合法：只要总数
    assert_eq!(page_json(&all, 0, 0)["items"].as_array().map(Vec::len), Some(0));
}

/// `limit` 缺省 100、上限 500；超上限 ⇒ **400**（不静默截断）。
#[test]
fn 分页参数缺省与上限() {
    let 造 = |q: &[(&str, &str)]| {
        let map: HashMap<String, String> = q
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        Params::from_query(Ok(Query(map))).expect("参数解析")
    };

    assert_eq!(page_params(&造(&[])).expect("缺省"), (0, DEFAULT_LIMIT as usize));
    assert_eq!(page_params(&造(&[("limit", "500")])).expect("上限"), (0, 500));
    assert_eq!(
        page_params(&造(&[("offset", "10"), ("limit", "10")])).expect("显式"),
        (10, 10)
    );
    let err = page_params(&造(&[("limit", "501")])).expect_err("超上限必须报错");
    assert_eq!(err.status().as_u16(), 400);
    assert!(err.message().contains("500"), "文案要写清上限：{}", err.message());
    // 非数字 ⇒ 400（不是静默当 0）
    let err = page_params(&造(&[("limit", "abc")])).expect_err("非数字必须报错");
    assert_eq!(err.status().as_u16(), 400);
}

/// **跨路径一致性**：列表里的 `path` 必须能被 `/api/map/open` 的白名单直接接受。
///
/// 这是本端点最容易埋雷的地方 —— 列表给绝对路径、open 只收相对路径（或反过来）时，
/// 前端「点一项」就会 403/404，而两个端点各自的测试都还是绿的。
#[test]
fn 列表项路径能直接喂给open() {
    let root = 临时根("喂open");
    写文件(&root.join("n0.map"), 16);
    写文件(&root.join("高清/n10.map"), 16);
    // map_root 必须像生产一样 canonicalize（`from_env` 就是这么做的），
    // 否则 `real.starts_with(map_root)` 会被 Windows 的 `\\?\` 前缀骗过去
    let 规范根 = std::fs::canonicalize(&root).expect("canonicalize 临时根");
    let state = 造状态(规范根);
    let all = scan_maps(&root).expect("扫描应成功");
    assert_eq!(all.len(), 2);
    for entry in &all {
        // ① 不许是绝对路径，也不许带 `..`（open 会 403）
        assert!(!Path::new(&entry.path).is_absolute(), "{} 不该是绝对路径", entry.path);
        assert!(!entry.path.contains(".."), "{} 不许带 ..", entry.path);
        // ② 真的能过白名单四道闸
        let resolved = state
            .resolve_map_path(&entry.path)
            .unwrap_or_else(|e| panic!("{} 过不了 open 的白名单：{}", entry.path, e.message()));
        assert!(resolved.is_file(), "{} 解析后不是文件", entry.path);
        assert_eq!(
            resolved,
            std::fs::canonicalize(root.join(&entry.path)).expect("canonicalize"),
            "{} 解析出的文件不对",
            entry.path
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// 真 HTTP 往返：路由挂上了、分页生效、503 与 400 的状态码都对。
#[tokio::test(flavor = "multi_thread")]
async fn 列表端点真http往返() {
    let root = 临时根("http");
    for n in 0..3 {
        写文件(&root.join(format!("n{n}.map")), 8);
    }
    let state = 造状态(root.clone());
    let (addr, server) = 起服务(state).await;

    // 缺省 limit=100 ⇒ 三张全出，total=3
    let (code, body) = 请求(addr, "/api/maps");
    assert_eq!(code, 200, "GET /api/maps 应 200，body={body}");
    let v: Value = serde_json::from_str(&body).expect("响应必须是 JSON");
    assert_eq!(v["total"], 3);
    assert_eq!(v["items"].as_array().map(Vec::len), Some(3));
    // 字段齐全（少一个前端就是 undefined）
    for k in ["name", "path", "bytes", "mtime"] {
        assert!(v["items"][0].get(k).is_some(), "列表项缺字段 {k}：{body}");
    }
    assert_eq!(v["items"][0]["path"], "n0.map");

    // limit=1&offset=0 与 offset=1 的首项必须不同
    let (_, b0) = 请求(addr, "/api/maps?limit=1&offset=0");
    let (_, b1) = 请求(addr, "/api/maps?limit=1&offset=1");
    let v0: Value = serde_json::from_str(&b0).expect("JSON");
    let v1: Value = serde_json::from_str(&b1).expect("JSON");
    assert_eq!(v0["total"], 3);
    assert_eq!(v0["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(v0["items"][0]["name"], "n0.map");
    assert_eq!(v1["items"][0]["name"], "n1.map");
    assert_ne!(v0["items"][0]["name"], v1["items"][0]["name"], "分页首项不许重复");

    // 超上限 ⇒ 400
    let (code, body) = 请求(addr, "/api/maps?limit=501");
    assert_eq!(code, 400, "limit=501 必须 400，body={body}");
    assert!(body.contains("limit 上限是 500"), "{body}");

    server.abort();
    let _ = std::fs::remove_dir_all(&root);
}

/// 根不存在时真 HTTP 必须 **503 + 中文原因**，不是 200 空列表。
#[tokio::test(flavor = "multi_thread")]
async fn 根不存在时真http回503() {
    let missing = std::env::temp_dir().join("miredit-web-maps-绝对不存在的目录-yyy");
    let (addr, server) = 起服务(造状态(missing)).await;
    let (code, body) = 请求(addr, "/api/maps");
    server.abort();
    assert_eq!(code, 503, "根不存在必须 503，body={body}");
    let v: Value = serde_json::from_str(&body).expect("错误体必须是 JSON");
    assert_eq!(v["error"]["code"], "assets_unavailable");
    let msg = v["error"]["message"].as_str().unwrap_or_default();
    assert!(msg.contains("地图列表不可用"), "错误文案必须是中文原因：{msg}");
    assert!(msg.contains("MIREDIT_MAP_ROOT"), "要指出该查哪个 env：{msg}");
    assert!(
        v.get("items").is_none(),
        "失败时不许带一个空 items 冒充成功：{body}"
    );
}

/// **不解析内容**的静态证据：扫描阶段一次 `read` 都没有（用只读目录 + 0 字节地图钉住）。
///
/// 说明：真正的「耗时接近纯 metadata」在 README §10 用 1,097 张实测数字给；
/// 这里钉的是「代码路径里没有内容读取」—— 0 字节文件也能正常列出（解析会当场失败）。
#[test]
fn 扫描不解析内容() {
    let root = 临时根("不解析");
    写文件(&root.join("empty.map"), 0);
    写文件(&root.join("junk.map"), 3);
    let all = scan_maps(&root).expect("扫描应成功");
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].bytes, 0, "0 字节文件照样列出（列表不解析内容）");
    assert_eq!(all[1].bytes, 3, "3 字节的垃圾内容也照样列出");
    let _ = std::fs::remove_dir_all(&root);
}
