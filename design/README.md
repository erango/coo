# Pigeon design bundle

Plain HTML + CSS + SVG for the Dioxus 0.7 desktop app. No framework runtime,
no build step. The visual source of truth is the Claude Design canvas
("Pigeon — Mail + Calendar"); this folder is the portable version of it.

```
design/
  tokens.css          colors (dark + light), type, space, radius, motion, account hues
  base.css            reset, typography, shared components
  screens/
    shell.css         app shell: sidebar, panes, nav, next-up agenda, splitter
    mail-list.css     message rows + all states, hover actions, inline invite, skeletons
    reading.css       reading pane, thread, email body frame, attachments, invite card
    composer.css      pop-out composer, recipients + autocomplete, format bar, inline reply
    calendar.css      mini month, calendar list, week/month grid, events, popover
    event-editor.css  event form, guests, availability strip, conferencing picker
    palette.css       ⌘K command palette
    search.css        search bar, filter chips, server-results affordance
    onboarding.css    add account: welcome, waiting, consent error, first sync
    settings.css      Settings › Accounts
    states.css        inbox zero, auth-expired overlay, list footer
  mail-adapted.css    injected into the sandboxed srcdoc of simple emails (dark-adapt)
  icons.svg           SVG sprite, 64 symbols incl. the pigeon silhouette
  logo/               mark.svg (theme-aware), mark-light.svg, mark-template.svg, app-icon.svg
  reference/          inbox.html, calendar.html: real markup using the classes
```

## Using it from Dioxus

Copy (or symlink) the folder into the app crate's `assets/` when
`crates/pigeon-bin` exists. It lives at the repo root for now because the
workspace globs `crates/*`, so any folder there must be a crate.

```rust
use dioxus::prelude::*;

const TOKENS: Asset = asset!("/assets/design/tokens.css");
const BASE: Asset = asset!("/assets/design/base.css");
const SHELL: Asset = asset!("/assets/design/screens/shell.css");
const MAIL_LIST: Asset = asset!("/assets/design/screens/mail-list.css");
// …one per screen file you use
const ICONS: &str = include_str!("../assets/design/icons.svg");

#[component]
fn App() -> Element {
    let theme = use_signal(|| "system"); // from Settings: "system" | "dark" | "light"
    rsx! {
        document::Stylesheet { href: TOKENS }
        document::Stylesheet { href: BASE }
        document::Stylesheet { href: SHELL }
        document::Stylesheet { href: MAIL_LIST }
        // Inline the sprite once so <use href="#i-…"> resolves everywhere.
        div { dangerous_inner_html: ICONS }
        // …
    }
}
```

Set the theme on `<html>` (e.g. `document::eval` with
`document.documentElement.dataset.theme = 'dark'`). Leave it unset, or set it
to `system`, to follow macOS appearance live.

Icons: `svg { class: "icon", r#use { href: "#i-archive" } }`. Size with
`icon--xs|sm|lg|xl`; color follows `color`.

## Conventions

- **State is attributes, not classes.** Rows use `data-unread`,
  `aria-selected="true"`, `data-focused`, `data-state="draft|sending|failed|queued"`,
  `data-stale`, `data-leaving` (archive animation). Events use
  `data-rsvp="tentative|pending|declined"`, `aria-selected`, `data-past`. Toggle
  attributes from signals; class lists stay static.
- **Account color is `data-hue="1".."8"`.** Any element with it gets `--hue`,
  `--hue-rgb` and `--hue-text`; dots, pips, events, chips and avatars read
  those. The engine picks `hash(address) % 8 + 1`; Settings can override.
  Up to 4 accounts show `.dot`; 5+ switch rows to lettered `.pip`.
- **Rows are a fixed `--row-h` (76 px).** The virtualizer can assume it.
  `data-density="compact"` on `<html>` swaps the size tokens (56 px rows) when
  compact mode lands.
- **Calendar events** are positioned by the view model:
  `top = (start − firstHour) × --hour-h`, `height = duration × --hour-h − 2px`,
  overlap columns via `style="--col: 1; --cols: 2"`.
- **Bidi:** names, subjects, snippets and bodies carry `dir="auto"` (rows set
  `unicode-bidi: plaintext` already).
- **Motion:** transform and opacity only; durations are tokens and drop to 0
  under `prefers-reduced-motion`. Navigation and selection never animate.
- **Email bodies:** render sanitized HTML in `iframe.mail-frame[sandbox]`.
  Simple mail gets `data-render="adapted"` and `mail-adapted.css` injected into
  the srcdoc (substitute the four `--mail-*` values from the current theme);
  complex HTML gets `data-render="paper"` and keeps its own white background.
  A per-message toggle flips between them.

## Fonts

Reference pages load Inter and JetBrains Mono from Google Fonts. The app
should bundle the font files locally (offline-first) and declare them with
`@font-face` before `tokens.css`; the font stacks fall back to SF Pro / SF Mono.

## Previewing

```bash
python3 -m http.server -d design 8000
```

Then open `http://localhost:8000/reference/inbox.html` or
`reference/calendar.html`. The switch in the bottom-right corner toggles
System / Dark / Light. (A server is needed: browsers block `<use>` references
to `icons.svg` over `file://`.)

The other screens (composer, event editor, palette, search, onboarding,
settings, list states) are styled here but drawn only on the canvas; build
their markup from the class comments at the top of each screen file.
