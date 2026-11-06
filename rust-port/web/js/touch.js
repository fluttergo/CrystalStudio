/* =============================================================================
   touch.js —— 指针手势：工具落点 / 平移 / 缩放
   · 统一走 Pointer Events（鼠标、触摸、触控笔同一套代码）
   · 左键（或单指）= 当前工具落点；中键 / Alt+左键 / 「平移」工具 = 平移；滚轮 / 捏合 = 缩放档
   · **触摸单指落笔要等 TOOL_DELAY_MS（120 ms）**：捏合的第二根手指通常在第一根落下后
     几十毫秒内到位，这段等待就是留给它的。等待期内第二指落下 ⇒ 转捏合，**一笔都不发**。
     （旧实现按下即落笔，第二指落下只取消选区拖动、不回滚已发的命令 ⇒ 每次捏合都白改一笔。）
   · 缩放是「档位式」（25/50/100/200），不做连续缩放：取图倍率只跟档位走
     （ME.tileScaleFor：25% ⇒ 0.25、50% ⇒ 0.5、100%/200% ⇒ 1）
   ============================================================================= */
(function (ME) {
  'use strict';

  var PINCH_IN = 1.35;    // 两指距离放大到 1.35 倍 → 升一档
  var PINCH_OUT = 0.74;   // 缩到 0.74 倍 → 降一档
  var DRAG_SLOP = 3;      // 超过这个位移才算拖动（否则算点击）
  var TOOL_DELAY_MS = 120;  // 触摸单指「按下 → 落笔」的等待窗口（留给第二根手指）
  var TOOL_FLUSH_PX = 10;   // 等待期内位移超过这个值 ⇒ 认定是在拖/画，立刻落笔（不丢笔迹）

  var pointers = new Map();     // pointerId → {x, y, type}
  var mode = 'idle';            // 'idle' | 'tool' | 'pan' | 'pinch'
  var last = { x: 0, y: 0 };
  var downCell = null;
  var moved = false;
  var pinchDist0 = 0;
  var el = null;
  var pendingDown = null;       // {timer, px, py, cell} —— 等待中的触摸落笔（鼠标/触控笔不等待）
  var stats = {
    tools: 0, pans: 0, pinches: 0, zooms: 0, wheel: 0,
    defers: 0,          // 走了等待窗口的落笔次数（= 触摸单指按下次数）
    flushOnMove: 0,     // 等待期内被判成拖动的次数（提前落笔）
    pinchCancels: 0     // 等待期内第二指落下、**成功取消**掉的落笔次数（= 捏合没白改地图）
  };

  function local(ev) {
    var r = el.getBoundingClientRect();
    return { x: ev.clientX - r.left, y: ev.clientY - r.top };
  }

  function dist(a, b) {
    var dx = a.x - b.x, dy = a.y - b.y;
    return Math.sqrt(dx * dx + dy * dy);
  }

  function twoPointers() {
    var it = pointers.values();
    return [it.next().value, it.next().value];
  }

  function zoomTo(dir, ax, ay, reason) {
    var before = ME.canvas.view.scale;
    if (ME.canvas.zoomStep(dir, ax, ay)) {
      stats.zooms++;
      if (reason.indexOf('pinch') === 0) stats.pinches++;
      ME.debug.pushGesture('zoom', { reason: reason, from: before, to: ME.canvas.view.scale });
    }
  }

  /* ---------- 触摸单指的「延迟落笔」 -------------------------------------------
     为什么要延迟：捏合的第一根手指落下时，第二根手指往往还在路上（几十毫秒）。
     旧实现 `pointerdown` 立刻 `canvas:down` ⇒ 第二根落下时那一笔**已经发出去了**，
     `canvas:cancel` 只取消选区拖动，后端已经改了图。
     现在的取舍：触摸单指按下后等 TOOL_DELAY_MS 再落笔；
       · 期间第二指落下 ⇒ 取消等待，转捏合，**一条命令都不发**；
       · 期间位移超过 TOOL_FLUSH_PX ⇒ 是在拖/画，立刻补发，笔迹不缺一截；
       · 期间就抬手了（快速点按）⇒ 抬手时补发，点按照常生效。
     鼠标与触控笔**不等待**（单指针设备上不可能捏合，等待只会让点击变慢）。
     -------------------------------------------------------------------------- */
  function armToolDown(px, py, cell) {
    cancelPending(null);
    pendingDown = {
      timer: setTimeout(function () {
        pendingDown = null;
        stats.defers++;
        ME.emit('canvas:down', { px: px, py: py, cell: cell });
      }, TOOL_DELAY_MS),
      px: px, py: py, cell: cell
    };
  }

  /** 取消等待中的落笔；`reason` 非空时记一条手势日志（供 CDP 断言） */
  function cancelPending(reason) {
    if (!pendingDown) return false;
    clearTimeout(pendingDown.timer);
    pendingDown = null;
    if (reason) {
      stats.pinchCancels++;
      ME.debug.pushGesture('tool-cancel', { reason: reason, at: Date.now() });
    }
    return true;
  }

  /** 立刻把等待中的落笔发出去（拖动 / 抬手时用） */
  function flushPending() {
    if (!pendingDown) return false;
    var d = pendingDown;
    clearTimeout(d.timer);
    pendingDown = null;
    stats.defers++;
    ME.emit('canvas:down', { px: d.px, py: d.py, cell: d.cell });
    return true;
  }

  function onDown(ev) {
    if (ev.pointerType === 'mouse' && ev.button !== 0 && ev.button !== 1) return;
    var p = local(ev);
    pointers.set(ev.pointerId, { x: p.x, y: p.y, type: ev.pointerType });
    try { el.setPointerCapture(ev.pointerId); } catch (e) { /* 某些环境不支持，忽略 */ }

    // 平移触发：中键 / Alt+左键 / 当前工具就是「平移」（手机单指平移的唯一入口）
    var wantPan = (ev.button === 1) || ev.altKey ||
      (!!ME.state && ME.state.tool === 'pan');

    if (pointers.size === 1) {
      last = p;
      moved = false;
      // 按下就更新悬停格：**触摸点击没有 pointermove**，不在这里更新的话
      // 「粘贴锚点 = 指针所在格」会停在上一次的位置（手机上粘错地方）。
      ME.canvas.setHover(p.x, p.y);
      if (wantPan) {
        mode = 'pan';
        el.classList.add('is-panning');
      } else {
        mode = 'tool';
        downCell = ME.canvas.cellAt(p.x, p.y);
        if (ev.pointerType === 'touch') {
          armToolDown(p.x, p.y, downCell);   // 触摸：等 120 ms，给第二根手指留时间
        } else {
          ME.emit('canvas:down', { px: p.x, py: p.y, cell: downCell });
        }
      }
    } else if (pointers.size === 2) {
      // 双指：取消当前落笔（**还在等待窗口里就一笔都不发**），转捏合
      if (mode === 'tool') {
        if (!cancelPending('second-pointer')) ME.emit('canvas:cancel', {});
      }
      mode = 'pinch';
      var two = twoPointers();
      pinchDist0 = dist(two[0], two[1]);
      moved = true;
    }
    ME.debug.pushGesture('pointerdown', {
      id: ev.pointerId, type: ev.pointerType, x: Math.round(p.x), y: Math.round(p.y),
      pointers: pointers.size, mode: mode, deferred: !!pendingDown
    });
  }

  function onMove(ev) {
    var p = local(ev);
    if (!pointers.has(ev.pointerId)) {
      // 鼠标悬停（没按键）也更新单元格高亮
      ME.canvas.setHover(p.x, p.y);
      return;
    }
    pointers.set(ev.pointerId, { x: p.x, y: p.y, type: ev.pointerType });

    if (mode === 'pan' && pointers.size === 1) {
      var dx = p.x - last.x, dy = p.y - last.y;
      if (Math.abs(dx) > DRAG_SLOP || Math.abs(dy) > DRAG_SLOP) moved = true;
      if (moved) {
        ME.canvas.panBy(dx, dy);
        stats.pans++;
      }
      last = p;
      ME.canvas.setHover(p.x, p.y);
    } else if (mode === 'tool' && pointers.size === 1) {
      if (Math.abs(p.x - last.x) > DRAG_SLOP || Math.abs(p.y - last.y) > DRAG_SLOP) moved = true;
      if (pendingDown) {
        // 还在等待窗口里：轻微抖动（捏合时手指必然有点位移）不算数，不落笔；
        // 超过 TOOL_FLUSH_PX ⇒ 用户是在拖/画，立刻补发 down，别让笔迹缺一截。
        if (Math.abs(p.x - pendingDown.px) > TOOL_FLUSH_PX ||
            Math.abs(p.y - pendingDown.py) > TOOL_FLUSH_PX) {
          stats.flushOnMove++;
          flushPending();
        } else {
          last = p;
          ME.canvas.setHover(p.x, p.y);
          return;
        }
      }
      ME.emit('canvas:drag', { px: p.x, py: p.y, cell: ME.canvas.cellAt(p.x, p.y) });
      ME.canvas.setHover(p.x, p.y);
    } else if (mode === 'pinch' && pointers.size >= 2) {
      var two = twoPointers();
      var d = dist(two[0], two[1]);
      var mid = { x: (two[0].x + two[1].x) / 2, y: (two[0].y + two[1].y) / 2 };
      var ratio = pinchDist0 > 0 ? d / pinchDist0 : 1;
      ME.debug.pushGesture('pinch', { dist: Math.round(d), ratio: Math.round(ratio * 100) / 100 });
      if (ratio >= PINCH_IN) {
        zoomTo(+1, mid.x, mid.y, 'pinch-in');
        pinchDist0 = d;
      } else if (ratio <= PINCH_OUT) {
        zoomTo(-1, mid.x, mid.y, 'pinch-out');
        pinchDist0 = d;
      }
    }
  }

  function onUp(ev, canceled) {
    var had = pointers.has(ev.pointerId);
    var p = had ? pointers.get(ev.pointerId) : null;
    pointers.delete(ev.pointerId);
    try { el.releasePointerCapture(ev.pointerId); } catch (e) { /* 忽略 */ }

    var wasMode = mode;
    if (pointers.size === 0) {
      mode = 'idle';
      el.classList.remove('is-panning');
      if (wasMode === 'tool') {
        // 快速点按：等不到 120 ms 也必须落笔（点一下要生效）；
        // pointercancel（浏览器抢走手势）则**不补发** —— 宁可少一笔，也不在被打断时改图。
        if (canceled) cancelPending('pointercancel');
        else flushPending();
        ME.emit('canvas:up', { px: p ? p.x : 0, py: p ? p.y : 0, moved: moved });
        stats.tools++;
      }
      downCell = null;
    } else if (pointers.size === 1) {
      // 双指松开一指 → 无缝转回单指平移（不再落笔，避免误改地图）
      cancelPending(null);          // 等待中的落笔一律作废（此时已判明是多指手势）
      var rest = pointers.values().next().value;
      mode = 'pan';
      last = { x: rest.x, y: rest.y };
      moved = true;
      pinchDist0 = 0;
      el.classList.add('is-panning');
    }
    ME.debug.pushGesture('pointerup', {
      id: ev.pointerId, pointers: pointers.size, mode: wasMode, moved: moved, canceled: !!canceled
    });
  }

  function onWheel(ev) {
    ev.preventDefault();
    stats.wheel++;
    var p = local(ev);
    zoomTo(ev.deltaY < 0 ? +1 : -1, p.x, p.y, 'wheel');
  }

  ME.touch = {
    stats: stats,
    get mode() { return mode; },
    /** 等待中的触摸落笔（null = 没有）。供 CDP / 自动化断言「第二指落下时它被取消了」 */
    get deferred() { return pendingDown ? { px: pendingDown.px, py: pendingDown.py } : null; },
    get pointerCount() { return pointers.size; },
    bind: function (canvasEl) {
      el = canvasEl;
      el.addEventListener('pointerdown', onDown);
      el.addEventListener('pointermove', onMove);
      el.addEventListener('pointerup', onUp);
      el.addEventListener('pointercancel', function (ev) { onUp(ev, true); });
      el.addEventListener('pointerleave', function (ev) {
        if (!pointers.has(ev.pointerId)) ME.canvas.clearHover();
      });
      el.addEventListener('wheel', onWheel, { passive: false });
      // 兜底：部分移动浏览器仍会派发 touchstart 做双击缩放
      el.addEventListener('touchstart', function (ev) {
        if (ev.touches && ev.touches.length > 1) ev.preventDefault();
      }, { passive: false });
      // 中键拖动会触发浏览器自动滚动，禁掉
      el.addEventListener('auxclick', function (ev) { if (ev.button === 1) ev.preventDefault(); });
      ME.debug.pushGesture('bind', { zooms: ME.ZOOMS.slice() });
    }
  };
})(window.ME);
