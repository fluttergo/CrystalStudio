/* =============================================================================
   canvas.js —— canvas 2D 渲染器
   · 按「块」拉图（一块 = 16×16 世界格）
   · **请求倍率随缩放档**（ME.tileScaleFor）：25% ⇒ 后端出 192×128 的图，画布 1:1 贴图；
     50% ⇒ 384×256；100% ⇒ 768×512。缩小档不再由 drawImage 缩（服务端 Box 面积加权更准、
     体积小一个数量级）
   · 200% ⇒ 仍请求 1×，由画布放大（关平滑保像素感）：2× 在 64×48 会超 64 MiB 上限（413）
   · 只画「已取到的真实瓦片」；未取到画斜纹并标注「无瓦片」，绝不画假地图
   ============================================================================= */
(function (ME) {
  'use strict';

  var CELL = ME.CELL;
  var CHUNK = ME.CHUNK;
  var ZOOMS = ME.ZOOMS;

  var cv = null, ctx = null;
  var dpr = 1;
  var W = 0, H = 0;                 // 画布 CSS 像素尺寸
  var map = null;                   // {width,height}
  var layersParam = 'ground,middle,front';
  var collisionParam = 'none';
  var doorsParam = '0';             // '0' = 闭门（缺省，与改动前逐字节相同）、'1' = 门全开
  var lightingParam = 'day';        // 光照档位（缺省白天 = 与改动前逐字节相同）
  var hover = null;                 // {x,y} 世界格
  var rafId = 0;
  var hatch = null;
  var sel = null;                   // 已提交的选区 {x0,y0,x1,y1}（闭区间，世界格）
  var selPreview = null;            // 拖动中的选区（优先于 sel 绘制）

  // 视口：ox/oy = 左上角所在的世界格（浮点），scale = 缩放档
  var view = { ox: 0, oy: 0, scale: 1 };

  function cellPx() { return CELL.w * view.scale; }
  function cellPy() { return CELL.h * view.scale; }

  /** 视口钳制：最多允许露出半屏地图外区域，避免把地图拖丢 */
  function clampView() {
    if (!map) return;
    var visW = W / cellPx(), visH = H / cellPy();
    var halfW = visW * 0.5, halfH = visH * 0.5;
    view.ox = ME.util.clamp(view.ox, -halfW, Math.max(-halfW, map.width - halfW));
    view.oy = ME.util.clamp(view.oy, -halfH, Math.max(-halfH, map.height - halfH));
  }

  function ensureSize() {
    var rect = cv.getBoundingClientRect();
    var w = Math.max(1, Math.round(rect.width));
    var h = Math.max(1, Math.round(rect.height));
    var d = window.devicePixelRatio || 1;
    if (w === W && h === H && d === dpr) return;

    // 视口尺寸变了（旋转屏幕 / 断点切换）：保持「视口中心的世界格」不动
    var had = map && W > 0 && H > 0;
    var center = had ? { x: view.ox + (W / 2) / cellPx(), y: view.oy + (H / 2) / cellPy() } : null;

    W = w; H = h; dpr = d;
    cv.width = Math.round(w * d);
    cv.height = Math.round(h * d);

    if (had) {
      view.ox = center.x - (W / 2) / cellPx();
      view.oy = center.y - (H / 2) / cellPy();
      clampView();
    }
  }

  function getHatch() {
    if (hatch) return hatch;
    var c = document.createElement('canvas');
    c.width = 8; c.height = 8;
    var g = c.getContext('2d');
    g.fillStyle = '#efeade';
    g.fillRect(0, 0, 8, 8);
    g.strokeStyle = '#ded5bf';
    g.lineWidth = 1;
    g.beginPath(); g.moveTo(0, 8); g.lineTo(8, 0); g.stroke();
    hatch = ctx.createPattern(c, 'repeat');
    return hatch;
  }

  /** 主绘制 */
  function draw() {
    rafId = 0;
    if (!cv || !ctx) return;
    ensureSize();
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, W, H);

    // 画布外底色（地图外区域）
    ctx.fillStyle = '#e7e2d4';
    ctx.fillRect(0, 0, W, H);

    if (!map) return;

    var cpx = cellPx(), cpy = cellPy();
    var ox = view.ox, oy = view.oy;

    // 地图矩形（纸面）
    var mx = -ox * cpx, my = -oy * cpy;
    var mw = map.width * cpx, mh = map.height * cpy;
    ctx.fillStyle = '#f7f5ef';
    ctx.fillRect(mx, my, mw, mh);

    // 位图**已按缩放档出图**：scale<1 时目标矩形与位图尺寸逐像素相等（1:1 贴图，平滑无意义）；
    // 只有 200%（请求 1×、显示 2×）才是放大，像素画放大必须关平滑。
    ctx.imageSmoothingEnabled = false;

    // 本帧的请求档位（与 view.scale 不同：200% 也请求 1×，见 config.js 的 ME.tileScaleFor）
    var reqScale = ME.tileScaleFor(view.scale);

    // ---- 逐块绘制 ----
    var visX1 = ox + W / cpx, visY1 = oy + H / cpy;
    var nChunkX = Math.ceil(map.width / CHUNK);
    var nChunkY = Math.ceil(map.height / CHUNK);
    var c0x = Math.max(0, Math.floor(ox / CHUNK));
    var c0y = Math.max(0, Math.floor(oy / CHUNK));
    var c1x = Math.min(nChunkX - 1, Math.floor(visX1 / CHUNK));
    var c1y = Math.min(nChunkY - 1, Math.floor(visY1 / CHUNK));

    for (var cy = c0y; cy <= c1y; cy++) {
      for (var cx = c0x; cx <= c1x; cx++) {
        var sx = Math.round((cx * CHUNK - ox) * cpx);
        var sy = Math.round((cy * CHUNK - oy) * cpy);
        var sw = Math.round(CHUNK * cpx);
        var sh = Math.round(CHUNK * cpy);
        // 块可能超出地图边界：按地图边界裁一下（只影响占位/斜纹的绘制范围）
        var cw = Math.min(CHUNK, map.width - cx * CHUNK) * cpx;
        var ch = Math.min(CHUNK, map.height - cy * CHUNK) * cpy;

        var e = ME.tiles.entry({
          x: cx * CHUNK, y: cy * CHUNK, w: CHUNK, h: CHUNK,
          scale: reqScale,
          layers: layersParam, collision: collisionParam,
          doors: doorsParam, lighting: lightingParam
        });

        if (e.state === 'ok') {
          // reqScale<1 时 `sw/sh` 与位图尺寸相等（1:1）；200% 时是 2× 放大（最近邻）
          ctx.drawImage(e.bitmap, sx, sy, sw, sh);
        } else if (e.state === 'pending') {
          ctx.fillStyle = '#eee9dc';
          ctx.fillRect(sx, sy, cw, ch);
          ctx.strokeStyle = '#e3dccb';
          ctx.lineWidth = 1;
          ctx.strokeRect(sx + .5, sy + .5, cw - 1, ch - 1);
        } else {
          ctx.fillStyle = getHatch();
          ctx.fillRect(sx, sy, cw, ch);
          if (sw > 110 && sh > 64) {
            ctx.fillStyle = 'rgba(120,110,90,.75)';
            ctx.font = '12px ' + getComputedStyle(document.body).fontFamily;
            ctx.textAlign = 'center';
            ctx.textBaseline = 'middle';
            ctx.fillText('无瓦片', sx + cw / 2, sy + ch / 2);
            ctx.textAlign = 'start';
            ctx.textBaseline = 'alphabetic';
          }
        }
      }
    }

    // ---- 网格（一格 ≥ 20px 才画，避免糊成一片）----
    if (cpx >= 20) {
      ctx.strokeStyle = 'rgba(90, 78, 54, .12)';
      ctx.lineWidth = 1;
      ctx.beginPath();
      var gx0 = Math.max(0, Math.floor(ox)), gx1 = Math.min(map.width, Math.ceil(visX1));
      var gy0 = Math.max(0, Math.floor(oy)), gy1 = Math.min(map.height, Math.ceil(visY1));
      for (var gx = gx0; gx <= gx1; gx++) {
        var px = Math.round((gx - ox) * cpx) + .5;
        ctx.moveTo(px, Math.max(my, 0));
        ctx.lineTo(px, Math.min(my + mh, H));
      }
      for (var gy = gy0; gy <= gy1; gy++) {
        var py = Math.round((gy - oy) * cpy) + .5;
        ctx.moveTo(Math.max(mx, 0), py);
        ctx.lineTo(Math.min(mx + mw, W), py);
      }
      ctx.stroke();
    }

    // ---- 地图边界 ----
    ctx.strokeStyle = 'rgba(168, 132, 43, .85)';
    ctx.lineWidth = 1.5;
    ctx.setLineDash([6, 4]);
    ctx.strokeRect(mx + .5, my + .5, mw - 1, mh - 1);
    ctx.setLineDash([]);

    // ---- 选区（金色虚线 + 半透明填充）----
    var live = selPreview || sel;
    if (live) {
      var sx = Math.round((live.x0 - ox) * cpx);
      var sy = Math.round((live.y0 - oy) * cpy);
      var sw2 = Math.round((live.x1 - live.x0 + 1) * cpx);
      var sh2 = Math.round((live.y1 - live.y0 + 1) * cpy);
      if (sw2 > 0 && sh2 > 0) {
        ctx.fillStyle = selPreview ? 'rgba(200, 160, 60, .14)' : 'rgba(200, 160, 60, .22)';
        ctx.fillRect(sx, sy, sw2, sh2);
        ctx.strokeStyle = selPreview ? 'rgba(168, 132, 43, .8)' : '#a8842b';
        ctx.lineWidth = 2;
        ctx.setLineDash([7, 4]);
        ctx.strokeRect(sx + 1, sy + 1, sw2 - 2, sh2 - 2);
        ctx.setLineDash([]);
        // 角标：一眼看出这是「选区」而不是网格线
        ctx.fillStyle = 'rgba(168, 132, 43, .9)';
        ctx.fillRect(sx, sy, Math.min(6, sw2), Math.min(6, sh2));
      }
    }

    // ---- 悬停单元格 ----
    if (hover && hover.x >= 0 && hover.y >= 0 && hover.x < map.width && hover.y < map.height) {
      var hx = (hover.x - ox) * cpx, hy = (hover.y - oy) * cpy;
      ctx.strokeStyle = '#c8a03c';
      ctx.lineWidth = 2;
      ctx.strokeRect(Math.round(hx) + 1, Math.round(hy) + 1, Math.round(cpx) - 2, Math.round(cpy) - 2);
    }
  }

  function schedule() {
    if (rafId) return;
    rafId = requestAnimationFrame(draw);
  }

  /* ---------- 对外接口 ---------- */
  var canvasApi = {
    init: function (el) {
      cv = el;
      ctx = cv.getContext('2d');
      window.addEventListener('resize', function () { ensureSize(); schedule(); });
      if (typeof ResizeObserver === 'function') {
        new ResizeObserver(function () { ensureSize(); schedule(); }).observe(cv.parentElement || cv);
      }
      // 瓦片是异步到位的：每来一块就重绘一次，否则画布永远停在「载入中」
      ME.on('tiles:loaded', schedule);
      ME.on('tiles:failed', schedule);
      ensureSize();
      schedule();
    },
    setMap: function (m) {
      map = m;
      // 换图 ⇒ 选区一定失效（后端换图会重建会话，选区/剪贴板随之清空）
      sel = null;
      selPreview = null;
      clampView();
      schedule();
    },
    /** 已提交的选区（null = 没有）。画布上画成金色虚线框 + 半透明填充 */
    setSelection: function (r) {
      sel = r ? { x0: r.x0, y0: r.y0, x1: r.x1, y1: r.y1 } : null;
      schedule();
    },
    /** 拖动中的选区预览（松手后由 setSelection 接管） */
    setSelectionPreview: function (r) {
      selPreview = r ? { x0: r.x0, y0: r.y0, x1: r.x1, y1: r.y1 } : null;
      schedule();
    },
    get selection() { return sel; },
    /** 光标态：'stamping' = 印章待放置（十字）；'selecting' = 选区工具（十字） */
    setCursor: function (kind) {
      if (!cv) return;
      cv.classList.toggle('is-stamping', kind === 'stamping');
      cv.classList.toggle('is-selecting', kind === 'selecting');
    },
    /**
     * 图层可见性（名字列表，见 `ME.LAYERS[*].tile`）+ 阻挡 + 开启门 + 光照档；变化时重取。
     *
     * `doors` / `lighting` 都进缓存键（`tiles.js::keyOf`）⇒ 切一下就是换一批键：
     * 勾上/切档重新取图，切回原档命中旧块（0 新请求，但画面确实变回来）。
     */
    setLayers: function (layers, collision, doors, lighting) {
      var d = ME.doorsParam(doors);
      var lit = ME.lightingParam(lighting);
      if (layers === layersParam && collision === collisionParam &&
          d === doorsParam && lit === lightingParam) return;
      layersParam = layers;
      collisionParam = collision;
      doorsParam = d;
      lightingParam = lit;
      schedule();
    },
    get layers() { return layersParam; },
    get collision() { return collisionParam; },
    get doors() { return doorsParam; },
    get lighting() { return lightingParam; },
    get map() { return map; },
    get view() { return view; },
    get size() { return { w: W, h: H, dpr: dpr }; },
    schedule: schedule,
    draw: draw,

    /** 屏幕坐标（CSS 像素，相对画布）→ 世界格坐标（浮点） */
    screenToCell: function (px, py) {
      return { x: view.ox + px / cellPx(), y: view.oy + py / cellPy() };
    },
    /** 屏幕坐标 → 整数格；`inside` 表示是否落在地图内 */
    cellAt: function (px, py) {
      var c = canvasApi.screenToCell(px, py);
      var x = Math.floor(c.x), y = Math.floor(c.y);
      return {
        x: x, y: y,
        inside: !!map && x >= 0 && y >= 0 && x < map.width && y < map.height
      };
    },
    /** 世界格 → 屏幕坐标 */
    cellToScreen: function (x, y) {
      return { x: (x - view.ox) * cellPx(), y: (y - view.oy) * cellPy() };
    },

    panBy: function (dxPx, dyPx) {
      if (!map) return;
      view.ox -= dxPx / cellPx();
      view.oy -= dyPx / cellPy();
      clampView();
      schedule();
    },

    /** 以 (ax,ay)（画布内 CSS 像素）为锚点切到某个缩放档 */
    setZoom: function (level, ax, ay) {
      if (!map) return;
      var target = ZOOMS[0];
      for (var i = 0; i < ZOOMS.length; i++) {
        if (Math.abs(ZOOMS[i] - level) < Math.abs(target - level)) target = ZOOMS[i];
      }
      if (target === view.scale) return;
      var axx = (typeof ax === 'number') ? ax : W / 2;
      var ayy = (typeof ay === 'number') ? ay : H / 2;
      var anchor = canvasApi.screenToCell(axx, ayy);
      view.scale = target;
      view.ox = anchor.x - axx / cellPx();
      view.oy = anchor.y - ayy / cellPy();
      clampView();
      ME.emit('view:zoom', { scale: view.scale });
      schedule();
    },

    /** dir=+1 放大一档 / -1 缩小一档 */
    zoomStep: function (dir, ax, ay) {
      var idx = ZOOMS.indexOf(view.scale);
      if (idx < 0) idx = ZOOMS.indexOf(1);
      var next = ME.util.clamp(idx + dir, 0, ZOOMS.length - 1);
      if (next === idx) return false;
      canvasApi.setZoom(ZOOMS[next], ax, ay);
      return true;
    },

    /** 以某个缩放档居中到地图中心——打开地图时的默认视图 */
    centerAt: function (level) {
      if (!map) return;
      view.scale = level;
      view.ox = map.width / 2 - (W / cellPx()) / 2;
      view.oy = map.height / 2 - (H / cellPy()) / 2;
      clampView();
      ME.emit('view:zoom', { scale: view.scale });
      schedule();
    },

    /** 适应地图：取「能装下整张地图」的最大档位，然后居中 */
    fit: function () {
      if (!map) return;
      var pad = 24;
      var pick = ZOOMS[0];
      for (var i = ZOOMS.length - 1; i >= 0; i--) {
        if (map.width * CELL.w * ZOOMS[i] <= W - pad && map.height * CELL.h * ZOOMS[i] <= H - pad) {
          pick = ZOOMS[i];
          break;
        }
      }
      view.scale = pick;
      view.ox = map.width / 2 - (W / cellPx()) / 2;
      view.oy = map.height / 2 - (H / cellPy()) / 2;
      clampView();
      ME.emit('view:zoom', { scale: view.scale });
      schedule();
    },

    setHover: function (px, py) {
      if (!map) { hover = null; schedule(); return; }
      var c = canvasApi.cellAt(px, py);
      if (!c.inside) {
        if (hover !== null) { hover = null; schedule(); ME.emit('hover:cell', null); }
        return;
      }
      if (!hover || hover.x !== c.x || hover.y !== c.y) {
        hover = { x: c.x, y: c.y };
        schedule();
        ME.emit('hover:cell', hover);
      }
    },

    /**
     * **显式清空**悬停格。
     * ⚠ 不能用 `setHover(-1,-1)` 代替：视口原点落在图内时（如 ox=196, oy=190），
     * 屏幕 (-1,-1) 反算出来正好是**左上角那一格**，于是「指针离开画布」会把
     * 悬停格钉在左上角 —— 粘贴锚点（= 指针所在格）就会粘错地方。
     */
    clearHover: function () {
      if (hover === null) return;
      hover = null;
      schedule();
      ME.emit('hover:cell', null);
    },

    get hover() { return hover; },
    /** 当前取图参数（调试 / 单元格检查器用）；`tileScale` = 该档实际请求的倍率 */
    params: function () {
      return {
        scale: view.scale,
        tileScale: ME.tileScaleFor(view.scale),
        layers: layersParam,
        collision: collisionParam,
        doors: doorsParam,
        lighting: lightingParam
      };
    }
  };

  ME.canvas = canvasApi;
})(window.ME);
