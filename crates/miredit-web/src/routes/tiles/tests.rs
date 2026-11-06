use super::*;

/// 把 `(&str, &str)` 列表包成 handler 用的查询参数。
fn mk(pairs: &[(&str, &str)]) -> Params {
    let m: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    Params::from_query(Ok(Query(m))).expect("查询参数应可解析")
}

#[test]
fn 图层两种写法等价() {
    let by_name = parse_layers(Some("ground,middle,front")).expect("层名");
    let by_bits = parse_layers(Some("7")).expect("位掩码");
    assert_eq!(by_name, by_bits);
    assert_eq!(by_name, LayerMask::all());
    // 只开地表
    let only_ground = parse_layers(Some("ground")).expect("只开地表");
    assert!(only_ground.ground && !only_ground.middle && !only_ground.front);
    assert_eq!(parse_layers(Some("1")).expect("1"), only_ground);
    // 全关
    assert_eq!(parse_layers(Some("none")).expect("none"), LayerMask::none());
    assert_eq!(parse_layers(Some("0")).expect("0"), LayerMask::none());
    // 缺省 = 全开
    assert_eq!(parse_layers(None).expect("缺省"), LayerMask::all());
}

#[test]
fn 图层非法一律400() {
    for bad in ["8", "shadow", "ground,sky"] {
        let err = parse_layers(Some(bad)).expect_err(bad);
        assert_eq!(err.status(), axum::http::StatusCode::BAD_REQUEST, "{bad}");
    }
}

#[test]
fn 阻挡两种写法等价且缺省全关() {
    assert_eq!(
        parse_collision(None).expect("缺省"),
        CollisionOverlay::none()
    );
    assert_eq!(
        parse_collision(Some("low,high")).expect("层名"),
        CollisionOverlay::all()
    );
    assert_eq!(
        parse_collision(Some("3")).expect("位掩码"),
        CollisionOverlay::all()
    );
    assert!(parse_collision(Some("1")).expect("1").low);
    assert!(!parse_collision(Some("1")).expect("1").high);
    assert!(parse_collision(Some("4")).is_err());
}

#[test]
fn 视口参数校验() {
    let vp = parse_viewport(&mk(&[("x", "0"), ("y", "0"), ("w", "64"), ("h", "48")]))
        .expect("合法视口");
    assert_eq!((vp.cells_w, vp.cells_h), (64, 48));
    assert_eq!(vp.pixel_size(), Some((64 * 48, 48 * 32)));
    // w 越界
    assert!(parse_viewport(&mk(&[("w", "65")])).is_err());
    assert!(parse_viewport(&mk(&[("w", "0")])).is_err());
    // 非数字
    assert!(parse_viewport(&mk(&[("x", "abc")])).is_err());
    // scale **不再由** parse_viewport 管（挪到 parse_scale，四档全合法）
    assert!(parse_viewport(&mk(&[("scale", "2")])).is_ok());
}

#[test]
fn 倍率四档与别名大小写不敏感() {
    // 缺省 = 1（= 改动前的行为）
    assert_eq!(parse_scale(&mk(&[])).expect("缺省"), RenderScale::One);
    // 数字写法
    assert_eq!(
        parse_scale(&mk(&[("scale", "0.25")])).expect("0.25"),
        RenderScale::Quarter
    );
    assert_eq!(
        parse_scale(&mk(&[("scale", "0.5")])).expect("0.5"),
        RenderScale::Half
    );
    assert_eq!(
        parse_scale(&mk(&[("scale", "1")])).expect("1"),
        RenderScale::One
    );
    assert_eq!(
        parse_scale(&mk(&[("scale", "2")])).expect("2"),
        RenderScale::Two
    );
    // 名字写法 + 大小写不敏感（Quarter / HALF / One / tWo）
    for (raw, want) in [
        ("quarter", RenderScale::Quarter),
        ("Quarter", RenderScale::Quarter),
        ("QUARTER", RenderScale::Quarter),
        ("half", RenderScale::Half),
        ("HALF", RenderScale::Half),
        ("One", RenderScale::One),
        ("tWo", RenderScale::Two),
        (" 0.25 ", RenderScale::Quarter),
        ("1.0", RenderScale::One),
        ("2.0", RenderScale::Two),
    ] {
        assert_eq!(parse_scale(&mk(&[("scale", raw)])).expect(raw), want, "{raw}");
    }
}

