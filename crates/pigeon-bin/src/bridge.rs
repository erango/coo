//! The one long-lived JS bridge: keyboard shortcuts, composer RTL behavior,
//! mail-frame sizing and link interception, OS appearance. JS → Rust
//! messages are `{k, v}` objects.

use dioxus::prelude::*;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Msg {
    pub k: String,
    #[serde(default)]
    pub v: String,
}

pub const JS: &str = r#"
(function () {
  const send = (k, v) => dioxus.send({ k, v: String(v ?? '') });

  // ---- OS appearance (for Theme::System and mail-frame colors) ----
  const mq = matchMedia('(prefers-color-scheme: dark)');
  const os = () => send('os', mq.matches ? 'dark' : 'light');
  mq.addEventListener('change', os); os();
  // Window focus, so new-mail notifications skip a window the user is looking at.
  addEventListener('focus', () => send('focus', '1'));
  addEventListener('blur', () => send('focus', '0'));

  // ---- Keyboard shortcuts (outside editors) ----
  const KEYS = new Set(['J','j','k','e','#','s','u','r','a','f','c','z','/','x','v','l','Escape','Enter','ArrowUp','ArrowDown',
                        'meta+1','meta+2','meta+k','meta+n','meta+Enter','meta+shift+z','meta+,',
                        // Apple Mail shortcuts (the single keys above stay as extras).
                        'meta+r','meta+shift+r','meta+shift+f','meta+Backspace','ctrl+meta+a','meta+shift+l',
                        'meta+shift+u','meta+shift+n','meta+f']);
  const editable = (el) => el && (el.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName));
  let lastShift = 0; // 1 = left, 2 = right
  document.addEventListener('keydown', (e) => {
    // RTL/LTR paragraph override, Outlook style: Ctrl + Right Shift = RTL, Ctrl + Left Shift = LTR.
    if (e.key === 'Shift') lastShift = e.location;
    const ed = e.target.closest && e.target.closest('[data-editor]');
    if (ed && !e.metaKey && !e.altKey &&
        ((e.key === 'Shift' && e.ctrlKey) || (e.key === 'Control' && e.shiftKey))) {
      const side = e.key === 'Shift' ? e.location : lastShift;
      const dir = side === 2 ? 'rtl' : side === 1 ? 'ltr' : null;
      if (dir) { setBlockDir(ed, dir); e.preventDefault(); }
      return;
    }
    // Text size: always handled, even in editors and inputs. `code` covers
    // layouts where + / - / 0 sit elsewhere.
    if (e.metaKey && !e.ctrlKey && !e.altKey) {
      const z = (e.key === '=' || e.key === '+' || e.code === 'Equal' || e.code === 'NumpadAdd') ? 'zoom-in'
              : (e.key === '-' || e.key === '_' || e.code === 'Minus' || e.code === 'NumpadSubtract') ? 'zoom-out'
              : (e.key === '0' || e.code === 'Digit0' || e.code === 'Numpad0') ? 'zoom-reset' : null;
      if (z) { e.preventDefault(); send('key', z); return; }
    }
    let key = e.key;
    if (e.shiftKey && key.length > 1 && e.metaKey) key = 'shift+' + key;
    if (e.shiftKey && e.metaKey && key.length === 1) key = 'shift+' + key.toLowerCase();
    if (e.metaKey) key = 'meta+' + key;
    if (e.metaKey && e.ctrlKey) key = 'ctrl+' + key.toLowerCase(); // ⌃⌘A archive
    if (editable(e.target)) {
      // An open autocomplete owns Escape (the input closes it itself).
      if (key === 'Escape' && e.target.dataset && e.target.dataset.suggestOpen === 'true') return;
      if (key === 'meta+Enter' || key === 'Escape') { e.preventDefault(); send('key', key); }
      return;
    }
    if (e.altKey || (e.ctrlKey && !e.metaKey)) return;
    if (KEYS.has(key)) { e.preventDefault(); send('key', key); }
  }, true);

  // ---- Composer: every block gets dir="auto" unless overridden ----
  function blockOf(ed) {
    let n = getSelection().anchorNode;
    while (n && n.parentNode !== ed) n = n.parentNode;
    if (!n) return null;
    if (n.nodeType === 3) { // bare text at the root: wrap it in a block
      const div = document.createElement('div');
      ed.insertBefore(div, n); div.appendChild(n);
      const r = document.createRange(); r.selectNodeContents(div); r.collapse(false);
      getSelection().removeAllRanges(); getSelection().addRange(r);
      return div;
    }
    return n;
  }
  function setBlockDir(ed, dir) { const b = blockOf(ed); if (b && b.setAttribute) b.setAttribute('dir', dir); }
  document.addEventListener('input', (e) => {
    const ed = e.target.closest && e.target.closest('[data-editor]');
    if (!ed) return;
    if (ed.firstChild && ed.firstChild.nodeType === 3) blockOf(ed);
    for (const c of ed.children) if (!c.hasAttribute('dir')) c.setAttribute('dir', 'auto');
    // Autosave: tell Rust once typing pauses.
    clearTimeout(ed._pgSave);
    if (ed.dataset.sigEditor !== undefined) { // Settings → signature editor
      ed._pgSave = setTimeout(() => send('sig-input', JSON.stringify([ed.dataset.sigEditor, ed.innerHTML])), 400);
      return;
    }
    ed._pgSave = setTimeout(() => send('draft-input', ed.id), 1500);
  }, true);

  // ---- Composer: remember each editor's caret, so things inserted from
  // outside (Google Drive links) land where the user was typing ----
  document.addEventListener('selectionchange', () => {
    const sel = getSelection();
    if (!sel.rangeCount) return;
    const n = sel.anchorNode;
    const el = n && (n.nodeType === 1 ? n : n.parentElement);
    const ed = el && el.closest && el.closest('[data-editor]');
    if (ed) ed._pgRange = sel.getRangeAt(0).cloneRange();
  });
  // Insert HTML at the remembered caret, else at the end of the typed text
  // (the signature and quote live outside the editor).
  window.__pigeonInsertHtml = (edId, html) => {
    const ed = document.getElementById(edId);
    if (!ed) return false;
    ed.focus();
    const sel = getSelection();
    const r = ed._pgRange;
    if (r && ed.contains(r.startContainer)) {
      sel.removeAllRanges(); sel.addRange(r);
      document.execCommand('insertHTML', false, html + '&nbsp;');
    } else {
      const div = document.createElement('div');
      div.setAttribute('dir', 'auto');
      div.innerHTML = html;
      ed.appendChild(div);
      const end = document.createRange(); end.selectNodeContents(div); end.collapse(false);
      sel.removeAllRanges(); sel.addRange(end);
      ed.dispatchEvent(new Event('input', { bubbles: true }));
    }
    return true;
  };

  // ---- Composer: floating format bar over a selection ----
  // Commands go through execCommand on the focused editor; new blocks pick
  // up dir="auto" from the input handler above, explicit dirs are kept.
  const BAR_CMDS = [
    ['bold', '<b>B</b>', 'Bold (⌘B)'], ['italic', '<i>I</i>', 'Italic (⌘I)'], ['link', '🔗', 'Link'], ['sep'],
    ['insertUnorderedList', '•', 'Bulleted list'], ['insertOrderedList', '1.', 'Numbered list'], ['quote', '❝', 'Quote'],
  ];
  let bar = null, barEditor = null;
  function hideBar() { if (bar) bar.style.display = 'none'; }
  function ensureBar() {
    if (bar) return bar;
    bar = document.createElement('div');
    bar.className = 'format-bar';
    bar.style.position = 'fixed';
    bar.style.zIndex = '60'; // above the composer overlay
    bar.style.display = 'none';
    bar.setAttribute('role', 'toolbar');
    bar.addEventListener('mousedown', (e) => { if (e.target.tagName !== 'INPUT') e.preventDefault(); });
    for (const [cmd, label, tip] of BAR_CMDS) {
      if (cmd === 'sep') { const sp = document.createElement('span'); sp.className = 'format-bar__sep'; bar.appendChild(sp); continue; }
      const b = document.createElement('button');
      b.type = 'button'; b.innerHTML = label; b.title = tip; b.dataset.cmd = cmd;
      b.addEventListener('click', () => runBar(cmd));
      bar.appendChild(b);
    }
    document.body.appendChild(bar);
    return bar;
  }
  function runBar(cmd) {
    if (!barEditor) return;
    if (cmd === 'link') {
      const sel = getSelection(); const range = sel.rangeCount ? sel.getRangeAt(0).cloneRange() : null;
      const input = document.createElement('input');
      input.className = 'input input--sm'; input.placeholder = 'https://…'; input.style.width = '200px';
      bar.replaceChildren(input); input.focus();
      input.addEventListener('keydown', (e) => {
        if (e.key === 'Enter' || e.key === 'Escape') {
          e.preventDefault(); e.stopPropagation();
          const url = input.value.trim();
          barEditor.focus();
          if (range) { sel.removeAllRanges(); sel.addRange(range); }
          if (e.key === 'Enter' && /^(https?:|mailto:)/i.test(url)) document.execCommand('createLink', false, url);
          bar.remove(); bar = null; hideBar();
        }
      });
      return;
    }
    if (cmd === 'quote') document.execCommand('formatBlock', false, 'blockquote');
    else document.execCommand(cmd, false, null);
    for (const c of barEditor.querySelectorAll('div,p,li,blockquote,ul,ol')) if (!c.hasAttribute('dir')) c.setAttribute('dir', 'auto');
    placeBar();
  }
  function placeBar() {
    const sel = getSelection();
    if (!sel.rangeCount || sel.isCollapsed) return hideBar();
    const node = sel.anchorNode && (sel.anchorNode.nodeType === 1 ? sel.anchorNode : sel.anchorNode.parentElement);
    const ed = node && node.closest && node.closest('[data-editor]');
    if (!ed) return hideBar();
    barEditor = ed;
    const r = sel.getRangeAt(0).getBoundingClientRect();
    const b = ensureBar(); b.style.display = 'flex';
    for (const btn of b.querySelectorAll('button[data-cmd]')) {
      const c = btn.dataset.cmd;
      try { btn.setAttribute('aria-pressed', ['bold','italic','insertUnorderedList','insertOrderedList'].includes(c) && document.queryCommandState(c) ? 'true' : 'false'); } catch (_) {}
    }
    const w = b.offsetWidth || 220;
    b.style.left = Math.max(8, Math.min(innerWidth - w - 8, r.left + r.width / 2 - w / 2)) + 'px';
    b.style.top = Math.max(8, r.top - 44) + 'px';
  }
  document.addEventListener('selectionchange', () => {
    if (bar && bar.contains(document.activeElement)) return; // typing a link
    placeBar();
  });
  window.__pigeonFormatBarDemo = (edId) => { // debug screenshots only
    const ed = document.getElementById(edId); if (!ed) return;
    ed.innerHTML = '<div dir="auto">Thanks — the <b>revised deck</b> looks great.</div><div dir="auto">שלום, נדבר מחר</div>';
    const t = ed.querySelector('b'); const range = document.createRange(); range.selectNodeContents(t);
    ed.focus(); getSelection().removeAllRanges(); getSelection().addRange(range); placeBar();
  };

  // ---- Calendar drag: create on empty grid, move events, resize by the bottom edge ----
  // Draws a ghost owned by this script (Dioxus never sees it) and reports
  // column-local CSS px on drop; Rust snaps and applies.
  (() => {
    let drag = null, swallowClick = false;
    const colAt = (x, y) => {
      for (const el of document.elementsFromPoint(x, y)) { const c = el.closest && el.closest('.day-col[data-day-start]'); if (c) return c; }
      return null;
    };
    // Column-local y in layout px (CSS zoom scales client rects).
    const localY = (col, clientY) => { const r = col.getBoundingClientRect(); return (clientY - r.top) * (col.offsetHeight / (r.height || 1)); };
    document.addEventListener('pointerdown', (e) => {
      if (e.button !== 0) return;
      const col = e.target.closest && e.target.closest('.day-col[data-day-start]');
      if (!col) return;
      const ev = e.target.closest('.event');
      if (ev && !ev.dataset.drag) return; // read-only event: click only
      const y = localY(col, e.clientY);
      if (ev) {
        const top = parseFloat(ev.style.top) || 0;
        drag = { kind: e.target.closest('.event__resize') ? 'resize' : 'move', ev, col, y0: y, grab: y - top, top, h: ev.offsetHeight, moved: false };
      } else {
        drag = { kind: 'create', col, y0: y, moved: false };
      }
    });
    document.addEventListener('pointermove', (e) => {
      if (!drag) return;
      const over = drag.kind === 'move' ? (colAt(e.clientX, e.clientY) || drag.col) : drag.col;
      const y = localY(over, e.clientY);
      if (!drag.moved && Math.abs(y - drag.y0) < 4 && over === drag.col) return;
      if (!drag.moved) {
        drag.moved = true;
        drag.ghost = document.createElement('div');
        drag.ghost.className = drag.kind === 'create' ? 'event event--draft' : drag.ev.className;
        if (drag.ev) {
          drag.ghost.innerHTML = drag.ev.innerHTML;
          drag.ghost.dataset.hue = drag.ev.dataset.hue || '';
          drag.ghost.setAttribute('data-dragging', 'true');
          drag.ghost.style.cssText = drag.ev.style.cssText;
          drag.ghost.style.setProperty('--col', '0'); drag.ghost.style.setProperty('--cols', '1');
          drag.ev.style.opacity = '0.35';
        }
        drag.ghost.style.pointerEvents = 'none';
        drag.ghost.style.zIndex = '4';
      }
      if (drag.ghost.parentNode !== over) over.appendChild(drag.ghost);
      drag.over = over;
      if (drag.kind === 'create') {
        const a = Math.min(drag.y0, y), b = Math.max(drag.y0, y);
        drag.ghost.style.top = a + 'px'; drag.ghost.style.height = Math.max(14, b - a) + 'px';
        drag.y1 = y;
      } else if (drag.kind === 'move') {
        drag.newTop = Math.max(0, y - drag.grab);
        drag.ghost.style.top = drag.newTop + 'px';
      } else {
        drag.bottom = Math.max(drag.top + 14, y);
        drag.ghost.style.height = (drag.bottom - drag.top) + 'px';
      }
    });
    const end = () => {
      if (!drag) return;
      const d = drag; drag = null;
      if (d.ghost) d.ghost.remove();
      if (d.ev) d.ev.style.opacity = '';
      if (!d.moved) return;
      swallowClick = true; setTimeout(() => { swallowClick = false; }, 0);
      if (d.kind === 'create') send('cal-create', [d.col.dataset.dayStart, d.y0, d.y1].join('|'));
      else if (d.kind === 'move') send('cal-move', [d.ev.dataset.drag, d.over.dataset.dayStart, d.newTop].join('|'));
      else send('cal-resize', [d.ev.dataset.drag, d.col.dataset.dayStart, d.bottom].join('|'));
    };
    document.addEventListener('pointerup', end);
    document.addEventListener('pointercancel', () => { if (drag) { if (drag.ghost) drag.ghost.remove(); if (drag.ev) drag.ev.style.opacity = ''; drag = null; } });
    // The click that ends a drag must not also open the editor/popover.
    document.addEventListener('click', (e) => { if (swallowClick) { e.stopPropagation(); e.preventDefault(); swallowClick = false; } }, true);
  })();

  // ---- Context menu: WebKit's own menu only where it's useful ----
  // Its default offers Reload / Open Frame in New Window / Inspect, which
  // make no sense in an app. Keep it for editable fields (paste, spelling)
  // and selected text (Copy, Look Up); suppress it everywhere else,
  // including inside mail frames (wired in `wire` below).
  function contextGuard(e, doc) {
    const sel = doc.getSelection && doc.getSelection();
    const hasText = sel && !sel.isCollapsed && sel.toString().trim() !== '';
    if (editable(e.target) || hasText) return;
    e.preventDefault();
  }
  document.addEventListener('contextmenu', (e) => contextGuard(e, document));

  // ---- Settings: hairline under the sticky header once scrolled ----
  document.addEventListener('scroll', (e) => {
    const el = e.target;
    if (!el.classList || !el.classList.contains('settings')) return;
    const head = el.querySelector('.settings__crumbs');
    if (head) head.toggleAttribute('data-scrolled', el.scrollTop > 0);
  }, true);

  // ---- Window: drag by the title strips, double-click to zoom ----
  // The titlebar is transparent with content under it, and WKWebView ignores
  // -webkit-app-region, so empty parts of the top bars move the window
  // through the native drag (Rust side: window().drag()).
  const DRAG_ZONES = '.sidebar__top, .pane-header, .reader-toolbar, .cal-header, .settings__crumbs, .onboard__chrome, .composer__titlebar';
  const INTERACTIVE = 'button, a, input, textarea, select, label, [role="tab"], [role="button"], [contenteditable="true"], .segmented, .search-field, .chip, kbd';
  const inDragZone = (t) => t.closest && t.closest(DRAG_ZONES) && !t.closest(INTERACTIVE);
  document.addEventListener('mousedown', (e) => {
    if (e.button !== 0 || e.detail > 1 || !inDragZone(e.target)) return;
    send('win', 'drag');
  });
  document.addEventListener('dblclick', (e) => {
    if (inDragZone(e.target)) send('win', 'zoom');
  });

  // ---- Pane splitters: drag to resize the sidebar or the message list,
  // double-click resets. Bounds match ui::{SIDEBAR,LIST}_W_{MIN,MAX}. ----
  const SPLITS = {
    sidebar: { pane: '.sidebar', prop: '--sidebar-w', min: 200, max: 360, msg: 'sidebar-w' },
    list: { pane: '.pane--list', prop: '--list-w', min: 300, max: 760, msg: 'list-w' },
  };
  const splitOf = (t) => {
    const sp = t.closest && t.closest('.splitter');
    return sp ? SPLITS[sp.dataset.pane || 'list'] : null;
  };
  document.addEventListener('mousedown', (e) => {
    const cfg = splitOf(e.target);
    if (!cfg || e.button !== 0) return;
    const pane = document.querySelector(cfg.pane);
    if (!pane) return;
    e.preventDefault();
    const z = parseFloat(getComputedStyle(document.documentElement).zoom) || 1;
    const startX = e.clientX, startW = pane.offsetWidth;
    let w = startW;
    document.body.dataset.resizing = '1';
    const move = (m) => {
      w = Math.round(Math.min(cfg.max, Math.max(cfg.min, startW + (m.clientX - startX) / z)));
      document.documentElement.style.setProperty(cfg.prop, w + 'px');
    };
    const up = () => {
      document.removeEventListener('mousemove', move);
      document.removeEventListener('mouseup', up);
      delete document.body.dataset.resizing;
      send(cfg.msg, w);
    };
    document.addEventListener('mousemove', move);
    document.addEventListener('mouseup', up);
  }, true);
  document.addEventListener('dblclick', (e) => {
    const cfg = splitOf(e.target);
    if (!cfg) return;
    document.documentElement.style.removeProperty(cfg.prop);
    send(cfg.msg, 0);
  }, true);

  // ---- Mail frames: fit height to content, open links in the browser ----
  // The frame's `load` waits for every image, so a long message showed as a
  // short box first. Instead, hook the new document as soon as it has a
  // body (polled per frame), keep the frame hidden until its first fit, and
  // re-fit as images arrive.
  function wire(f) {
    const fit = () => {
      try {
        const d = f.contentDocument; if (!d || !d.documentElement) return;
        // No inner scrollbar, and no padding on top of the content height:
        // mail styled with height:100% / 100vh grows with the frame, so a
        // "+2px" fit fed the resize observer forever.
        d.documentElement.style.overflow = 'hidden';
        // The app's text size is CSS zoom on the root. WebKit lays a
        // frame's document out at the zoomed width and then scales it
        // again, so mail ran ~10% past the frame's right edge, and RTL mail
        // (whose direction reaches the viewport) lost each line's start.
        // Pin the document to the frame's unzoomed width, at the left.
        const z = parseFloat(getComputedStyle(document.documentElement).zoom) || 1;
        const r = d.documentElement.style;
        r.width = z === 1 ? '' : f.clientWidth + 'px';
        r.marginRight = z === 1 ? '' : 'auto';
        const h = d.documentElement.scrollHeight;
        if (Math.abs(h - f.getBoundingClientRect().height) > 1) f.style.height = h + 'px';
        f.dataset.fit = '1';
      } catch (_) {}
    };
    const hook = () => {
      try {
        const d = f.contentDocument;
        if (!d || d.URL !== 'about:srcdoc' || !d.body) return false;
        if (!d._pgHooked) {
          d._pgHooked = true;
          d.addEventListener('click', (e) => {
            const a = e.target.closest && e.target.closest('a[href]');
            if (a) { e.preventDefault(); send('link', a.href); }
          });
          d.addEventListener('contextmenu', (e) => contextGuard(e, d));
          new ResizeObserver(fit).observe(d.documentElement);
          for (const img of d.images) img.addEventListener('load', fit);
        }
        fit();
        return true;
      } catch (_) { return true; }
    };
    f._pgFit = fit;
    if (f._pgSrc !== f.srcdoc) {
      f._pgSrc = f.srcdoc;
      delete f.dataset.fit;
      const t0 = performance.now();
      const poll = () => { if (!hook() && performance.now() - t0 < 3000) requestAnimationFrame(poll); };
      requestAnimationFrame(poll);
    }
    if (!f.dataset.wired) {
      f.dataset.wired = '1';
      // The pinned width must follow the frame (window and pane resizes).
      new ResizeObserver(() => f._pgFit && f._pgFit()).observe(f);
      f.addEventListener('load', () => { hook(); fit(); });
    }
  }
  const scan = () => document.querySelectorAll('iframe.mail-frame').forEach(wire);
  // Re-fit every frame (after a text-size change).
  window.pigeonFitFrames = () => document.querySelectorAll('iframe.mail-frame').forEach((f) => f._pgFit && f._pgFit());
  new MutationObserver(scan).observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ['srcdoc'] });
  scan();
})();
"#;

