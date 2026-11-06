/* =============================================================================
   selection.js —— 矩形选区 + 剪贴板（前端交互层）
   · 后端七条命令全通（`miredit-core::Command` 的变体，见 web/README.md §3.5）：
       {"SetSelection":{"x0","y0","x1","y1"}} / "ClearSelection" / "CopySelection"
       "CutSelection" / {"PasteSelection":{"x","y"}} / "DeleteSelection" / "FillSelection"
   · ⚠ **选区不是工具**：后端 `SetTool` 只收 brush|bucket|eraser|pickup 四个键，
     所以「选择」工具**只在前端切态**（`ME.TOOLS.select.toolKey = null`），一条 SetTool 都不发。
   · 选区真值以**后端回执 note 原文**为准（`SetSelection` 会越界裁剪）：
     解析得到就覆盖本地值；后端说「已取消选区」就清空；解析不出来就保留本地值（不猜）。
   · 剪贴板尺寸同样只认 `CopySelection` / `CutSelection` 回执里的「W×H 格」。
   ============================================================================= */
(function (ME) {
  'use strict';

  var sel = null;        // 已提交选区 {x0,y0,x1,y1}（闭区间，世界格）
  var clip = null;       // 剪贴板 {w,h}
  var drag = null;       // 拖动中 {ax,ay,cur:{x,y}}
  var lastCell = null;   // 最近一次指针所在格（粘贴锚点兜底）

  /* ---------- 几何 ---------- */
  function mapSize() {
    var m = ME.canvas.map;
    return m ? { w: m.width, h: m.height } : null;
  }

  function rectOf(a, b) {
    return {
      x0: Math.min(a.x, b.x), y0: Math.min(a.y, b.y),
      x1: Math.max(a.x, b.x), y1: Math.max(a.y, b.y)
    };
  }

  function rectNow() {
    return drag ? rectOf({ x: drag.ax, y: drag.ay }, drag.cur) : null;
  }

  function wOf(r) { return r ? (r.x1 - r.x0 + 1) : 0; }
  function hOf(r) { return r ? (r.y1 - r.y0 + 1) : 0; }

  /* ---------- 后端 note 解析（原文口径，见 cmd.rs） ---------- */
  /** `选区：3×3 格（(216,5)..(218,7)，闭区间）` → {x0,y0,x1,y1} */
  function parseRect(note) {
    var m = /\((\d+),(\d+)\)\.\.\((\d+),(\d+)\)/.exec(note || '');
    if (!m) return null;
    return { x0: +m[1], y0: +m[2], x1: +m[3], y1: +m[4] };
  }

  /** `已复制 3×3 格（9 格进剪贴板，含空格）` / `剪切 3×3 格（…）` → {w,h} */
  function parseWH(note) {
    var m = /(\d+)×(\d+)\s*格/.exec(note || '');
    if (!m) return null;
    return { w: +m[1], h: +m[2] };
  }

  /* ---------- 状态广播 ---------- */
  function changed() {
    ME.emit('selection:changed', { sel: sel, clip: clip });
  }

  /* ---------- 拖动框选（由 app.js 的 canvas:down/drag/up 显式路由） ---------- */
  /**
   * 按下。返回 `true` = 这次按下已被选区接管（app.js 不再落笔）。
   * 只有当前工具是 `select` 才接管；地图外的按下也吃掉（不落笔、不建选区）。
   */
  function begin(cell) {
    if (ME.state.tool !== 'select') return false;
    if (!cell || !cell.inside) { drag = null; return true; }
    drag = { ax: cell.x, ay: cell.y, cur: { x: cell.x, y: cell.y } };
    ME.canvas.setSelectionPreview(rectNow());
    return true;
  }

  /** 拖动。返回 `true` = 已接管（不落笔）。端点夹在地图内（marquee 常规行为）。 */
  function move(cell) {
    if (!drag) return false;
    var m = mapSize();
    if (!m || !cell) return true;
    drag.cur = {
      x: ME.util.clamp(cell.x, 0, m.w - 1),
      y: ME.util.clamp(cell.y, 0, m.h - 1)
    };
    ME.canvas.setSelectionPreview(rectNow());
    return true;
  }

  /** 松手 ⇒ 提交选区（发 `SetSelection`）。返回 `true` = 已接管。 */
  function end() {
    if (!drag) return false;
    var r = rectNow();
    drag = null;
    ME.canvas.setSelectionPreview(null);
    if (!r) return true;
    // 先本地显示（拖动预览收尾不闪），后端回执到了再按原文校正
    sel = r;
    ME.canvas.setSelection(r);
    changed();
    ME.edit.setSelection(r);
    return true;
  }

  function cancelDrag() {
    if (!drag) return false;
    drag = null;
    ME.canvas.setSelectionPreview(null);
    return true;
  }

  /* ---------- 剪贴板 / 选区命令（按钮与快捷键共用这一份） ---------- */
  function needMap(what) {
    if (ME.state.map) return true;
    ME.ui.toast(what + '需要先打开地图（后端会回 409 no_map）');
    return false;
  }

  function copy() {
    if (!needMap('复制')) return false;
    ME.edit.copySelection();
    return true;
  }

  function cut() {
    if (!needMap('剪切')) return false;
    ME.edit.cutSelection();
    return true;
  }

  function del() {
    if (!needMap('删除')) return false;
    ME.edit.deleteSelection();
    return true;
  }

  function fill() {
    if (!needMap('填充选区')) return false;
    ME.edit.fillSelection();
    return true;
  }

  /** 粘贴锚点 = **当前指针（鼠标 / 触摸）所在格**；指针不在画布上时退回最近一次所在格 */
  function anchor() {
    return ME.canvas.hover || lastCell;
  }

  function paste() {
    if (!needMap('粘贴')) return false;
    var a = anchor();
    if (!a) {
      ME.ui.toast('粘贴锚点 = 指针所在格：把鼠标（或用手指点一下）移到目标格再按 Ctrl+V');
      return false;
    }
    ME.edit.pasteSelection(a.x, a.y);
    return true;
  }

  /** 取消选区（Esc / 按钮）：**不清剪贴板**（后端语义如此，这里如实照做） */
  function clear() {
    var had = !!sel || !!drag;
    drag = null;
    ME.canvas.setSelectionPreview(null);
    sel = null;
    ME.canvas.setSelection(null);
    changed();
    if (ME.state.map) ME.edit.clearSelection();
    return had;
  }

  /** 换图 / 重开图：后端重建会话 ⇒ 选区与剪贴板都没了，本地必须同步清空（不显示旧值） */
  function reset() {
    sel = null;
    clip = null;
    drag = null;
    lastCell = null;
    ME.canvas.setSelection(null);
    ME.canvas.setSelectionPreview(null);
    changed();
  }

  /* ---------- 状态文本（状态栏 / 左栏脚注共用） ---------- */
  function selText() {
    return sel ? ('选区 ' + wOf(sel) + '×' + hOf(sel)) : '选区 —';
  }
  function clipText() {
    return clip ? ('剪贴板 ' + clip.w + '×' + clip.h) : '剪贴板 —';
  }

  /* ---------- 回执 → 本地状态 ---------- */
  /** 成功回执也上屏（`changed > 0` 时 app.js 不弹，只有 changed=0 才弹后端原因） */
  function say(note) {
    if (note) ME.ui.toast(note, 4200);
  }

  function onResult(p) {
    var r = p.result || {};
    var name = (typeof p.cmd === 'string') ? p.cmd : Object.keys(p.cmd)[0];
    var note = r.note || '';

    if (name === 'SetSelection') {
      var rect = parseRect(note);
      if (rect) {
        sel = rect;                       // 后端裁剪后的真值
      } else if (/已取消选区/.test(note)) {
        sel = null;                       // 整块在地图外 ⇒ 后端已取消
      }
      ME.canvas.setSelection(sel);
      changed();
      ME.debug.push('sel:set', { sel: sel, note: note });
    } else if (name === 'ClearSelection') {
      sel = null;
      ME.canvas.setSelection(null);
      changed();
      ME.debug.push('sel:clear', { note: note });
    } else if (name === 'CopySelection' || name === 'CutSelection') {
      var wh = parseWH(note);
      if (wh) {
        clip = wh;
        changed();
      }
      if (r.changed > 0) say(note);
      ME.debug.push('sel:clip', { name: name, clip: clip, note: note });
    } else if (name === 'PasteSelection' || name === 'DeleteSelection' || name === 'FillSelection') {
      if (r.changed > 0) say(note);
      ME.debug.push('sel:region', { name: name, changed: r.changed, note: note });
    }
  }

  function init() {
    ME.on('edit:result', onResult);
    ME.on('hover:cell', function (h) { if (h) lastCell = h; });
    changed();
  }

  ME.selection = {
    init: init,
    reset: reset,
    begin: begin,
    move: move,
    end: end,
    cancelDrag: cancelDrag,
    copy: copy,
    cut: cut,
    paste: paste,
    del: del,
    fill: fill,
    clear: clear,
    anchor: anchor,
    selText: selText,
    clipText: clipText,
    parseRect: parseRect,
    parseWH: parseWH,
    get sel() { return sel; },
    get clip() { return clip; },
    get dragging() { return !!drag; }
  };
})(window.ME);
