//! The one long-lived JS bridge: keyboard shortcuts, composer RTL behavior,
//! mail-frame sizing and link interception, OS appearance. JS → Rust
//! messages are `{k, v}` objects. Composer formatting is `richtext`'s
//! script, run alongside (`script`).

use dioxus::prelude::*;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Msg {
    pub k: String,
    #[serde(default)]
    pub v: String,
}

/// Everything a window runs: this bridge, then the composer's rich text.
pub fn script() -> String {
    // The message-drag page side lives inside the bridge's scope (`send`).
    let js = JS.replacen("  // ---- Search box:", &format!("{}\n  // ---- Search box:", crate::ui::mail_drag::JS), 1);
    format!("{js}\n{}\n{}", crate::richtext::script(), crate::shortcuts::bridge_js(*crate::shortcuts::STYLE.peek()))
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
  // The keys of the shortcut style in use (Gmail or Outlook), from Rust
  // (shortcuts.rs) at start and on a switch in Settings.
  let KEYS = new Set();
  window.__pigeonKeys = (list) => { KEYS = new Set(list); };
  const editable = (el) => el && (el.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName));
  let lastShift = 0; // 1 = left, 2 = right
  let listFocus = false;
  // Which pane ⌘F searches: the list after a click in it (not on a row,
  // which opens a message), the open message after a click or key there.
  let pane = 'reader';
  const setPane = (p) => { if (p !== pane) { pane = p; send('pane', p); } };
  document.addEventListener('mousedown', (e) => {
    const c = (sel) => e.target.closest && e.target.closest(sel);
    if (c('.pane--list')) setPane(c('.row') ? 'reader' : 'list');
    else if (c('.pane--reader')) setPane('reader');
    listFocus = !!(e.target.closest && e.target.closest('.pane--list .list'));
  }, true);
  // A key event's shortcut name: "r", "J" (⇧J), "meta+shift+r", "ctrl+e"...
  function keyName(e) {
    let key = e.key;
    // A non-Latin layout (Hebrew, Russian...) types "ר" for R: shortcuts go
    // by the physical key then.
    if (key.length === 1 && /[^\x00-\x7f]/.test(key)) {
      if (/^Key[A-Z]$/.test(e.code)) key = e.shiftKey ? e.code.slice(3) : e.code.slice(3).toLowerCase();
      else if (e.code === 'Slash') key = '/';
    }
    if (e.shiftKey && key.length > 1 && e.metaKey) key = 'shift+' + key;
    // ⇧⌘3 types a symbol: name the digit key, as macOS does.
    if (e.metaKey && /^Digit[0-9]$/.test(e.code)) key = e.code.slice(5);
    if (e.shiftKey && e.metaKey && key.length === 1) key = 'shift+' + key.toLowerCase();
    // ⌥ changes e.key (⌥F is ƒ): name the physical key.
    if (e.metaKey && e.altKey && !e.ctrlKey && /^Key[A-Z]$/.test(e.code)) key = 'alt+' + e.code.slice(3).toLowerCase();
    if (e.metaKey) key = 'meta+' + key;
    // ⌃ shortcuts (Outlook's ⌃E archive, ⌃] next) go by the physical key.
    if (e.ctrlKey && !e.altKey) {
      const base = /^Key[A-Z]$/.test(e.code) ? e.code.slice(3).toLowerCase()
                 : e.code === 'BracketRight' ? ']' : e.code === 'BracketLeft' ? '[' : null;
      if (base) key = e.metaKey ? 'ctrl+meta+' + base : 'ctrl+' + base;
    }
    return key;
  }
  // Outside editors: a key of the current set goes to Rust as a shortcut.
  function shortcut(e, key, trusted) {
    if (e.altKey && !key.startsWith('meta+alt+')) return false;
    if (e.ctrlKey && !key.startsWith('ctrl+')) return false;
    // ⌘A selects every message only while the list has focus (last click or
    // j/k there, with a row selected); elsewhere it selects text as usual.
    if (key === 'meta+a') {
      if (!(listFocus && document.querySelector('.pane--list .row[aria-selected="true"], .pane--list .row[data-multi]'))) return false;
      send('key', key);
      return true;
    }
    if (['j','k','ArrowUp','ArrowDown','ctrl+]','ctrl+['].includes(key)) listFocus = true;
    if (!KEYS.has(key)) return false;
    // Untrusted: passed up from a test script; screenshot runs that ignore
    // the real keyboard still take these.
    send(trusted ? 'key' : 'key-synth', key);
    return true;
  }
  // Settings › Keyboard shortcuts records the next key pressed (Esc cancels).
  let recording = false;
  window.__cooRecordKey = (on) => { recording = !!on; };
  document.addEventListener('keydown', (e) => {
    if (recording) {
      if (['Shift', 'Meta', 'Control', 'Alt', 'CapsLock', 'Fn'].includes(e.key)) return;
      e.preventDefault(); e.stopPropagation();
      recording = false;
      const k = keyName(e);
      send('shortcut-rec', k === 'Escape' ? '' : (e.altKey && !e.metaKey) ? 'alt+' + k : k);
      return;
    }
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
    const key = keyName(e);
    if (editable(e.target)) {
      // ⌘F in the folder's search field: select what's typed, to replace it.
      if (key === 'meta+f' && e.target.id === 'search-input') { e.preventDefault(); e.target.select(); return; }
      // An open autocomplete owns Escape (the input closes it itself).
      if (key === 'Escape' && e.target.dataset && e.target.dataset.suggestOpen === 'true') return;
      if (key === 'meta+Enter' || key === 'Escape') { e.preventDefault(); send('key', key); }
      return;
    }
    // Enter on a focused button or link presses it, as anywhere else.
    if (key === 'Enter' && e.target.closest && e.target.closest('button, a[href]')) return;
    if (shortcut(e, key, e.isTrusted)) e.preventDefault();
  }, true);
  // Keys typed while a message has focus: its frame runs no script, so the
  // keydown above never sees them. The app's key monitor (keymon.rs) hands
  // them over; they count only if this window has focus and it's in a
  // read-only message (scrolling keys stay with the message).
  window.__pigeonNativeKey = (e) => {
    const f = document.activeElement;
    if (!document.hasFocus() || !f || f.tagName !== 'IFRAME' || !f.classList.contains('mail-frame')) return;
    try { if (f.contentDocument.body.isContentEditable) return; } catch (_) { return; }
    const key = keyName(e);
    if (['ArrowUp','ArrowDown','Enter','meta+a'].includes(key)) return;
    setPane('reader');
    shortcut(e, key, true);
  };

  // ---- Recipient chips drag between To, Cc and Bcc ----
  // WebKit decides on a drop during dragover, before Rust could answer:
  // allow it here on another field's row (Rust moves the address on drop).
  let chipFrom = null;
  document.addEventListener('dragstart', (e) => {
    const c = e.target.closest && e.target.closest('.recipient[draggable="true"]');
    if (!c) return;
    chipFrom = c.closest('.recipients');
    e.dataTransfer.setData('text/plain', c.title || '');
    e.dataTransfer.effectAllowed = 'move';
  }, true);
  document.addEventListener('dragover', (e) => {
    if (!chipFrom) return;
    const r = e.target.closest && e.target.closest('.recipients');
    document.querySelectorAll('.recipients[data-chip-over]').forEach((x) => { if (x !== r) delete x.dataset.chipOver; });
    if (r && r !== chipFrom) { e.preventDefault(); e.dataTransfer.dropEffect = 'move'; r.dataset.chipOver = '1'; }
  }, true);
  const chipDone = () => {
    chipFrom = null;
    document.querySelectorAll('.recipients[data-chip-over]').forEach((x) => delete x.dataset.chipOver);
  };
  document.addEventListener('dragend', chipDone, true);
  document.addEventListener('drop', () => setTimeout(chipDone, 0), true);

  // ---- Archive bursts: the row that slides up under a still pointer ----
  // WebKit only re-evaluates :hover when the mouse moves, so after a row
  // leaves, the next one sliding under the pointer would show no buttons
  // and a quick second click would open it instead of archiving it. While
  // the list animates, mark the row under the pointer (`data-hover`, styled
  // like :hover); a real mouse move takes over again.
  let px = -1, py = -1, hoverUntil = 0;
  const clearHover = () => document.querySelectorAll('.pane--list .row[data-hover]').forEach((r) => delete r.dataset.hover);
  document.addEventListener('mousemove', (e) => { px = e.clientX; py = e.clientY; if (hoverUntil) { hoverUntil = 0; clearHover(); } }, true);
  const hoverTick = () => {
    if (!hoverUntil) return;
    const el = px < 0 ? null : document.elementFromPoint(px, py);
    const row = el && el.closest && el.closest('.pane--list .row:not([data-leaving])');
    document.querySelectorAll('.pane--list .row[data-hover]').forEach((r) => { if (r !== row) delete r.dataset.hover; });
    if (row) row.dataset.hover = '1';
    if (performance.now() < hoverUntil) requestAnimationFrame(hoverTick);
  };
  new MutationObserver((ms) => {
    if (!ms.some((m) => m.target.closest && m.target.closest('.pane--list') &&
        (m.type === 'childList' || m.attributeName === 'data-leaving'))) return;
    const start = !hoverUntil;
    hoverUntil = performance.now() + 400;
    if (start) requestAnimationFrame(hoverTick);
  }).observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ['data-leaving'] });

  // ---- Messages dragged from the list onto a composer (attached as .eml) ----
  document.addEventListener('dragover', (e) => {
    const t = e.dataTransfer && e.dataTransfer.types;
    if (!t || !Array.from(t).includes('application/x-coo-mail')) return;
    if (e.target.closest && e.target.closest('[data-mail-attach]')) { e.preventDefault(); e.dataTransfer.dropEffect = 'copy'; }
  }, true);

  // ---- Search box: report the caret (operator autocomplete) ----
  let lastCaret = '';
  const searchCaret = () => {
    const e = document.activeElement;
    if (!e || e.id !== 'search-input') return;
    const v = JSON.stringify([e.value, e.selectionStart ?? e.value.length]);
    if (v !== lastCaret) { lastCaret = v; send('search-caret', v); }
  };
  document.addEventListener('selectionchange', searchCaret);
  document.addEventListener('input', searchCaret, true);
  // On focus (also a window coming back) WebKit puts the caret back only
  // after the event: read it a moment later.
  for (const t of ['keyup', 'mouseup', 'focus']) document.addEventListener(t, () => setTimeout(searchCaret, 30), true);

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
  // In a list, the item under the caret; otherwise its top-level block.
  function setBlockDir(ed, dir) {
    const n = getSelection().anchorNode, el = n && (n.nodeType === 1 ? n : n.parentElement);
    const li = el && el.closest('li');
    const b = li && ed.contains(li) ? li : blockOf(ed);
    if (b && b.setAttribute) { b.setAttribute('dir', dir); delete b.dataset.dirGuess; }
  }
  // A new, still empty line (paragraph or list item) starts in the
  // direction of the one above it, so a Hebrew paragraph's next line opens
  // on the right; the first letter typed hands it back to dir="auto" (its
  // own text decides). Digits and punctuation keep the guess.
  const STRONG = /\p{L}/u;
  function predictDir(ed) {
    const n = getSelection().anchorNode, el = n && (n.nodeType === 1 ? n : n.parentElement);
    if (!el || !ed.contains(el)) return;
    const li = el.closest('li');
    const b = li && ed.contains(li) ? li : blockOf(ed);
    if (!b || !b.getAttribute) return;
    const text = b.textContent.replace(/[\u200b\ufeff]/g, '');
    if (b.dataset.dirGuess !== undefined) {
      if (STRONG.test(text)) { b.setAttribute('dir', 'auto'); delete b.dataset.dirGuess; }
      return;
    }
    const prev = b.previousElementSibling;
    if (text.trim() !== '' || !prev || b.getAttribute('dir') !== 'auto') return;
    b.setAttribute('dir', getComputedStyle(prev).direction === 'rtl' ? 'rtl' : 'ltr');
    b.dataset.dirGuess = '';
  }
  document.addEventListener('input', (e) => {
    const ed = e.target.closest && e.target.closest('[data-editor]');
    if (!ed) return;
    if (ed.firstChild && ed.firstChild.nodeType === 3) blockOf(ed);
    for (const c of ed.children) if (!c.hasAttribute('dir')) c.setAttribute('dir', 'auto');
    predictDir(ed);
    // Autosave: tell Rust once typing pauses.
    clearTimeout(ed._pgSave);
    if (ed.dataset.sigEditor !== undefined) { // Settings → signature editor
      ed._pgSave = setTimeout(() => send('sig-input', JSON.stringify([ed.dataset.sigEditor, ed.innerHTML])), 400);
      return;
    }
    ed._pgSave = setTimeout(() => send('draft-input', ed.id), 1500);
  }, true);

  // ---- Composer: a click in the message area but outside the editor
  // (below the text, beside it, the margins, the signature) puts the caret
  // at the nearest spot: the click clamped into the text, so below the
  // last line lands on that line, at the closest horizontal position. ----
  const NOT_THE_BODY = 'button, a, input, textarea, select, label, [contenteditable="true"], .fmt-bar, ' +
    '.inline-reply__head, .inline-reply__footer, .composer__footer, .composer__quote, .inline-reply__quoted, ' +
    '.quote-box, .composer__attachments, .attachment, .drive-sheet, .drive-waiting, .cross-account-hint, .popover, .menu';
  document.addEventListener('mousedown', (e) => {
    if (e.button !== 0 || !e.target.closest) return;
    const area = e.target.closest('.composer__body, .inline-reply--open');
    if (!area || e.target.closest(NOT_THE_BODY)) return;
    const ed = area.querySelector('[data-editor]');
    if (!ed) return;
    e.preventDefault();
    ed.focus();
    const all = document.createRange(); all.selectNodeContents(ed);
    const lines = all.getClientRects();
    const box = ed.getBoundingClientRect();
    const last = lines.length ? lines[lines.length - 1] : null;
    const top = lines.length ? lines[0].top : box.top;
    const bottom = last ? last.bottom : box.top + 4;
    const y = Math.min(Math.max(e.clientY, top + 2), bottom - 2);
    const x = Math.min(Math.max(e.clientX, box.left + 2), box.right - 2);
    let r = document.caretRangeFromPoint && document.caretRangeFromPoint(x, y);
    if (!r || !ed.contains(r.startContainer)) { r = document.createRange(); r.selectNodeContents(ed); r.collapse(false); }
    const sel = getSelection(); sel.removeAllRanges(); sel.addRange(r);
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

  // ---- Tooltips: Coo's own, not the system's ----
  // Every `title` moves to `data-tip` as it appears (so WebKit never shows
  // its plain tooltip); hovering shows ours after a short wait, the name
  // first and a trailing "(⌘R)" as a keycap. Once one is showing, the next
  // shows at once. Below the element, or above when there's no room.
  (function () {
    const ATTR = 'data-tip';
    const take = (el) => {
      const t = el.getAttribute('title');
      if (t == null) return;
      el.removeAttribute('title');
      if (!t) return;
      el.setAttribute(ATTR, t);
      if (!el.hasAttribute('aria-label') && !(el.textContent || '').trim()) el.setAttribute('aria-label', t);
    };
    const sweep = (root) => {
      if (root.nodeType !== 1) return;
      if (root.hasAttribute('title')) take(root);
      root.querySelectorAll('[title]').forEach(take);
    };
    sweep(document.documentElement);
    new MutationObserver((ms) => {
      for (const m of ms) {
        if (m.type === 'attributes') take(m.target);
        else m.addedNodes.forEach(sweep);
      }
    }).observe(document.documentElement, { subtree: true, childList: true, attributes: true, attributeFilter: ['title'] });

    const KEY = /^(.*\S)\s+\(([⌘⇧⌥⌃↵⌫⌦↑↓←→#!\/,.\[\]+−\-A-Z0-9 ]{1,10}|Esc)\)$/;
    let tip = null, timer = 0, current = null, warmUntil = 0;
    const el = () => {
      if (tip) return tip;
      tip = document.createElement('div');
      tip.className = 'coo-tip';
      tip.setAttribute('role', 'tooltip');
      document.body.appendChild(tip);
      return tip;
    };
    const hide = () => {
      clearTimeout(timer);
      if (current && tip && tip.dataset.shown) warmUntil = performance.now() + 350;
      current = null;
      if (tip) delete tip.dataset.shown;
    };
    const show = (target) => {
      const text = target.getAttribute(ATTR);
      if (!text || !target.isConnected) return;
      const t = el();
      const m = text.match(KEY);
      t.textContent = m ? m[1] : text;
      if (m) { const k = document.createElement('kbd'); k.textContent = m[2]; t.appendChild(k); }
      t.style.left = '0px'; t.style.top = '0px';
      t.dataset.place = 'below';
      const r = target.getBoundingClientRect(), w = t.offsetWidth, h = t.offsetHeight;
      const vw = document.documentElement.clientWidth, vh = document.documentElement.clientHeight;
      let top = r.bottom + 8;
      if (top + h > vh - 6) { top = r.top - h - 8; t.dataset.place = 'above'; }
      const center = r.left + r.width / 2;
      const left = Math.max(6, Math.min(vw - w - 6, center - w / 2));
      t.style.left = left + 'px'; t.style.top = top + 'px';
      t.style.setProperty('--caret-x', Math.max(10, Math.min(w - 10, center - left)) + 'px');
      t.dataset.shown = '';
    };
    document.addEventListener('mouseover', (e) => {
      const target = e.target.closest && e.target.closest('[' + ATTR + ']');
      if (target === current) return;
      if (!target) { hide(); return; }
      clearTimeout(timer);
      if (tip) delete tip.dataset.shown;
      current = target;
      const warm = performance.now() < warmUntil || (tip && tip.dataset.shown !== undefined);
      timer = setTimeout(() => { if (current === target) show(target); }, warm ? 0 : 450);
    }, true);
    document.addEventListener('mouseout', (e) => { if (!e.relatedTarget) hide(); }, true);
    ['mousedown', 'keydown', 'wheel', 'blur'].forEach((ev) => addEventListener(ev, hide, true));
  })();

  // ---- Message list: a selection rectangle dragged from the background ----
  // (not from a row: that drags the mail). Rust maps the rectangle's span in
  // the virtualized rows' coordinates (measured from the first spacer, below
  // any sending rows) to rows; near the top or bottom edge the list scrolls.
  (function () {
    let band = null, swallowClick = false;
    const EDGE = 28;
    const origin = (l) => {
      const sp = l.querySelector(':scope > div[aria-hidden="true"]');
      return sp ? sp.getBoundingClientRect().top - l.getBoundingClientRect().top + l.scrollTop : 0;
    };
    const update = () => {
      const b = band, l = b.l, r = l.getBoundingClientRect();
      const x = Math.max(0, Math.min(l.clientWidth, b.cx - r.left));
      const y = Math.max(0, Math.min(l.scrollHeight, b.cy - r.top + l.scrollTop));
      const top = Math.min(b.y0, y), left = Math.min(b.x0, x);
      Object.assign(b.el.style, { top: top + 'px', left: left + 'px', width: Math.abs(x - b.x0) + 'px', height: Math.abs(y - b.y0) + 'px' });
      const o = origin(l);
      const v = [Math.round(b.y0 - o), Math.round(y - o), b.add ? 1 : 0, b.sent ? 0 : 1].join('|');
      if (v.slice(0, v.lastIndexOf('|')) !== b.last) {
        b.last = v.slice(0, v.lastIndexOf('|'));
        b.sent = true;
        send('list-band', v);
      }
    };
    const tick = () => {
      if (!band || !band.moved) return;
      const l = band.l, r = l.getBoundingClientRect();
      const d = band.cy < r.top + EDGE ? band.cy - (r.top + EDGE) : band.cy > r.bottom - EDGE ? band.cy - (r.bottom - EDGE) : 0;
      if (d) { l.scrollTop += Math.max(-24, Math.min(24, d / 2)); update(); }
      requestAnimationFrame(tick);
    };
    document.addEventListener('pointerdown', (e) => {
      if (e.button !== 0 || e.ctrlKey) return;
      const l = e.target.closest && e.target.closest('.pane--list .list[role=listbox]');
      if (!l || e.target.closest('.row, button, a, input, .list-banner')) return;
      const r = l.getBoundingClientRect();
      if (e.clientX >= r.left + l.clientWidth) return; // the scrollbar
      band = { l, x0: e.clientX - r.left, y0: e.clientY - r.top + l.scrollTop, cx: e.clientX, cy: e.clientY,
        add: e.metaKey || e.shiftKey, moved: false, sent: false, last: '' };
      e.preventDefault();
    });
    document.addEventListener('pointermove', (e) => {
      if (!band) return;
      band.cx = e.clientX; band.cy = e.clientY;
      if (!band.moved) {
        const r = band.l.getBoundingClientRect();
        if (Math.hypot(e.clientX - r.left - band.x0, e.clientY - r.top + band.l.scrollTop - band.y0) < 4) return;
        band.moved = true;
        band.el = document.createElement('div');
        band.el.className = 'list-band';
        band.l.appendChild(band.el);
        band.l.dataset.banding = '';
        requestAnimationFrame(tick);
      }
      update();
    });
    const end = () => {
      if (!band) return;
      const b = band; band = null;
      if (b.el) b.el.remove();
      delete b.l.dataset.banding;
      if (b.moved) { swallowClick = true; setTimeout(() => { swallowClick = false; }, 0); }
    };
    document.addEventListener('pointerup', end);
    document.addEventListener('pointercancel', end);
    document.addEventListener('click', (e) => { if (swallowClick) { e.stopPropagation(); e.preventDefault(); swallowClick = false; } }, true);
  })();

  // ---- Context menu: WebKit's own menu only where it's useful ----
  // Keep it for editable fields (paste, spelling), selected text (Copy,
  // Look Up), images (Copy Image) and links (Copy Link); suppress it
  // everywhere else, including inside mail frames (wired in `wire` below).
  // Items that open windows, download or reload are removed natively
  // (ctxmenu.rs).
  function contextGuard(e, doc) {
    const sel = doc.getSelection && doc.getSelection();
    const hasText = sel && !sel.isCollapsed && sel.toString().trim() !== '';
    const media = e.target.closest && e.target.closest('img, a[href]');
    // The image under the pointer, for the menu's Save Image… (ctxmenu.rs).
    const img = e.target.closest && e.target.closest('img');
    if (img && (img.currentSrc || img.src)) send('ctx-img', img.currentSrc || img.src);
    if (editable(e.target) || hasText || media) return;
    e.preventDefault();
  }
  document.addEventListener('contextmenu', (e) => contextGuard(e, document));

  // ---- Tooltips: icon buttons show their label ("Archive (E)") on hover
  // unless they set a title of their own; follows label changes. ----
  document.addEventListener('mouseover', (e) => {
    const b = e.target.closest && e.target.closest('button[aria-label]');
    if (!b || (b.hasAttribute('title') && !b.dataset.autoTitle)) return;
    const label = b.getAttribute('aria-label');
    if (b.title !== label) { b.title = label; b.dataset.autoTitle = '1'; }
  }, true);

  // ---- Settings: hairline under the sticky header once scrolled ----
  // The reader's toolbar casts a soft shadow once the message scrolls under it.
  document.addEventListener('scroll', (e) => {
    const el = e.target;
    if (!el.classList) return;
    if (el.classList.contains('reader')) {
      const bar = el.parentElement && el.parentElement.querySelector(':scope > .reader-toolbar');
      if (bar) bar.toggleAttribute('data-scrolled', el.scrollTop > 0);
      return;
    }
    if (el.classList.contains('cal-list')) { calEdges(el); return; }
    if (!el.classList.contains('settings')) return;
    const head = el.querySelector('.settings__crumbs');
    if (head) head.toggleAttribute('data-scrolled', el.scrollTop > 0);
  }, true);

  // ---- Calendar sidebar: the calendars list scrolls between the month and
  // the footer; data-scrolled shades the month's edge once it's scrolled,
  // data-more the footer's while anything is still below. Re-checked as
  // the window or the list changes size. ----
  const calEdges = (el) => {
    el.toggleAttribute('data-scrolled', el.scrollTop > 0);
    el.toggleAttribute('data-more', el.scrollTop + el.clientHeight < el.scrollHeight - 1);
  };
  setInterval(() => { const el = document.querySelector('.sidebar .cal-list'); if (el) calEdges(el); }, 400);

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
        // The content's own height: the root's scrollHeight never drops
        // below the frame's, so a frame could grow but never shrink back,
        // and mail sized to its frame (height:100%, mailframe.rs) grew it
        // on every fit.
        const h = Math.ceil(d.documentElement.getBoundingClientRect().height);
        if (h > 0 && Math.abs(h - f.getBoundingClientRect().height) > 1) f.style.height = h + 'px';
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
            if (a && !d.body.isContentEditable) { e.preventDefault(); send('link', a.href); }
            else if (a) e.preventDefault(); // editing: a click places the caret
          });
          // A composer's quoted original: editable in place. Edits autosave
          // with the draft (Rust reads them back through __pigeonQuote). Its
          // `input` events didn't reach listeners added from here (the frame
          // runs no script), so an observer watches for edits instead.
          if (f.dataset.editFor) {
            d.body.contentEditable = 'true';
            new MutationObserver(() => {
              f.dataset.dirty = '1';
              clearTimeout(f._pgSave);
              f._pgSave = setTimeout(() => send('draft-input', f.dataset.editFor), 1500);
            }).observe(d.body, { subtree: true, childList: true, characterData: true, attributes: true });
          }
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
  // ---- Attachments: drag one out to Finder (Rust starts a native drag
  // with a file promise once the pointer has moved a few pixels).
  document.addEventListener('mousedown', (e) => {
    const chip = e.button === 0 && e.target.closest && e.target.closest('[data-drag-out]');
    if (!chip) return;
    e.preventDefault(); // no text selection
    const x0 = e.clientX, y0 = e.clientY;
    const move = (m) => {
      if (Math.hypot(m.clientX - x0, m.clientY - y0) < 5) return;
      done();
      send('attach-drag', chip.dataset.dragOut);
    };
    const done = () => {
      document.removeEventListener('mousemove', move, true);
      document.removeEventListener('mouseup', done, true);
    };
    document.addEventListener('mousemove', move, true);
    document.addEventListener('mouseup', done, true);
  }, true);

  // A click into a message moves keyboard focus into its frame, where the
  // shortcuts (r, a, f, j/k...) can't hear keys: the frame runs no script.
  // Unless text is selected there (kept for ⌘C) or it's an editable quote,
  // hand focus back once the click is done.
  document.addEventListener('focus', (e) => {
    if (!e.target.classList || !e.target.classList.contains('mail-frame')) return;
    let calm = 0;
    const t = setInterval(() => {
      const f = document.activeElement;
      if (!f || f.tagName !== 'IFRAME' || !f.classList.contains('mail-frame')) return clearInterval(t);
      try {
        const d = f.contentDocument;
        if (d.body.isContentEditable || !d.getSelection().isCollapsed) { calm = 0; return; }
        // Twice in a row: not the first instant of a drag-select.
        if (++calm < 2) return;
        // blur() leaves WebKit's focus in the frame: focus the pane around it.
        const pane = f.closest('.pane--reader, .reader-window, main') || document.body;
        if (!pane.hasAttribute('tabindex')) pane.setAttribute('tabindex', '-1');
        pane.focus({ preventScroll: true });
        clearInterval(t);
      } catch (_) { clearInterval(t); }
    }, 200);
  }, true);
  const scan = () => document.querySelectorAll('iframe.mail-frame').forEach(wire);
  // An edited quote's HTML (null when untouched: it goes out as received).
  window.__pigeonQuote = (editor) => {
    const f = document.getElementById(editor + '-quote');
    try { return f && f.dataset.dirty ? f.contentDocument.body.innerHTML : null; } catch (_) { return null; }
  };
  // Re-fit every frame (after a text-size change).
  window.pigeonFitFrames = () => document.querySelectorAll('iframe.mail-frame').forEach((f) => f._pgFit && f._pgFit());
  new MutationObserver(scan).observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ['srcdoc'] });
  scan();
})();
"#;

