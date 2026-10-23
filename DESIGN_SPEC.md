# Pigeon — Design Spec

> A beautiful, blazing-fast mail + calendar desktop app — an Outlook alternative.
> This is the design brief for **Claude Design**. It defines product intent,
> principles, information architecture, screen-by-screen layout, component
> inventory, and the visual system to produce mockups against.

---

## 1. Product in one line

Pigeon is a native desktop app for **email and calendar** across Microsoft 365,
Google Workspace/Gmail and iCloud accounts — with the calm, premium feel of
Linear and the speed of Superhuman, without the bloat of Outlook.

Lineage: Superhuman, Notion Mail, Mimestream, Notion Calendar (Cron), Spark,
Fantastical. Pigeon aims to be the fastest and most beautiful of the family, and
the one that treats M365 as a first-class citizen.

---

## 2. The two north stars

### Beautiful
- **Typography first.** Built from type, space, and restraint, not borders and
  boxes. An inbox is a wall of text; hierarchy (sender / subject / snippet /
  time) must be readable at a glance. Reference feel: **Linear, Superhuman,
  Notion Calendar, Raycast.**
- **Meticulous dual theme.** Dark and light both first-class, both tuned.
  Email bodies are authored for white backgrounds; define how they render in
  dark mode (smart-invert vs. "paper" card on dark chrome).
- **Calm.** New mail arrives without jumps; lists never reflow under the cursor.

### Fast (and *felt* as fast)
- Everything is served from a local cache. No spinners on navigation, ever.
  Design for **instant** states; network activity is a quiet, peripheral
  indicator.
- Keyboard-first: every action has a shortcut, shortcuts are discoverable
  (hints in menus/tooltips, `?` cheat sheet, ⌘K palette).
- Optimistic actions: archive/delete/move happen instantly with an **undo**
  toast.

---

## 3. Tech stack (context for the designer)

- Rust + **Dioxus 0.7 desktop** — the UI is HTML/CSS rendered in the system
  webview. Deliver **plain HTML + CSS** (CSS custom properties for tokens,
  `data-theme="dark|light"` on root). No React/Tailwind runtime; small vanilla JS
  only where unavoidable.
- macOS-first: native traffic-light window controls, ~28 px inset title-bar
  area, vibrancy-friendly sidebar.
- Lists can hold 100k+ items: assume virtualized rows of **fixed height**.

---

## 4. Target users & jobs

Knowledge workers juggling **5+ accounts** (several work M365 / Workspace
tenants, support/shared addresses, personal Gmail) who live in email and
meetings. Design every account-scoped surface (sidebar, From picker, calendar
list, unified views, settings) for 5–10 accounts without clutter: collapsible
account sections, account color + short label, and quick account filtering.

Jobs:
1. Triage the inbox to zero quickly (read, archive, reply, snooze, flag).
2. Find any email in under 2 seconds (search).
3. See today's and this week's meetings; join a call in one click.
4. Schedule a meeting with people across orgs, with a Zoom/Meet/Teams link.
5. Respond to invites (accept/decline/tentative) without leaving the inbox.

---

## 5. v1 scope

In: 5+ accounts (M365 + Google + iCloud), unified inbox, folders (M365) and labels
(Gmail) in one tree, conversation view, message reading, compose / reply /
forward, attachments, search, flag/star, archive, delete, move, calendar
day/week/month, event detail, create/edit event, RSVP, multiple calendars,
attendee availability, notifications, settings, onboarding/login.

Phase 2 (design the hook now, low-fi is fine): meeting-link picker in the event
composer (**Zoom / Google Meet / Microsoft Teams**), snooze, send later, undo
send, templates.

Out: rules editor, tasks/to-do, notes, contacts manager, chat.

---

## 6. Information architecture

```
┌ Sidebar ──────────┬ List ───────────────┬ Reading pane ──────────────┐
│ ⌘K search         │ thread rows         │ thread / message           │
│ Mail / Calendar   │                     │                            │
│ Unified: Inbox,   │                     │                            │
│   Flagged, Sent,  │                     │                            │
│   Drafts          │                     │                            │
│ ── Account A ──   │                     │                            │
│   folders/labels  │                     │                            │
│ ── Account B ──   │                     │                            │
│ Upcoming agenda   │                     │                            │
│ (next 2 meetings) │                     │                            │
└───────────────────┴─────────────────────┴────────────────────────────┘
```

- Two top-level modes: **Mail** and **Calendar** (⌘1 / ⌘2), sharing the sidebar.
- Each account gets an auto-assigned accent color (hash of address into an
  ~8-hue palette); show it as a small dot on rows in unified views so you
  always know which account a message/event belongs to.
