/* =============================================================================
   api.js —— 后端 HTTP / WebSocket 客户端
   契约见 web/README.md §3（**按 crates/miredit-web 的实际实现写的**，不是草案）。
   任何失败都抛 ApiError{code,message,status}，绝不编造成功结果。
   ============================================================================= */
(function (ME) {
  'use strict';

  /**
   * 契约错误。`code` 取后端的机器码（bad_request / forbidden_path / not_found /
   * no_map / parse_failed / io_error / assets_unavailable / internal），
   * 本地失败用 network_error / bad_response。
   */
  function ApiError(code, message, status) {
    this.name = 'ApiError';
    this.code = code || 'unknown';
    this.message = message || '';
    this.status = status || 0;
  }
  ApiError.prototype = Object.create(Error.prototype);
  ApiError.prototype.constructor = ApiError;
  ME.ApiError = ApiError;

  /** 把错误翻成一句中文（错误码标签 + 后端原文），供 toast / 横幅直接显示 */
  function describe(e) {
    if (!e) return '未知错误';
    var code = e.code || 'unknown';
    var label = ME.ERROR_LABEL[code];
    var msg = e.message || String(e);
    if (code === 'forbidden_path') {
      return label + '：' + msg + '。路径必须位于 MIREDIT_MAP_ROOT 之内，且不能包含 “..”。';
    }
    // 后端这句就是「尚未打开地图」，再拼一遍会重复；补一句可操作的指引
    if (code === 'no_map') return label + '：请先点「打开地图…」选择一张 .map';
    return label ? (label + '：' + msg) : msg;
  }

  /** 档位三态 → 状态栏中文串（纯函数）：真值一致 / 读不到显示未知 / 真值与请求不符显式标注。
   * 「读不到」不等于「相符」；不许在 fetchTile 用请求值兜底，否则「不符」是死代码。 */
  function describeScale(actual, requested) {
    var act = (actual === null || actual === undefined) ? '' : String(actual).trim();
    var req = (requested === null || requested === undefined) ? '' : String(requested).trim();
    // 读不到就说读不到：不拿 req 填 act，也不把这种情况算进「不符」
    if (!act) return 'scale 未知（响应头不可读）';
    if (req && act !== req) return 'scale ' + act + '（请求 ' + req + '，与请求不符）';
    return 'scale ' + act;
  }

  /** 光照档位 → 状态栏中文串（纯函数）：只认响应头真值；读不到显示「未知（响应头不可读）」，
   * 不认识的值原文照抄，绝不用请求值冒充实测值。 */
  function describeLighting(actual) {
    var act = (actual === null || actual === undefined) ? '' : String(actual).trim();
    if (!act) return '光照 未知（响应头不可读）';
    for (var i = 0; i < ME.LIGHTINGS.length; i++) {
      var d = ME.LIGHTINGS[i];
      if (d.value === act) return '光照 ' + d.label;
    }
    return '光照 ' + act + '（响应头档位未识别）';
  }

  function joinUrl(base, path) {
    if (!base) return path;
    return String(base).replace(/\/+$/, '') + path;
  }

  /** 统一 JSON 请求：失败按契约错误体抛 ApiError */
  async function request(path, options) {
    var opt = options || {};
    var url = joinUrl(ME.config.apiBase, path);
    var init = {
      method: opt.method || 'GET',
      headers: {},
      // 本编辑器不依赖 Cookie，统一不带凭据，避免跨域时的 CORS 复杂度
      credentials: 'omit',
      cache: 'no-store'
    };
    if (opt.body !== undefined) {
      init.headers['Content-Type'] = 'application/json';
      init.body = JSON.stringify(opt.body);
    }

    var resp;
    try {
      resp = await fetch(url, init);
    } catch (e) {
      throw new ApiError('network_error', (e && e.message) || '网络不可达', 0);
    }

    var text = await resp.text();
    var data = null;
    if (text) {
      try { data = JSON.parse(text); } catch (e) {
        throw new ApiError('bad_response', 'HTTP ' + resp.status + ' 的响应不是 JSON', resp.status);
      }
    }
    if (!resp.ok) {
      var err = data && data.error ? data.error : null;
      throw new ApiError(
        (err && err.code) || 'http_' + resp.status,
        (err && err.message) || ('HTTP ' + resp.status + ' ' + resp.statusText),
        resp.status
      );
    }
    return data;
  }

  var api = {
    /**
     * GET /api/health —— 探活。**只判「进程是否应答」**：
     * 后端恒返回 200 {ok:true,version}，图库缺失由 /api/tiles 的 503 体现，
     * 所以这里**不再**用 health 判断资源是否就绪。
     * 不抛错：返回 {ok:true,version} 或 {ok:false,reason}。
     */
    async health() {
      var ctrl = typeof AbortController !== 'undefined' ? new AbortController() : null;
      var timer = ctrl ? setTimeout(function () { ctrl.abort(); }, ME.config.healthTimeout) : null;
      try {
        var resp = await fetch(joinUrl(ME.config.apiBase, '/api/health'), {
          method: 'GET', credentials: 'omit', cache: 'no-store',
          signal: ctrl ? ctrl.signal : undefined
        });
        if (!resp.ok) return { ok: false, reason: 'HTTP ' + resp.status };
        var data = await resp.json();
        if (!data || data.ok !== true) return { ok: false, reason: 'ok != true' };
        return { ok: true, version: data.version || '版本未知' };
      } catch (e) {
        var msg = e && e.name === 'AbortError' ? '超时' : ((e && e.message) || '网络错误');
        return { ok: false, reason: msg };
      } finally {
        if (timer) clearTimeout(timer);
      }
    },

    /**
     * GET /api/map →
     * `{path,width,height,format,dirty,tolerant_trailer_bytes,
     *   format_label,editable,ground_paint_supported,edit_warning,disk_cell_len}`
     *
     * 没开图时 `path` 与四个能力位（含两个布尔）全为 `null`、`width/height` 为 0。
     */
    getMap() {
      return request('/api/map');
    },

    /** POST /api/map/open {path} → 同上。path 一律传**相对 MIREDIT_MAP_ROOT 的路径** */
    openMap(path) {
      return request('/api/map/open', { method: 'POST', body: { path: path } });
    },

    /**
     * GET /api/maps?offset=&limit= →
     * `{total, items:[{name,path,bytes,mtime}]}`
     *
     * · `total` 是**总数**（不是本页条数）；
     * · `path` 是**相对 MIREDIT_MAP_ROOT** 的路径，可直接喂 [`openMap`]；
     * · `mtime` 是 Unix 秒（取不到为 `null`）；
     * · 后端 `limit` 上限 500（超了回 400，不静默截断）⇒ 拉全量要自己翻页
     *   （见 `js/maplist.js`：500/页翻到 `total`）。
     * · 根目录不可用 ⇒ **503 + 中文原因**（不是 200 空列表）。
     */
    listMaps(offset, limit) {
      var qs = 'offset=' + (offset || 0);
      if (limit) qs += '&limit=' + limit;
      return request('/api/maps?' + qs);
    },

    /**
     * POST /api/edit {cmd} → {changed,dirty,note,can_undo,can_redo}
     * cmd 是 `miredit_core::Command` 的 JSON 直译：{"Paint":{"x":1,"y":2}} / "Undo" …
     * 另加两条网页侧命令：{"SetTool":{"tool":"eraser"}} / {"SetBrushFromPalette":{…}}
     */
    edit(cmd) {
      return request('/api/edit', { method: 'POST', body: { cmd: cmd } });
    },

    /** POST /api/save → {saved,bytes,note} */
    save() {
      return request('/api/save', { method: 'POST', body: {} });
    },

    /** GET /api/palette?kind=&page=&size= → {items:[{lib,name,frames}],total} */
    palette(kind, page, size) {
      var qs = 'kind=' + encodeURIComponent(kind || 'ground') + '&page=' + (page || 0);
      if (size) qs += '&size=' + size;
      return request('/api/palette?' + qs);
    },

    /** GET /api/stamps?kind=&q=&page=&size= → {items:[{id,name,kind,kind_zh,cells,bounds}],total} */
    stamps(kind, q, page, size) {
      var qs = 'kind=' + encodeURIComponent(kind || '') +
               '&q=' + encodeURIComponent(q || '') +
               '&page=' + (page || 0);
      if (size) qs += '&size=' + size;
      return request('/api/stamps?' + qs);
    },

    /** 拼 /api/tiles 取图 URL：scale / doors / lighting 都走归一化真源，避免另写一套判断。 */
    tileUrl(p) {
      var qs = 'x=' + p.x + '&y=' + p.y + '&w=' + p.w + '&h=' + p.h +
               '&scale=' + (p.scale || '1') +
               '&layers=' + encodeURIComponent(p.layers) +
               '&collision=' + encodeURIComponent(p.collision || 'none') +
               '&doors=' + ME.doorsParam(p) +
               '&lighting=' + encodeURIComponent(ME.lightingParam(p));
      return joinUrl(ME.config.apiBase, '/api/tiles?' + qs);
    },

    /**
     * 取一块瓦片。失败抛 ApiError（503/409/400/413 都带后端原文）。
     *
     * **档位三态**（读不到就说读不到，绝不用请求值冒充实际值）：
     * · `scaleActual`    = 响应头 `X-Render-Scale` 的真值（后端取自**实际产物**，不是回显请求）；
     * · `scaleUnknown`   = 读不到（跨源未暴露 / 反代删了头 / 后端没写）⇒ `true`，
     *                      此时 `scaleActual === null`，**调用方必须显示「未知」**；
     * · `scaleRequested` = 本次请求拼进 URL 的档位，只代表「要了什么」。
     *
     * `scale` 是给 `tiles.js` 的**兼容别名，恒等于 `scaleActual`**（读不到就是 `null`）。
     * ⚠ 旧实现是 `resp.headers.get('X-Render-Scale') || (p.scale || '1')` —— 用请求值兜底，
     * 等于让「实际档位」永远等于「请求档位」，`ui.js` 的「不符」判据成了死代码，
     * 而且界面上看不出它死了。**该兜底已删除，不许加回来。**
     *
     * **光照档位同口径**：`lightingActual` 只认响应头 `X-Render-Lighting`；读不到就是
     * `null` + `lightingUnknown = true`，状态栏必须显示「未知（响应头不可读）」，
     * 绝不能用 `lightingRequested`（本次请求值）冒充实测值。
     *
     * @returns {Promise<{blob:Blob, bytes:number,
     *   scaleActual:string|null, scaleUnknown:boolean,
     *   scaleRequested:string, scale:string|null,
     *   lightingActual:string|null, lightingUnknown:boolean,
     *   lightingRequested:string}>}
     */
    async fetchTile(p) {
      var resp;
      try {
        resp = await fetch(api.tileUrl(p), { credentials: 'omit', cache: 'no-store' });
      } catch (e) {
        throw new ApiError('network_error', (e && e.message) || '网络不可达', 0);
      }
      if (!resp.ok) {
        var text = await resp.text();
        var err = null;
        try { err = JSON.parse(text).error; } catch (e) { /* 非 JSON：用状态码 */ }
        throw new ApiError(
          (err && err.code) || 'http_' + resp.status,
          (err && err.message) || ('取图失败：HTTP ' + resp.status),
          resp.status
        );
      }
      var blob = await resp.blob();
      // 请求档位：与 tileUrl 拼 URL 时的口径**逐字一致**（缺省 '1' / 'day'）
      var requested = (p && p.scale) ? String(p.scale) : '1';
      var lightingRequested = ME.lightingParam(p);
      // 实际档位：只认响应头。读不到（跨源未 expose / 代理删头 / 后端没写 / 空串）一律 null
      var raw = resp.headers.get('X-Render-Scale');
      var actual = (raw === null || raw === undefined || String(raw).trim() === '')
        ? null
        : String(raw).trim();
      var lightingRaw = resp.headers.get('X-Render-Lighting');
      var lightingActual = (lightingRaw === null || lightingRaw === undefined ||
                            String(lightingRaw).trim() === '')
        ? null
        : String(lightingRaw).trim();
      return {
        blob: blob,
        bytes: blob.size,
        scaleActual: actual,
        scaleUnknown: actual === null,
        scaleRequested: requested,
        scale: actual,         // 兼容别名（tiles.js 仍读 r.scale）：**不填请求值**
        lightingActual: lightingActual,
        lightingUnknown: lightingActual === null,
        lightingRequested: lightingRequested  // 只代表「要了什么」，绝不冒充实际值
      };
    }
  };

  /**
   * 解一帧 WS 报文。
   *
   * **解析失败必须交给 `onBadFrame`**（旧实现是 `catch { return; }`，等于静默丢帧：
   * 连接还在、横幅不响、脏标记不刷，用户只觉得「改了没生效」）。
   * 返回解析出的对象；解析不了或没有 `type` 时返回 `null` 并回调 `onBadFrame(info)`。
   *
   * @param {string} raw  `ev.data`
   * @param {function} onEvent  解析成功 → 交给它分派
   * @param {function} [onBadFrame]  解析失败 → 交给它报警（`{reason, raw}`）
   */
  function decodeFrame(raw, onEvent, onBadFrame) {
    var msg = null;
    var text = typeof raw === 'string' ? raw : String(raw);
    try {
      msg = JSON.parse(text);
    } catch (e) {
      if (onBadFrame) onBadFrame({ reason: 'JSON 解析失败（' + ((e && e.message) || e) + '）', raw: text.slice(0, 200) });
      return null;
    }
    if (!msg || typeof msg !== 'object' || typeof msg.type !== 'string' || !msg.type) {
      if (onBadFrame) onBadFrame({ reason: '报文里没有 type 字段', raw: text.slice(0, 200) });
      return null;
    }
    if (onEvent) onEvent(msg);
    return msg;
  }

  /**
   * 订阅 WS /api/events（服务端单向推送）。
   * 返回 {close()}；断开时回调 onClose，**不自动重连**（由 app.js 决定策略）。
   * @param {function} onEvent  一条已解析的事件
   * @param {function} onClose  断开
   * @param {function} onOpen   连上
   * @param {function} [onBadFrame]  一帧解析失败（app.js 据此显示「事件通道异常」横幅）
   */
  function openEvents(onEvent, onClose, onOpen, onBadFrame) {
    var base = ME.config.apiBase;
    var wsUrl;
    if (base) {
      wsUrl = String(base).replace(/^http/, 'ws').replace(/\/+$/, '') + '/api/events';
    } else {
      var proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
      wsUrl = proto + '//' + location.host + '/api/events';
    }
    var ws;
    try {
      ws = new WebSocket(wsUrl);
    } catch (e) {
      if (onClose) onClose(e);
      return { close: function () {} };
    }
    ws.addEventListener('open', function () { if (onOpen) onOpen(); });
    ws.addEventListener('message', function (ev) { decodeFrame(ev.data, onEvent, onBadFrame); });
    ws.addEventListener('close', function () { if (onClose) onClose(null); });
    ws.addEventListener('error', function () { /* 由 close 统一处理 */ });
    return { close: function () { try { ws.close(); } catch (e) { /* 忽略 */ } } };
  }

  api.openEvents = openEvents;
  api.decodeFrame = decodeFrame;
  ME.api = api;
  ME.api.describe = describe;
  // 纯函数，UI 显示「实际档位 / 实际光照」时调它（三态判定只有这一处真源，禁在 ui.js 里另写一套）
  ME.api.describeScale = describeScale;
  ME.api.describeLighting = describeLighting;
})(window.ME);