/// The composer's quoted original if the user edited it, else None.
pub async fn quote_html(editor: &str) -> Option<String> {
    let id = serde_json::to_string(editor).unwrap_or_default();
    let js = format!("return window.__pigeonQuote ? window.__pigeonQuote({id}) : null;");
    document::eval(&js).join::<Option<String>>().await.ok().flatten()
}

/// The editor's HTML as it goes out (default spacing stamped on each
/// paragraph), or None when it couldn't be read (no such editor in this
/// window, or the script failed). Sending must not treat that as "".
pub async fn try_editor_export(id: &str) -> Option<String> {
    let js = format!("return window.__pigeonExport ? window.__pigeonExport({id:?}) : null;");
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
    let value = match theme {
        "dark" | "light" => format!("{theme:?}"),
        _ => "null".to_string(),
    };
    document::eval(&format!("{SWAP_JS}('theme', {value})"));
}

pub fn set_palette(palette: &str) {
    document::eval(&format!("{SWAP_JS}('palette', {palette:?})"));
}

/// Set a root `data-*` that changes the colors (theme, palette). A real
/// change after the first one cross-fades every color for a moment
/// (app.css `[data-theme-swap]`); the first, at startup, just applies.
const SWAP_JS: &str = "((key, v) => { const r = document.documentElement; const old = r.dataset[key]; \
    const settled = r.dataset.colorsSet; r.dataset.colorsSet = '1'; \
    if ((old ?? null) === v) return; \
    if (settled && !matchMedia('(prefers-reduced-motion: reduce)').matches) { \
      r.setAttribute('data-theme-swap', ''); void document.body.offsetHeight; \
      clearTimeout(window.__cooSwapT); window.__cooSwapT = setTimeout(() => r.removeAttribute('data-theme-swap'), 520); } \
    if (v === null) delete r.dataset[key]; else r.dataset[key] = v; })";

