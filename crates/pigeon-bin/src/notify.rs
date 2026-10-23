//! Desktop notifications: new mail (from the engine's `Delta::NewMail`) and
//! event reminders, plus where a click should take the user.
//!
//! A bundled, signed Coo.app posts through UNUserNotificationCenter (click
//! opens the thread/event, "Join" opens the meeting link). An unbundled
//! binary (`cargo app`) has no bundle identity, which UNUserNotificationCenter
//! requires, so it falls back to `osascript display notification` (no click
//! routing).

use std::collections::HashSet;

use pigeon_core::model::{EventRow, MessageRow, Response};

use crate::prefs::Prefs;

/// Where a notification click goes. Encoded into the request identifier so
/// no untyped userInfo dictionary is needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Thread { account: String, thread: String },
    Inbox,
    Event { account: String, event: String, start_at: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub title: String,
    pub body: String,
    pub target: Target,
    /// Event reminders with a meeting link get a "Join" action.
    pub join_url: Option<String>,
}

/// What a click asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Click {
    Open(Target),
    Join(String),
}

const SEP: char = '\u{1f}';

/// `coo␟kind␟fields…␟nonce`: unique per post, decodable on click.
pub fn encode(target: &Target, join: Option<&str>, nonce: u64) -> String {
    let fields: Vec<String> = match target {
        Target::Thread { account, thread } => vec!["thread".into(), account.clone(), thread.clone()],
        Target::Inbox => vec!["inbox".into()],
        Target::Event { account, event, start_at } => vec![
            "event".into(),
            account.clone(),
            event.clone(),
            start_at.to_string(),
            join.unwrap_or_default().to_string(),
        ],
    };
    format!("coo{SEP}{}{SEP}{nonce}", fields.join(&SEP.to_string()))
}

/// Decode an identifier; `action` is the action id ("join") or default.
pub fn decode(id: &str, action: &str) -> Option<Click> {
    let parts: Vec<&str> = id.split(SEP).collect();
    if parts.first() != Some(&"coo") {
        return None;
    }
    let target = match *parts.get(1)? {
        "thread" => Target::Thread { account: parts.get(2)?.to_string(), thread: parts.get(3)?.to_string() },
        "inbox" => Target::Inbox,
        "event" => {
            let url = parts.get(5).filter(|u| !u.is_empty());
            if action == JOIN_ACTION
                && let Some(u) = url
            {
                return Some(Click::Join(u.to_string()));
            }
            Target::Event {
                account: parts.get(2)?.to_string(),
                event: parts.get(3)?.to_string(),
                start_at: parts.get(4)?.parse().ok()?,
            }
        }
        _ => return None,
    };
    Some(Click::Open(target))
}

pub const JOIN_ACTION: &str = "join";
const EVENT_CATEGORY: &str = "event";

/// Notifications for newly arrived mail, honoring prefs. One message gets
/// its own notification; a burst is coalesced ("5 new messages").
pub fn mail_notes(rows: &[MessageRow], prefs: &Prefs) -> Vec<Note> {
    if !prefs.notify_new_mail {
        return Vec::new();
    }
    let rows: Vec<&MessageRow> = rows.iter().filter(|r| !prefs.notify_accounts_muted.contains(&r.account_id)).collect();
    let who =
        |r: &MessageRow| r.header.from.as_ref().map(|a| a.display().to_string()).unwrap_or_else(|| "Someone".into());
    match rows.as_slice() {
        [] => Vec::new(),
        [r] => vec![Note {
            title: who(r),
            body: if r.header.subject.is_empty() { r.header.snippet.clone() } else { r.header.subject.clone() },
            target: Target::Thread { account: r.account_id.clone(), thread: r.header.thread_id.clone() },
            join_url: None,
        }],
        many => {
            let mut names: Vec<String> = Vec::new();
            for r in many {
                let n = who(r);
                if !names.contains(&n) {
                    names.push(n);
                }
            }
            let body = match names.len() {
                1 => format!("From {}", names[0]),
                2 => format!("From {} and {}", names[0], names[1]),
                n => format!("From {}, {} and {} more", names[0], names[1], n - 2),
            };
            vec![Note { title: format!("{} new messages", many.len()), body, target: Target::Inbox, join_url: None }]
        }
    }
}