#[test]
fn 非法倍率400且列出合法取值() {
    for bad in ["3", "0.3", "1.5", "2x", "abc", "-1"] {
        let e = parse_scale(&mk(&[("scale", bad)])).expect_err(bad);
        assert_eq!(e.status(), axum::http::StatusCode::BAD_REQUEST, "{bad}");
        assert_eq!(e.code(), "bad_request", "{bad}");
        // 错误消息必须列全四个合法取值，并带上收到的原文
        for must in ["0.25", "0.5", "quarter", "half", "one", "two", bad] {
            assert!(e.message().contains(must), "{bad} 的文案缺 {must}：{}", e.message());
        }
    }
}

#[test]
fn 超限映射413其余映射400() {
    let vp = Viewport::new(0, 0, 64, 48);
    // 目标图超限 ⇒ 413
    let e = render_error_to_api(
        RenderError::TooLarge {
            want: 75_497_472,
            limit: 67_108_864,
        },
        &vp,
        RenderScale::Two,
    );
    assert_eq!(e.status(), axum::http::StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(e.code(), "too_large");
    for must in ["75497472", "67108864", "72.0 MiB", "64 MiB", "缩小视口格数", "降低倍率"] {
        assert!(e.message().contains(must), "缺 {must}：{}", e.message());
    }
    // 1× 中间图超限 ⇒ 同样 413（不是 400，更不是 500）
    let e = render_error_to_api(
        RenderError::IntermediateTooLarge {
            want: 184_320_000,
            limit: 134_217_728,
        },
        &vp,
        RenderScale::Quarter,
    );
    assert_eq!(e.status(), axum::http::StatusCode::PAYLOAD_TOO_LARGE);
    assert!(e.message().contains("184320000") || e.message().contains("184_320_000"));
    assert!(e.message().contains("缩小视口格数"), "{}", e.message());
    // 其它渲染错误仍是 400
    let e = render_error_to_api(RenderError::ZeroCells, &vp, RenderScale::Half);
    assert_eq!(e.status(), axum::http::StatusCode::BAD_REQUEST);
}

#[test]
fn 四档尺寸与公式一致() {
    // 验收门 §五.3 的口径：round(w*48*s) × round(h*32*s)
    for (w, h, s, want) in [
        (64u32, 48u32, RenderScale::Quarter, (768u32, 384u32)),
        (64, 48, RenderScale::Half, (1536, 768)),
        (64, 48, RenderScale::One, (3072, 1536)),
        (16, 12, RenderScale::Two, (1536, 768)),
    ] {
        assert_eq!(s.pixel_size(w, h), Some(want), "{}×{} @{}", w, h, s.label());
    }
}

#[test]
fn 二倍档在64x48必超限() {
    // 6144×3072×4 = 75,497,472 B > 64 MiB ⇒ 渲染库报 TooLarge（本 crate 映射成 413）
    let (w, h) = RenderScale::Two.pixel_size(64, 48).expect("尺寸可算");
    assert_eq!((w, h), (6144, 3072));
    assert!(u64::from(w) * u64::from(h) * 4 > 64 * 1024 * 1024);
    // 16×12 的 2× 则在护栏内（1536×768×4 = 4.7 MB）
    let (w, h) = RenderScale::Two.pixel_size(16, 12).expect("尺寸可算");
    assert!(u64::from(w) * u64::from(h) * 4 <= 64 * 1024 * 1024);
}

/* ================= 「开启门」（`doors`） + 「地图光照」（`lighting`） ================= */

/// 本机真图（与 `routes/mod.rs` 同一条口径；不存在就跳过，换机器也能跑）。
/// 路径**相对资产根** `MIREDIT_ASSET_ROOT`（缺省本机真实根）。
const 真地图: &str = "GodotClient/maps/0.map";

/// 资产根缺省值：本机真实根（**整个文件里只出现这一处**）。换机器用 `MIREDIT_ASSET_ROOT`。
const 缺省资产根: &str = r"E:\GodM2";

/// 资产根：`MIREDIT_ASSET_ROOT` → 缺省 [`缺省资产根`]；根不存在 ⇒ `None`。
///
/// 只做 `var_os` + `PathBuf`，不引入任何第三方库。
fn 资产根() -> Option<std::path::PathBuf> {
    let root = std::env::var_os("MIREDIT_ASSET_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(缺省资产根));
    root.is_dir().then_some(root)
}

/// 真实地图的**绝对路径**（相对段挂在资产根下）；缺根 ⇒ `None`。
fn 真地图路径() -> Option<std::path::PathBuf> {
    Some(资产根()?.join(真地图))
}

/// 打开真图 + 真图库；环境缺资源时返回 `None`（调用方 `let Some(..) = .. else { return }` 跳过）。
fn 真图与图源() -> Option<(mir2_map::Map, mir2_assets::WilSource)> {
    let Some(map_path_buf) = 真地图路径() else {
        eprintln!("[跳过] 无资产根 MIREDIT_ASSET_ROOT（需要真实地图素材）");
        return None;
    };
    let map_path = map_path_buf.as_path();
    if !map_path.is_file() {
        eprintln!("[跳过] 真实地图不存在：{}", map_path.display());
        return None;
    }
    let Some(root) = mir2_assets::client_root() else {
        eprintln!(
            "[跳过] 未设置 {}（图库扫描根）",
            mir2_assets::CLIENT_ROOT_ENV
        );
        return None;
    };
    let Ok(paths) = mir2_assets::LibPaths::discover(&root) else {
        eprintln!("[跳过] 图库根扫描失败：{}", root.display());
        return None;
    };
    if paths.is_empty() {
        eprintln!("[跳过] 图库根下一个库都没有：{}", root.display());
        return None;
    }
    let src = mir2_assets::WilSource::open(&paths).expect("图库打开");
    let bytes = std::fs::read(map_path).expect("读地图");
    let map = mir2_map::Map::parse(&bytes).expect("解析地图");
    Some((map, src))
}

/// 发一条 HTTP/1.1 GET（`Connection: close`），返回 `(状态行, 头文本小写, 响应体字节)`。
///
/// 头体按 `\r\n\r\n` 切开 —— PNG 是二进制，**不能**当字符串读（会被 UTF-8 有损替换，
/// 那样「逐字节相同」的断言就假了）。
fn http_get(addr: std::net::SocketAddr, path: &str) -> (String, String, Vec<u8>) {
    use std::io::{Read as _, Write as _};
    let mut s = std::net::TcpStream::connect(addr).expect("应能连上测试服务");
    let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(60)));
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).expect("应能发出请求");
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).expect("应能读到完整响应");
    let pos = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or_else(|| panic!("响应里没有头体分隔：{path}"));
    let head = String::from_utf8_lossy(&buf[..pos]).to_ascii_lowercase();
    let status = head.lines().next().unwrap_or_default().to_string();
    (status, head, buf[pos + 4..].to_vec())
}