/// Set a text input's value and caret (a UTF-16 offset) and focus it. The
/// render that follows sets the same value; put the caret back if it moved.
/// Like [`focus`], waits a few frames for an input that isn't there yet.
pub fn set_input(id: &str, value: &str, caret: usize) {
    focus_webview();
    let (id, value) = (serde_json::to_string(id).unwrap_or_default(), serde_json::to_string(value).unwrap_or_default());
    // Keeps at it for a few frames: a re-render can swap the field, and a
    // click's own focus can land after ours.
    document::eval(&format!(
        "(() => {{ const v = {value}; let n = 0, set = false; const go = () => {{ const e = document.getElementById({id}); \
         if (!e) {{ if (n++ < 40) setTimeout(go, 25); return; }} \
         if (!set) {{ e.value = v; set = true; }} \
         if (document.activeElement !== e) {{ window.focus(); e.focus(); e.setSelectionRange({caret}, {caret}); }} \
         if (n++ < 12) setTimeout(go, 25); }}; setTimeout(go, 0); }})()"
    ));
}

/// Make the web view the window's key view, so a field focused in the page
/// takes typing even when something native had the keyboard (a menu, a
/// panel, the menu bar popover).
fn focus_webview() {
    let _ = dioxus::desktop::window().webview.focus();
}

