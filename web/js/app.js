/* app.js —— 启动流程与状态中枢：探活 / 装配画布 / 命令派发 / WS 事件。
   后端不通时如实报错不画假地图；所有编辑命令与状态刷新都按真回执。 */
(function (ME) {
  'use strict';

  var state = {
    mode: 'offline',              // 'api' | 'offline'
    version: '',
    map: null,                    // {path,width,height,format,dirty,tolerant_trailer_bytes}
    tool: 'brush',
    // 后端会话的笔刷边长（3..=31 的奇数）。**真值只来自 `SetBrushSize` 回执**
    // 的 note「笔刷尺寸：N×N」；缺省 3 = 内核 BRUSH_DEFAULT（brush.rs:294）
    brushSize: 3,
    editLayer: 'ground',
    layers: { ground: true, middle: true, object: true, low: false, high: false },
    doorsOpen: false,             // 「开启门」（视图选项，不发后端命令）⇒ /api/tiles?doors=1
    lighting: 'day',              // 「地图光照」（三档互斥，不发后端命令）⇒ /api/tiles?lighting=
    canUndo: false,
    canRedo: false,
    lastChanged: null,            // 最近一条命令的 changed（后端回执原值）
    totalChanged: 0,              // 本次会话累计
    brush: null,                  // {text, sprite} —— 来自 Pick 的真实回执
    eventsBroken: false,
    // 事件通道健康度：badFrames = 解析不了的帧数；unknownStreak = **连续**未知类型条数
    ws: { badFrames: 0, unknown: 0, unknownStreak: 0, anomaly: false }
  };
  ME.state = state;

  var lastPaintCell = '';         // 同一次拖动里已发过的格（合并重复请求）
  var lastNote = '';              // 上一条 note（同一句不重复弹）

  /* ================= 布局断点 ================= */
  var mq = {
    pc: window.matchMedia('(min-width: 1024px)'),
    tablet: window.matchMedia('(min-width: 768px) and (max-width: 1023.98px)'),
    mobile: window.matchMedia('(max-width: 767.98px)')
  };
  var layout = { name: 'pc' };

  function applyLayout() {
    var name = mq.pc.matches ? 'pc' : (mq.tablet.matches ? 'tablet' : 'mobile');
    layout.name = name;
    document.body.classList.remove('layout-pc', 'layout-tablet', 'layout-mobile');
    document.body.classList.add('layout-' + name);
    document.body.dataset.layout = name;
    ME.debug.push('layout', { name: name });
    ME.canvas.schedule();
  }

  function watchLayout() {
    applyLayout();
    Object.keys(mq).forEach(function (k) {
      var m = mq[k];
      if (typeof m.addEventListener === 'function') m.addEventListener('change', applyLayout);
      else if (typeof m.addListener === 'function') m.addListener(applyLayout);
    });
  }

  /* ================= 地图 ================= */
  function showEmpty(show, title, note) {
    ME.ui.el.stageEmpty.hidden = !show;
    if (show) {
      if (title) ME.ui.el.stageEmpty.querySelector('.stage__empty-title').textContent = title;
      if (note) ME.ui.el.stageEmpty.querySelector('.stage__empty-note').textContent = note;
    }
  }

  /**
   * `format_label` → 短名：取「（」之前那段。
   *
   * 例：`Old12（12B/格，TMapInfo 旧版）` ⇒ `Old12`。
   * **只做截断、不改写**（原文仍由 `edit_warning` 整句上屏）。
   */
  function shortFormat(label) {
    if (!label) return '格式未知';
    var s = String(label);
    var i = s.indexOf('（');
    return i > 0 ? s.slice(0, i) : s;
  }

  /**
   * 把后端 `edit_warning` **原文**上屏（常驻黄条）。
   *
   * 文案一个字都不自己编：两种来源（未解尾块 / 该格式不承诺编辑语义）由后端
   * `EditSession::edit_warning()` 拼好，可能同时成立（用「；」连接）。
   * 额外标记只在 `editable === false` 时加（`null` = 没开图，不加）。
   */
  function applyWarn() {
    var map = state.map;
    if (!map || !map.edit_warning) { ME.ui.hideWarn(); return; }
    var tag = map.editable === false
      ? (shortFormat(map.format_label) + ' · 未解区语义未知')
      : '';
    ME.ui.showWarn(map.edit_warning, tag);
  }

  /**
   * 地表「不可落笔」：后端声明 `ground_paint_supported === false`（九种里只有 Type5）
   * **且**当前编辑层会写到地表（`ground` / `low_collision`，与内核 `ground_refusal` 同口径）。
   */
  function groundBlocked() {
    if (!state.map || state.map.ground_paint_supported !== false) return false;
    return state.editLayer === 'ground' || state.editLayer === 'low';
  }

  /** 地表被拦时的中文原因（含格式真名，来源 = 后端 `format_label`）。 */
  function groundBlockNote() {
    var label = (state.map && state.map.format_label) || '未知格式';
    return '地表不可落笔：本格式（' + label + '）的背景图号按半分辨率 2×2 块编码，' +
      '逐格写 bk_img 会在写回时被丢弃（后端也会拒绝这笔）。' +
      '请切到「中层图层 / 物体图层」，或换一张非 Type5（Mir3Wemade）格式的图。';
  }

  function applyMap(meta) {
    state.map = {
      path: meta.path,
      width: meta.width,
      height: meta.height,
      format: meta.format,
      dirty: !!meta.dirty,
      tolerant_trailer_bytes: meta.tolerant_trailer_bytes,
      // 2026-11-05 新增的四项能力位（后端原值，前端不改写）
      format_label: meta.format_label || '',
      editable: meta.editable,                       // bool | null（null = 没开图）
      ground_paint_supported: meta.ground_paint_supported,  // bool | null
      edit_warning: meta.edit_warning || null,        // string | null（后端原文）
      disk_cell_len: meta.disk_cell_len               // int | null（变长格式为 null）
    };
    state.canUndo = false;
    state.canRedo = false;
    // 换图 ⇒ 后端重建会话，笔刷尺寸回到内核缺省 3（`EditSession::new`）
    state.brushSize = 3;
    state.lastChanged = null;
    state.totalChanged = 0;
    ME.tiles.clear();
    ME.canvas.setMap({ width: meta.width, height: meta.height });
    ME.canvas.fit();
    // 换图 ⇒ 后端重建会话：选区与剪贴板都没了，本地必须同步清空（不显示上一张图的旧值）
    ME.selection.reset();
    showEmpty(false);

    applyWarn();
    // 新会话的内核工具恒为 Brush（`EditSession::new` 的缺省），前端若停在别的工具上
    // 必须显式同步一次，否则「选了橡皮擦再开图」会画不出东西。
    // ⚠ 「选择」没有后端工具位（`toolKey === null`）⇒ 一条 SetTool 都不发。
    if (state.tool !== 'brush' && ME.TOOLS[state.tool] && ME.TOOLS[state.tool].toolKey) {
      ME.edit.setTool(ME.TOOLS[state.tool].toolKey);
    }
    syncCursor();
    ME.ui.syncLayers();
    ME.ui.refresh();
    ME.debug.push('map:open', {
      path: meta.path, width: meta.width, height: meta.height,
      format: meta.format, format_label: meta.format_label,
      editable: meta.editable, ground_paint_supported: meta.ground_paint_supported,
      trailer: meta.tolerant_trailer_bytes, warning: meta.edit_warning
    });
  }

  async function loadMap() {
    try {
      var m = await ME.api.getMap();
      if (m && m.path && m.width > 0) {
        applyMap(m);
      } else {
        showEmpty(true, '后端已连接，但没有打开地图',
          '点「打开地图…」从列表里选一张（列表来自 MIREDIT_MAP_ROOT）。');
        ME.ui.hideWarn();
        ME.ui.refresh();
      }
    } catch (e) {
      ME.ui.toast('读取 /api/map 失败：' + ME.api.describe(e), 6000);
      showEmpty(true, '读取地图信息失败', ME.api.describe(e));
    }
  }

  async function openMap(path) {
    if (state.mode !== 'api') { ME.ui.toast('后端未连接，无法打开地图'); return false; }
    ME.ui.toast('正在打开 ' + path + ' …', 2500);
    try {
      var m = await ME.api.openMap(path);
      applyMap(m);
      ME.ui.rememberPath(path);
      ME.ui.toast('已打开 ' + ME.util.baseName(m.path) + '（' + m.width + '×' + m.height + ' 格 / ' +
        (m.format || '格式未知') + '）');
      ME.debug.push('map:open-ok', { path: path });
      return true;
    } catch (e) {
      var msg = ME.api.describe(e);
      ME.ui.toast('打开失败 · ' + msg, 7000);
      ME.debug.push('map:open-error', { path: path, code: e.code, message: e.message });
      return false;
    }
  }

  /* ================= 编辑 ================= */
  function commandName(cmd) {
    if (typeof cmd === 'string') return cmd;
    return Object.keys(cmd)[0];
  }

  function noteOnce(text) {
    if (!text || text === lastNote) return;
    lastNote = text;
    ME.ui.toast(text, 4200);
  }

  /** 从 Pick 回执里读出笔刷（后端原文形如「吸管：地表 图号 1234 库 0（来自 (10,10)）」） */
  function updateBrushFromNote(note) {
    if (!note) return;
    var m = /图号\s*(\d+)/.exec(note);
    state.brush = {
      text: note.replace(/^吸管：/, ''),
      sprite: m ? Number(m[1]) : null
    };
    ME.debug.push('brush', state.brush);
  }

  function applyToolAt(cell) {
    if (!cell) return;
    var tool = state.tool;
    // 「选择」/「平移」不吃落笔：
    // · 选择 —— 按下/拖动由 selection.js 接管（不发 SetTool，也不发 Paint）
    // · 平移 —— 由 touch.js 的 wantPan 接管（单指拖动 = 挪视口，绝不改图）
    if (tool === 'select' || tool === 'pan') return;

    // ① 地表不可落笔（Type5）：**前端先拦**，别每点一下都白跑一趟网络等后端拒绝。
    //    吸管是只读的，不拦。
    if (tool !== 'pick' && groundBlocked()) {
      ME.ui.toast(groundBlockNote(), 6000);
      return;
    }

    if (!state.map) {
      // 没有地图也把命令真发出去：后端回 409 no_map，用户看到的是**后端原话**
      // （前端自己编一句「请先打开地图」不算错，但不如让契约说话）
      var x = ME.util.clamp(cell.x, 0, 65535), y = ME.util.clamp(cell.y, 0, 65535);
      if (tool === 'fill') ME.edit.fill(x, y);
      else if (tool === 'pick') ME.edit.pick(x, y);
      else if (tool === 'eraser') ME.edit.erase(x, y);
      else ME.edit.paint(x, y);
      return;
    }
    if (!cell.inside) return;   // 点在地图外：什么都不做（后端也会按越界拒绝）
    if (tool === 'brush') {
      var k = cell.x + ',' + cell.y;
      if (k === lastPaintCell) return;
      lastPaintCell = k;
      ME.edit.paint(cell.x, cell.y);
    } else if (tool === 'eraser') {
      // 橡皮与笔刷同样按格发（同一格在一次拖动里只发一次）
      var ke = 'e' + cell.x + ',' + cell.y;
      if (ke === lastPaintCell) return;
      lastPaintCell = ke;
      ME.edit.erase(cell.x, cell.y);
    } else if (tool === 'fill') {
      ME.edit.fill(cell.x, cell.y);
    } else if (tool === 'pick') {
      ME.edit.pick(cell.x, cell.y);
    }
  }

  /**
   * 画布光标：待放置态（印章）> 选区工具 > 默认。
   * 三者互斥，统一在这里算，避免两个模块各设各的互相覆盖。
   */
  function syncCursor() {
    if (ME.stampMode.armed) ME.canvas.setCursor('stamping');
    else if (state.tool === 'select') ME.canvas.setCursor('selecting');
    else ME.canvas.setCursor(null);
  }

  function bindCanvasEditing() {
    ME.on('canvas:down', function (p) {
      lastPaintCell = '';
      lastNote = '';
      // 优先级：印章待放置态 > 选区框选 > 当前工具落笔
      if (ME.stampMode.place(p.cell)) return;
      if (ME.selection.begin(p.cell)) return;
      applyToolAt(p.cell);
    });
    ME.on('canvas:drag', function (p) {
      if (ME.selection.move(p.cell)) return;
      if (ME.stampMode.armed) return;         // 待放置态不画、不擦
      // 笔刷与橡皮都吃拖动（都是「按尺寸落一笔」）；填充/吸管只在按下那一下生效
      if (state.tool !== 'brush' && state.tool !== 'eraser') return;
      applyToolAt(p.cell);
    });
    ME.on('canvas:up', function () {
      lastPaintCell = '';
      ME.selection.end();                     // 松手 ⇒ 提交选区（发 SetSelection）
    });
    ME.on('canvas:cancel', function () {
      lastPaintCell = '';
      ME.selection.cancelDrag();
    });
  }

  /** 「整块」类命令：成功后必须整屏作废瓦片缓存（WS rerender 只带锚点一格，详见 README §9）。
   * `Fill` 是 BFS 连通填充，一次可能改满整张图，必须在这个表里。 */
  var REGION_CMDS = {
    Fill: 1, PlaceStamp: 1, PasteSelection: 1, DeleteSelection: 1,
    CutSelection: 1, FillSelection: 1
  };

  function bindEditEvents() {
    ME.on('edit:result', function (p) {
      var r = p.result || {};
      state.canUndo = !!r.can_undo;
      state.canRedo = !!r.can_redo;
      if (typeof r.changed === 'number') {
        state.lastChanged = r.changed;
        state.totalChanged += r.changed;
      }
      if (state.map && typeof r.dirty === 'boolean') state.map.dirty = r.dirty;

      var name = commandName(p.cmd);
      if (name === 'Pick' && r.note) updateBrushFromNote(r.note);
      // 笔刷尺寸：真值只认后端回执里的「笔刷尺寸：N×N」—— 它已把 999 这类值夹成 31，
      // 前端照抄这个数上屏，不自己夹（夹一次就等于伪造后端没做过的事）
      if (name === 'SetBrushSize' && r.note) {
        var bm = /笔刷尺寸：(\d+)×/.exec(r.note);
        if (bm) state.brushSize = Number(bm[1]);
      }
      if (name === 'SetLayer' && r.note) noteOnce(r.note);
      if (name !== 'Paint' && r.note && r.changed === 0) noteOnce(r.note);
      if (r.changed > 0 && REGION_CMDS[name]) {
        ME.tiles.invalidate(null);       // null ⇒ 清空全部缓存，重取可见块
        ME.canvas.schedule();
      }

      ME.debug.push('edit:ok', { cmd: p.cmd, changed: r.changed, dirty: r.dirty, note: r.note });
      ME.ui.refresh();
    });

    ME.on('edit:error', function (p) {
      var e = p.error || {};
      var msg = ME.api.describe(e);
      ME.ui.toast(commandName(p.cmd) + ' 失败 · ' + msg, 7000);
      ME.debug.push('edit:error', { cmd: p.cmd, code: e.code, message: e.message });
      if (e.code === 'no_map') showEmpty(true, '尚未打开地图', '先「打开地图…」再编辑。');
    });
  }

  /**
   * 保存。**回执必须自己说成功**（`saved === true` 且 `bytes` 是数字）：
   * 只看 HTTP 200 是不够的 —— 代理回 200 空体、或后端将来加 `saved:false` 分支，
   * 都会让「不查就当成功」的写法报出「已保存 undefined 字节」并把脏标记清掉。
   * 失败时**保留脏标记**并显示原因（绝不假装成功）。
   */
  async function saveMap() {
    if (state.mode !== 'api') { ME.ui.toast('后端未连接：保存需 POST /api/save，当前不会写盘。'); return false; }
    if (!state.map) { ME.ui.toast('尚未打开地图（保存会返回 409 no_map）'); return false; }
    var path = state.map.path;
    try {
      var r = await ME.edit.save();
      if (!r || r.saved !== true) {
        var why = (r && r.note) ? r.note : ('回执里 saved=' + JSON.stringify(r && r.saved) + '（不是 true）');
        ME.ui.toast('保存未确认 · ' + why, 9000);
        ME.debug.push('save:unconfirmed', { receipt: r, reason: 'saved != true', dirtyLeft: state.map ? state.map.dirty : null });
        ME.ui.refresh();
        return false;
      }
      if (typeof r.bytes !== 'number' || !isFinite(r.bytes)) {
        ME.ui.toast('保存未确认 · 回执缺 bytes（收到 ' + JSON.stringify(r.bytes) +
          '），无法核对落盘长度', 9000);
        ME.debug.push('save:unconfirmed', { receipt: r, reason: 'bytes 不是数字', dirtyLeft: state.map ? state.map.dirty : null });
        ME.ui.refresh();
        return false;
      }
      if (state.map && state.map.path === path) state.map.dirty = false;
      ME.ui.refresh();
      ME.ui.toast('已保存 ' + r.bytes + ' 字节' + (r.note ? ' · ' + r.note : ''));
      ME.debug.push('save:ok', { bytes: r.bytes, note: r.note, saved: r.saved });
      return true;
    } catch (e) {
      // HTTP 层失败（500 io_error / 403 / 409 …）：原因照后端原文显示，脏标记一个字都不动
      ME.ui.toast('保存失败 · ' + ME.api.describe(e), 9000);
      ME.debug.push('save:error', { code: e.code, message: e.message, dirty: state.map ? state.map.dirty : null });
      ME.ui.refresh();
      return false;
    }
  }

  function undo() {
    if (!state.map) { ME.ui.toast('尚未打开地图（撤销会返回 409 no_map）'); return; }
    if (!state.canUndo) { ME.ui.toast('没有可撤销的步骤'); return; }
    ME.edit.undo();
  }

  function redo() {
    if (!state.map) { ME.ui.toast('尚未打开地图（重做会返回 409 no_map）'); return; }
    if (!state.canRedo) { ME.ui.toast('没有可重做的步骤'); return; }
    ME.edit.redo();
  }

  /* ================= 事件绑定 ================= */
  function bindActions() {
    ME.on('tool:select', function (tool) {
      if (!ME.TOOLS[tool]) return;
      if (!ME.TOOLS[tool].usable) {
        ME.ui.toast(ME.TOOLS[tool].why, 5200);
        return;
      }
      state.tool = tool;
      ME.ui.syncTools();
      ME.ui.refresh();
      syncCursor();
      // 工具必须**同步到后端**：`Command::Erase` 执行后内核工具留在橡皮
      // （`cmd.rs` 的注释写明与 GUI 的 `--edit-erase` 一致），而 `Paint` 是按当前工具
      // 分派的 ⇒ 不显式切回，擦完再落笔会继续擦。没开图时不发（会白拿一个 409）。
      // ⚠「选择」的 `toolKey` 是 null（后端只收 brush/bucket/eraser/pickup）⇒ **不发 SetTool**。
      if (state.map && ME.TOOLS[tool].toolKey) ME.edit.setTool(ME.TOOLS[tool].toolKey);
    });

    /**
     * 笔刷尺寸：**值原样发**（`{"SetBrushSize":{"size":N}}`），夹取/抬奇数由后端做。
     * 走 `edit.js` 已有的 `setBrushSize`（不另写一份请求），回执走 `edit:result` 的
     * 通用 toast 通路（`changed === 0` + note ⇒ 弹「笔刷尺寸：N×N」）。
     * 未开图时不发：后端这条命令要写会话，没会话只会白拿一个 409 no_map。
     */
    ME.on('brush:size', function (n) {
      if (!state.map) { ME.ui.toast('尚未打开地图：笔刷尺寸要写进后端会话，先「打开地图…」'); return; }
      ME.edit.setBrushSize(n);
    });

    ME.on('layer:toggle', function (key) {
      if (!ME.LAYERS[key]) return;
      state.layers[key] = !state.layers[key];
      ME.ui.syncLayers();       // → canvas.setLayers(名字列表) → 重新取图（缓存命中则零请求）
      ME.ui.refresh();
    });

    // 开启门：只改视图选项（不发 /api/edit）；缓存键含 doors ⇒ 勾上重取、取消勾选命中旧块
    ME.on('doors:toggle', function (on) {
      state.doorsOpen = !!on; ME.ui.syncLayers(); ME.ui.refresh();
    });

    // 地图光照：只改视图选项（不发 /api/edit）；缓存键含 lighting ⇒ 切档重取、切回命中旧块。
    // 档位合法性由 ME.lightingParam 归一（未知值不当成新档，界面也不会假装切了）。
    ME.on('lighting:select', function (value) {
      state.lighting = ME.lightingParam(value); ME.ui.syncLayers(); ME.ui.refresh();
    });

    ME.on('layer:current', function (key) {
      if (!ME.LAYERS[key]) return;
      state.editLayer = key;
      ME.ui.syncLayers();
      ME.ui.refresh();
      // 切编辑层 = 一条真实命令（SetLayer），回执里会带该层的图号/库号字段名
      if (state.map) ME.edit.setLayer(ME.LAYERS[key].cmd);
    });

    ME.on('map:open-path', function (path) { openMap(path); });
    ME.on('map:save', saveMap);
    ME.on('map:undo', undo);
    ME.on('map:redo', redo);
  }

  /* ================= WS 事件 ================= */
  /**
   * 已知事件类型：只有这三种（后端 `state.rs:88-100`）。
   * 认不出的类型**不是**「正常但暂时用不上」，而是协议对不上 —— 连接还活着，
   * 脏标记与瓦片失效却都不会再自动刷新，这正是最该报警的形态。
   */
  var KNOWN_EVENTS = { dirty: 1, rerender: 1, error: 1 };
  var WS_UNKNOWN_LIMIT = 3;      // 连续几条未知类型就报「事件通道异常」

  function wsAnomaly(note) {
    state.ws.anomaly = true;
    ME.ui.showBanner('事件通道异常', note);
    ME.ui.refresh();             // 状态栏同步显示「事件通道异常」
  }

  /** 一条事件报文（已解析成对象）→ 分派；未知类型累计到 N 条就报「事件通道异常」 */
  function dispatchEvent(msg) {
    if (!msg || typeof msg.type !== 'string' || !msg.type) {
      onBadFrame({ reason: '没有 type 字段', raw: JSON.stringify(msg === undefined ? null : msg).slice(0, 200) });
      return;
    }
    var p = msg.payload || {};
    if (!KNOWN_EVENTS[msg.type]) {
      state.ws.unknown++;
      state.ws.unknownStreak++;
      ME.debug.push('ws:unknown', { type: msg.type, streak: state.ws.unknownStreak, payload: p });
      if (state.ws.unknownStreak >= WS_UNKNOWN_LIMIT) {
        wsAnomaly('连接还在，但连续 ' + state.ws.unknownStreak + ' 条报文的事件类型都不认识（最近一条 type=' +
          msg.type + '，累计 ' + state.ws.unknown + ' 条）。协议可能已变更：脏标记与瓦片失效都不会再自动刷新，' +
          '改完请手动「保存地图」并刷新页面核对。');
      }
      return;
    }
    state.ws.unknownStreak = 0;      // 认识的那条一来，连续计数清零
    if (msg.type === 'dirty') {
      if (state.map) state.map.dirty = !!p.dirty;
      ME.ui.refresh();
    } else if (msg.type === 'rerender') {
      ME.tiles.invalidate(p);
      ME.canvas.schedule();
    } else if (msg.type === 'error') {
      ME.ui.toast('后端事件错误：' + (p.message || p.code || '未知'), 5000);
    }
    ME.debug.push('ws:' + msg.type, p);
  }

  /** 一帧解析失败 / 没有 type —— **必须可见**：旧实现 `catch { return; }` 是静默吞掉 */
  function onBadFrame(info) {
    state.ws.badFrames++;
    ME.debug.push('ws:bad-frame', info);
    wsAnomaly('收到 ' + state.ws.badFrames + ' 条无法解析的事件报文（' + info.reason +
      (info.raw ? '，最近一帧：' + info.raw : '') +
      '）。连接还在，但前端已经读不懂后端在说什么：脏标记与瓦片失效不会自动刷新，' +
      '改完请手动「保存地图」并刷新页面核对。');
  }

  function subscribeEvents() {
    ME.api.openEvents(dispatchEvent, function () {
      state.eventsBroken = true;
      ME.ui.refresh();
      ME.ui.showBanner('事件通道断开（不影响编辑）',
        'WebSocket /api/events 已断开：编辑仍走 HTTP（/api/edit），只是不会再自动收到 dirty / rerender 推送。');
      ME.debug.push('ws:closed', {});
    }, function () {
      state.eventsBroken = false;
      if (!state.ws.anomaly) ME.ui.hideBanner();   // 协议异常横幅不被「重连成功」冲掉
      ME.debug.push('ws:open', {});
    }, onBadFrame);
  }

  /* ================= 启动 ================= */
  async function bootstrap() {
    ME.ui.init();
    ME.canvas.init(document.getElementById('mapCanvas'));
    ME.touch.bind(document.getElementById('mapCanvas'));
    ME.selection.init();
    ME.stampMode.init();
    watchLayout();
    bindActions();
    bindCanvasEditing();
    bindEditEvents();
    ME.on('stamp:armed', syncCursor);

    var health = await ME.api.health();
    if (health.ok) {
      state.mode = 'api';
      state.version = health.version;
      document.body.dataset.mode = 'api';
      document.body.dataset.connected = 'true';
      subscribeEvents();
      await loadMap();
      ME.panels.loadLibs('ground');
      ME.panels.loadStamps('', 0);
    } else {
      state.mode = 'offline';
      document.body.dataset.mode = 'offline';
      document.body.dataset.connected = 'false';
      ME.ui.showBanner('后端未连接：/api/health 无响应',
        '（' + (health.reason || '未探测') + '）请先启动 miredit-web（MIREDIT_BIND / MIREDIT_MAP_ROOT 是环境变量）。' +
        '未连接时画布不显示任何地图，也不发送编辑请求。');
      showEmpty(true, '后端未连接', '/api/health 无响应。启动后端后刷新页面。');
    }

    ME.ui.syncLayers();
    ME.ui.refresh();
    ME.debug.push('boot', { mode: state.mode, version: state.version, layout: layout.name, health: health });
  }

  ME.app = {
    openMap: openMap,
    save: saveMap,
    undo: undo,
    redo: redo,
    loadMap: loadMap,
    /**
     * 供 CDP / 自动化：把**一帧原始报文**喂给与 WebSocket 完全同一条
     * 「解析 → 分派」通路（`ME.api.decodeFrame` + `dispatchEvent` / `onBadFrame`）。
     * 不是另写一套判定：真 WS 的 `message` 监听调的就是这两个函数。
     */
    feedEventFrame: function (raw) {
      return ME.api.decodeFrame(raw, dispatchEvent, onBadFrame);
    },
    /** 供 CDP / 自动化：直接喂一条已解析的事件对象（同 `dispatchEvent`） */
    feedEvent: dispatchEvent,
    get layout() { return layout.name; }
  };

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', bootstrap);
  } else {
    bootstrap();
  }
})(window.ME);