- A compact **"next up" agenda** lives at the bottom of the sidebar in Mail mode
  (next 1–2 meetings, with a Join button when < 10 min away).

---

## 7. Screen-by-screen

### 7.1 Onboarding / add account
- Welcome screen with three choices: "Microsoft 365 / Outlook", "Google", and
  "iCloud".
- **iCloud has no browser sign-in:** design a short guided flow — explain
  app-specific passwords, a button that opens appleid.apple.com, then fields
  for Apple ID email + app-specific password, with a verify step.
- Microsoft and Google sign-in happens in the system browser; design the "waiting for browser"
  state, success, and error (e.g. admin consent required).
- First-sync state: inbox appears progressively; show it's usable immediately.

### 7.2 Inbox / message list (the workhorse)
- Row: account dot, sender (bold if unread), subject, snippet (muted), time,
  icons for attachment / flag / calendar invite, thread count.
- Hover actions on right: archive, delete, flag, snooze, mark read.
- Selection (multi-select with shift/⌘), keyboard focus ring distinct from
  selection.
- Grouping headers: Today / Yesterday / This week / Older.
- States: unread, read, selected, focused, flagged, draft, sending, failed.
- Inline **invite row**: meeting invites show date/time and Accept / Tentative /
  Decline directly in the list.

### 7.3 Reading pane
- Thread view: collapsed earlier messages (sender + snippet), latest expanded.
- Header: subject large, participants, account, labels/folder chips, actions
  (reply, reply all, forward, archive, delete, more).
- HTML email body area (sandboxed). Design the **"remote images blocked"**
  banner with "Load images" / "Always for this sender".
- Attachments strip with type icons, size, preview/download.
- Invite card for meeting emails: time, location/link, attendees, RSVP buttons,
  mini day view showing conflicts.
- Inline reply composer at the bottom of the thread.

### 7.4 Composer
- Inline (in thread) and pop-out window variants.
- **From is switchable at any time**, including mid-reply: a customer wrote to
  address 1, the user answers from address 2 (a completely different account).
  The picker lists every account plus its aliases / send-as addresses, grouped
  by account with the account color. Switching keeps body, recipients,
  attachments and the quoted thread; signature swaps to the new account's.
  Show a subtle hint when replying from a different account than the one that
  received the message ("Replying from work@… — received at support@…").
- To/Cc/Bcc with contact chips + autocomplete,
  subject, rich-text body with a minimal floating toolbar, attachments
  (drag & drop zone), signature, send button with "send later" dropdown.
- States: draft saved, sending, sent (undo toast), failed.

### 7.4.1 Right-to-left (Hebrew/Arabic) — first-class
- **Automatic per-paragraph direction:** each paragraph takes its direction from
  its first strong character as the user types (Hebrew → RTL, Latin → LTR),
  including alignment, list bullets, quote bars, and caret behavior.
- **Manual override shortcuts** for the current paragraph (Outlook convention):
  **Ctrl + Right Shift → RTL**, **Ctrl + Left Shift → LTR**, plus a direction
  toggle in the rich-text toolbar and the ⌘K palette. Show the shortcut in the
  tooltip.
- Mixed-direction content everywhere: subjects, snippets, sender names, chips,
  event titles render with per-item auto direction (no garbled punctuation in
  "Re: שלום (2)"). Design list rows and the reading pane with RTL samples.
- RTL email bodies render in their own direction inside the reading pane.
- UI chrome stays LTR in v1 (no Hebrew UI localization yet); content is bidi.

### 7.5 Search
- ⌘K / `/` opens search. Results instant from local index; show filter chips
  (from:, to:, has:attachment, account, date range). "Search server for older
  results" affordance at the bottom.

### 7.6 Calendar — week view (default)
- Time grid, all-day row, current-time line, multiple calendars color-coded,
  overlapping events laid out side by side.
- Mini month + calendar list (toggle visibility, per-account) in sidebar.
- Event block: title, time, location/video icon, RSVP state (tentative =
  hatched/outlined, declined = struck/faded).
- Drag to create, drag to move/resize.
- Day and month views as variants.

### 7.7 Event detail & event composer
- Popover on click; full panel for editing.
- Fields: title, calendar/account, date/time + timezone, all-day, repeat,
  attendees (chips with RSVP status), **attendee availability strip**
  (free/busy timeline across invitees), location, **conferencing picker
  (None / Zoom / Google Meet / Microsoft Teams)**, description, reminders.
- "Join" button prominent when a meeting link exists.