/// 缺省 = 闭门；只认 `0` / `1`。本函数只验布尔解析，组合在 `两个开关一次组合不互相覆盖` 里钉住。
#[test]
fn 开启门缺省闭门且只认01() {
    assert!(!parse_doors_open(None).expect("缺省"));
    assert!(!parse_doors_open(Some("0")).expect("0"));
    assert!(parse_doors_open(Some("1")).expect("1"));
    // 与渲染库的构造函数同义（本 crate 不自己拼结构体字面量）
    assert!(RenderOptions::all_doors_open().doors_open);
    assert!(!RenderOptions::closed().doors_open);
    assert_eq!(RenderOptions::closed(), RenderOptions::default());
}

/// 非法值一律 400 + 中文原因，**不静默当成 0**。
#[test]
fn 开启门非法值400且列出合法取值() {
    for bad in ["2", "-1", "true", "yes", "01", "0.0", "1x"] {
        let e = parse_doors_open(Some(bad)).expect_err(bad);
        assert_eq!(e.status(), axum::http::StatusCode::BAD_REQUEST, "{bad:?}");
        assert_eq!(e.code(), "bad_request", "{bad:?}");
        for must in ["0", "1", "闭门", "门全开", "不静默当成 0", bad] {
            assert!(
                e.message().contains(must),
                "{bad:?} 的文案缺 {must}：{}",
                e.message()
            );
        }
    }
    // 空值（`?doors=`）也是「给了」，同样 400 —— 不是「没给」
    let e = parse_doors_open(Some("")).expect_err("空值");
    assert_eq!(e.status(), axum::http::StatusCode::BAD_REQUEST);
    assert!(e.message().contains("空值"), "空值必须显式说出来：{}", e.message());
}