/// Events starting within `minutes` that haven't been reminded yet.
/// Skips all-day, cancelled, declined and muted-account events.
pub fn due_reminders<'a>(
    events: &'a [EventRow],
    now: i64,
    prefs: &Prefs,
    already: &HashSet<String>,
) -> Vec<&'a EventRow> {
    if !prefs.notify_events {
        return Vec::new();
    }
    let lead = i64::from(prefs.event_reminder_minutes) * 60_000;
    events
        .iter()
        .filter(|e| !e.event.all_day && !e.event.cancelled && e.event.my_response != Response::Declined)
        .filter(|e| !prefs.notify_accounts_muted.contains(&e.account_id))
        .filter(|e| e.event.start_at > now && e.event.start_at - now <= lead)
        .filter(|e| !already.contains(&reminder_key(e)))
        .collect()
}

pub fn reminder_key(e: &EventRow) -> String {
    format!("{}|{}|{}", e.account_id, e.event.id, e.event.start_at)
}

pub fn event_note(e: &EventRow, now: i64) -> Note {
    let mins = ((e.event.start_at - now) / 60_000).max(0);
    let when = if mins == 0 { "Starting now".to_string() } else { format!("In {mins} min") };
    let place = e.event.location.as_deref().filter(|l| !l.is_empty()).map(|l| format!(" · {l}")).unwrap_or_default();
    Note {
        title: e.event.title.clone(),
        body: format!("{when}{place}"),
        target: Target::Event { account: e.account_id.clone(), event: e.event.id.clone(), start_at: e.event.start_at },
        // Links in the location count too; Zoom etc. open in their app.
        join_url: pigeon_core::meeting::meeting_link(&e.event)
            .map(|l| pigeon_core::meeting::native_url(&l).unwrap_or(l.url)),
    }
}

// ---------------------------------------------------------------------------
// Platform

static CLICKS: std::sync::OnceLock<tokio::sync::mpsc::UnboundedSender<Click>> = std::sync::OnceLock::new();
static CLICK_RX: std::sync::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<Click>>> = std::sync::Mutex::new(None);

/// Clicks on our notifications; taken once by the UI.
pub fn take_clicks() -> Option<tokio::sync::mpsc::UnboundedReceiver<Click>> {
    CLICK_RX.lock().ok()?.take()
}

fn on_click(id: &str, action: &str) {
    if let (Some(tx), Some(click)) = (CLICKS.get(), decode(id, action)) {
        let _ = tx.send(click);
    }
}

/// Set up the click channel and (bundled) the notification center delegate
/// and categories. Call once on the main thread at startup.
pub fn init() {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let _ = CLICKS.set(tx);
    if let Ok(mut slot) = CLICK_RX.lock() {
        *slot = Some(rx);
    }
    #[cfg(target_os = "macos")]
    mac::init();
}

pub fn post(note: &Note) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NONCE: AtomicU64 = AtomicU64::new(1);
    let id = encode(&note.target, note.join_url.as_deref(), NONCE.fetch_add(1, Ordering::Relaxed));
    #[cfg(target_os = "macos")]
    mac::post(note, &id);
    #[cfg(not(target_os = "macos"))]
    let _ = id;
}

#[cfg(target_os = "macos")]
mod mac {
    use std::sync::atomic::{AtomicBool, Ordering};

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{AllocAnyThread, define_class, msg_send};
    use objc2_foundation::{NSArray, NSBundle, NSError, NSSet, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationAction,
        UNNotificationActionOptions, UNNotificationCategory, UNNotificationCategoryOptions,
        UNNotificationInterruptionLevel, UNNotificationPresentationOptions, UNNotificationRequest,
        UNNotificationResponse, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
    };

