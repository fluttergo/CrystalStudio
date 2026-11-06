//! `lighting` 参数的单元测试：解析、非法值、响应头真值、与 `doors` 一次组合。
//!
//! 这一组不依赖真图 / 真图库，换机器也必跑；HTTP 层与字节差异由外部验收实跑。

use super::*;

/// 缺省白天、三档可解析、大小写不敏感；库短名 `dawn-dusk` 不是接口值。
#[test]
fn 光照三档大小写不敏感且缺省白天() {
    assert_eq!(parse_lighting(None).expect("缺省"), MapLighting::Day);
    for (raw, want) in [
        ("day", MapLighting::Day),
        ("DAY", MapLighting::Day),
        ("Day", MapLighting::Day),
        ("dawn_dusk", MapLighting::DawnDusk),
        ("DAWN_DUSK", MapLighting::DawnDusk),
        ("night", MapLighting::Night),
        ("NiGhT", MapLighting::Night),
    ] {
        assert_eq!(parse_lighting(Some(raw)).expect(raw), want, "{raw}");
    }
    assert_eq!(MapLighting::DawnDusk.name(), "dawn-dusk", "库短名是连字符");
    assert_eq!(
        lighting_label(RenderOptions::with_lighting(MapLighting::DawnDusk)),
        "dawn_dusk",
        "接口响应头是下划线"
    );
}

/// 非法 `lighting` 一律 400 + 中文原因；空值、库短名连字符、无下划线别名都不静默当白天。
#[test]
fn 光照非法值400且列出合法取值() {
    for bad in ["xyz", "", "dawn-dusk", "dawndusk", "Dawn Dusk", "2", "auto"] {
        let e = parse_lighting(Some(bad)).expect_err(bad);
        assert_eq!(e.status(), axum::http::StatusCode::BAD_REQUEST, "{bad:?}");
        assert_eq!(e.code(), "bad_request", "{bad:?}");
        for must in ["day", "dawn_dusk", "night", "不静默当成 day"] {
            assert!(e.message().contains(must), "{bad:?} 文案缺 {must}：{}", e.message());
        }
        let want = if bad.is_empty() { "空值" } else { bad };
        assert!(e.message().contains(want), "{bad:?} 文案缺收到原文：{}", e.message());
    }
}

/// 响应头与**真正传给渲染库**的那份选项同源；三档短名与接口契约逐字一致。
#[test]
fn 光照响应头与选项同源() {
    for (档, want) in [
        (MapLighting::Day, "day"),
        (MapLighting::DawnDusk, "dawn_dusk"),
        (MapLighting::Night, "night"),
    ] {
        assert_eq!(lighting_label(RenderOptions::with_lighting(档)), want);
        assert_eq!(
            lighting_label(RenderOptions::with_doors_and_lighting(true, 档)),
            want,
            "门态与光照组合后光照不能丢"
        );
    }
}

/// ⚠ 两个开关必须一次组合：门态与光照互不覆盖，缺省仍等于 `RenderOptions::default()`。
#[test]
fn 两个开关一次组合不互相覆盖() {
    let 缺省 = RenderOptions::with_doors_and_lighting(
        parse_doors_open(None).expect("缺省闭门"),
        parse_lighting(None).expect("缺省白天"),
    );
    assert_eq!(缺省, RenderOptions::default());
    assert_eq!(缺省, RenderOptions::closed());
    assert_eq!(doors_label(缺省), "0");
    assert_eq!(lighting_label(缺省), "day");

    let 夜间开门 = RenderOptions::with_doors_and_lighting(
        parse_doors_open(Some("1")).expect("门全开"),
        parse_lighting(Some("NIGHT")).expect("夜晚"),
    );
    assert!(夜间开门.doors_open, "门态不能被光照构造器覆盖");
    assert_eq!(夜间开门.lighting, MapLighting::Night, "光照不能被门态构造器覆盖");
    assert_eq!(doors_label(夜间开门), "1");
    assert_eq!(lighting_label(夜间开门), "night");

    let 黄昏闭门 = RenderOptions::with_doors_and_lighting(
        parse_doors_open(Some("0")).expect("闭门"),
        parse_lighting(Some("Dawn_Dusk")).expect("黎明·黄昏"),
    );
    assert!(!黄昏闭门.doors_open);
    assert_eq!(黄昏闭门.lighting, MapLighting::DawnDusk);
    assert_eq!(lighting_label(黄昏闭门), "dawn_dusk");
}
