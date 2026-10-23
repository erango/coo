# Google OAuth verification: submission pack

Everything to paste into Google Cloud Console → **Google Auth Platform**
(project `pigeon-512506`, "Coo Mail") when submitting Coo for verification,
plus the demo video script. Coo requests one **restricted** scope
(`gmail.modify`) and one **sensitive** scope (`calendar`).

Status before submitting:

- [x] Branding: app name **Coo**, logo, support email
- [x] Home page `https://erango.github.io/coo/`
- [x] Privacy policy `https://erango.github.io/coo/privacy.html` (includes the
      Google API Services User Data Policy / Limited Use statement)
- [x] Terms `https://erango.github.io/coo/terms.html`
- [x] Authorized domain `erango.github.io`, verified in Search Console
- [x] Publishing status: In production
- [ ] Demo video uploaded to YouTube (unlisted), link pasted below
- [ ] Scope justifications pasted (Data Access page)
- [ ] Submitted for verification

## 1. Scopes (Data Access page)

Add exactly these; nothing else is requested by the app
(`crates/pigeon-core/src/auth.rs`, `GOOGLE_SCOPES`):

| Scope | Class |
|---|---|
| `openid`, `.../auth/userinfo.email`, `.../auth/userinfo.profile` | non-sensitive |
| `https://www.googleapis.com/auth/calendar` | sensitive |
| `https://www.googleapis.com/auth/gmail.modify` | restricted |

## 2. Justifications (paste per scope)

**gmail.modify**

> Coo is a desktop email client for macOS. The user adds their Gmail account
> to read, search and manage their own mail in the app. gmail.modify is the
> narrowest scope that covers what an email client does: list and read
> messages and threads (messages.list/get, history.list for incremental sync),
> show labels (labels.list), apply the user's actions (mark read/unread, star,
> archive, move to a label, move to Trash and restore, via
> messages.batchModify / trash / untrash), download attachments the user
> opens, keep drafts in sync (drafts create/update/send/delete), send mail and
> replies (messages.send), and list the user's verified send-as aliases
> (settings.sendAs.list) so they can pick a From address. Coo never
> permanently deletes mail, so the full mail.google.com scope is not
> requested; gmail.readonly is not enough because sending, archiving and
> labelling are core features.

**calendar**

> Coo shows the user's calendars next to their mail. It lists the user's
> calendars (calendarList), shows their events, lets them create, edit,
> move and delete their own events, answer meeting invitations (update their
> attendee response), and check attendees' availability when scheduling
> (freeBusy). It also shows the meeting's join link so the user can join with
> one click.

**Data handling (answer to "how is the data used / stored")**

> Coo is a native macOS application with no backend. All Google API calls go
> directly from the user's Mac to Google. Mail and calendar data is stored
> only on the user's own Mac, in the app's local database, to make the app
> fast and usable offline; OAuth tokens are stored in the macOS Keychain.
> No Google user data is transmitted to or stored on any server operated by
> the developer or any third party, it is not sold, not used for advertising,
> and not used to train AI models. Optional crash reports (Sentry) contain
> only technical diagnostics; email addresses, message content, subjects,
> names and URL details are removed on the device before a report is sent,
> and the user can turn them off in Settings › Privacy.
>
> The user can optionally connect an AI assistant that runs on their own
> computer (via Coo's local command-line tool / MCP server). That connection
> is set up explicitly by the user, runs locally, is read-only by default, and
> only acts on the user's own requests; Coo does not send Google data to any
> AI service itself.

**Why a security assessment (CASA) should not apply**

> Google's policy requires a security assessment when restricted-scope data
> is stored on or transmitted through servers. Coo has no servers: data flows
> only between Google and the user's device. We'd ask the reviewer to confirm
> the app qualifies as a client-side-only application. Source code is public
> at https://github.com/erango/coo for verification.

## 3. Demo video (YouTube, unlisted, ~3 minutes, English)

Google requires the video to show the **OAuth consent screen with the app
name and the client ID visible in the browser address bar**, and then each
requested scope being used in the app. Record the screen with QuickTime
(File › New Screen Recording), with a clean test Gmail account.

1. **Intro (10 s):** Coo's window. "This is Coo, a desktop mail and calendar
   app for macOS."
2. **Sign-in (40 s):** Settings › Accounts › Add Google (or onboarding).
   The browser opens Google's consent screen. **Zoom into the address bar**
   so `client_id=...apps.googleusercontent.com` is readable, then show the
   consent screen listing "Coo" and the Gmail and Calendar permissions.
   Click Allow. Back in Coo, the account appears.
3. **gmail.modify, reading (30 s):** the inbox fills; open a thread; search for
   a word; open an attachment.
4. **gmail.modify, managing (30 s):** mark a message unread and read; star it;
   archive it (it leaves the inbox); move one to a label (V); move one to
   Trash and undo.
5. **gmail.modify, sending (30 s):** reply to a message and send; show it in
   Sent. Start a new message, pick a send-as alias in From, close it and show
   it in Drafts.
6. **calendar (40 s):** switch to Calendar (⌘2); create an event; drag it to
   another time; open an invitation and click Accept; open the event editor
   and show the attendee availability strip (freeBusy).
7. **Data stays local (15 s):** open Settings › Privacy, show the crash-report
   switch, and say: "Coo has no servers; mail stays on this Mac."
8. **Sign out (10 s):** Settings › Accounts › Sign out, showing the account
   and its data are removed.

Upload as **Unlisted**, paste the link into the verification form.

## 4. After submitting

- Google emails within days; replies go to the developer contact address.
  They often ask for small changes (wording on the privacy page, a clearer
  video moment). Answer in the same thread.
- If they decide CASA is required anyway, the cheapest authorized option
  reported is TAC Security Tier 2 (roughly $540–$1,800/year).
- Until approval, users see the "unverified app" screen and the 100-user cap
  applies; nothing breaks for existing users.