    use super::{EVENT_CATEGORY, JOIN_ACTION, Note, on_click};

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "CooNotificationDelegate"]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl UNUserNotificationCenterDelegate for Delegate {
            // Show banners even while Coo is frontmost (we only post when
            // the window isn't focused, but reminders can still matter).
            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn will_present(
                &self,
                _center: &UNUserNotificationCenter,
                _notification: &UNNotification,
                handler: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
            ) {
                handler.call((UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::List
                    | UNNotificationPresentationOptions::Sound,));
            }

            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn did_receive(
                &self,
                _center: &UNUserNotificationCenter,
                response: &UNNotificationResponse,
                handler: &block2::DynBlock<dyn Fn()>,
            ) {
                let id = response.notification().request().identifier().to_string();
                let action = response.actionIdentifier().to_string();
                on_click(&id, &action);
                handler.call(());
            }
        }
    );

    impl Delegate {
        fn new() -> Retained<Self> {
            let this = Self::alloc().set_ivars(());
            // SAFETY: NSObject's init on a freshly allocated instance.
            unsafe { msg_send![super(this), init] }
        }
    }

    static BUNDLED: AtomicBool = AtomicBool::new(false);
    static ASKED: AtomicBool = AtomicBool::new(false);

    /// UNUserNotificationCenter throws for processes without a bundle id.
    fn bundled() -> bool {
        NSBundle::mainBundle().bundleIdentifier().is_some()
    }

    pub fn init() {
        if !bundled() {
            tracing::info!("not running from Coo.app: notifications use osascript (no click actions)");
            return;
        }
        BUNDLED.store(true, Ordering::Relaxed);
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let delegate = Delegate::new();
        center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        // The center keeps only a weak reference; keep ours for the app's life.
        std::mem::forget(delegate);
        let join = UNNotificationAction::actionWithIdentifier_title_options(
            &NSString::from_str(JOIN_ACTION),
            &NSString::from_str("Join"),
            UNNotificationActionOptions::Foreground,
        );
        let category = UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
            &NSString::from_str(EVENT_CATEGORY),
            &NSArray::from_retained_slice(&[join]),
            &NSArray::new(),
            UNNotificationCategoryOptions::empty(),
        );
        center.setNotificationCategories(&NSSet::from_retained_slice(&[category]));
    }

    fn ask_once(center: &UNUserNotificationCenter) {
        if ASKED.swap(true, Ordering::Relaxed) {
            return;
        }
        let done = RcBlock::new(|granted: Bool, _err: *mut NSError| {
            if !granted.as_bool() {
                tracing::info!("notifications not allowed by the user");
            }
        });
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound | UNAuthorizationOptions::Badge,
            &done,
        );
    }

    pub fn post(note: &Note, id: &str) {
        if !BUNDLED.load(Ordering::Relaxed) {
            return osascript(note);
        }
        let center = UNUserNotificationCenter::currentNotificationCenter();
        ask_once(&center);
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&note.title));
        content.setBody(&NSString::from_str(&note.body));
        // Banners are silent: Coo plays its own theme sound for mail and
        // reminders (sound.rs).
        // Meeting reminders are Time Sensitive, so Focus / Do Not Disturb
        // lets them through. Needs the time-sensitive entitlement (signed
        // builds with a provisioning profile); without it macOS treats the
        // request as a normal notification.
        if matches!(note.target, super::Target::Event { .. }) && crate::prefs::get().reminders_time_sensitive {
            content.setInterruptionLevel(UNNotificationInterruptionLevel::TimeSensitive);
        }
        if note.join_url.is_some() {
            content.setCategoryIdentifier(&NSString::from_str(EVENT_CATEGORY));
        }
        let request =
            UNNotificationRequest::requestWithIdentifier_content_trigger(&NSString::from_str(id), &content, None);
        center.addNotificationRequest_withCompletionHandler(&request, None);
    }

    /// Unbundled fallback. Arguments go to osascript as argv, never spliced
    /// into the script, so titles can't inject AppleScript.
    fn osascript(note: &Note) {
        let script = "on run argv\n display notification (item 2 of argv) with title (item 1 of argv)\nend run";
        let _ = std::process::Command::new("osascript")
            .args(["-e", script, &note.title, &note.body])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}

#[cfg(test)]
mod tests {
    use pigeon_core::model::{Address, Event, MessageHeader};

    use super::*;

    fn row(acct: &str, id: &str, from: &str, subject: &str) -> MessageRow {
        MessageRow {
            account_id: acct.into(),
            header: MessageHeader {
                id: id.into(),
                thread_id: format!("t-{id}"),
                subject: subject.into(),
                from: Some(Address::new(Some(from), &format!("{id}@x.com"))),
                ..Default::default()
            },
        }
    }

