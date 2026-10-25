//! Composer rich text: the formatting commands (toolbar buttons and their
//! shortcuts) and list autoformat ("1. " or "- " at the start of a line
//! starts a list). The DOM work is the script below, run with the bridge in
//! every window; the shortcut table and the list pattern live here as data,
//! handed to the script, so the tests check what the editor really does.
//!
//! Formatting goes through `execCommand` so ⌘Z undoes it. Character styles
//! use tags (`<b>`, `<i>`, `<u>`, `<strike>`), colors and sizes inline
//! `style`s, so recipients' mail clients show them without our CSS.

/// (command, key combo as the script builds it, label, what it does).
/// Combos use the physical key (`KeyboardEvent.code`), so they work on a
/// Hebrew or Arabic layout too. Outlook for Mac's keys where it has them.
pub const SHORTCUTS: &[(&str, &str, &str, &str)] = &[
    ("bold", "meta+b", "⌘B", "Bold"),
    ("italic", "meta+i", "⌘I", "Italic"),
    ("underline", "meta+u", "⌘U", "Underline"),
    ("strikeThrough", "meta+shift+x", "⇧⌘X", "Strikethrough"),
    ("grow", "meta+shift+.", "⇧⌘>", "Bigger text"),
    ("shrink", "meta+shift+,", "⇧⌘<", "Smaller text"),
    ("link", "meta+k", "⌘K", "Insert or edit link"),
    ("insertUnorderedList", "meta+shift+8", "⇧⌘8", "Bulleted list"),
    ("insertOrderedList", "meta+shift+7", "⇧⌘7", "Numbered list"),
    ("indent", "meta+]", "⌘]", "Increase indent"),
    ("outdent", "meta+[", "⌘[", "Decrease indent"),
    ("justifyLeft", "meta+l", "⌘L", "Align left"),
    ("justifyCenter", "meta+e", "⌘E", "Center"),
    ("justifyRight", "meta+r", "⌘R", "Align right"),
    ("quote", "meta+shift+9", "⇧⌘9", "Quote"),
    ("removeFormat", "meta+\\", "⌘\\", "Clear formatting"),
    ("undo", "meta+z", "⌘Z", "Undo"),
    ("redo", "meta+shift+z", "⇧⌘Z", "Redo"),
];

/// Editing keys that aren't commands, for Settings › Keyboard shortcuts.
pub const LIST_KEYS: &[(&str, &str)] = &[
    ("1.  or  -  then Space", "Start a numbered or bulleted list"),
    ("↵ on an empty item", "End the list"),
    ("⌫ right after", "Undo the automatic list"),
    ("Tab · ⇧Tab in a list", "Indent · outdent the item"),
];

/// The tooltip for a toolbar button: "Bold (⌘B)".
pub fn tip(cmd: &str) -> String {
    match SHORTCUTS.iter().find(|s| s.0 == cmd) {
        Some((_, _, keys, what)) => format!("{what} ({keys})"),
        None => cmd.to_string(),
    }
}

/// What the text before the caret must be, when Space is typed, to start a
/// list: a bullet (`-`, `*`, `•`), a number (`1.` / `1)`, Western or
/// Arabic-Indic digits), or `a.` / `A.`. Bidi marks and spaces in front are
/// allowed (RTL lines often start with one). Groups: 1 bullet, 2 number,
/// 3 letter. Written to mean the same in JavaScript and Rust regex syntax.
pub const AUTO_LIST_RE: &str = "^[ \u{a0}\u{200e}\u{200f}\u{61c}]*(?:([-*•])|([0-9٠-٩۰-۹]{1,3})[.)]|([aA])[.)])$";

/// The script: formatting toolbar, shortcuts, and list autoformat.
pub fn script() -> String {
    let keys: Vec<(&str, &str, &str)> = SHORTCUTS.iter().map(|s| (s.1, s.0, s.2)).collect();
    JS.replace("__KEYS__", &serde_json::to_string(&keys).unwrap_or_default())
        .replace("__AUTO_LIST_RE__", &serde_json::to_string(AUTO_LIST_RE).unwrap_or_default())
}

