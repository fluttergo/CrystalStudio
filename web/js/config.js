/* =============================================================================
   config.js —— 全局配置、契约常量、事件总线、调试钩子
   · 契约常量全部对齐**已实现的后端**（crates/miredit-web），见 web/README.md §3
   · 无第三方依赖：不引框架、不引 CDN、不引 npm 包
   ============================================================================= */
window.ME = window.ME || {};

(function (ME) {
  'use strict';

  /* ---------- 1. 可覆盖配置 ---------------------------------------------------
     部署/联调时在 index.html 的 config.js **之前**写：

       <script>
         window.__API_BASE__ = 'http://127.0.0.1:18080'; // 后端根；同源可留空
       </script>

     优先级：window.__XXX__ 全局 > window.__MIREDIT_CONFIG__ > window.__MIREDIT_RUNTIME__
     （最后一项由 docker/entrypoint.sh 在容器启动时生成，仓库里那份是空默认值）
     -------------------------------------------------------------------------- */
  var overrides = window.__MIREDIT_CONFIG__ || {};
  var runtime = window.__MIREDIT_RUNTIME__ || {};

  function pick(globalName, key, fallback) {
    if (typeof window[globalName] === 'string') return window[globalName];
    if (overrides[key] !== undefined) return overrides[key];
    if (runtime[key] !== undefined && runtime[key] !== '') return runtime[key];
    return fallback;
  }

  ME.config = {
    // 后端根，空串 = 同源（后端自己就 serve 本目录，见 MIREDIT_WEB_ROOT）
    apiBase: pick('__API_BASE__', 'apiBase', ''),
    // /api/health 探测超时（毫秒）
    healthTimeout: overrides.healthTimeout || 2500,
    // 瓦片 LRU 容量（块数）
    // ⚠ 缓存键含 scale（四档共用一份 LRU）：25% 档一屏要 ~64 块，若沿用 96 会把
    //    100% 档的旧块挤掉，切回原档就得重取。160 足够四档同时驻留（25% 的块只有
    //    192×128×4 ≈ 98 KB，160 块 ≈ 15 MB 上限）。
    tileCacheSize: overrides.tileCacheSize || 160,
    // 「上次打开的地图路径」在 localStorage 里的键（只是输入框预填，不是缓存数据）
    lastMapKey: 'miredit.lastMapPath'
  };

  /* ---------- 2. 与后端契约共用的常量 --------------------------------------
     CELL / CHUNK 必须与 /api/tiles 的口径一致（web/README.md §3.4）：
     一格 = 48×32 像素（scale=1），一次请求 = 16×16 格。
     ⚠ 后端**支持四档 scale**（0.25 / 0.5 / 1 / 2），由 ME.tileScaleFor 按缩放档换算；
       2× 档在 64×48 视口会超 64 MiB 上限（413），所以**前端永不请求 2**。
     -------------------------------------------------------------------------- */
  ME.CELL = { w: 48, h: 32 };
  ME.CHUNK = 16;
  ME.ZOOMS = [0.25, 0.5, 1, 2];        // 25% / 50% / 100% / 200%

  /**
   * 缩放档 → 请求后端的 `scale` 参数（字符串，直接拼进 URL）。
   *
   * | 显示 | 请求 scale | 说明 |
   * |---|---|---|
   * | 25% | `0.25` | 服务端出 1/4 图，画布 1:1 贴图（不再 drawImage 缩小） |
   * | 50% | `0.5` | 同上，1/2 图 |
   * | 100% | `1` | 1× 原图（与旧行为逐字节相同） |
   * | 200% | **`1`** | **故意不请求 2**：见下 |
   *
   * 为什么 200% 不请求 `scale=2`：渲染库的目标图上限是 64 MiB，而 `64×48` 格 @2× 是
   * `6144×3072×4 = 72 MiB` ⇒ 后端必然回 **413**。而放大本来只需覆盖**可见的** 16×16 块，
   * 2× 会为同样可见面积多拉 4 倍体积的 PNG。放大由 canvas `drawImage` 用最近邻完成
   * （关 `imageSmoothingEnabled`，像素画放大不许插值），视觉结果与服务端 2× 的 Nearest 一致。
   */
  ME.tileScaleFor = function (zoom) {
    if (zoom <= 0.25) return '0.25';
    if (zoom <= 0.5) return '0.5';
    return '1';
  };

  /**
   * 「开启门」→ 请求后端的 `doors` 参数（**只有 `'0'` / `'1'` 两个值**）。
   *
   * 归一化只有这一处真源：`api.js` 拼 URL、`tiles.js` 拼缓存键、`canvas.js` 收 setLayers
   * 都调它。**任何一处自己写 `p.doors ? '1' : '0'` 都会埋雷**：
   * 缓存键写 `true`、URL 写 `1` ⇒ 两套口径；反过来则「勾了没反应」（键变了却没换图）。
   *
   * 后端只认 `0` / `1`（其它值 400），所以这里把任何非真值统一成 `'0'`、
   * `1` / `'1'` / `true` 统一成 `'1'` —— 前端是**唯一**决定发什么的地方，不许发半个值。
   */
  ME.doorsParam = function (p) {
    var v = (p && typeof p === 'object') ? p.doors : p;
    return (v === 1 || v === '1' || v === true) ? '1' : '0';
  };

  /**
   * 「地图光照」三档（互斥）。顺序与后端 `/api/tiles?lighting=` 同口径：
   * `day`（白天，默认）/ `dawn_dusk`（黎明·黄昏）/ `night`（夜晚）。
   *
   * ⚠ 第二档协议值是**下划线** `dawn_dusk`，渲染库日志短名 `dawn-dusk`（连字符）
   * 是另一套契约；前端只用这里定义的值拼 URL / 缓存键 / 按钮 data 属性。
   */
  ME.LIGHTINGS = [
    { value: 'day', label: '白天' },
    { value: 'dawn_dusk', label: '黎明·黄昏' },
    { value: 'night', label: '夜晚' }
  ];

  /**
   * 光照档 → 请求后端的 `lighting` 参数（**归一化唯一真源**）。
   *
   * 只认上面三档，其它（含缺省 / `null` / 未知值）统一成 `day`：这是**前端视图状态**
   * 的缺省，不是后端参数校验的兜底 —— 后端对非法 `lighting` 一律 400，不静默当白天。
   * `api.js` 拼 URL、`tiles.js` 拼缓存键、`canvas.js` 存参数都调它，不许各写一套。
   */
  ME.lightingParam = function (p) {
    var v = (p && typeof p === 'object') ? p.lighting : p;
    for (var i = 0; i < ME.LIGHTINGS.length; i++) {
      if (ME.LIGHTINGS[i].value === v) return ME.LIGHTINGS[i].value;
    }
    return 'day';
  };

  /**
   * 工具 → 命令。
   *
   * - `cmd`：落笔时发的**内核命令**（`miredit_core::Command` 的变体名）；
   * - `toolKey`：切工具时发给后端的 `{"SetTool":{"tool":…}}` 键。
   *   ⚠ **线上词汇表只有这四个**（`brush` / `bucket` / `eraser` / `pickup`，严格小写）——
   *   本表的键（`fill` / `pick`）是**前端词汇**，绝不发到线上（别名只在这里翻译）。
   *   ⚠ `select` 的 `toolKey` 是 **`null`**：选区在后端只是 `SetSelection` 命令，
   *   **没有对应工具位**（`cmd::set_tool_by_key` 只认那四个键，发别的会 400 / 或回
   *   `changed=0 + 中文原因`）⇒ 切到「选择」时**一条 SetTool 都不发**，纯前端工具态。
   *   ⚠ `pan` 同理（`toolKey: null`、`cmd: null`）：平移是**纯前端视口操作**，
   *   后端没有「平移」这种东西，一条命令都不发。它是手机单指平移的唯一入口
   *   （`touch.js` 的 `wantPan` 认 `ME.state.tool === 'pan'`）。
   * - `usable`：`false` ⇒ 按钮置灰 + 点了只提示 `why`，不发请求（当前六个全为 `true`）。
   */
  ME.TOOLS = {
    brush:  { label: '笔刷',   key: 'B', cmd: 'Paint', toolKey: 'brush',  usable: true },
    fill:   { label: '填充',   key: 'F', cmd: 'Fill',  toolKey: 'bucket', usable: true },
    eraser: { label: '橡皮擦', key: 'E', cmd: 'Erase', toolKey: 'eraser', usable: true },
    pick:   { label: '吸管',   key: 'I', cmd: 'Pick',  toolKey: 'pickup', usable: true },
    select: { label: '选择',   key: 'S', cmd: 'SetSelection', toolKey: null, usable: true },
    pan:    { label: '平移',   key: 'H', cmd: null,    toolKey: null,     usable: true }
  };

  /**
   * 图层定义。
   * - `mask` / `group`：拼 /api/tiles 的 `layers` / `collision` 位掩码时用；
   * - `tile`：拼 /api/tiles 的**名字列表**时用（ground / middle / front、low / high）；
   * - `cmd`：`SetLayer` 命令认的五个键之一（miredit-core::EditLayer::key）。
   */
  ME.LAYERS = {
    ground: { label: '地表图层', mask: 1, group: 'layers',    tile: 'ground', cmd: 'ground' },
    middle: { label: '中层图层', mask: 2, group: 'layers',    tile: 'middle', cmd: 'middle' },
    object: { label: '物体图层', mask: 4, group: 'layers',    tile: 'front',  cmd: 'front' },
    low:    { label: '低位阻挡', mask: 1, group: 'collision', tile: 'low',    cmd: 'low_collision' },
    high:   { label: '高位阻挡', mask: 2, group: 'collision', tile: 'high',   cmd: 'high_collision' }
  };

  /** 后端错误码 → 中文短标签（完整说明见 web/README.md §3.10） */
  ME.ERROR_LABEL = {
    bad_request: '参数不合法（400）',
    forbidden_path: '路径越权（403）',
    not_found: '文件不存在（404）',
    no_map: '尚未打开地图（409）',
    too_large: '出图超限（413）',
    parse_failed: '地图解析失败（422）',
    io_error: '磁盘读写失败（500）',
    assets_unavailable: '资源不可用（503）',
    internal: '后端内部错误（500）',
    network_error: '无法连接后端',
    bad_response: '响应不是合法 JSON'
  };

  /* ---------- 3. 极小事件总线 ------------------------------------------------ */
  var handlers = Object.create(null);

  ME.on = function (evt, fn) {
    (handlers[evt] || (handlers[evt] = [])).push(fn);
  };

  ME.emit = function (evt, payload) {
    var list = handlers[evt];
    if (!list) return;
    for (var i = 0; i < list.length; i++) {
      try { list[i](payload); } catch (e) { console.error('[事件处理出错]', evt, e); }
    }
  };

  /* ---------- 4. 调试钩子 ---------------------------------------------------
     供 CDP / 自动化断言用：请求参数、编辑命令、手势序列都能读出来。
     -------------------------------------------------------------------------- */
  var MAX_LOG = 400;
  ME.debug = {
    log: [],
    gesture: [],
    push: function (kind, data) {
      this.log.push({ t: Date.now(), kind: kind, data: data });
      if (this.log.length > MAX_LOG) this.log.shift();
    },
    pushGesture: function (evt, data) {
      this.gesture.push({ t: Date.now(), evt: evt, data: data });
      if (this.gesture.length > MAX_LOG) this.gesture.shift();
    }
  };
  window.__ME_DEBUG__ = ME.debug;

  /* ---------- 5. 小工具 ------------------------------------------------------ */
  ME.util = {
    /** 0.25 → "25%" */
    pct: function (scale) { return Math.round(scale * 100) + '%'; },
    /** 只取文件名（状态栏用；Windows 与 POSIX 分隔符都吃） */
    baseName: function (p) {
      if (!p) return '';
      var s = String(p).replace(/\\/g, '/');
      return s.slice(s.lastIndexOf('/') + 1);
    },
    clamp: function (v, lo, hi) { return v < lo ? lo : (v > hi ? hi : v); },
    /** 拼 HTML 片段时的转义（后端返回的名字里可能带 & < >） */
    esc: function (s) {
      return String(s === null || s === undefined ? '' : s)
        .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;');
    }
  };
})(window.ME);
