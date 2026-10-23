// Reference pages only: theme switcher (System / Dark / Light) and a
// density toggle, so each page can be checked in both themes. The app sets
// data-theme from Settings instead.
(function () {
  const root = document.documentElement;
  const saved = localStorage.getItem("pg-theme");
  if (saved && saved !== "system") root.dataset.theme = saved;

  const bar = document.createElement("div");
  bar.className = "ref-switch segmented segmented--sm";
  bar.setAttribute("role", "radiogroup");
  bar.setAttribute("aria-label", "Theme");
  for (const mode of ["system", "dark", "light"]) {
    const b = document.createElement("button");
    b.type = "button";
    b.setAttribute("role", "radio");
    b.textContent = mode[0].toUpperCase() + mode.slice(1);
    const sync = () => b.setAttribute("aria-checked", String((root.dataset.theme || "system") === mode));
    b.addEventListener("click", () => {
      if (mode === "system") delete root.dataset.theme; else root.dataset.theme = mode;
      localStorage.setItem("pg-theme", mode);
      bar.querySelectorAll("button").forEach((x) => x.dispatchEvent(new Event("pg-sync")));
    });
    b.addEventListener("pg-sync", sync);
    sync();
    bar.appendChild(b);
  }
  const style = document.createElement("style");
  style.textContent = ".ref-switch{position:fixed;right:12px;bottom:12px;z-index:100;box-shadow:var(--shadow-popover)}";
  document.head.appendChild(style);
  document.addEventListener("DOMContentLoaded", () => document.body.appendChild(bar));
})();