    #[test]
    fn single_message_opens_its_thread() {
        let notes = mail_notes(&[row("a", "m1", "Dana Cohen", "הזמנה 4471")], &Prefs::default());
        assert_eq!(notes.len(), 1);
        assert_eq!((notes[0].title.as_str(), notes[0].body.as_str()), ("Dana Cohen", "הזמנה 4471"));
        assert_eq!(notes[0].target, Target::Thread { account: "a".into(), thread: "t-m1".into() });
    }

    #[test]
    fn bursts_coalesce_and_respect_prefs() {
        let rows = vec![
            row("a", "1", "Ann", "x"),
            row("a", "2", "Ben", "y"),
            row("a", "3", "Ann", "z"),
            row("a", "4", "Cy", "w"),
            row("b", "5", "Dee", "v"),
        ];
        let notes = mail_notes(&rows, &Prefs::default());
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].title, "5 new messages");
        assert_eq!(notes[0].body, "From Ann, Ben and 2 more");
        assert_eq!(notes[0].target, Target::Inbox);

        let muted = Prefs { notify_accounts_muted: vec!["a".into()], ..Prefs::default() };
        let n = mail_notes(&rows, &muted);
        assert_eq!((n.len(), n[0].title.as_str()), (1, "Dee"));
        assert!(mail_notes(&rows, &Prefs { notify_new_mail: false, ..Prefs::default() }).is_empty());
    }

    fn ev(id: &str, start: i64, resp: Response, url: Option<&str>) -> EventRow {
        EventRow {
            account_id: "a".into(),
            event: Event {
                id: id.into(),
                calendar_id: "c".into(),
                title: format!("Event {id}"),
                start_at: start,
                end_at: start + 1_800_000,
                my_response: resp,
                online_url: url.map(String::from),
                ..Default::default()
            },
        }
    }

    #[test]
    fn reminders_fire_once_within_the_lead_time() {
        let now = 1_000_000_000;
        let events = vec![
            ev("soon", now + 9 * 60_000, Response::Accepted, Some("https://meet.google.com/x")),
            ev("later", now + 30 * 60_000, Response::Accepted, None),
            ev("declined", now + 5 * 60_000, Response::Declined, None),
            ev("past", now - 60_000, Response::Accepted, None),
        ];
        let prefs = Prefs::default(); // 10 minutes
        let mut seen = HashSet::new();
        let due = due_reminders(&events, now, &prefs, &seen);
        assert_eq!(due.iter().map(|e| e.event.id.as_str()).collect::<Vec<_>>(), ["soon"]);
        let note = event_note(due[0], now);
        assert_eq!(note.body, "In 9 min");
        assert_eq!(note.join_url.as_deref(), Some("https://meet.google.com/x"));
        seen.insert(reminder_key(due[0]));
        assert!(due_reminders(&events, now, &prefs, &seen).is_empty());
        assert!(
            due_reminders(&events, now, &Prefs { notify_events: false, ..Prefs::default() }, &HashSet::new())
                .is_empty()
        );

        // A meeting link only in the location still gets Join.
        let mut loc = ev("loc", now + 60_000, Response::Accepted, None);
        loc.event.location = Some("Teams: https://teams.microsoft.com/l/meetup-join/abc".into());
        assert!(event_note(&loc, now).join_url.is_some_and(|u| u.contains("teams")));
    }

    #[test]
    fn identifiers_roundtrip_and_join_action() {
        let t = Target::Thread { account: "google:a@b.c".into(), thread: "t|1".into() };
        assert_eq!(
            decode(&encode(&t, None, 7), "com.apple.UNNotificationDefaultActionIdentifier"),
            Some(Click::Open(t))
        );
        let e = Target::Event { account: "a".into(), event: "e1".into(), start_at: 42 };
        let id = encode(&e, Some("https://zoom.us/j/1"), 1);
        assert_eq!(decode(&id, JOIN_ACTION), Some(Click::Join("https://zoom.us/j/1".into())));
        assert_eq!(decode(&id, "default"), Some(Click::Open(e)));
        assert_eq!(decode(&encode(&Target::Inbox, None, 2), ""), Some(Click::Open(Target::Inbox)));
        assert_eq!(decode("other-app-id", ""), None);
    }
}