const JS: &str = r##"
(function () {
  const TABLE = __KEYS__;
  const KEYS = new Map(TABLE.map(([k, c]) => [k, c]));
  const LABEL = Object.fromEntries(TABLE.map(([, c, l]) => [c, l]));
  const AUTO_LIST = new RegExp(__AUTO_LIST_RE__);
  const editorOf = (n) => { const el = n && (n.nodeType === 1 ? n : n.parentElement); return el && el.closest ? el.closest('[data-editor]') : null; };
  const barFor = (ed) => ed && ed.id ? document.querySelector('.fmt-bar[data-for="' + ed.id + '"]') : null;
  const caretNode = () => { const s = getSelection(); return s.rangeCount ? s.anchorNode : null; };
  // Nearest element matching `sel` around the caret, inside the editor.
  const around = (ed, sel) => {
    const n = caretNode(); const el = n && (n.nodeType === 1 ? n : n.parentElement);
    const hit = el && el.closest(sel);
    return hit && ed.contains(hit) && hit !== ed ? hit : null;
  };
  const BLOCK = /^(DIV|P|LI|BLOCKQUOTE|H[1-6])$/;
  const blockAt = (ed, n) => {
    for (; n && n !== ed; n = n.parentNode) if (n.nodeType === 1 && BLOCK.test(n.tagName)) return n;
    return null;
  };
  // A block's direction: an explicit dir on it, around it or first inside
  // it, else its first strong character (what dir="auto" does).
  const isRtl = (el) => {
    const set = el.closest('[dir="rtl"],[dir="ltr"]') || el.querySelector('[dir="rtl"],[dir="ltr"]');
    if (set) return set.getAttribute('dir') === 'rtl';
    const m = el.textContent.match(/[A-Za-z\u00c0-\u02af\u0370-\u052f]|[\u0590-\u08ff\ufb1d-\ufdff\ufe70-\ufeff]/);
    return m ? m[0] >= '\u0590' : getComputedStyle(el).direction === 'rtl';
  };
  const exec = (cmd, value, css) => {
    document.execCommand('styleWithCSS', false, !!css);
    return document.execCommand(cmd, false, value ?? null);
  };
  const state = (c) => { try { return document.queryCommandState(c); } catch (_) { return false; } };

  // Run a command in an editor that may not have focus (toolbar clicks
  // from the To field): put the caret back where the user left it.
  function focusEditor(ed) {
    if (editorOf(caretNode()) === ed && document.activeElement === ed) return;
    ed.focus();
    const sel = getSelection();
    const r = ed._pgRange;
    if (r && ed.contains(r.startContainer)) { sel.removeAllRanges(); sel.addRange(r); }
    else { const end = document.createRange(); end.selectNodeContents(ed); end.collapse(false); sel.removeAllRanges(); sel.addRange(end); }
  }

  // Indents are blockquotes with a start margin; WebKit always puts it on
  // the left. RTL paragraphs need it on the right (physical sides, since
  // Outlook's renderer has no margin-inline).
  function fixIndents(ed) {
    for (const q of ed.querySelectorAll('blockquote:not([data-coo-quote])')) {
      const l = q.style.marginLeft, r = q.style.marginRight;
      if (isRtl(q) && l === '40px' && (!r || r === '0px')) { q.style.marginLeft = '0px'; q.style.marginRight = '40px'; }
      else if (!isRtl(q) && r === '40px' && (!l || l === '0px')) { q.style.marginRight = '0px'; q.style.marginLeft = '40px'; }
    }
  }
  function quoteStyle(q) {
    const side = isRtl(q) ? 'right' : 'left';
    q.setAttribute('style', 'margin:0 0 0 0;margin-' + side + ':.8ex;border-' + side + ':2px solid #ccc;padding-' + side + ':1ex;color:#555');
  }
  // Quote toggles with formatBlock (so ⌘Z undoes it). A quote holding
  // several blocks comes apart by hand.
  function toggleQuote(ed) {
    const q = around(ed, 'blockquote[data-coo-quote]');
    if (q) {
      exec('formatBlock', 'div');
      const still = around(ed, 'blockquote[data-coo-quote]');
      if (still) { still.replaceWith(...still.childNodes); ed.dispatchEvent(new Event('input', { bubbles: true })); }
      return;
    }
    exec('formatBlock', 'blockquote');
    const nq = around(ed, 'blockquote');
    if (nq) { nq.setAttribute('data-coo-quote', ''); quoteStyle(nq); }
  }

  // Font sizes are the legacy 1–7 steps, written as CSS keywords
  // (small, medium, large…). "Normal" is medium: the recipient's own size.
  const SIZES = [['Small', '2'], ['Normal', '3'], ['Large', '4'], ['Larger', '5'], ['Huge', '6']];
  function stepSize(dir) {
    const cur = parseInt(document.queryCommandValue('fontSize'), 10) || 3;
    exec('fontSize', String(Math.min(6, Math.max(1, cur + dir))), true);
  }

  function normalizeUrl(v) {
    v = v.trim();
    if (!v) return null;
    if (/^(https?:|mailto:|tel:)/i.test(v)) return v;
    if (/^[^\s@\/]+@[^\s@]+\.[^\s@]+$/.test(v)) return 'mailto:' + v;
    if (/^[\w-]+(\.[\w-]+)+([\/?#:].*)?$/.test(v)) return 'https://' + v;
    return null;
  }
  const esc = (s) => s.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));

  function run(ed, cmd, value, btn) {
    if (!ed) return;
    focusEditor(ed);
    switch (cmd) {
      case 'link': return linkPop(ed, btn);
      case 'size': case 'color': case 'highlight': case 'align': return menuPop(ed, cmd, btn);
      case 'quote': toggleQuote(ed); break;
      case 'grow': stepSize(1); break;
      case 'shrink': stepSize(-1); break;
      case 'fontSize': exec('fontSize', value, true); break;
      case 'foreColor': exec('foreColor', value, true); break;
      case 'hiliteColor': exec('hiliteColor', value, true); break;
      case 'removeFormat': exec('removeFormat'); exec('unlink'); break;
      default: exec(cmd, value);
    }
    if (cmd === 'indent' || cmd === 'outdent') fixIndents(ed);
    refresh(ed);
  }

  // ---- Popovers: size, colors, alignment, link ----
  let pop = null;
  function closePop() { if (pop) { pop.remove(); pop = null; } }
  function openPop(btn, el) {
    closePop();
    pop = el; el.classList.add('menu', 'fmt-pop'); el.setAttribute('role', 'menu');
    el.addEventListener('mousedown', (e) => { if (e.target.tagName !== 'INPUT') e.preventDefault(); });
    document.body.appendChild(el);
    const r = btn.getBoundingClientRect(), z = parseFloat(getComputedStyle(document.documentElement).zoom) || 1;
    const w = el.offsetWidth * z;
    el.style.left = Math.max(8, Math.min(innerWidth - w - 8, r.left)) / z + 'px';
    el.style.top = (r.bottom + 4) / z + 'px';
  }
  const TEXT_COLORS = ['#d93025', '#e8710a', '#b38600', '#188038', '#12857a', '#1a73e8', '#3c4fb8', '#9334e6', '#c2185b', '#80868b', '#5f6368', '#000000'];
  const MARKS = ['#fff475', '#ccff90', '#a7ffeb', '#cbf0f8', '#ffd1dc', '#ffe0b2'];
  function item(label, onPick, extra) {
    const b = document.createElement('button');
    b.type = 'button'; b.className = 'menu-item'; b.setAttribute('role', 'menuitem');
    b.innerHTML = label; if (extra) extra(b);
    b.addEventListener('click', () => { closePop(); onPick(); });
    return b;
  }
  const icon = (name) => '<svg class="icon icon--sm" aria-hidden="true"><use href="#i-' + name + '"></use></svg>';
  function menuPop(ed, kind, btn) {
    const el = document.createElement('div');
    const again = (c, v) => () => run(ed, c, v);
    if (kind === 'size') {
      const cur = document.queryCommandValue('fontSize');
      for (const [label, v] of SIZES) el.appendChild(item('<span class="fmt-size" data-size="' + v + '">' + label + '</span>', again('fontSize', v),
        (b) => b.setAttribute('data-active', String(cur === v))));
    } else if (kind === 'align') {
      for (const [c, i] of [['justifyLeft', 'align-left'], ['justifyCenter', 'align-center'], ['justifyRight', 'align-right']]) {
        el.appendChild(item(icon(i) + (c === 'justifyLeft' ? 'Left' : c === 'justifyCenter' ? 'Center' : 'Right') +
          '<span class="menu-item__hint">' + (LABEL[c] || '') + '</span>', again(c),
          (b) => b.setAttribute('data-active', String(alignOf(ed) === c))));
      }
    } else {
      const color = kind === 'color';
      el.appendChild(item(icon(color ? 'fmt-color' : 'fmt-highlight') + (color ? 'Automatic' : 'No highlight'),
        again(color ? 'foreColor' : 'hiliteColor', color ? 'inherit' : 'transparent')));
      const grid = document.createElement('div'); grid.className = 'fmt-swatches';
      for (const c of color ? TEXT_COLORS : MARKS) {
        const s = document.createElement('button');
        s.type = 'button'; s.className = 'fmt-swatch'; s.title = c; s.style.setProperty('--sw', c);
        s.addEventListener('click', () => { closePop(); run(ed, color ? 'foreColor' : 'hiliteColor', c); });
        grid.appendChild(s);
      }
      el.appendChild(grid);
    }
    openPop(btn, el);
  }
  function linkPop(ed, btn) {
    const sel = getSelection();
    const range = sel.rangeCount ? sel.getRangeAt(0).cloneRange() : null;
    const a = around(ed, 'a[href]');
    const el = document.createElement('div');
    el.className = 'fmt-link';
    const field = (ph, v) => {
      const i = document.createElement('input');
      i.className = 'input input--sm'; i.placeholder = ph; i.value = v || '';
      i.dataset.suggestOpen = 'true'; // Escape closes this, not the composer
      el.appendChild(i); return i;
    };
    const text = (!a && range && range.collapsed) ? field('Text to show') : null;
    const url = field('Address: https://… or name@example.com', a ? a.getAttribute('href') : '');
    const row = document.createElement('div'); row.className = 'fmt-link__row';
    const mk = (label, cls, fn) => { const b = document.createElement('button'); b.type = 'button'; b.className = 'btn btn--sm ' + cls; b.textContent = label; b.addEventListener('click', fn); row.appendChild(b); return b; };
    const back = () => { closePop(); ed.focus(); if (range) { sel.removeAllRanges(); sel.addRange(range); } };
    const apply = () => {
      const href = normalizeUrl(url.value);
      if (!href) { url.setAttribute('aria-invalid', 'true'); url.focus(); return; }
      back();
      if (a) { const r = document.createRange(); r.selectNodeContents(a); sel.removeAllRanges(); sel.addRange(r); exec('createLink', href); }
      else if (range && range.collapsed) exec('insertHTML', '<a href="' + esc(href) + '">' + esc((text && text.value.trim()) || url.value.trim()) + '</a>');
      else exec('createLink', href);
      refresh(ed);
    };
    if (a) mk('Remove link', 'btn--ghost', () => { back(); const r = document.createRange(); r.selectNodeContents(a); sel.removeAllRanges(); sel.addRange(r); exec('unlink'); refresh(ed); });
    const sp = document.createElement('span'); sp.className = 'spacer'; row.appendChild(sp);
    mk(a ? 'Update' : 'Add link', 'btn--primary', apply);
    el.appendChild(row);
    for (const i of el.querySelectorAll('input')) i.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') { e.preventDefault(); apply(); }
      else if (e.key === 'Escape') { e.preventDefault(); back(); }
    });
    openPop(btn, el);
    (text || url).focus();
  }
  document.addEventListener('mousedown', (e) => { if (pop && !pop.contains(e.target) && !(e.target.closest && e.target.closest('.fmt-bar'))) closePop(); }, true);
  // On window, ahead of the bridge's Escape (which closes the composer).
  addEventListener('keydown', (e) => { if (pop && e.key === 'Escape' && !pop.contains(e.target)) { e.preventDefault(); e.stopPropagation(); closePop(); } }, true);

  // ---- Toolbar: clicks, and pressed state following the caret ----
  document.addEventListener('mousedown', (e) => {
    const b = e.target.closest && e.target.closest('.fmt-bar');
    if (b) e.preventDefault(); // keep the editor's selection
  });
  document.addEventListener('click', (e) => {
    const btn = e.target.closest && e.target.closest('.fmt-bar [data-fmt]');
    if (!btn) return;
    const ed = document.getElementById(btn.closest('.fmt-bar').dataset.for);
    if (pop && pop._for === btn) { closePop(); return; }
    run(ed, btn.dataset.fmt, null, btn);
    if (pop) pop._for = btn;
  });
  function alignOf(ed) {
    const b = blockAt(ed, caretNode()) || ed;
    const a = getComputedStyle(b).textAlign;
    const rtl = isRtl(b);
    if (a === 'center' || a === '-webkit-center') return 'justifyCenter';
    if (a === 'right' || a === '-webkit-right' || (rtl && (a === 'start' || a === 'justify')) || (!rtl && a === 'end')) return 'justifyRight';
    return 'justifyLeft';
  }
  const ALIGN_ICON = { justifyLeft: 'align-left', justifyCenter: 'align-center', justifyRight: 'align-right' };
  function refresh(ed) {
    const bar = barFor(ed);
    if (!bar || editorOf(caretNode()) !== ed) return;
    for (const b of bar.querySelectorAll('[data-fmt]')) {
      const c = b.dataset.fmt;
      let on = null;
      if (['bold', 'italic', 'underline', 'strikeThrough', 'insertUnorderedList', 'insertOrderedList'].includes(c)) on = state(c);
      else if (c === 'link') on = !!around(ed, 'a[href]');
      else if (c === 'quote') on = !!around(ed, 'blockquote[data-coo-quote]');
      if (on !== null) b.setAttribute('aria-pressed', String(on));
    }
    const al = bar.querySelector('[data-fmt="align"] use');
    if (al) al.setAttribute('href', '#i-' + ALIGN_ICON[alignOf(ed)]);
    const swatch = (c, v) => { const b = bar.querySelector('[data-fmt="' + c + '"]'); if (b) b.style.setProperty('--fmt-swatch', v); };
    const fg = document.queryCommandValue('foreColor'), bg = document.queryCommandValue('hiliteColor');
    const n = caretNode(); const el = n && (n.nodeType === 1 ? n : n.parentElement);
    const own = el && el.closest('[style*="color"]');
    swatch('color', own && ed.contains(own) && own !== ed && fg ? fg : 'currentColor');
    swatch('highlight', bg && !/^(transparent|rgba\(0, 0, 0, 0\))$/.test(bg) ? bg : 'currentColor');
  }
  document.addEventListener('selectionchange', () => {
    const ed = editorOf(caretNode());
    if (ed) refresh(ed);
  });

  // ---- List autoformat and list keys ----
  // `auto` remembers the list Space just made, so an immediate Backspace
  // turns it back into the typed text (like Word).
  let auto = null;
  const listCmd = (li) => li.parentElement && li.parentElement.tagName === 'OL' ? 'insertOrderedList' : 'insertUnorderedList';
  const nested = (li) => { const p = li.parentElement && li.parentElement.parentElement; return !!p && /^(LI|OL|UL)$/.test(p.tagName); };
  const caretRange = () => { const s = getSelection(); return s.rangeCount ? s.getRangeAt(0) : null; };
  // Text between the start of `block` and the caret (null when there's a
  // line break or an image in between).
  function before(block, r) {
    const pre = document.createRange();
    pre.setStart(block, 0); pre.setEnd(r.startContainer, r.startOffset);
    const f = pre.cloneContents();
    if (f.querySelector && f.querySelector('br,img')) return null;
    return pre.toString();
  }
  const emptyItem = (li) => li.textContent.replace(/[\u200b\u00a0\s]/g, '') === '' && !li.querySelector('img');
  function autoList(ed, r) {
    const block = blockAt(ed, r.startContainer);
    if (!block || block.tagName === 'LI' || block.closest('li')) return false;
    const typed = before(block, r);
    const m = typed !== null && AUTO_LIST.exec(typed);
    if (!m) return false;
    const dir = block.getAttribute('dir');
    const sel = getSelection();
    const pre = document.createRange(); pre.setStart(block, 0); pre.setEnd(r.startContainer, r.startOffset);
    sel.removeAllRanges(); sel.addRange(pre);
    exec('delete');
    exec(m[1] ? 'insertUnorderedList' : 'insertOrderedList');
    const li = around(ed, 'li');
    if (!li) return true;
    const list = li.parentElement;
    if (m[2]) {
      const n = [...m[2]].reduce((v, c) => v * 10 + (c.codePointAt(0) % 16), 0);
      if (n !== 1 && list.children.length === 1) list.setAttribute('start', String(n));
    } else if (m[3]) {
      list.setAttribute('type', m[3]);
      list.style.listStyleType = m[3] === 'a' ? 'lower-alpha' : 'upper-alpha';
    }
    if (dir === 'rtl' || dir === 'ltr') li.setAttribute('dir', dir);
    auto = { li, text: typed + ' ' };
    return true;
  }
  document.addEventListener('keydown', (e) => {
    const ed = e.target.closest && e.target.closest('[data-editor]');
    if (!ed) return;
    if (e.isComposing) return;
    const was = auto;
    if (!['Shift', 'Control', 'Alt', 'Meta'].includes(e.key)) auto = null;
    // Formatting shortcuts, by physical key.
    if (e.metaKey && !e.ctrlKey && !e.altKey) {
      const c = e.code || '';
      const base = c.startsWith('Key') ? c.slice(3).toLowerCase() : c.startsWith('Digit') ? c.slice(5)
        : ({ BracketLeft: '[', BracketRight: ']', Backslash: '\\', Period: '.', Comma: ',' })[c];
      const cmd = base && KEYS.get('meta+' + (e.shiftKey ? 'shift+' : '') + base);
      if (cmd) {
        e.preventDefault(); e.stopPropagation();
        const btn = barFor(ed) && barFor(ed).querySelector('[data-fmt="' + (cmd.startsWith('justify') ? 'align' : cmd) + '"]');
        run(ed, cmd, null, btn || ed);
      }
      return;
    }
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const r = caretRange();
    if (!r) return;
    if (e.key === ' ' && r.collapsed) {
      if (autoList(ed, r)) { e.preventDefault(); refresh(ed); }
      return;
    }
    const li = around(ed, 'li');
    if (!li || !r.collapsed) return;
    if (e.key === 'Enter' && !e.shiftKey && emptyItem(li)) {
      // A second Enter (on an empty item) ends the list, or steps a
      // nested item out one level.
      e.preventDefault();
      exec(nested(li) ? 'outdent' : listCmd(li));
      fixIndents(ed); refresh(ed);
    } else if (e.key === 'Backspace' && before(li, r) === '') {
      e.preventDefault();
      if (was && was.li === li && li.isConnected) {
        exec(listCmd(li));
        exec('insertText', was.text);
      } else {
        exec(nested(li) ? 'outdent' : listCmd(li));
      }
      fixIndents(ed); refresh(ed);
    } else if (e.key === 'Tab') {
      e.preventDefault();
      exec(e.shiftKey ? 'outdent' : 'indent');
      fixIndents(ed); refresh(ed);
    }
  }, true);
  document.addEventListener('mousedown', () => { auto = null; }, true);
  // List items get their own direction, so a Hebrew item in an English
  // list (or the other way round) reads right.
  document.addEventListener('input', (e) => {
    const ed = e.target.closest && e.target.closest('[data-editor]');
    if (!ed) return;
    for (const li of ed.querySelectorAll('li:not([dir])')) li.setAttribute('dir', 'auto');
    scrub(ed);
  }, true);
  // WebKit keeps the look of text it moves (lists, quotes, insertHTML) by
  // copying the styles around it onto a span: our CSS variables, which mean
  // nothing in the recipient's mail, and the theme's text color, which would
  // send dark mode's light gray. Drop those; what the user chose stays.
  function scrub(ed) {
    const own = getComputedStyle(ed).color;
    for (const el of ed.querySelectorAll('[style]')) {
      const st = el.style;
      for (let i = st.length - 1; i >= 0; i--) {
        const p = st[i], v = st.getPropertyValue(p);
        if (v.includes('var(--') || (p === 'color' && (v === own || v === 'inherit')) ||
            (p === 'display' && el.tagName === 'A')) st.removeProperty(p);
      }
      if (!st.length) el.removeAttribute('style');
    }
  }

  window.__pigeonFormatDemo = (edId) => { // debug screenshots only
    const ed = document.getElementById(edId); if (!ed) return;
    ed.innerHTML = '<div dir="auto">Thanks — the <b>revised deck</b> looks <span style="color: rgb(26, 115, 232);">great</span>, ' +
      '<span style="background-color: rgb(255, 244, 117);">two notes</span>:</div>' +
      '<ol dir="auto"><li dir="auto">Move the <i>pricing</i> slide up</li><li dir="auto">Add the <a href="https://example.com">Q3 numbers</a></li></ol>' +
      '<ul dir="auto"><li dir="auto">שלום, נדבר מחר</li></ul>' +
      '<blockquote data-coo-quote="" dir="auto" style="margin:0 0 0 0;margin-left:.8ex;border-left:2px solid #ccc;padding-left:1ex;color:#555">Ship it.</blockquote>';
    const t = ed.querySelector('b').firstChild; const r = document.createRange(); r.setStart(t, 3); r.collapse(true);
    ed.focus(); getSelection().removeAllRanges(); getSelection().addRange(r); refresh(ed);
  };
})();
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    enum Auto {
        Bullet,
        Numbered(u32),
        Lettered(char),
    }

    /// The script's reading of a match, in Rust.
    fn auto_list(typed: &str) -> Option<Auto> {
        let re = regex::Regex::new(AUTO_LIST_RE).expect("pattern compiles");
        let m = re.captures(typed)?;
        if m.get(1).is_some() {
            Some(Auto::Bullet)
        } else if let Some(n) = m.get(2) {
            // Each digit's code point ends in its value (0x30, 0x660, 0x6F0).
            Some(Auto::Numbered(n.as_str().chars().fold(0, |v, c| v * 10 + (c as u32 % 16))))
        } else {
            m.get(3).and_then(|l| l.as_str().chars().next()).map(Auto::Lettered)
        }
    }

    #[test]
    fn bullets() {
        for b in ["-", "*", "•", " -", "\u{a0}*", "\u{200f}-", "\u{200e}•"] {
            assert_eq!(auto_list(b), Some(Auto::Bullet), "{b:?}");
        }
    }

    #[test]
    fn asterisk_space_starts_a_bulleted_list() {
        assert_eq!(auto_list("*"), Some(Auto::Bullet));
        assert_eq!(auto_list("**"), None);
        assert_eq!(auto_list("*bold*"), None);
    }

    #[test]
    fn numbers_and_letters() {
        assert_eq!(auto_list("1."), Some(Auto::Numbered(1)));
        assert_eq!(auto_list("1)"), Some(Auto::Numbered(1)));
        assert_eq!(auto_list("12."), Some(Auto::Numbered(12)));
        assert_eq!(auto_list("3)"), Some(Auto::Numbered(3)));
        // Arabic-Indic and Extended Arabic-Indic digits (RTL text).
        assert_eq!(auto_list("١."), Some(Auto::Numbered(1)));
        assert_eq!(auto_list("\u{200f}۲٣)"), Some(Auto::Numbered(23)));
        assert_eq!(auto_list("a."), Some(Auto::Lettered('a')));
        assert_eq!(auto_list("A)"), Some(Auto::Lettered('A')));
    }

    #[test]
    fn ordinary_text_stays_text() {
        for t in [
            "", " ", "1", "1.2.", "10.5", "1234.", "-1.", "--", "- x", "a", "b.", "e.g.", "I.", "שלום", "1. x", "x 1.",
            "(1)",
        ] {
            assert_eq!(auto_list(t), None, "{t:?}");
        }
    }

    #[test]
    fn shortcuts_are_unique_and_documented() {
        let mut combos: Vec<&str> = SHORTCUTS.iter().map(|s| s.1).collect();
        combos.sort_unstable();
        combos.dedup();
        assert_eq!(combos.len(), SHORTCUTS.len(), "two commands share a key");
        // The editor keeps every key the app forwards outside editors
        // (⌘K palette, ⌘R reply…) only while the editor has focus; the
        // bridge's own editor keys must never be taken.
        for k in ["meta+Enter", "meta+=", "meta+-", "meta+0"] {
            assert!(SHORTCUTS.iter().all(|s| s.1 != k), "{k} belongs to the bridge");
        }
        assert_eq!(tip("bold"), "Bold (⌘B)");
        let js = script();
        assert!(js.contains(r#"["meta+shift+x","strikeThrough","⇧⌘X"]"#), "keys reach the script");
        assert!(!js.contains("__KEYS__") && !js.contains("__AUTO_LIST_RE__"));
    }
}
