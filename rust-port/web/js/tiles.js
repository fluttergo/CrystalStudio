/* =============================================================================
   tiles.js —— 瓦片取图 + LRU 缓存
   · 走后端 GET /api/tiles?...&scale=<0.25|0.5|1>&doors=<0|1>&lighting=<day|dawn_dusk|night>
   · **缓存键 = 图身份 + 8 个出图参数**：`<mapToken>|<x,y,w,h,scale,layers,collision,doors,lighting>`
       —— 不含 scale 会把 1× 的 768×512 图和 0.25× 的 192×128 图混在一起（串图）；
       —— 不含 mapToken 会把**上一张图**的瓦片按同一个键写进这张图的缓存（换图串图）；
       —— 不含 doors 就会「勾了开启门没反应」：URL 变了、键没变 ⇒ 直接命中旧块，
          地图字节全对，界面就是不变（最难查的一种）；
       —— 不含 lighting 同款：「切了夜晚/黎明却没反应」也是 URL 变、键没变。
   · **在途请求带票**：每次 load 记下 (mapToken, 该键的作废序号)，回来时对不上就**丢弃**
     （关位图、不落缓存）。`clear()` 自增 mapToken，`invalidate()` 自增该键的序号
     ⇒ 「开 A → 请求在途 → 立刻开 B」时 A 的 PNG 不会落到 B 的画布上；
       撤销 / 放印章之后，在途的旧块也不会复活。
   · 命中即不再发请求；切到另一档 = 换键 = 重新取；切回原档 = 原键还在 ⇒ 0 新请求
   · 状态栏用：lastScale / lastBytes（上次瓦片的后端档位与 PNG 字节数）
   ============================================================================= */
