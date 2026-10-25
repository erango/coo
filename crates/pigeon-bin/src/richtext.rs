//! Composer rich text: the formatting commands (toolbar buttons and their
//! shortcuts), the emoji picker, and autoformat as you type: lists ("1. ",
//! "- "), inline code (`x`), code blocks (```), emoticons (":) " → 🙂).
//! The DOM work is the script below, run with the bridge in every window;
//! the shortcut table and the patterns live here as data, handed to the
//! script, so the tests check what the editor really does.
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
    ("code", "meta+shift+c", "⇧⌘C", "Inline code"),
    ("codeBlock", "meta+alt+shift+c", "⌥⇧⌘C", "Code block"),
    ("removeFormat", "meta+\\", "⌘\\", "Clear formatting"),
    ("undo", "meta+z", "⌘Z", "Undo"),
    ("redo", "meta+shift+z", "⇧⌘Z", "Redo"),
];

/// Typing that formats, for Settings › Keyboard shortcuts.
pub const TYPING_KEYS: &[(&str, &str)] = &[
    ("1.  or  -  then Space", "Start a numbered or bulleted list"),
    ("↵ on an empty item", "End the list"),
    ("Tab · ⇧Tab in a list", "Indent · outdent the item"),
    ("`code`", "Inline code"),
    ("``` then ↵", "Code block (↵ on two empty lines leaves it)"),
    (":)  ;)  <3 … then Space", "Emoji"),
    ("⌫ right after", "Undo the automatic formatting"),
    ("⌃⌘Space", "All emoji and symbols (macOS)"),
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

/// Inline code: the text before the caret, when the closing backtick is
/// typed, ends with an opening backtick (at a word start) and the code. It
/// can't start or end with a space, so "`  `" and a lone "`" stay. Groups:
/// 1 what's before the backtick, 2 the code. Same in JS and Rust syntax.
pub const INLINE_CODE_RE: &str = "(^|[\\s(\\[{\"'])`([^`\\s](?:[^`]*[^`\\s])?)$";

/// A line that starts a code block when Enter is typed: ``` and an
/// optional language name.
pub const CODE_FENCE_RE: &str = "^[ \u{a0}]*```[A-Za-z0-9+#-]*[ \u{a0}]*$";

/// Inline styles for code, so recipients' mail shows it as code.
pub const CODE_STYLE: &str = "font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:.9em;\
     background-color:#f2f3f5;border-radius:4px;padding:1px 4px";
pub const PRE_STYLE: &str = "font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:13px;\
     line-height:1.45;white-space:pre-wrap;tab-size:4;text-align:left;background-color:#f6f8fa;\
     border:1px solid #e1e4e8;border-radius:6px;padding:8px 10px;margin:4px 0";

/// Emoticons typed as a word of their own (line start or after a space)
/// and followed by a space, Enter or `.,!?` become emoji. Unambiguous ones
/// only: nothing that turns up in URLs, times ("10:30") or code ("a:b"),
/// and no "B)", which is also "plan B)" and a list marker.
pub const EMOTICONS: &[(&str, &str)] = &[
    (":-)", "🙂"),
    (":)", "🙂"),
    (":-D", "😃"),
    (":D", "😃"),
    (";-)", "😉"),
    (";)", "😉"),
    (":-(", "🙁"),
    (":(", "🙁"),
    (":'(", "😢"),
    (":-P", "😛"),
    (":P", "😛"),
    (":-p", "😛"),
    (":p", "😛"),
    (":-O", "😮"),
    (":O", "😮"),
    (":o", "😮"),
    ("<3", "❤️"),
    ("B-)", "😎"),
    ("8-)", "😎"),
];

/// Keys that end an emoticon.
pub const EMOTICON_ENDS: &[&str] = &[" ", "Enter", ".", ",", "!", "?"];

/// The script: formatting toolbar, shortcuts, emoji picker, autoformat.
pub fn script() -> String {
    let keys: Vec<(&str, &str, &str)> = SHORTCUTS.iter().map(|s| (s.1, s.0, s.2)).collect();
    let json = |v: &str| serde_json::to_string(v).unwrap_or_default();
    JS.replace("__KEYS__", &serde_json::to_string(&keys).unwrap_or_default())
        .replace("__AUTO_LIST_RE__", &json(AUTO_LIST_RE))
        .replace("__INLINE_CODE_RE__", &json(INLINE_CODE_RE))
        .replace("__CODE_FENCE_RE__", &json(CODE_FENCE_RE))
        .replace("__CODE_STYLE__", &json(CODE_STYLE))
        .replace("__PRE_STYLE__", &json(PRE_STYLE))
        .replace("__EMOTICONS__", &serde_json::to_string(EMOTICONS).unwrap_or_default())
        .replace("__EMOTICON_ENDS__", &serde_json::to_string(EMOTICON_ENDS).unwrap_or_default())
        .replace("__EMOJI__", &crate::emoji::json())
}

const JS: &str = r##"
(function () {
  const TABLE = __KEYS__;
  const KEYS = new Map(TABLE.map(([k, c]) => [k, c]));
  const LABEL = Object.fromEntries(TABLE.map(([, c, l]) => [c, l]));
  const AUTO_LIST = new RegExp(__AUTO_LIST_RE__);
  const INLINE_CODE = new RegExp(__INLINE_CODE_RE__), FENCE = new RegExp(__CODE_FENCE_RE__);
  const CODE_STYLE = __CODE_STYLE__, PRE_STYLE = __PRE_STYLE__;
  const EMOTICONS = __EMOTICONS__, EMOTICON_ENDS = new Set(__EMOTICON_ENDS__);
  const editorOf = (n) => { const el = n && (n.nodeType === 1 ? n : n.parentElement); return el && el.closest ? el.closest('[data-editor]') : null; };
  const barFor = (ed) => ed && ed.id ? document.querySelector('.fmt-bar[data-for="' + ed.id + '"]') : null;
  const caretNode = () => { const s = getSelection(); return s.rangeCount ? s.anchorNode : null; };
  // Nearest element matching `sel` around the caret, inside the editor.
  const around = (ed, sel) => {
    const n = caretNode(); const el = n && (n.nodeType === 1 ? n : n.parentElement);
    const hit = el && el.closest(sel);
    return hit && ed.contains(hit) && hit !== ed ? hit : null;
  };
  const BLOCK = /^(DIV|P|LI|BLOCKQUOTE|PRE|H[1-6])$/;
  const blockAt = (ed, n) => {
    for (; n && n !== ed; n = n.parentNode) if (n.nodeType === 1 && BLOCK.test(n.tagName)) return n;
    return null;
  };
  const STRONG = /[A-Za-z\u00c0-\u02af\u0370-\u052f]|[\u0590-\u08ff\ufb1d-\ufdff\ufe70-\ufeff]/;
  // A block's direction: an explicit dir on it, around it or first inside
  // it, else its first strong character (what dir="auto" does).
  const isRtl = (el) => {
    const set = el.closest('[dir="rtl"],[dir="ltr"]') || el.querySelector('[dir="rtl"],[dir="ltr"]');
    if (set) return set.getAttribute('dir') === 'rtl';
    const m = el.textContent.match(STRONG);
    return m ? m[0] >= '\u0590' : getComputedStyle(el).direction === 'rtl';
  };
  // An item's own direction: its explicit dir, else its first strong
  // character; null while it has no letters yet.
  const itemDir = (li) => {
    const a = li.getAttribute('dir');
    if (a === 'rtl' || a === 'ltr') return a;
    const m = li.textContent.match(STRONG);
    return m ? (m[0] >= '\u0590' ? 'rtl' : 'ltr') : null;
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

  // ---- Code: inline <code> and <pre> blocks, always left to right ----
  // WebKit's insertHTML turns <code> into a styled span, so inline code goes
  // in through the DOM (Backspace or ⌘Z right after still takes it back).
  const changed = (ed) => ed.dispatchEvent(new Event('input', { bubbles: true }));
  function makeCode(t) {
    const c = document.createElement('code');
    c.setAttribute('dir', 'ltr'); c.setAttribute('style', CODE_STYLE); c.textContent = t;
    return c;
  }
  // Put `c` in place of range `r`, caret after it (or the code selected).
  // A zero-width space after the code keeps what's typed next out of it.
  function placeCode(ed, r, c, select) {
    r.deleteContents(); r.insertNode(c);
    const z = document.createTextNode('\u200b'); c.after(z);
    const after = document.createRange();
    if (select) after.selectNodeContents(c); else { after.setStart(z, 1); after.collapse(true); }
    const sel = getSelection(); sel.removeAllRanges(); sel.addRange(after);
    changed(ed);
  }
  const stylePre = (p) => { if (p) { p.setAttribute('dir', 'ltr'); p.setAttribute('style', PRE_STYLE); } };
  function toggleCode(ed) {
    const sel = getSelection();
    const c = around(ed, 'code');
    if (c) {
      const t = document.createTextNode(c.textContent);
      c.replaceWith(t);
      const r = document.createRange(); r.selectNodeContents(t); sel.removeAllRanges(); sel.addRange(r);
      changed(ed);
      return;
    }
    if (sel.rangeCount && !sel.isCollapsed) { const r = sel.getRangeAt(0); placeCode(ed, r, makeCode(r.toString()), true); }
  }
  // A block's lines as plain blocks again (a <pre> keeps them as "\n").
  function unPre(ed, pre) {
    const blocks = pre.textContent.replace(/\n$/, '').split('\n').map((l) => {
      const d = document.createElement('div'); d.setAttribute('dir', 'auto');
      if (l) d.textContent = l; else d.appendChild(document.createElement('br'));
      return d;
    });
    pre.replaceWith(...blocks);
    caretAt(blocks[blocks.length - 1], false);
    ed.dispatchEvent(new Event('input', { bubbles: true }));
  }
  function caretAt(node, start) {
    const r = document.createRange(); r.selectNodeContents(node); r.collapse(!!start);
    const sel = getSelection(); sel.removeAllRanges(); sel.addRange(r);
  }
  function toggleCodeBlock(ed) {
    const pre = around(ed, 'pre');
    if (pre) return unPre(ed, pre);
    exec('formatBlock', 'pre');
    stylePre(around(ed, 'pre'));
  }
  // Leave a code block: a plain line after it, caret there.
  function leavePre(ed, pre) {
    const d = document.createElement('div'); d.setAttribute('dir', 'auto'); d.appendChild(document.createElement('br'));
    pre.after(d); caretAt(d, true);
    ed.dispatchEvent(new Event('input', { bubbles: true }));
  }
  // The range of characters [from, to) of `root`'s text.
  function textRange(root, from, to) {
    const r = document.createRange();
    const w = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    let n, at = 0, started = false;
    while ((n = w.nextNode())) {
      const end = at + n.data.length;
      if (!started && from <= end) { r.setStart(n, from - at); started = true; }
      if (started && to <= end) { r.setEnd(n, to - at); return r; }
      at = end;
    }
    return started ? (r.setEnd(root, root.childNodes.length), r) : null;
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
      case 'code': toggleCode(ed); break;
      case 'codeBlock': toggleCodeBlock(ed); break;
      case 'emoji': return emojiPop(ed, btn);
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
    // Whole pixels, so the popover's text and icons stay sharp.
    el.style.left = Math.round(Math.max(8, Math.min(innerWidth - w - 8, r.left)) / z) + 'px';
    el.style.top = Math.round((r.bottom + 4) / z) + 'px';
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
  // Emoji: sections plus "Frequently used" (this Mac only), a search box,
  // arrow keys through the grid, Enter inserts, Esc closes.
  const [EMOJI, STARTERS] = __EMOJI__;
  const recent = () => { try { return JSON.parse(localStorage.getItem('coo.emoji') || '{}'); } catch (_) { return {}; } };
  function used(e) {
    const r = recent(); r[e] = (r[e] || 0) + 1;
    try { localStorage.setItem('coo.emoji', JSON.stringify(r)); } catch (_) {}
  }
  function frequent() {
    const r = recent();
    const top = Object.keys(r).sort((a, b) => r[b] - r[a]).slice(0, 16);
    for (const s of STARTERS) if (top.length < 16 && !top.includes(s)) top.push(s);
    return top;
  }
  const NAMES = new Map(EMOJI.flatMap(([, l]) => l));
  function emojiPop(ed, btn) {
    const sel = getSelection();
    const range = sel.rangeCount ? sel.getRangeAt(0).cloneRange() : null;
    const el = document.createElement('div'); el.className = 'fmt-emoji';
    const q = document.createElement('input');
    q.className = 'input input--sm'; q.placeholder = 'Search emoji'; q.dataset.suggestOpen = 'true';
    q.setAttribute('aria-label', 'Search emoji');
    const tabs = document.createElement('div'); tabs.className = 'fmt-emoji__tabs'; tabs.setAttribute('role', 'tablist');
    const grid = document.createElement('div'); grid.className = 'fmt-emoji__grid'; grid.setAttribute('role', 'grid');
    el.append(q, tabs, grid);
    const insert = (e) => {
      closePop(); ed.focus();
      if (range) { sel.removeAllRanges(); sel.addRange(range); }
      exec('insertText', e); used(e);
    };
    const show = (title, list) => {
      grid.replaceChildren();
      const h = document.createElement('div'); h.className = 'menu__label'; h.textContent = title; grid.appendChild(h);
      if (!list.length) { const n = document.createElement('div'); n.className = 'fmt-emoji__none'; n.textContent = 'No emoji found'; grid.appendChild(n); }
      for (const e of list) {
        const b = document.createElement('button');
        b.type = 'button'; b.className = 'fmt-emoji__cell'; b.textContent = e;
        b.title = NAMES.get(e) || ''; b.setAttribute('aria-label', b.title || e);
        b.addEventListener('click', () => insert(e));
        grid.appendChild(b);
      }
    };
    const sections = [['Frequently used', frequent()], ...EMOJI.map(([n, l]) => [n, l.map((x) => x[0])])];
    sections.forEach(([n, list], i) => {
      const t = document.createElement('button');
      t.type = 'button'; t.className = 'fmt-emoji__tab'; t.setAttribute('role', 'tab'); t.title = n;
      t.textContent = i === 0 ? '🕘' : list[0];
      t.addEventListener('click', () => { q.value = ''; select(i); });
      tabs.appendChild(t);
    });
    const select = (i) => {
      [...tabs.children].forEach((t, j) => t.setAttribute('aria-selected', String(i === j)));
      show(sections[i][0], sections[i][1]);
    };
    q.addEventListener('input', () => {
      const words = q.value.trim().toLowerCase().split(/\s+/).filter(Boolean);
      if (!words.length) return select(0);
      [...tabs.children].forEach((t) => t.setAttribute('aria-selected', 'false'));
      const hits = [...NAMES].filter(([, n]) => words.every((w) => n.split(' ').some((x) => x.startsWith(w)))).map((x) => x[0]);
      show('Results', hits.slice(0, 64));
    });
    const cells = () => [...grid.querySelectorAll('.fmt-emoji__cell')];
    el.addEventListener('keydown', (e) => {
      const all = cells(), i = all.indexOf(document.activeElement);
      const cols = 8;
      if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); closePop(); ed.focus(); if (range) { sel.removeAllRanges(); sel.addRange(range); } return; }
      if (e.target === q) {
        if (e.key === 'ArrowDown' && all.length) { e.preventDefault(); all[0].focus(); }
        else if (e.key === 'Enter' && all.length) { e.preventDefault(); insert(all[0].textContent); }
        return;
      }
      if (i < 0) return;
      if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); insert(all[i].textContent); return; }
      const to = { ArrowRight: i + 1, ArrowLeft: i - 1, ArrowDown: i + cols, ArrowUp: i - cols }[e.key];
      if (to !== undefined) {
        e.preventDefault();
        if (to < 0) q.focus(); else if (all[Math.min(to, all.length - 1)]) all[Math.min(to, all.length - 1)].focus();
      }
    });
    select(0);
    openPop(btn, el);
    q.focus();
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
    auto = { kind: 'list', li, text: typed + ' ' };
    return true;
  }

  // ":)" then Space (or Enter, or .,!?) → 🙂. Only a standalone word: at
  // the line start or after a space, so URLs, times and "a:b" stay.
  function emoticon(r, key) {
    const n = r.startContainer;
    if (n.nodeType !== 3) return false;
    const typed = n.data.slice(0, r.startOffset);
    for (const [t, emoji] of EMOTICONS) {
      if (!typed.endsWith(t)) continue;
      const rest = typed.slice(0, typed.length - t.length);
      if (rest !== '' && !/\s$/.test(rest)) continue;
      const x = document.createRange(); x.setStart(n, rest.length); x.setEnd(n, r.startOffset);
      const sel = getSelection(); sel.removeAllRanges(); sel.addRange(x);
      exec('insertText', emoji);
      auto = { kind: 'emoticon', text: t, emoji, key };
      return true;
    }
    return false;
  }
  // "`code" then ` → <code>code</code>.
  function inlineCode(ed, r) {
    const n = r.startContainer;
    if (n.nodeType !== 3) return false;
    const m = INLINE_CODE.exec(n.data.slice(0, r.startOffset));
    if (!m) return false;
    const x = document.createRange(); x.setStart(n, m.index + m[1].length); x.setEnd(n, r.startOffset);
    const c = makeCode(m[2]);
    placeCode(ed, x, c);
    auto = { kind: 'code', el: c, text: '`' + m[2] + '`' };
    return true;
  }
  // "```" (or "```js") then Enter → a code block.
  function fence(ed, r) {
    const block = blockAt(ed, r.startContainer);
    if (!block || block.tagName === 'LI' || block.closest('li')) return false;
    const typed = before(block, r);
    if (typed === null || typed !== block.textContent || !FENCE.test(typed)) return false;
    const x = document.createRange(); x.selectNodeContents(block);
    const sel = getSelection(); sel.removeAllRanges(); sel.addRange(x);
    exec('delete');
    exec('formatBlock', 'pre');
    stylePre(around(ed, 'pre'));
    return true;
  }
  // Keys inside a code block: Enter is a new line; Enter on an empty last
  // line (or a closing ```) leaves the block; Tab is a tab.
  function preKey(ed, pre, r, e) {
    if (e.key === 'Tab') { e.preventDefault(); exec('insertText', '\t'); return; }
    if (e.key !== 'Enter' || !r.collapsed) return;
    e.preventDefault();
    const head = (() => { const x = document.createRange(); x.selectNodeContents(pre); x.setEnd(r.startContainer, r.startOffset); return x.toString(); })();
    const tail = (() => { const x = document.createRange(); x.selectNodeContents(pre); x.setStart(r.startContainer, r.startOffset); return x.toString(); })();
    const line = head.slice(head.lastIndexOf('\n') + 1);
    if (/^\n?$/.test(tail) && (line.trim() === '```' || (line === '' && head.endsWith('\n')))) {
      const cut = textRange(pre, head.length - line.length - 1, pre.textContent.length);
      if (cut) { const sel = getSelection(); sel.removeAllRanges(); sel.addRange(cut); exec('delete'); }
      leavePre(ed, pre);
      return;
    }
    if (head === '' && tail === '') return unPre(ed, pre);
    exec('insertLineBreak');
  }
  function undoAuto(ed, w) {
    const sel = getSelection();
    if (w.kind === 'code') {
      // Through the DOM like the conversion (insertText would keep the
      // code's look on the backticks).
      if (!w.el.isConnected) return false;
      const z = w.el.nextSibling;
      if (z && z.nodeType === 3 && z.data.startsWith('\u200b')) z.deleteData(0, 1);
      const t = document.createTextNode(w.text);
      w.el.replaceWith(t);
      const x = document.createRange(); x.setStart(t, t.length); x.collapse(true);
      sel.removeAllRanges(); sel.addRange(x);
      changed(ed);
      return true;
    }
    if (w.kind !== 'emoticon') return false;
    if (w.key === 'Enter') exec('delete'); // back up onto the line it ended
    const r = sel.getRangeAt(0), n = r.startContainer;
    if (n.nodeType !== 3) return false;
    const typed = n.data.slice(0, r.startOffset);
    const last = w.key === 'Enter' ? '' : typed.slice(-1);
    if (w.key !== 'Enter' && !(last === w.key || (w.key === ' ' && last === '\u00a0'))) return false;
    if (!typed.slice(0, typed.length - last.length).endsWith(w.emoji)) return false;
    const x = document.createRange(); x.setStart(n, r.startOffset - last.length - w.emoji.length); x.setEnd(n, r.startOffset);
    sel.removeAllRanges(); sel.addRange(x);
    exec('insertText', w.text + (last ? w.key : ''));
    return true;
  }
  document.addEventListener('keydown', (e) => {
    const ed = e.target.closest && e.target.closest('[data-editor]');
    if (!ed) return;
    if (e.isComposing) return;
    const was = auto;
    if (!['Shift', 'Control', 'Alt', 'Meta'].includes(e.key)) auto = null;
    // Formatting shortcuts, by physical key. (⌃⌘Space, the Character
    // Viewer, has Control down and passes through.)
    if (e.metaKey && !e.ctrlKey) {
      const c = e.code || '';
      const base = c.startsWith('Key') ? c.slice(3).toLowerCase() : c.startsWith('Digit') ? c.slice(5)
        : ({ BracketLeft: '[', BracketRight: ']', Backslash: '\\', Period: '.', Comma: ',' })[c];
      const cmd = base && KEYS.get('meta+' + (e.altKey ? 'alt+' : '') + (e.shiftKey ? 'shift+' : '') + base);
      if (cmd === 'undo' && was && was.kind === 'code' && undoAuto(ed, was)) { e.preventDefault(); return; }
      if (cmd) {
        e.preventDefault(); e.stopPropagation();
        const btn = barFor(ed) && barFor(ed).querySelector('[data-fmt="' + (cmd.startsWith('justify') ? 'align' : cmd) + '"]');
        run(ed, cmd, null, btn || ed);
      }
      return;
    }
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    let r = caretRange();
    if (!r) return;
    // Backspace right after an autoformat puts back what was typed.
    if (e.key === 'Backspace' && was && r.collapsed && was.kind !== 'list' && undoAuto(ed, was)) { e.preventDefault(); refresh(ed); return; }
    const pre = around(ed, 'pre');
    if (pre) return preKey(ed, pre, r, e);
    if (r.collapsed && !around(ed, 'code') && EMOTICON_ENDS.has(e.key) && emoticon(r, e.key)) r = caretRange();
    if (e.key === '`' && r.collapsed && !around(ed, 'code') && inlineCode(ed, r)) { e.preventDefault(); return; }
    if (e.key === 'Enter' && !e.shiftKey && r.collapsed && fence(ed, r)) { e.preventDefault(); return; }
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
      if (was && was.kind === 'list' && was.li === li && li.isConnected) {
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
  // list (or the other way round) reads right. The list itself (bullet
  // side, indent) follows its first item: dir="auto" on it would skip
  // items that have a dir of their own and always come out LTR.
  document.addEventListener('input', (e) => {
    const ed = e.target.closest && e.target.closest('[data-editor]');
    if (!ed) return;
    for (const li of ed.querySelectorAll('li:not([dir])')) li.setAttribute('dir', 'auto');
    for (const l of ed.querySelectorAll('ul,ol')) {
      const first = l.querySelector('li'), d = first && itemDir(first);
      if (d && l.getAttribute('dir') !== d) l.setAttribute('dir', d);
    }
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
      '<ol dir="ltr"><li dir="auto">Move the <i>pricing</i> slide up</li><li dir="auto">Add the <a href="https://example.com">Q3 numbers</a></li></ol>' +
      '<ul dir="rtl"><li dir="auto">שלום, נדבר מחר</li></ul>' +
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

    /// The script's emoticon rule, in Rust: the emoticon ending the text
    /// before the caret, standing alone.
    fn emoticon(typed: &str) -> Option<&'static str> {
        EMOTICONS.iter().find_map(|(t, e)| {
            let rest = typed.strip_suffix(t)?;
            (rest.is_empty() || rest.ends_with(char::is_whitespace)).then_some(*e)
        })
    }

    #[test]
    fn emoticons_become_emoji() {
        assert_eq!(emoticon(":)"), Some("🙂"));
        assert_eq!(emoticon("great :-)"), Some("🙂"));
        assert_eq!(emoticon("ok\u{a0}:D"), Some("😃"));
        assert_eq!(emoticon("שלום ;)"), Some("😉"));
        assert_eq!(emoticon("oh :'("), Some("😢"));
        assert_eq!(emoticon("love <3"), Some("❤️"));
        assert_eq!(emoticon(":p"), Some("😛"));
        assert_eq!(emoticon("B-)"), Some("😎"));
        for t in
            ["http://", "see http://x.io/:)", "10:30", "a:b", "x:)", "(:)", "plan B)", "B)", ":", "<", "a<3", "::)"]
        {
            assert_eq!(emoticon(t), None, "{t:?}");
        }
        assert!(EMOTICON_ENDS.contains(&" ") && EMOTICON_ENDS.contains(&"Enter") && !EMOTICON_ENDS.contains(&":"));
    }

    fn inline_code(typed: &str) -> Option<String> {
        let re = regex::Regex::new(INLINE_CODE_RE).expect("pattern compiles");
        re.captures(typed).map(|m| m[2].to_string())
    }

    #[test]
    fn backticks_make_inline_code() {
        assert_eq!(inline_code("`x").as_deref(), Some("x"));
        assert_eq!(inline_code("run `cargo test").as_deref(), Some("cargo test"));
        assert_eq!(inline_code("(`a.b").as_deref(), Some("a.b"));
        assert_eq!(inline_code("הרץ `ls -la").as_deref(), Some("ls -la"));
        for t in ["`", "``", "` x", "`x ", "a`b", "it`s", "no code", "`a`b"] {
            assert_eq!(inline_code(t), None, "{t:?}");
        }
    }

    #[test]
    fn fences_start_code_blocks() {
        let re = regex::Regex::new(CODE_FENCE_RE).expect("pattern compiles");
        for t in ["```", "```rust", " ```js ", "```c++"] {
            assert!(re.is_match(t), "{t:?}");
        }
        for t in ["``", "x```", "```a b", "````!", "`` `"] {
            assert!(!re.is_match(t), "{t:?}");
        }
        assert!(CODE_STYLE.contains("monospace") && PRE_STYLE.contains("white-space:pre-wrap"));
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
        assert!(!js.contains("__KEYS__") && !js.contains("__AUTO_LIST_RE__") && !js.contains("__EMOJI__"));
    }
}