/// Focus an element by id. Called while a component first renders, so the
/// element may not be in the page yet: try again for a few frames.
pub fn focus(id: &str) {
    focus_webview();
    document::eval(&format!(
        "(() => {{ let n = 0; const go = () => {{ const e = document.getElementById({id:?}); \
         if (e) e.focus(); else if (n++ < 40) setTimeout(go, 25); }}; setTimeout(go, 0); }})()"
    ));
}

#[cfg(test)]
mod js_syntax {
    /// The bridge as the app sends it (with the message-drag part, the rich
    /// text editor and a shortcut style) must parse: one syntax error stops
    /// all of it, and with it message bodies, shortcuts and links. Checked
    /// with Node when it's installed.
    #[test]
    fn bridge_parses() {
        let js = super::JS.replacen(
            "  // ---- Search box:",
            &format!("{}\n  // ---- Search box:", crate::ui::mail_drag::JS),
            1,
        );
        assert!(js.contains("__pigeonMailDragAt"), "the message-drag part is in");
        let all = format!(
            "{js}\n{}\n{}",
            crate::richtext::script(),
            crate::shortcuts::bridge_js(crate::shortcuts::Style::Outlook)
        );
        let path = std::env::temp_dir().join(format!("coo-bridge-{}.js", std::process::id()));
        std::fs::write(&path, all).unwrap();
        match std::process::Command::new("node").arg("--check").arg(&path).output() {
            Ok(out) => assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr)),
            Err(_) => eprintln!("node not found: JS syntax not checked"),
        }
        let _ = std::fs::remove_file(path);
    }
}