/// `raw_verbatim` 与 `raw` 的**唯一**区别：空串不按「没给」处理。
///
/// 这条是本轮的关键口径：`?doors=` 若被当成缺省 0，就等于静默降级。
#[test]
fn 开启门空串按给了处理() {
    let 带空格 = mk(&[("doors", " 1 ")]);
    assert!(parse_doors_open(带空格.raw_verbatim("doors")).expect("trim 后合法"));

    let 空值 = mk(&[("doors", "")]);
    assert_eq!(空值.raw_verbatim("doors"), Some(""), "raw_verbatim 必须保留空串");
    assert!(parse_doors_open(空值.raw_verbatim("doors")).is_err(), "空串 ⇒ 400");
    // 对照：`raw` 会把空串当「没给」⇒ 缺省 0（`layers`/`collision` 的口径，doors 不用它）
    assert_eq!(空值.raw("doors"), None);

    // 完全没给 ⇒ 缺省 0
    let 没给 = mk(&[]);
    assert_eq!(没给.raw_verbatim("doors"), None);
    let 缺省 = RenderOptions::with_doors_and_lighting(
        parse_doors_open(没给.raw_verbatim("doors")).expect("缺省"),
        parse_lighting(没给.raw_verbatim("lighting")).expect("缺省白天"),
    );
    assert_eq!(doors_label(缺省), "0");
}

/// 响应头值与**真正传给渲染库**的那份选项同源。
#[test]
fn 开启门响应头与选项同源() {
    assert_eq!(doors_label(RenderOptions::closed()), "0");
    assert_eq!(doors_label(RenderOptions::all_doors_open()), "1");
    assert_eq!(
        doors_label(RenderOptions::with_doors_and_lighting(
            parse_doors_open(None).expect("缺省"),
            parse_lighting(None).expect("缺省白天"),
        )),
        "0"
    );
    assert_eq!(
        doors_label(RenderOptions::with_doors_and_lighting(
            parse_doors_open(Some("1")).expect("1"),
            parse_lighting(None).expect("缺省白天"),
        )),
        "1"
    );
}

/// **闭门四档与旧入口逐字节相同**（「缺省不变」的结构性证明，四档全覆盖）。
#[test]
fn 闭门四档与旧入口逐字节相同() {
    let Some((map, src)) = 真图与图源() else {
        return;
    };
    let vp = Viewport::new(106, 316, 3, 3);
    let mask = LayerMask::all();
    let none = CollisionOverlay::none();
    for 档 in [
        RenderScale::Quarter,
        RenderScale::Half,
        RenderScale::One,
        RenderScale::Two,
    ] {
        let 新 = render(&map, &src, &vp, &mask, none, 档, RenderOptions::closed())
            .expect("新入口（闭门）");
        let 旧 = if 档 == RenderScale::One {
            miredit_render::try_render_region_with(&map, &src, &vp, &mask, none)
        } else {
            miredit_render::try_render_region_scaled(&map, &src, &vp, &mask, none, 档)
        }
        .expect("旧入口");
        assert_eq!(
            (旧.width, 旧.height),
            (新.width, 新.height),
            "{} 档尺寸",
            档.label()
        );
        assert_eq!(旧.rgba, 新.rgba, "{} 档 RGBA 必须逐字节相同", 档.label());
    }
}