### 7.8 Command palette (⌘K)
- Actions + navigation + search in one. Shows shortcuts on the right.

### 7.9 Notifications & toasts
- Undo toast (archive/delete/send), error toast with retry, sync status.
- Event reminder with Join button.

### 7.10 Settings
- Accounts (add/remove, re-auth state), appearance (theme), signatures,
  notifications, keyboard shortcuts, privacy (remote images).

---

## 8. Component inventory

- App shell: sidebar, list pane, reading pane, resizable splitters
- Sidebar nav items, account sections, folder/label tree, unread counts
- Virtualized thread row (all states listed in 7.2)
- Thread / message card (collapsed + expanded)
- Email body container + remote-images banner
- Attachment chip / tile
- Invite card + inline RSVP buttons
- Composer (inline + window), recipient chip input, rich-text toolbar
- Calendar time grid, event block, all-day chip, mini month, calendar list
- Event popover + event form, availability strip, conferencing picker
- Command palette, search with filter chips
- Buttons (primary, secondary, ghost, destructive), icon buttons, segmented control
- Avatars (photo + initials fallback, account-color ring)
- Toasts, dialogs, dropdowns, context menu, tooltips with shortcut hints
- Sync/connection indicator (synced, syncing, offline, auth needed)
- Empty states (inbox zero — make it delightful), loading-cold, error

---

## 9. Visual system — what to define

Token set, dark and light:

- **Color:** 3–4 background elevation layers, 3 text emphasis levels,
  whisper-thin dividers, brand accent, semantic (success, warning, error, info),
  unread indicator, **~8 account/calendar accent hues** that work in both
  themes, event fill/border/text derivation rules from a calendar hue. WCAG AA.
- **Typography:** UI sans (Inter/Geist class) + mono for headers/raw source.
  Tight scale; define list-row, reading, and calendar-grid sizes.
- **Spacing:** base unit + scale. Row heights for list (comfortable).
- **Radius, borders, shadows:** minimal, soft. Depth via subtle elevation.
- **Motion:** durations/easing for row removal (archive), toast in/out, palette,
  pane transitions. Transform/opacity only. Calm, never bouncy.
- **Iconography:** single consistent stroke set; deliver as an SVG sprite.

---

## 10. Design principles (decision rules)

1. **Type and space over lines and boxes.**
2. **Instant, then quiet.** Never block on network; show sync peripherally.
3. **Always know which account.** Account color everywhere in unified views.
4. **Every action reversible.** Undo over confirmation dialogs.
5. **Keyboard is first-class.** Shortcut hints everywhere.
6. **Both themes, equally loved** — including email bodies in dark mode.
7. **Calendar and mail are one app.** Invites, agenda, and join links cross over.
8. **Progressive disclosure.** Simple front; depth one click deeper.

---

## 11. Resolved design decisions

- **Layout:** three-pane (sidebar / list / reading pane), resizable; reading
  pane can toggle off for a full-width list.
- **Default landing:** unified inbox.
- **Threaded and unthreaded views both supported** via a user toggle (setting +
  quick switch in the list header). Design both: threaded rows show
  participants + message count and the reading pane shows the stacked thread;
  unthreaded rows are one message each and the reading pane shows a single
  message (with a "view whole conversation" link).
- **Dark mode is a hard requirement**, equal to light. Theme setting is
  System / Dark / Light (default: System, following macOS appearance live).
  Every mockup ships in both themes. Email bodies in dark mode: design both
  options, (a) smart dark-adapt of simple emails and (b) a "paper" light card
  for complex HTML newsletters, plus a per-message "view in light" toggle.
- **Density:** comfortable only in v1; tokens should allow a compact mode later.
- **Brand:** pigeon / carrier-pigeon motif — simple, geometric, reads at 16 px
  (menu bar, dock badge) and scales up. "Pigeon" wordmark.

---

## 12. Deliverables requested from Claude Design

1. Token system (color / type / space / radius / shadow / motion), dark + light.
2. High-fidelity mockups, both themes: Onboarding, Unified inbox + reading pane
   (threaded **and** unthreaded variants), Thread with invite card, Composer (inline + window), Search, Calendar week
   view, Event composer (with conferencing picker + availability), Command
   palette, Settings > Accounts.
3. Key list states: unread/read/selected/focused, sending/failed, inbox zero,
   cold first sync, offline, auth-expired.
4. Component sheet covering §8.
5. Logo / wordmark direction + app icon.
6. Output as an HTML/CSS bundle (tokens file + per-screen CSS + SVG icon
   sprite) that can be ported verbatim into the Dioxus app.