/// Read an editor's HTML (by element id).
/// The editor's HTML, or None when it couldn't be read (no such editor in
/// this window, or the script failed). Sending must not treat that as "".
pub async fn try_editor_html(id: &str) -> Option<String> {
    let js = format!("const e = document.getElementById({id:?}); return e ? e.innerHTML : null;");
    match document::eval(&js).join::<Option<String>>().await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "could not read the composer editor");
            None
        }
    }
}

pub async fn editor_html(id: &str) -> String {
    let js = format!("return document.getElementById({id:?})?.innerHTML ?? ''");
    document::eval(&js).join::<String>().await.unwrap_or_default()
}

/// Insert HTML into an editor at its last caret (or at the end of the
/// typed text). The editor's input event then autosaves the draft.
pub fn insert_html(editor: &str, html: &str) {
    let (id, html) =
        (serde_json::to_string(editor).unwrap_or_default(), serde_json::to_string(html).unwrap_or_default());
    document::eval(&format!("window.__pigeonInsertHtml && window.__pigeonInsertHtml({id}, {html})"));
}

pub fn set_theme(theme: &str) {
    let js = match theme {
        "dark" | "light" => format!("document.documentElement.dataset.theme = {theme:?}"),
        _ => "delete document.documentElement.dataset.theme".to_string(),
    };
    document::eval(&js);
}

pub fn set_palette(palette: &str) {
    document::eval(&format!("document.documentElement.dataset.palette = {palette:?}"));
}

/// Focus an element by id. Called while a component first renders, so the
/// element may not be in the page yet: try again for a few frames.
pub fn focus(id: &str) {
    document::eval(&format!(
        "(() => {{ let n = 0; const go = () => {{ const e = document.getElementById({id:?}); \
         if (e) e.focus(); else if (n++ < 40) setTimeout(go, 25); }}; setTimeout(go, 0); }})()"
    ));
}