(function (ME) {
  'use strict';

  var CHUNK = ME.CHUNK;
  var MAX = ME.config.tileCacheSize;

  /** Map 保持插入顺序：命中时删除再插入 = 移到队尾，队首即最久未用 */
  var cache = new Map();
  var pending = Object.create(null);   // 键 → 票 {token, gen}
  var mapToken = 0;                    // 「图身份 / 世代」：clear() 自增，进缓存键
  var gen = Object.create(null);       // 键 → 作废序号（invalidate 用；键里已含 mapToken）
  var stats = {
    requests: 0, hits: 0, misses: 0, evicted: 0, failed: 0,
    dropped: 0,         // 在途请求回来时已作废、被丢弃的次数（换图 / 失效）
    bytes: 0            // 累计下载的 PNG 字节数（体积差的直接证据）
  };
  var lastError = null;      // 最近一次取图失败的 ApiError（UI 据此提示 503 / 413 等）
  var lastScale = null;      // 上次成功瓦片的后端档位（'0.25' / '0.5' / '1'）
  var lastLighting = null;   // 上次成功瓦片的光照档位（响应头真值，读不到为 null）
  var lastBytes = 0;         // 上次成功瓦片的 PNG 字节数
  var probe = null;          // 采样 1 像素用的离屏画布（复用）

  /**
   * 缓存键：**必须含 scale**（否则四档共用键会串图），
   * **必须含 mapToken**（否则换图后上一张图的瓦片会按同键落进新图），
   * **必须含 doors**（否则「开启门」勾了也命中旧块 ⇒ 画面不变，且字节全对、极难查），
   * **必须含 lighting**（否则切光照档也命中旧块 ⇒ 画面不变，同样极难查）。
   *
   * `doors` / `lighting` 两段分别用 `ME.doorsParam` / `ME.lightingParam` 归一化，
   * 与 `api.js` 拼 URL 的口径同源。
   */
  function keyOf(p) {
    return mapToken + '|' + p.x + ',' + p.y + ',' + p.w + ',' + p.h + ',' + (p.scale || '1') +
      ',' + p.layers + ',' + (p.collision || 'none') + ',' + ME.doorsParam(p) +
      ',' + ME.lightingParam(p);
  }

  /** 从缓存键里拆回「图身份」与几何：`<token>|<x>,<y>,<w>,<h>,…` */
  function keyParts(k) {
    var i = k.indexOf('|');
    var body = i >= 0 ? k.slice(i + 1) : k;
    var a = body.split(',');
    return {
      token: i >= 0 ? +k.slice(0, i) : 0,
      x: +a[0], y: +a[1], w: +a[2], h: +a[3]
    };
  }

  /** 这张票还有效吗：图身份没换、且这个键自请求发出后没有被 invalidate 过 */
  function stillValid(k, ticket) {
    return ticket.token === mapToken && (gen[k] || 0) === ticket.gen;
  }

  /** 把 blob 变成可 drawImage 的位图（优先 ImageBitmap，回退 <img>） */
  async function toBitmap(blob) {
    if (typeof createImageBitmap === 'function') return await createImageBitmap(blob);
    return await new Promise(function (resolve, reject) {
      var img = new Image();
      var url = URL.createObjectURL(blob);
      img.onload = function () { URL.revokeObjectURL(url); resolve(img); };
      img.onerror = function () { URL.revokeObjectURL(url); reject(new Error('图片解码失败')); };
      img.src = url;
    });
  }

  function put(k, entry) {
    cache.delete(k);
    cache.set(k, entry);
    while (cache.size > MAX) {
      var oldest = cache.keys().next().value;
      var dead = cache.get(oldest);
      cache.delete(oldest);
      if (dead && dead.bitmap && typeof dead.bitmap.close === 'function') dead.bitmap.close();
      stats.evicted++;
    }
  }

  function load(k, p) {
    if (pending[k]) return;
    var ticket = { token: mapToken, gen: gen[k] || 0 };
    pending[k] = ticket;
    stats.requests++;
    var url = ME.api.tileUrl(p);
    ME.debug.push('tile:request', {
      url: url, key: k, scale: p.scale || '1', doors: ME.doorsParam(p),
      lighting: ME.lightingParam(p)
    });

    ME.api.fetchTile(p)
      .then(function (r) {
        return toBitmap(r.blob).then(function (bitmap) { return { bitmap: bitmap, r: r }; });
      })
      .then(function (got) {
        delete pending[k];
        if (!stillValid(k, ticket)) {
          // 票作废（换图 / 该块被 invalidate）：**丢弃**，绝不写进缓存
          dropTile(k, ticket, got.bitmap);
          return;
        }
        stats.bytes += got.r.bytes;
        lastBytes = got.r.bytes;
        lastScale = got.r.scale;
        lastLighting = got.r.lightingActual;
        put(k, {
          state: 'ok',
          bitmap: got.bitmap,
          scale: got.r.scale,       // 后端实际档位（响应头 X-Render-Scale）
          lighting: got.r.lightingActual, // 后端实际光照（响应头 X-Render-Lighting）
          bytes: got.r.bytes
        });
        ME.emit('tiles:loaded', {
          key: k, params: p, bytes: got.r.bytes, scale: got.r.scale,
          lighting: got.r.lightingActual, lightingUnknown: got.r.lightingUnknown
        });
      })
      .catch(function (e) {
        delete pending[k];
        if (!stillValid(k, ticket)) {
          dropTile(k, ticket, null);
          return;
        }
        stats.failed++;
        lastError = e;
        // 网络/参数类失败允许 3 秒后重试（后端刚起来、图库刚就绪都能自愈）；
        // 413 这类「换个档才可能成功」的错也走同一条重试窗口，不静默降级。
        put(k, { state: 'missing', reason: ME.api.describe(e), soft: true, ts: Date.now() });
        ME.emit('tiles:failed', { key: k, params: p, error: e });
      });
  }

  /** 作废的瓦片：关掉位图、计数、留一条可断言的日志（**不落缓存**） */
  function dropTile(k, ticket, bitmap) {
    if (bitmap && typeof bitmap.close === 'function') bitmap.close();
    stats.dropped++;
    var why = ticket.token !== mapToken ? 'map-changed' : 'invalidated';
    ME.debug.push('tile:drop', {
      key: k, reason: why,
      ticketToken: ticket.token, mapToken: mapToken,
      ticketGen: ticket.gen, nowGen: gen[k] || 0
    });
  }

  /**
   * 取一块瓦片（同步返回，异步填充）。
   * @param {{x,y,w,h,scale,layers,collision,doors,lighting}} p  `scale` 是**请求档位字符串**，
   *   `doors` 是 `'0'` / `'1'`、`lighting` 是三档之一（都进缓存键；由 `ME.*Param` 归一化）
   * @returns {{state:'ok'|'pending'|'missing', bitmap?:*, scale?:string, bytes?:number, reason?:string}}
   */
  function entry(p) {
    var k = keyOf(p);
    var hit = cache.get(k);
    if (hit) {
      if (hit.state === 'missing' && hit.soft && Date.now() - hit.ts > 3000) {
        cache.delete(k);
      } else {
        cache.delete(k); cache.set(k, hit);   // 移到队尾
        stats.hits++;
        return hit;
      }
    }
    stats.misses++;
    load(k, p);
    return { state: 'pending' };
  }

  /**
   * 采样某一格的像素（单元格检查器用）。
   * 位图尺寸 = `CHUNK*48*factor × CHUNK*32*factor`（`factor` 取自该块的请求档位），
   * 所以格内偏移也要乘 `factor`，否则 25% 档会采到隔壁格。
   */
  function sampleCell(x, y, p) {
    var cx = Math.floor(x / CHUNK) * CHUNK;
    var cy = Math.floor(y / CHUNK) * CHUNK;
    var scale = ME.tileScaleFor(p.scale);
    var e = cache.get(keyOf({
      x: cx, y: cy, w: CHUNK, h: CHUNK, scale: scale,
      layers: p.layers, collision: p.collision, doors: p.doors, lighting: p.lighting
    }));
    if (!e || e.state !== 'ok' || !e.bitmap) return null;

    var f = parseFloat(e.scale || scale) || 1;
    if (!probe) {
      probe = document.createElement('canvas');
      probe.width = 1; probe.height = 1;
    }
    var ctx = probe.getContext('2d', { willReadFrequently: true });
    var lx = Math.floor((x - cx) * ME.CELL.w * f);
    var ly = Math.floor((y - cy) * ME.CELL.h * f);
    lx = ME.util.clamp(lx, 0, e.bitmap.width - 1);
    ly = ME.util.clamp(ly, 0, e.bitmap.height - 1);
    try {
      ctx.clearRect(0, 0, 1, 1);
      ctx.drawImage(e.bitmap, -lx, -ly);
      var d = ctx.getImageData(0, 0, 1, 1).data;
      return { r: d[0], g: d[1], b: d[2], a: d[3] };
    } catch (err) {
      // 跨域画布被污染时读不到像素 —— 如实返回 null，不编颜色
      return null;
    }
  }

  /** 关位图 + 删缓存 + 让该键的在途请求作废（序号 +1，回来时对不上就丢弃） */
  function evictKey(k) {
    var e = cache.get(k);
    if (e && e.bitmap && typeof e.bitmap.close === 'function') e.bitmap.close();
    cache.delete(k);
    gen[k] = (gen[k] || 0) + 1;
  }

  /**
   * 地图内容变了（WS rerender）：丢掉与给定区域相交的块；w/h = 0 表示整屏。
   * **在途请求一并作废**：只删缓存不拦在途请求的话，撤销/放印章之后旧块会重新落回来。
   */
  function invalidate(region) {
    if (!region || !region.w || !region.h) { clearCache(); return; }
    var x1 = region.x + region.w, y1 = region.y + region.h;
    var dead = [];
    function touches(k) {
      var g = keyParts(k);
      return g.x < x1 && g.x + g.w > region.x && g.y < y1 && g.y + g.h > region.y;
    }
    cache.forEach(function (e, k) { if (touches(k)) dead.push(k); });
    // 在途（还没落缓存）的块也要算进来，否则它们回来照样写进缓存
    Object.keys(pending).forEach(function (k) { if (touches(k)) dead.push(k); });
    dead.forEach(evictKey);
    ME.debug.push('tile:invalidate', {
      region: { x: region.x, y: region.y, w: region.w, h: region.h }, dropped: dead.length
    });
  }

  /** 清空整份缓存 = 换一代（`mapToken` 自增）：旧键、旧在途请求一律作废 */
  function clearCache() {
    cache.forEach(function (e) {
      if (e.bitmap && typeof e.bitmap.close === 'function') e.bitmap.close();
    });
    cache.clear();
    pending = Object.create(null);
    gen = Object.create(null);
    mapToken++;
    lastError = null;
    lastScale = null;
    lastLighting = null;
    lastBytes = 0;
    stats.hits = 0; stats.misses = 0; stats.evicted = 0; stats.failed = 0;
    stats.requests = 0; stats.bytes = 0;
    ME.debug.push('tile:clear', { mapToken: mapToken });
  }

  ME.tiles = {
    entry: entry,
    sampleCell: sampleCell,
    invalidate: invalidate,
    stats: stats,
    get pendingCount() { return Object.keys(pending).length; },
    get lastError() { return lastError; },
    get lastScale() { return lastScale; },
    get lastLighting() { return lastLighting; },
    get lastBytes() { return lastBytes; },
    /** 当前「图身份 / 世代」—— 缓存键的第一段；换图/整屏失效后必变 */
    get mapToken() { return mapToken; },
    /** 调试快照（CDP / 自动化断言用）：键、在途、计数一眼可见 */
    dump: function () {
      return {
        mapToken: mapToken,
        keys: Array.from(cache.keys()),
        pending: Object.keys(pending),
        stats: {
          requests: stats.requests, hits: stats.hits, misses: stats.misses,
          evicted: stats.evicted, failed: stats.failed, dropped: stats.dropped,
          bytes: stats.bytes
        }
      };
    },
    clear: clearCache
  };
})(window.ME);
