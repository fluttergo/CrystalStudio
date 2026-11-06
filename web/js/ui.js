/* =============================================================================
   ui.js —— 界面外壳：顶栏 / 工具 / 图层 / 状态栏 / 横幅 / 抽屉 / 浮层 / 对话框 / 轻提示
   · 全中文；术语取自 tools/miredit/layout/crystalstudio-strings.json
     （地表图层 / 中层图层 / 物体图层 / 低位阻挡 / 高位阻挡 / 撤销 / 重做 / 保存地图）
   · 这里只碰 DOM，不发请求；命令由 app.js 发（见 js/edit.js）
   ============================================================================= */
(function (ME) {
  'use strict';

  var $ = function (id) { return document.getElementById(id); };
  var el = {};
  var toastTimer = 0;
  var lastTilesError = '';
  /** 上一块成功瓦片的事实（来自 `tiles:loaded`）：响应头真值 + 字节数 + 图世代 token。
   * 真值为 `null` = 响应头读不到（显示「未知」）；token 变了即作废，不拿旧图冒充新图。
   */
  var lastTile = null;
  /** 手指/鼠标还按在尺寸滑杆上（拖拽中不回填，否则滑块会被弹回旧值） */
  var brushBusy = false;

  function labelOf(key) { return ME.LAYERS[key] ? ME.LAYERS[key].label : key; }

  /* ================= 状态栏 / 按钮态 ================= */
  function refresh() {
    var s = ME.state;
    var map = s.map;
    var v = ME.canvas.view;

    el.stFile.textContent = map ? ME.util.baseName(map.path) : '未打开地图';
    el.stSize.textContent = map ? (map.width + '×' + map.height + ' 格') : '—×— 格';
    el.stFormat.hidden = !map || !map.format;
    el.stFormatSep.hidden = el.stFormat.hidden;
    el.stFormat.textContent = map && map.format ? map.format : '—';
    // 悬停能看到**九种格式的真名**（format_label 由后端给，前端不改写）
    el.stFormat.title = map && map.format_label
      ? ('格式真名：' + map.format_label +
         (map.disk_cell_len ? ' · 磁盘 ' + map.disk_cell_len + ' B/格' : ' · 变长格式'))
      : '';

    el.stTool.textContent = ME.TOOLS[s.tool] ? ME.TOOLS[s.tool].label : s.tool;
    el.stLayer.textContent = labelOf(s.editLayer);
    syncBrushSize();
    el.stZoom.textContent = ME.util.pct(v.scale);
    el.zoomVal.textContent = ME.util.pct(v.scale);
    refreshTileStat();

    var h = ME.canvas.hover;
    el.stCell.textContent = h ? ('单元格 ' + h.x + ', ' + h.y) : '单元格 —, —';

    if (s.lastChanged === null) {
      el.stChanged.textContent = '改动 —';
    } else {
      el.stChanged.textContent = '改动 ' + s.lastChanged + ' 格 · 累计 ' + s.totalChanged + ' 格';
    }

    el.fileTab.textContent = map ? ME.util.baseName(map.path) : '未打开地图';
    el.fileDirty.hidden = !(map && map.dirty);
    // 标签页名与 `<title>` **逐字一致**（index.html）：品牌固定 `Crystal Studio · 地图编辑器`
    document.title = (map && map.dirty ? '* ' : '') +
      (map ? ME.util.baseName(map.path) + ' · ' : '') + 'Crystal Studio · 地图编辑器';

    // 撤销 / 重做：按后端回执的 can_undo / can_redo 禁用（未开图时也禁用）
    var ready = s.mode === 'api' && !!map;
    el.btnUndo.disabled = !ready || !s.canUndo;
    el.btnRedo.disabled = !ready || !s.canRedo;

    var modeText;
    if (s.mode === 'api') {
      modeText = '后端已连接 · ' + (s.version || '版本未知');
      if (s.eventsBroken) modeText += ' · 事件通道断开（不影响编辑）';
      // 连接还在、但协议对不上（未知 type / 解析不了的帧）—— 与「断开」是两回事，分开显示
      else if (s.ws && s.ws.anomaly) modeText += ' · 事件通道异常（见顶部横幅）';
    } else {
      modeText = '后端未连接';
    }
    el.stMode.textContent = modeText;
    el.conn.dataset.state = s.mode === 'api' ? 'on' : 'off';
    el.connText.textContent = s.mode === 'api' ? ('已连接 ' + (s.version || '')) : '未连接';

    el.brushFoot.textContent = s.brush
      ? ('笔刷：' + s.brush.text)
      : '笔刷：—（用吸管取一格；后端未选笔刷时落笔返回 changed=0）';
    el.layerFoot.textContent = '当前编辑层：' + labelOf(s.editLayer);

    /* --- 选区 / 剪贴板（真值全部来自后端回执，前端不自己算） --- */
    var selApi = ME.selection;
    var selTxt = selApi ? selApi.selText() : '选区 —';
    var clipTxt = selApi ? selApi.clipText() : '剪贴板 —';
    el.stSel.textContent = selTxt;
    el.stClip.textContent = clipTxt;
    el.selFoot.textContent = selTxt + ' · ' + clipTxt;
    var anchor = selApi ? selApi.anchor() : null;
    el.selAnchor.textContent = '粘贴锚点：指针所在格（当前 ' +
      (anchor ? (anchor.x + ', ' + anchor.y) : '—') + '）';
    syncSelButtons();
  }

  /** 状态栏瓦片 scale / 光照 / 字节数：三态由 describeScale / describeLighting 判定；
   * 读不到响应头显示「未知」，滞后用「上一块瓦片」如实标出，绝不拿请求值兜底。 */
  function refreshTileStat() {
    if (!el.stTile) return;
    // 换图 / 整屏失效（clearCache 自增 mapToken）：上一条记录属上一代，作废
    if (lastTile && lastTile.token !== ME.tiles.mapToken) lastTile = null;
    if (!lastTile) {
      el.stTile.textContent = '瓦片 scale —（取图中…） · 光照 —（取图中…）';
      return;
    }
    var want = ME.tileScaleFor(ME.canvas.view.scale);   // 当前缩放档要请求的档位
    var stale = lastTile.scaleRequested !== null && lastTile.scaleRequested !== want;
    var txt = (stale ? '上一块瓦片 ' : '瓦片 ') +
      ME.api.describeScale(lastTile.scaleActual, lastTile.scaleRequested) +
      ' · ' + ME.api.describeLighting(lastTile.lightingActual);
    if (lastTile.bytes !== null) txt += ' · ' + lastTile.bytes + ' B';
    el.stTile.textContent = txt;
  }

  /** 选区按钮可用态：只按本地已知真值（开图 / 有选区 / 剪贴板非空），不猜后端状态。 */
  function syncSelButtons() {
    if (!el.selBtns) return;
    var s = ME.state;
    var selApi = ME.selection;
    var hasMap = s.mode === 'api' && !!s.map;
    var hasSel = !!(selApi && selApi.sel);
    var hasClip = !!(selApi && selApi.clip);
    el.selCopy.disabled = !hasMap || !hasSel;
    el.selCut.disabled = !hasMap || !hasSel;
    el.selDel.disabled = !hasMap || !hasSel;
    el.selFill.disabled = !hasMap || !hasSel;
    el.selClear.disabled = !hasMap || !hasSel;
    el.selPaste.disabled = !hasMap || !hasClip;
  }

  /* ================= 笔刷尺寸 ================= */
  /** 笔刷尺寸显示只认后端 `SetBrushSize` 回执（前端不夹、不抬奇数，避免伪造后端行为）。 */
  function syncBrushSize() {
    if (!el.brushSizeRange) return;
    var n = (ME.state && ME.state.brushSize) || 3;   // 3 = 内核 BRUSH_DEFAULT（brush.rs:294）
    var txt = n + '×' + n;
    el.brushSizeVal.textContent = txt;
    el.stBrush.textContent = '笔刷 ' + txt;
    el.brushSizeBox.title = '后端会话的笔刷边长：' + txt + '（3..31 的奇数）';
    if (!brushBusy) el.brushSizeRange.value = String(n);
    var btns = el.brushPresets.querySelectorAll('.bsize__preset');
    for (var i = 0; i < btns.length; i++) {
      btns[i].setAttribute('aria-pressed', Number(btns[i].dataset.size) === n ? 'true' : 'false');
    }
  }

  /* ================= 工具 ================= */
  /** 同步工具按钮视觉态：当前项 `aria-checked=true`；不可用项置灰并只提示 `why`，不发请求。 */
  function syncTools() {
    var btns = el.toolGrid.querySelectorAll('.tool');
    for (var i = 0; i < btns.length; i++) {
      var key = btns[i].dataset.tool;
      var def = ME.TOOLS[key];
      var usable = !def || def.usable !== false;
      btns[i].setAttribute('aria-checked', key === ME.state.tool ? 'true' : 'false');
      btns[i].classList.toggle('tool--off', !usable);
      btns[i].disabled = !usable;
      var badge = btns[i].querySelector('.tool__badge');
      if (badge) badge.hidden = usable;
      if (!usable && def && def.why) btns[i].title = def.why;
    }
  }

  /* ================= 图层 ================= */
  /** 把眼睛开关拼成 /api/tiles 的**名字列表**（后端两种写法都收，这里用名字更可读） */
  function layerParams() {
    var s = ME.state, layers = [], collision = [];
    for (var k in ME.LAYERS) {
      if (!ME.LAYERS.hasOwnProperty(k)) continue;
      if (!s.layers[k]) continue;
      if (ME.LAYERS[k].group === 'layers') layers.push(ME.LAYERS[k].tile);
      else collision.push(ME.LAYERS[k].tile);
    }
    return {
      layers: layers.length ? layers.join(',') : 'none',
      collision: collision.length ? collision.join(',') : 'none',
      // 开启门 / 光照：视图选项，进 /api/tiles 参数**和**缓存键
      doors: ME.doorsParam(s.doorsOpen),
      lighting: ME.lightingParam(s.lighting)
    };
  }

  function syncLayers() {
    var s = ME.state;
    var rows = el.layerList.querySelectorAll('.layer');
    for (var i = 0; i < rows.length; i++) {
      var row = rows[i];
      var key = row.dataset.layer;
      var on = !!s.layers[key];
      row.dataset.on = on ? 'true' : 'false';
      row.classList.toggle('is-current', key === s.editLayer);
      var eye = row.querySelector('.layer__eye');
      eye.setAttribute('aria-pressed', on ? 'true' : 'false');
      var use = eye.querySelector('use');
      if (use) use.setAttribute('href', on ? '#i-eye' : '#i-eye-off');
    }
    // 地表「不可落笔」：只认后端 `ground_paint_supported === false`（`null` = 没开图，不显示）
    if (el.layerGroundFlag) {
      var blocked = !!(s.map && s.map.ground_paint_supported === false);
      el.layerGroundFlag.hidden = !blocked;
      if (blocked) {
        el.layerGroundFlag.title = '该格式（' + (s.map.format_label || s.map.format || '未知') +
          '）的背景图号按 2×2 块编码，逐格写会被写回时丢弃 ⇒ 地表不能逐格落笔';
      }
    }
    // 复选框态与状态同步（程序化改 state.doorsOpen 时也不许两边不一致）
    if (el.doorsToggle) el.doorsToggle.checked = !!s.doorsOpen;
    // 三档互斥按钮：只认归一化后的真值，任何时候恰有一个 aria-pressed="true"
    if (el.lightingButtons) {
      var lit = ME.lightingParam(s.lighting);
      for (var j = 0; j < el.lightingButtons.length; j++) {
        var b = el.lightingButtons[j];
        b.setAttribute('aria-pressed', b.dataset.lighting === lit ? 'true' : 'false');
      }
    }
    var p = layerParams();
    ME.canvas.setLayers(p.layers, p.collision, p.doors, p.lighting);
    el.layerFoot.textContent = '当前编辑层：' + labelOf(s.editLayer) +
      '（layers=' + p.layers + ' · collision=' + p.collision + ' · doors=' + p.doors +
      ' · lighting=' + p.lighting + '）';
  }

  /* ================= 轻提示 / 横幅 / 告警条 ================= */
  function toast(msg, ms) {
    el.toast.textContent = msg;
    el.toast.hidden = false;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(function () { el.toast.hidden = true; }, ms || 3600);
  }

  function showBanner(title, note) {
    el.bannerTitle.textContent = title;
    el.bannerNote.textContent = note || '';
    el.banner.hidden = false;
  }

  function hideBanner() { el.banner.hidden = true; }

  /* ================= 模式提示条 / 错误条 ================= */
  /** 顶部条：hint = 印章待放置态（金色），error = 后端原文（红色）。常驻以便 DOM 断言。 */
  function showMode(tone, text) {
    el.modeBar.dataset.tone = tone === 'error' ? 'error' : 'hint';
    el.modeBarMark.textContent = tone === 'error' ? '!' : '◎';
    el.modeBarText.textContent = text || '';
    el.modeBar.hidden = false;
  }

  function hideMode() {
    el.modeBar.hidden = true;
    el.modeBarText.textContent = '';
  }

  /** 常驻黄条：原样显示后端 `edit_warning`（不改写）；tag 由 app.js 按格式能力拼出。 */
  function showWarn(text, tag) {
    el.warnText.textContent = text || '';
    if (el.warnTag) {
      el.warnTag.textContent = tag || '';
      el.warnTag.hidden = !tag;
    }
    el.warnBar.hidden = false;
  }

  function hideWarn() {
    el.warnBar.hidden = true;
    if (el.warnTag) { el.warnTag.hidden = true; el.warnTag.textContent = ''; }
    el.warnText.textContent = '';
  }

  /* ================= 打开地图对话框 ================= */
  function recentPaths() {
    try {
      var raw = localStorage.getItem(ME.config.lastMapKey);
      return raw ? JSON.parse(raw) : [];
    } catch (e) { return []; }
  }

  function rememberPath(p) {
    try {
      var list = recentPaths().filter(function (x) { return x !== p; });
      list.unshift(p);
      localStorage.setItem(ME.config.lastMapKey, JSON.stringify(list.slice(0, 8)));
    } catch (e) { /* 隐私模式下写不进去：忽略，不影响打开 */ }
  }

  function openMapDialog() {
    var s = ME.state;
    if (s.mode !== 'api') {
      toast('后端未连接：打开地图需要 POST /api/map/open。', 5000);
      return;
    }
    el.openNote.textContent =
      '列表来自后端 MIREDIT_MAP_ROOT（GET /api/maps，只读文件名/大小/时间，不解析地图内容）。' +
      '点一项即打开；搜不到时展开「手打路径（退路）」按相对路径打开' +
      '（含 “..” 或指向根外会被拒绝，403 forbidden_path）。';
    var list = recentPaths();
    el.openPath.value = (s.map && s.map.path) ? '' : (list[0] || '');
    if (!el.openPath.value) el.openPath.placeholder = list[0] || 'GodotClient/maps/0.map';
    var html = '';
    for (var i = 0; i < list.length; i++) html += '<option value="' + ME.util.esc(list[i]) + '"></option>';
    el.recentPaths.innerHTML = html;
    if (el.mapSearch) el.mapSearch.value = '';
    if (typeof el.openDialog.showModal === 'function') el.openDialog.showModal();
    else el.openDialog.setAttribute('open', '');
    // 列表：首次打开时按 500/页拉全量，之后复用缓存（「刷新」按钮可强制重拉）
    ME.mapList.refresh(false);
    setTimeout(function () {
      if (el.mapSearch) el.mapSearch.focus();
      else el.openPath.focus();
    }, 30);
  }

  /* ================= 初始化 ================= */
  function init() {
    el = {
      warnBar: $('warnBar'), warnText: $('warnText'), warnClose: $('warnClose'), warnTag: $('warnTag'),
      modeBar: $('modeBar'), modeBarText: $('modeBarText'), modeBarMark: $('modeBarMark'),
      modeBarClose: $('modeBarClose'),
      banner: $('banner'), bannerTitle: $('bannerTitle'), bannerNote: $('bannerNote'), bannerClose: $('bannerClose'),
      fileTab: $('fileTab'), fileDirty: $('fileDirty'),
      conn: $('connBadge'), connText: $('connText'),
      btnOpen: $('btnOpen'), btnSave: $('btnSave'), btnUndo: $('btnUndo'), btnRedo: $('btnRedo'),
      btnSheet: $('btnSheet'), btnSheetClose: $('btnSheetClose'),
      toolGrid: $('toolGrid'), layerList: $('layerList'), layerFoot: $('layerFoot'), brushFoot: $('brushFoot'),
      doorsToggle: $('doorsToggle'),
      lightingGroup: $('lightingGroup'),
      lightingButtons: $('lightingGroup') ? $('lightingGroup').querySelectorAll('.lighting__btn') : [],
      brushSizeBox: $('brushSizeBox'), brushSizeVal: $('brushSizeVal'),
      brushSizeRange: $('brushSizeRange'), brushPresets: $('brushSizePresets'),
      stBrush: $('stBrushSize'),
      layerGroundFlag: $('layerGroundFlag'),
      stage: $('stage'), stageEmpty: $('stageEmpty'),
      zoomIn: $('zoomIn'), zoomOut: $('zoomOut'), zoomFit: $('zoomFit'), zoomVal: $('zoomVal'),
      stFile: $('stFile'), stSize: $('stSize'), stFormat: $('stFormat'), stFormatSep: $('stFormatSep'),
      stTool: $('stTool'), stLayer: $('stLayer'), stZoom: $('stZoom'), stCell: $('stCell'),
      stTile: $('stTile'),
      stSel: $('stSel'), stClip: $('stClip'),
      stChanged: $('stChanged'), stMode: $('stMode'),
      selBtns: $('selBtns'), selCopy: $('selCopy'), selCut: $('selCut'), selPaste: $('selPaste'),
      selDel: $('selDel'), selFill: $('selFill'), selClear: $('selClear'),
      selFoot: $('selFoot'), selAnchor: $('selAnchor'),
      insCell: $('insCell'), insChunk: $('insChunk'), insPix: $('insPix'),
      panelLeft: $('panelLeft'), panelRight: $('panelRight'), drawerHandle: $('drawerHandle'),
      openDialog: $('openDialog'), openForm: $('openForm'), openPath: $('openPath'),
      openNote: $('openNote'), recentPaths: $('recentPaths'),
      mapSearch: $('mapSearch'), mapRefresh: $('mapRefresh'),
      mapList: $('mapList'), mapListMeta: $('mapListMeta'), openFallback: $('openFallback'),
      libKinds: $('libKinds'), libList: $('libList'), libFoot: $('libFoot'),
      stampQ: $('stampQ'), stampSearch: $('stampSearch'), stampList: $('stampList'), stampFoot: $('stampFoot'),
      toast: $('toast')
    };

    ME.panels.init({
      libKinds: el.libKinds, libList: el.libList, libFoot: el.libFoot,
      stampQ: el.stampQ, stampSearch: el.stampSearch,
      stampList: el.stampList, stampFoot: el.stampFoot,
      insCell: el.insCell, insChunk: el.insChunk, insPix: el.insPix
    });

    // 地图列表（GET /api/maps）：点一项走与手打路径**完全相同**的 open 通路
    ME.mapList.mount({
      dialog: el.openDialog, search: el.mapSearch, refresh: el.mapRefresh,
      meta: el.mapListMeta, list: el.mapList
    }, function (path) { ME.emit('map:open-path', path); });

    /* --- 工具 --- */
    el.toolGrid.addEventListener('click', function (ev) {
      var btn = ev.target.closest('.tool');
      if (!btn) return;
      if (btn.disabled) {
        var t = ME.TOOLS[btn.dataset.tool];
        toast(t && t.why ? t.why : '该工具暂不可用');
        return;
      }
      ME.emit('tool:select', btn.dataset.tool);
    });

    /* --- 笔刷尺寸（值变化 ⇒ `brush:size` 事件，由 app.js 发一条 SetBrushSize） --- */
    el.brushSizeRange.addEventListener('input', function () {
      ME.emit('brush:size', Number(el.brushSizeRange.value));
    });
    el.brushSizeRange.addEventListener('pointerdown', function () { brushBusy = true; });
    el.brushSizeRange.addEventListener('pointercancel', function () { brushBusy = false; syncBrushSize(); });
    // 松手（含在滑杆外松手）都会来一次 change：此后允许按回执回填滑块
    el.brushSizeRange.addEventListener('change', function () { brushBusy = false; syncBrushSize(); });
    el.brushPresets.addEventListener('click', function (ev) {
      var b = ev.target.closest('.bsize__preset');
      if (!b) return;
      ME.emit('brush:size', Number(b.dataset.size));
    });

    /* --- 图层 --- */
    el.layerList.addEventListener('click', function (ev) {
      var row = ev.target.closest('.layer');
      if (!row) return;
      var key = row.dataset.layer;
      if (ev.target.closest('.layer__eye')) ME.emit('layer:toggle', key);
      else ME.emit('layer:current', key);
    });

    /* --- 开启门（视图选项：**不发后端命令**，只换 /api/tiles 的 doors 参数并重取瓦片） --- */
    el.doorsToggle.addEventListener('change', function () {
      ME.emit('doors:toggle', el.doorsToggle.checked);
    });

    /* --- 地图光照三选（视图选项：**不发后端命令**，只换 lighting 参数并重取瓦片） --- */
    if (el.lightingGroup) {
      el.lightingGroup.addEventListener('click', function (ev) {
        var btn = ev.target.closest('.lighting__btn');
        if (!btn) return;
        ME.emit('lighting:select', btn.dataset.lighting);
      });
    }

    /* --- 缩放 --- */
    el.zoomIn.addEventListener('click', function () { ME.canvas.zoomStep(+1); });
    el.zoomOut.addEventListener('click', function () { ME.canvas.zoomStep(-1); });
    el.zoomFit.addEventListener('click', function () { ME.canvas.fit(); });

    /* --- 顶栏 --- */
    el.btnOpen.addEventListener('click', openMapDialog);
    el.btnSave.addEventListener('click', function () { ME.emit('map:save'); });
    el.btnUndo.addEventListener('click', function () { ME.emit('map:undo'); });
    el.btnRedo.addEventListener('click', function () { ME.emit('map:redo'); });
    el.bannerClose.addEventListener('click', hideBanner);
    el.warnClose.addEventListener('click', hideWarn);
    // 关掉错误条时，如果还停在印章待放置态，把提示条接回来（否则用户以为模式没了）
    el.modeBarClose.addEventListener('click', function () {
      if (ME.stampMode.armed) ME.stampMode.refresh();
      else hideMode();
    });

    /* --- 选区 / 剪贴板按钮（PC 左栏 = 手机底部抽屉，同一套 DOM） --- */
    el.selCopy.addEventListener('click', function () { ME.selection.copy(); });
    el.selCut.addEventListener('click', function () { ME.selection.cut(); });
    el.selPaste.addEventListener('click', function () { ME.selection.paste(); });
    el.selDel.addEventListener('click', function () { ME.selection.del(); });
    el.selFill.addEventListener('click', function () { ME.selection.fill(); });
    el.selClear.addEventListener('click', function () { ME.selection.clear(); });

    /* --- 打开地图对话框提交 --- */
    el.openForm.addEventListener('submit', function (ev) {
      // 「关闭」按钮：交给 <form method="dialog"> 自己关
      if (ev.submitter && ev.submitter.value === 'cancel') return;
      // 其余情况（点「按此路径打开」/ 在路径框里回车）都是**手打路径**这条退路。
      // ⚠ 不用 `document.activeElement` 判「是不是在搜索框里回车」：搜索框的 Enter 已由
      // `maplist.js` 的 keydown 拦截并 preventDefault，走不到这里；而拿焦点位置去猜分支
      // 会在「搜索框还聚焦时点按钮」这类情况下走错路（实测会去打开列表第一项）。
      var path = el.openPath.value.trim();
      if (!path) { ev.preventDefault(); toast('请填写地图路径（或点列表里的一项）'); return; }
      ME.emit('map:open-path', path);
    });

    /* --- 手机抽屉 --- */
    function setDrawer(open) {
      el.panelLeft.classList.toggle('is-open', open);
      el.drawerHandle.setAttribute('aria-expanded', open ? 'true' : 'false');
    }
    el.drawerHandle.addEventListener('click', function () {
      setDrawer(!el.panelLeft.classList.contains('is-open'));
    });
    var dragY0 = null;
    el.drawerHandle.addEventListener('pointerdown', function (ev) { dragY0 = ev.clientY; });
    el.drawerHandle.addEventListener('pointerup', function (ev) {
      if (dragY0 === null) return;
      var dy = ev.clientY - dragY0;
      dragY0 = null;
      if (dy < -20) setDrawer(true);
      else if (dy > 20) setDrawer(false);
    });

    /* --- 手机浮层 --- */
    function setSheet(open) { el.panelRight.classList.toggle('is-open', open); }
    el.btnSheet.addEventListener('click', function () { setSheet(!el.panelRight.classList.contains('is-open')); });
    el.btnSheetClose.addEventListener('click', function () { setSheet(false); });

    /* --- 键盘快捷键 --- */
    document.addEventListener('keydown', function (ev) {
      var tag = (ev.target && ev.target.tagName) || '';
      if (tag === 'INPUT' || tag === 'TEXTAREA') return;
      var k = (ev.key || '').toLowerCase();
      var ctrl = ev.ctrlKey || ev.metaKey;

      if (ctrl && k === 'z' && !ev.shiftKey) { ev.preventDefault(); ME.emit('map:undo'); return; }
      if (ctrl && (k === 'y' || (k === 'z' && ev.shiftKey))) { ev.preventDefault(); ME.emit('map:redo'); return; }
      if (ctrl && k === 's') { ev.preventDefault(); ME.emit('map:save'); return; }

      /* 选区 / 剪贴板：Ctrl+C / Ctrl+X / Ctrl+V / Delete / Esc（取消选区） */
      if (ctrl && k === 'c') { ev.preventDefault(); ME.selection.copy(); return; }
      if (ctrl && k === 'x') { ev.preventDefault(); ME.selection.cut(); return; }
      if (ctrl && k === 'v') { ev.preventDefault(); ME.selection.paste(); return; }
      if (!ctrl && k === 'delete') { ev.preventDefault(); ME.selection.del(); return; }
      if (!ctrl && k === 'enter') {
        // 印章列表里选中一行后按 Enter ⇒ 进入待放置态（输入框里的 Enter 已在上面 return）
        if (ME.panels.armSelected()) ev.preventDefault();
        return;
      }

      var map = { b: 'brush', f: 'fill', e: 'eraser', i: 'pick', s: 'select', h: 'pan' };
      if (!ctrl && map[k]) { ME.emit('tool:select', map[k]); return; }
      // 笔刷尺寸 ±2（官方 MainWindow.axaml.cs:871：`[` / `]` 各走 2，夹 3..31）。
      // 越界也照发：夹取是后端的事，前端自己夹会掩盖「到底谁在规范化」。
      if (!ctrl && (k === '[' || k === ']')) {
        ME.emit('brush:size', ((ME.state && ME.state.brushSize) || 3) + (k === ']' ? 2 : -2));
        return;
      }
      if (k === '+' || k === '=') { ME.canvas.zoomStep(+1); return; }
      if (k === '-' || k === '_') { ME.canvas.zoomStep(-1); return; }
      if (k === '0') { ME.canvas.fit(); return; }
      if (k === 'escape') {
        // 优先级：先撤待放置态，再撤选区，最后才是关抽屉/浮层
        if (ME.stampMode.cancel()) return;
        if (ME.selection.sel || ME.selection.dragging) { ME.selection.clear(); return; }
        setSheet(false); setDrawer(false);
      }
    });

    /* --- 取图失败：只对「同一句原因」提示一次，避免刷屏 --- */
    ME.on('tiles:failed', function (p) {
      var msg = p.error ? ME.api.describe(p.error) : '取图失败';
      if (msg === lastTilesError) return;
      lastTilesError = msg;
      toast(msg, 6000);
    });
    ME.on('hover:cell', function () { refresh(); ME.panels.refreshInspector(); });
    ME.on('view:zoom', refresh);
    // 每来一块瓦片刷新「瓦片 scale / 字节数」（切档后字节数立刻变化，是体积差的肉眼证据）。
    // ⚠ 只认负载里的 `scale`（= `fetchTile` 的 `scaleActual`，读不到就是 null）；
    //   这里**不许**用 `p.params.scale` 去填实际值 —— 那正是本次要根除的冒充。
    ME.on('tiles:loaded', function (p) {
      var act = (p && p.scale !== null && p.scale !== undefined && String(p.scale).trim() !== '')
        ? String(p.scale).trim()
        : null;
      var lit = (p && p.lighting !== null && p.lighting !== undefined &&
                 String(p.lighting).trim() !== '')
        ? String(p.lighting).trim()
        : null;
      lastTile = {
        scaleActual: act,
        scaleRequested: (p && p.params && p.params.scale) ? String(p.params.scale) : null,
        lightingActual: lit,
        bytes: (p && typeof p.bytes === 'number') ? p.bytes : null,
        token: ME.tiles.mapToken
      };
      refreshTileStat();
    });
    ME.on('tool:select', function () { syncTools(); refresh(); });
    ME.on('selection:changed', function () { refresh(); });
    ME.on('stamp:armed', function () { refresh(); });

    syncTools();
    syncLayers();
    refresh();
  }

  ME.ui = {
    init: init,
    refresh: refresh,
    syncLayers: syncLayers,
    syncTools: syncTools,
    syncBrushSize: syncBrushSize,
    layerParams: layerParams,
    toast: toast,
    showBanner: showBanner,
    hideBanner: hideBanner,
    showWarn: showWarn,
    hideWarn: hideWarn,
    showMode: showMode,
    hideMode: hideMode,
    syncSelButtons: syncSelButtons,
    openMapDialog: openMapDialog,
    rememberPath: rememberPath,
    get el() { return el; }
  };
})(window.ME);
