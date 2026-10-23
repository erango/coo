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

  // ---- Keyboard shortcuts (outside editors) ----
  const KEYS = new Set(['j','k','e','#','s','u','r','a','f','c','z','/','x','Escape','Enter','ArrowUp','ArrowDown',
                        'meta+1','meta+2','meta+k','meta+n','meta+r','meta+Enter','meta+shift+z']);
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
    let key = e.key;
    if (e.shiftKey && key.length > 1 && e.metaKey) key = 'shift+' + key;
    if (e.shiftKey && e.metaKey && key.length === 1) key = 'shift+' + key.toLowerCase();
    if (e.metaKey) key = 'meta+' + key;
    if (editable(e.target)) {
      // An open autocomplete owns Escape (the input closes it itself).
      if (key === 'Escape' && e.target.dataset && e.target.dataset.suggestOpen === 'true') return;
      if (key === 'meta+Enter' || key === 'Escape') { e.preventDefault(); send('key', key); }
      return;
    }
    if (e.ctrlKey || e.altKey) return;
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
  }, true);

  // ---- Mail frames: fit height to content, open links in the browser ----
  function wire(f) {
    const fit = () => {
      try {
        const d = f.contentDocument; if (!d || !d.documentElement) return;
        f.style.height = (d.documentElement.scrollHeight + 2) + 'px';
      } catch (_) {}
    };
    if (!f.dataset.wired) {
      f.dataset.wired = '1';
      f.addEventListener('load', () => {
        fit();
        try {
          const d = f.contentDocument;
          d.addEventListener('click', (e) => {
            const a = e.target.closest && e.target.closest('a[href]');
            if (a) { e.preventDefault(); send('link', a.href); }
          });
          new ResizeObserver(fit).observe(d.documentElement);
          for (const img of d.images) img.addEventListener('load', fit);
        } catch (_) {}
      });
    }
    fit();
  }
  const scan = () => document.querySelectorAll('iframe.mail-frame').forEach(wire);
  new MutationObserver(scan).observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ['srcdoc'] });
  scan();
})();
"#;

/// Read an editor's HTML (by element id).
pub async fn editor_html(id: &str) -> String {
    let js = format!("return document.getElementById({id:?})?.innerHTML ?? ''");
    document::eval(&js).join::<String>().await.unwrap_or_default()
}

pub fn set_theme(theme: &str) {
    let js = match theme {
        "dark" | "light" => format!("document.documentElement.dataset.theme = {theme:?}"),
        _ => "delete document.documentElement.dataset.theme".to_string(),
    };
    document::eval(&js);
}

pub fn focus(id: &str) {
    document::eval(&format!("setTimeout(() => document.getElementById({id:?})?.focus(), 0)"));
}