/// **`doors=1` 真的不同**：同一块 `106,316 3×3 @1×`，门开与闭门必须是两份不同的像素。
#[test]
fn 开启门与闭门出图不同() {
    let Some((map, src)) = 真图与图源() else {
        return;
    };
    let vp = Viewport::new(106, 316, 3, 3);
    let mask = LayerMask::all();
    let none = CollisionOverlay::none();
    let 闭 = render(&map, &src, &vp, &mask, none, RenderScale::One, RenderOptions::closed())
        .expect("闭门出图");
    let 开 = render(
        &map,
        &src,
        &vp,
        &mask,
        none,
        RenderScale::One,
        RenderOptions::all_doors_open(),
    )
    .expect("门全开出图");
    assert_eq!(
        (闭.width, 闭.height),
        (开.width, 开.height),
        "门开合不改尺寸"
    );
    assert_ne!(闭.rgba, 开.rgba, "这块区域含门格 ⇒ 门全开的像素必须不同");
    let a = encode_png(&闭).expect("闭门编码");
    let b = encode_png(&开).expect("门全开编码");
    assert_ne!(a, b, "PNG 也必须不同");
    assert!(a.len() > 256 && b.len() > 256, "PNG 不该是空壳");
    eprintln!(
        "[开启门实测] 106,316 3×3 @1×：doors=0 = {} B，doors=1 = {} B",
        a.len(),
        b.len()
    );
}

/// 门开合在**缩小档**同样生效（证明 `try_render_region_scaled_with` 真的把 options 传下去了）。
#[test]
fn 开启门在缩小档也生效() {
    let Some((map, src)) = 真图与图源() else {
        return;
    };
    let vp = Viewport::new(106, 316, 3, 3);
    let mask = LayerMask::all();
    let none = CollisionOverlay::none();
    let 闭 = render(&map, &src, &vp, &mask, none, RenderScale::Half, RenderOptions::closed())
        .expect("0.5 闭门");
    let 开 = render(
        &map,
        &src,
        &vp,
        &mask,
        none,
        RenderScale::Half,
        RenderOptions::all_doors_open(),
    )
    .expect("0.5 门全开");
    assert_eq!(闭.scale, RenderScale::Half);
    assert_eq!((闭.width, 闭.height), (开.width, 开.height));
    assert_ne!(闭.rgba, 开.rgba, "0.5 档门全开也必须不同");
}

/// 门开合**只动前景层**：只出地表（`layers=ground`）时 `doors=0/1` 必须逐字节相同
/// —— 否则就是门偏移串到别的层去了。
#[test]
fn 开启门只影响前景层() {
    let Some((map, src)) = 真图与图源() else {
        return;
    };
    let vp = Viewport::new(106, 316, 3, 3);
    let 只地表 = LayerMask {
        ground: true,
        middle: false,
        front: false,
    };
    let none = CollisionOverlay::none();
    let 闭 = render(&map, &src, &vp, &只地表, none, RenderScale::One, RenderOptions::closed())
        .expect("只地表闭门");
    let 开 = render(
        &map,
        &src,
        &vp,
        &只地表,
        none,
        RenderScale::One,
        RenderOptions::all_doors_open(),
    )
    .expect("只地表门全开");
    assert_eq!(闭.rgba, 开.rgba, "只出地表时门开合不该有任何差异");
}

