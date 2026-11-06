/* =============================================================================
   panels.js —— 右栏三块：资源库（/api/palette）、印章（/api/stamps）、单元格检查器
   · 资源库 item 是**库级** {lib,name,frames}：这里只列库；点开只说明「图块级待接入」，
     因为后端**没有**按帧取图的端点（不画假图块、不猜图号）
   · 印章 item 是 {id,name,kind,kind_zh,cells,bounds}，**没有 preview 字段**：
     所以只用文字 + 格数展示，绝不画假缩略图
   ============================================================================= */
(function (ME) {
  'use strict';

  var el = {};
  var libKind = 'ground';
  var libData = null;         // {items,total}
  var libOpen = null;         // 展开的库号
  var libLoading = false;
  var stampQ = '';
  var stampPage = 0;
  var stampData = null;       // {items,total}
  var stampLoading = false;
  var stampSel = null;        // 当前选中的印章 id（Enter 用它进入待放置态）
  var STAMP_PAGE_SIZE = 20;
  var LONG_PRESS_MS = 550;    // 手机长按进入待放置态
  var LONG_PRESS_SLOP = 8;    // 手指移动超过这个距离就不算长按

  var KIND_LABEL = { ground: '地表', middle: '中层', object: '物体' };

  /* ================= 资源库 ================= */
  async function loadLibs(kind) {
    libKind = kind || libKind;
    libOpen = null;
    libLoading = true;
    libData = null;
    renderLibs();
    try {
      libData = await ME.api.palette(libKind, 0);
      ME.debug.push('palette:ok', { kind: libKind, total: libData.total });
    } catch (e) {
      libData = { error: e };
      ME.debug.push('palette:error', { kind: libKind, code: e.code });
    }
    libLoading = false;
    renderLibs();
  }

  function renderLibs() {
    if (!el.libList) return;
    var tabs = el.libKinds.querySelectorAll('.tab');
    for (var i = 0; i < tabs.length; i++) {
      var on = tabs[i].dataset.kind === libKind;
      tabs[i].classList.toggle('is-active', on);
      tabs[i].setAttribute('aria-selected', on ? 'true' : 'false');
    }

    if (libLoading) {
      el.libList.innerHTML = '<p class="listbox__hint">正在读取 <code>/api/palette</code> …</p>';
      el.libFoot.textContent = '—';
      return;
    }
    if (!libData || libData.error) {
      var e = libData ? libData.error : null;
      el.libList.innerHTML = '<p class="listbox__hint">读取失败：' +
        ME.util.esc(e ? ME.api.describe(e) : '未知原因') + '</p>';
      el.libFoot.textContent = 'GET /api/palette?kind=' + libKind;
      return;
    }
    if (!libData.items.length) {
      el.libList.innerHTML = '<p class="listbox__hint">该族没有登记到任何库。</p>';
      el.libFoot.textContent = 'total = 0';
      return;
    }

    var html = '';
    for (var j = 0; j < libData.items.length; j++) {
      var it = libData.items[j];
      var frames = (it.frames === null || it.frames === undefined) ? '帧数未知' : (it.frames + ' 帧');
      html += '<button class="row' + (libOpen === it.lib ? ' is-active' : '') + '" type="button" data-lib="' + it.lib + '">' +
              '<span class="row__name">' + ME.util.esc(it.name) + '</span>' +
              '<span class="row__meta">库 ' + it.lib + ' · ' + frames + '</span></button>';
      if (libOpen === it.lib) {
        html += '<div class="row row--static"><span class="row__note">' +
                '图块级待接入：后端 <code>/api/palette</code> 只到库级（<code>{lib,name,frames}</code>），' +
                '没有「按库取帧」的端点，所以这里不列图块、不猜图号。<br>' +
                '当前取笔刷的唯一通路是<b>吸管</b>（<code>{"Pick":{"x":…,"y":…}}</code>，取地图上该格当前层的图号）。' +
                '</span></div>';
      }
    }
    el.libList.innerHTML = html;
    el.libFoot.textContent = '共 ' + libData.total + ' 个' + KIND_LABEL[libKind] + '库 · 点一行展开';
  }

  /* ================= 印章 ================= */
  async function loadStamps(q, page) {
    stampQ = (q === undefined) ? stampQ : q;
    stampPage = page || 0;
    stampLoading = true;
    stampData = null;
    renderStamps();
    try {
      stampData = await ME.api.stamps('', stampQ, stampPage, STAMP_PAGE_SIZE);
      ME.debug.push('stamps:ok', { q: stampQ, page: stampPage, total: stampData.total });
    } catch (e) {
      stampData = { error: e };
      ME.debug.push('stamps:error', { code: e.code });
    }
    stampLoading = false;
    renderStamps();
  }

  function renderStamps() {
    if (!el.stampList) return;
    if (stampLoading) {
      el.stampList.innerHTML = '<p class="listbox__hint">正在读取 <code>/api/stamps</code> …</p>';
      el.stampFoot.textContent = '—';
      return;
    }
    if (!stampData || stampData.error) {
      var e = stampData ? stampData.error : null;
      el.stampList.innerHTML = '<p class="listbox__hint">读取失败：' +
        ME.util.esc(e ? ME.api.describe(e) : '未知原因') + '</p>';
      el.stampFoot.textContent = 'GET /api/stamps?q=' + stampQ;
      return;
    }
    if (!stampData.items.length) {
      el.stampList.innerHTML = '<p class="listbox__hint">没有匹配的印章。</p>';
      el.stampFoot.textContent = '共 0 条';
      return;
    }

    var armedId = (ME.stampMode && ME.stampMode.armed) ? ME.stampMode.armed.id : null;
    var html = '';
    for (var i = 0; i < stampData.items.length; i++) {
      var s = stampData.items[i];
      var cells = (s.cells === null || s.cells === undefined) ? '格数未知' : (s.cells + ' 格');
      var bounds = s.bounds ? ('[' + s.bounds.join(',') + ']') : '范围未知';
      var cls = 'row stamp-row' +
        (stampSel === s.id ? ' is-active' : '') +
        (armedId === s.id ? ' is-armed' : '');
      html += '<div class="' + cls + '" data-stamp="' + ME.util.esc(s.id) + '"' +
              ' tabindex="0" role="button" aria-label="放置印章 ' + ME.util.esc(s.name || s.id) + '">' +
              '<span class="row__name" title="' + ME.util.esc(s.id) + '">' + ME.util.esc(s.name || s.id) + '</span>' +
              '<span class="row__meta">' + ME.util.esc(s.kind_zh || '未分类') + ' · ' + cells + ' · ' + bounds + '</span>' +
              '<button class="btn btn--sm stamp-place" type="button" data-stamp-place="' + ME.util.esc(s.id) + '"' +
              ' title="进入待放置态，然后点画布落位（Esc 取消）">放置</button>' +
              '</div>';
    }
    el.stampList.innerHTML = html;

    var pages = Math.max(1, Math.ceil(stampData.total / STAMP_PAGE_SIZE));
    el.stampFoot.innerHTML = '共 ' + stampData.total + ' 条 · 第 ' + (stampPage + 1) + '/' + pages + ' 页' +
      ' <button class="btn btn--sm" type="button" data-stamp-page="prev"' + (stampPage <= 0 ? ' disabled' : '') + '>上一页</button>' +
      ' <button class="btn btn--sm" type="button" data-stamp-page="next"' +
      (stampPage + 1 >= pages ? ' disabled' : '') + '>下一页</button>';
    el.stampFoot.innerHTML += '<br>放置：点行内「放置」（或选中行按 Enter，手机长按该行）⇒ 画布光标变十字，' +
      '点目标格即发 <code>{"PlaceStamp":{…}}</code>；<b>Esc</b> 取消。';
  }

  /** 进入待放置态（行内按钮 / Enter / 长按 三条入口共用） */
  function armStamp(id) {
    if (!id) return false;
    var item = null;
    if (stampData && stampData.items) {
      for (var i = 0; i < stampData.items.length; i++) {
        if (stampData.items[i].id === id) { item = stampData.items[i]; break; }
      }
    }
    stampSel = id;
    markArmedRow();
    return ME.stampMode.arm(id, item ? (item.name || item.id) : id);
  }

  /** 选中行（单击行体）：只是选中，按 Enter 才进入待放置态 */
  function selectStamp(id) {
    stampSel = id;
    var rows = el.stampList.querySelectorAll('.stamp-row');
    for (var i = 0; i < rows.length; i++) {
      rows[i].classList.toggle('is-active', rows[i].dataset.stamp === id);
    }
  }

  /** 待放置态换人时只切高亮，不重排列表（避免把用户滚动位置顶掉） */
  function markArmedRow() {
    if (!el.stampList) return;
    var armedId = (ME.stampMode && ME.stampMode.armed) ? ME.stampMode.armed.id : null;
    var rows = el.stampList.querySelectorAll('.stamp-row');
    for (var i = 0; i < rows.length; i++) {
      rows[i].classList.toggle('is-armed', rows[i].dataset.stamp === armedId);
    }
  }

  /** 全局 Enter 快捷键入口：把「当前选中的印章」推进待放置态 */
  function armSelected() {
    if (!stampSel) return false;
    return armStamp(stampSel);
  }

  /* ================= 单元格检查器 ================= */
  function refreshInspector() {
    if (!el.insCell) return;
    var h = ME.canvas.hover;
    if (!h) {
      el.insCell.textContent = '—, —';
      el.insChunk.textContent = '—';
      el.insPix.textContent = '—';
      return;
    }
    var cx = Math.floor(h.x / ME.CHUNK), cy = Math.floor(h.y / ME.CHUNK);
    el.insCell.textContent = h.x + ', ' + h.y;
    el.insChunk.textContent = '(' + cx + ', ' + cy + ') 块 · 格内 ' + (h.x % ME.CHUNK) + ', ' + (h.y % ME.CHUNK);
    var px = ME.tiles.sampleCell(h.x, h.y, ME.canvas.params());
    if (!px) { el.insPix.textContent = '该块未加载'; return; }
    var hex = '#' + [px.r, px.g, px.b].map(function (n) {
      return ('0' + n.toString(16)).slice(-2);
    }).join('');
    el.insPix.textContent = hex + (px.a < 255 ? '（alpha ' + px.a + '）' : '');
  }

  /* ================= 初始化 ================= */
  function init(refs) {
    el = refs;

    el.libKinds.addEventListener('click', function (ev) {
      var b = ev.target.closest('.tab');
      if (b) loadLibs(b.dataset.kind);
    });

    el.libList.addEventListener('click', function (ev) {
      var row = ev.target.closest('.row[data-lib]');
      if (!row) return;
      var lib = Number(row.dataset.lib);
      libOpen = (libOpen === lib) ? null : lib;
      renderLibs();
    });

    el.stampSearch.addEventListener('click', function () {
      loadStamps(el.stampQ.value.trim(), 0);
    });
    el.stampQ.addEventListener('keydown', function (ev) {
      if (ev.key === 'Enter') { ev.preventDefault(); loadStamps(el.stampQ.value.trim(), 0); }
    });
    el.stampFoot.addEventListener('click', function (ev) {
      var b = ev.target.closest('[data-stamp-page]');
      if (!b || b.disabled) return;
      loadStamps(stampQ, b.dataset.stampPage === 'next' ? stampPage + 1 : stampPage - 1);
    });

    /* --- 印章：行内「放置」按钮 / 选中行 / Enter / 长按 --- */
    el.stampList.addEventListener('click', function (ev) {
      var btn = ev.target.closest('[data-stamp-place]');
      if (btn) { armStamp(btn.dataset.stampPlace); return; }
      var row = ev.target.closest('.stamp-row');
      if (row) selectStamp(row.dataset.stamp);
    });

    el.stampList.addEventListener('keydown', function (ev) {
      if (ev.key !== 'Enter' && ev.key !== ' ') return;
      var row = ev.target.closest('.stamp-row');
      if (!row) return;
      ev.preventDefault();
      armStamp(row.dataset.stamp);
    });

    // 长按（触摸为主，鼠标按住不放也算）：550ms 不动 ⇒ 进入待放置态
    var press = null;
    function pressStop() {
      if (press && press.timer) clearTimeout(press.timer);
      press = null;
    }
    el.stampList.addEventListener('pointerdown', function (ev) {
      if (ev.target.closest('[data-stamp-place]')) return;   // 按钮自己会处理
      var row = ev.target.closest('.stamp-row');
      if (!row) return;
      press = { id: row.dataset.stamp, x: ev.clientX, y: ev.clientY, timer: 0 };
      press.timer = setTimeout(function () {
        if (!press) return;
        var id = press.id;
        press = null;
        armStamp(id);
        ME.debug.push('stamp:longpress', { id: id });
      }, LONG_PRESS_MS);
    });
    el.stampList.addEventListener('pointermove', function (ev) {
      if (!press) return;
      if (Math.abs(ev.clientX - press.x) > LONG_PRESS_SLOP ||
          Math.abs(ev.clientY - press.y) > LONG_PRESS_SLOP) pressStop();
    });
    el.stampList.addEventListener('pointerup', pressStop);
    el.stampList.addEventListener('pointercancel', pressStop);
    el.stampList.addEventListener('pointerleave', pressStop);

    // 待放置态变化 ⇒ 行高亮跟着走（Esc 取消时全部熄灭）
    ME.on('stamp:armed', markArmedRow);

    ME.on('tiles:loaded', refreshInspector);
    ME.on('hover:cell', refreshInspector);
  }

  ME.panels = {
    init: init,
    loadLibs: loadLibs,
    loadStamps: loadStamps,
    refreshInspector: refreshInspector,
    armStamp: armStamp,
    armSelected: armSelected,
    get libKind() { return libKind; },
    get stampSel() { return stampSel; }
  };
})(window.ME);