/// 非法 `doors` 在 **HTTP 层**就是 400 —— 且**先于**「没开图（409）/ 没图库（503）」，
/// 因为参数校验在 handler 最前面。这条不依赖本机图库，换机器也必跑。
#[tokio::test(flavor = "multi_thread")]
async fn 非法doors返回400原文() {
    let state = crate::server::share(crate::state::AppState::from_env());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("回环端口应可绑定");
    let addr = listener.local_addr().expect("应能取到本地地址");
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, crate::server::app(state)).await;
    });

    let raw = std::thread::spawn(move || {
        let (status, head, body) = http_get(addr, "/api/tiles?x=0&y=0&w=2&h=2&doors=2");
        (status, head, String::from_utf8_lossy(&body).to_string())
    })
    .join()
    .expect("客户端线程不应 panic");
    server.abort();

    let (status, head, body) = raw;
    assert!(status.starts_with("http/1.1 400"), "非法 doors 必须是 400：{status}");
    assert!(
        head.contains("content-type: application/json; charset=utf-8"),
        "错误体必须是 JSON：{head}"
    );
    assert!(body.contains("\"code\":\"bad_request\""), "错误码必须是 bad_request：{body}");
    assert!(body.contains("doors 只支持"), "错误消息必须是中文原因：{body}");
    assert!(body.contains("收到：2"), "错误消息要带收到的原文：{body}");
}

/// **HTTP 层端到端**（真图 + 真图库 + 真 TCP）：
/// ① 不给 `doors` 与 `doors=0` 的**响应体逐字节相同**，响应头 `X-Render-Doors: 0`；
/// ② `doors=1` 的响应体不同，响应头 `X-Render-Doors: 1`。
#[tokio::test(flavor = "multi_thread")]
async fn 开启门http响应头与字节() {
    let st = crate::server::share(crate::state::AppState::from_env());
    if st.source.is_none() {
        eprintln!("[跳过] 图库不可用（AppState::from_env 扫不到 .wil）");
        return;
    }
    let Some(map_path_buf) = 真地图路径() else {
        eprintln!("[跳过] 无资产根 MIREDIT_ASSET_ROOT（需要真实地图素材）");
        return;
    };
    let map_path = map_path_buf.as_path();
    if !map_path.is_file() {
        eprintln!("[跳过] 真实地图不存在：{}", map_path.display());
        return;
    }
    // 直接把会话装进状态：不走 `/api/map/open`，免得依赖进程 cwd 的 `MIREDIT_MAP_ROOT`
    {
        let mut g = st.write_session();
        *g = Some(miredit_core::EditSession::load(map_path).expect("打开真图"));
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("回环端口应可绑定");
    let addr = listener.local_addr().expect("应能取到本地地址");
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, crate::server::app(st)).await;
    });

    let got = std::thread::spawn(move || {
        (
            http_get(addr, "/api/tiles?x=106&y=316&w=3&h=3&scale=1"),
            http_get(addr, "/api/tiles?x=106&y=316&w=3&h=3&scale=1&doors=0"),
            http_get(addr, "/api/tiles?x=106&y=316&w=3&h=3&scale=1&doors=1"),
        )
    })
    .join()
    .expect("客户端线程不应 panic");
    server.abort();

    let (缺省, 显式0, 门开) = got;
    for (名, r) in [("缺省", &缺省), ("doors=0", &显式0), ("doors=1", &门开)] {
        assert!(r.0.starts_with("http/1.1 200"), "{名} 应 200：{}", r.0);
        assert!(r.1.contains("content-type: image/png"), "{名} 必须是 PNG：{}", r.1);
        assert!(
            r.1.contains("x-render-scale: 1"),
            "{名} 的档位头必须是 1：{}",
            r.1
        );
    }
    assert!(缺省.1.contains("x-render-doors: 0"), "缺省的头必须是 0：{}", 缺省.1);
    assert!(显式0.1.contains("x-render-doors: 0"), "doors=0 的头必须是 0：{}", 显式0.1);
    assert!(门开.1.contains("x-render-doors: 1"), "doors=1 的头必须是 1：{}", 门开.1);
    assert_eq!(缺省.2, 显式0.2, "缺省与 doors=0 的 PNG 必须逐字节相同");
    assert_ne!(缺省.2, 门开.2, "doors=1 的 PNG 必须与闭门不同");
    eprintln!(
        "[开启门HTTP] 106,316 3×3 @1×：缺省/doors=0 = {} B，doors=1 = {} B",
        缺省.2.len(),
        门开.2.len()
    );
}
