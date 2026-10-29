use super::*;
use crate::now_ms;

#[test]
fn account_scopes_are_recorded_and_go_with_the_account() {
    let (s, _ms, g) = setup();
    assert_eq!(s.account_scopes(&g.id).unwrap(), None, "unknown until recorded");
    s.set_account_scopes(&g.id, "openid email").unwrap();
    s.set_account_scopes(&g.id, "openid https://www.googleapis.com/auth/drive.file").unwrap();
    assert_eq!(s.account_scopes(&g.id).unwrap().as_deref(), Some("openid https://www.googleapis.com/auth/drive.file"));
    s.delete_account(&g.id).unwrap();
    assert_eq!(s.account_scopes(&g.id).unwrap(), None);
}

fn account(provider: ProviderKind, email: &str) -> Account {
    Account { id: Account::make_id(provider, email), provider, email: email.into(), display_name: email.into() }
}

fn folder(id: &str, role: Option<FolderRole>) -> Folder {
    Folder { id: id.into(), name: id.to_uppercase(), parent_id: None, role, kind: FolderKind::Folder }
}

fn msg(id: &str, thread: &str, at: i64, folders: &[&str]) -> MessageHeader {
    MessageHeader {
        id: id.into(),
        thread_id: thread.into(),
        subject: format!("Subject {id}"),
        from: Some(Address::new(Some(&format!("Sender {id}")), &format!("{id}@example.com"))),
        to: vec![Address::new(Some("Me"), "me@example.com")],
        snippet: format!("snippet for {id}"),
        received_at: at,
        folder_ids: folders.iter().map(|f| f.to_string()).collect(),
        ..Default::default()
    }
}

fn setup() -> (Store, Account, Account) {
    let mut s = Store::open_in_memory().unwrap();
    let ms = account(ProviderKind::Microsoft, "Me@Work.com");
    let g = account(ProviderKind::Google, "me@gmail.com");
    s.upsert_account(&ms).unwrap();
    s.upsert_account(&g).unwrap();
    s.replace_folders(&ms.id, &[folder("in-ms", Some(FolderRole::Inbox)), folder("proj", None)]).unwrap();
    s.replace_folders(&g.id, &[folder("INBOX", Some(FolderRole::Inbox)), folder("STARRED", Some(FolderRole::Starred))])
        .unwrap();
    (s, ms, g)
}

#[test]
fn migrations_are_idempotent() {
    let dir = std::env::temp_dir().join(format!("pigeon-store-{}", crate::auth::random_token(6)));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("p.db");
    drop(Store::open(&path).unwrap());
    let s = Store::open(&path).unwrap();
    assert!(s.accounts().unwrap().is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn account_ids_are_lowercased() {
    let (s, ms, _) = setup();
    assert_eq!(ms.id, "microsoft:me@work.com");
    assert_eq!(s.accounts().unwrap().len(), 2);
}

#[test]
fn unified_inbox_merges_accounts_newest_first() {
    let (mut s, ms, g) = setup();
    s.apply_mail(&ms.id, "f", &[MailChange::Upsert(msg("a", "t1", 100, &["in-ms"]))], "c1").unwrap();
    s.apply_mail(&g.id, "mail", &[MailChange::Upsert(msg("b", "t2", 200, &["INBOX", "STARRED"]))], "h1").unwrap();
    s.apply_mail(&ms.id, "f", &[MailChange::Upsert(msg("c", "t3", 300, &["proj"]))], "c2").unwrap();

    let rows = s.list_messages(&ListQuery::unified(FolderRole::Inbox, 50)).unwrap();
    let ids: Vec<_> = rows.iter().map(|r| r.header.id.as_str()).collect();
    assert_eq!(ids, ["b", "a"]);
    assert_eq!(rows[0].account_id, g.id);
    assert_eq!(rows[0].header.from.as_ref().unwrap().display(), "Sender b");

    let q =
        ListQuery { account_id: Some(ms.id.clone()), folder: FolderSel::Id("proj".into()), limit: 50, before: None };
    assert_eq!(s.list_messages(&q).unwrap()[0].header.id, "c");

    let page = ListQuery { before: Some(200), ..ListQuery::unified(FolderRole::Inbox, 50) };
    assert_eq!(s.list_messages(&page).unwrap()[0].header.id, "a");

    assert_eq!(s.cursor(&ms.id, "f").unwrap().as_deref(), Some("c2"));
    assert_eq!(s.cursor(&g.id, "mail").unwrap().as_deref(), Some("h1"));
}

#[test]
fn folder_id_listing_requires_account() {
    let (s, ..) = setup();
    let q = ListQuery { account_id: None, folder: FolderSel::Id("proj".into()), limit: 5, before: None };
    assert!(s.list_messages(&q).is_err());
}

#[test]
fn upsert_replaces_membership_and_unlink_deletes_orphans() {
    let (mut s, ms, _) = setup();
    s.apply_mail(&ms.id, "f", &[MailChange::Upsert(msg("a", "t1", 100, &["in-ms"]))], "1").unwrap();
    // Moved: shows up in the new folder first, then the old folder reports removal.
    s.apply_mail(&ms.id, "f", &[MailChange::Upsert(msg("a", "t1", 100, &["proj"]))], "2").unwrap();
    s.apply_mail(&ms.id, "f", &[MailChange::Unlink { id: "a".into(), folder_id: "in-ms".into() }], "3").unwrap();
    assert!(s.list_messages(&ListQuery::unified(FolderRole::Inbox, 5)).unwrap().is_empty());
    let infos = s.folders(&ms.id).unwrap();
    let proj = infos.iter().find(|f| f.folder.id == "proj").unwrap();
    assert_eq!((proj.total, proj.unread), (1, 1));

    s.apply_mail(&ms.id, "f", &[MailChange::Unlink { id: "a".into(), folder_id: "proj".into() }], "4").unwrap();
    assert!(s.search("Subject", 10).unwrap().is_empty());
}

#[test]
fn delete_removes_message_everywhere() {
    let (mut s, _, g) = setup();
    s.apply_mail(&g.id, "mail", &[MailChange::Upsert(msg("b", "t", 1, &["INBOX", "STARRED"]))], "1").unwrap();
    s.apply_mail(&g.id, "mail", &[MailChange::Delete { id: "b".into() }], "2").unwrap();
    assert!(s.folders(&g.id).unwrap().iter().all(|f| f.total == 0));
}

#[test]
fn threads_group_and_count() {
    let (mut s, ms, _) = setup();
    let mut read = msg("a1", "t", 100, &["in-ms"]);
    read.is_read = true;
    s.apply_mail(
        &ms.id,
        "f",
        &[
            MailChange::Upsert(read),
            MailChange::Upsert(msg("a2", "t", 300, &["in-ms"])),
            // Same thread, other folder: counts toward the thread size only.
            MailChange::Upsert(msg("a3", "t", 200, &["proj"])),
            MailChange::Upsert(msg("b1", "u", 250, &["in-ms"])),
        ],
        "1",
    )
    .unwrap();
    let threads = s.list_threads(&ListQuery::unified(FolderRole::Inbox, 10)).unwrap();
    assert_eq!(threads.len(), 2);
    let t = &threads[0];
    assert_eq!((t.thread_id.as_str(), t.latest.id.as_str()), ("t", "a2"));
    assert_eq!((t.message_count, t.unread_count), (3, 1));
    assert_eq!(t.participants.len(), 2);
    assert_eq!(threads[1].thread_id, "u");

    let all = s.thread_messages(&ms.id, "t").unwrap();
    assert_eq!(all.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["a1", "a3", "a2"]);
}

#[test]
fn search_matches_prefixes_across_fields() {
    let (mut s, ms, _) = setup();
    let mut m = msg("a", "t", 1, &["in-ms"]);
    m.subject = "Quarterly invoice from Acme".into();
    m.cc = vec![Address::new(Some("Zoë Finance"), "zoe@acme.io")];
    s.apply_mail(&ms.id, "f", &[MailChange::Upsert(m.clone())], "1").unwrap();
    assert_eq!(s.search("invo acme", 10).unwrap().len(), 1);
    assert_eq!(s.search("zoe", 10).unwrap().len(), 1, "diacritics folded");
    assert_eq!(s.search("nomatch", 10).unwrap().len(), 0);
    assert_eq!(s.search(r#"bad " query*"#, 10).unwrap().len(), 0);

    // Updates re-index.
    m.subject = "Renamed".into();
    s.apply_mail(&ms.id, "f", &[MailChange::Upsert(m)], "2").unwrap();
    assert_eq!(s.search("invoice", 10).unwrap().len(), 0);
    assert_eq!(s.search("renamed", 10).unwrap().len(), 1);
}

#[test]
fn deleting_account_cascades() {
    let (mut s, ms, _) = setup();
    s.apply_mail(&ms.id, "f", &[MailChange::Upsert(msg("a", "t", 1, &["in-ms"]))], "1").unwrap();
    s.delete_account(&ms.id).unwrap();
    assert!(s.list_messages(&ListQuery::unified(FolderRole::Inbox, 5)).unwrap().is_empty());
    assert_eq!(s.cursor(&ms.id, "f").unwrap(), None);
    assert!(s.search("subject", 5).unwrap().is_empty());
}

fn event(id: &str, start: i64, end: i64) -> Event {
    Event {
        id: id.into(),
        calendar_id: "cal".into(),
        title: id.into(),
        start_at: start,
        end_at: end,
        attendees: vec![Attendee {
            address: Address::new(None, "x@y.z"),
            response: Response::Tentative,
            optional: true,
        }],
        ..Default::default()
    }
}

#[test]
fn events_replace_window_and_query_overlap() {
    let (mut s, ms, _) = setup();
    let cal = Calendar { id: "cal".into(), name: "Work".into(), color: None, is_primary: true, can_edit: true };
    s.replace_calendars(&ms.id, std::slice::from_ref(&cal)).unwrap();
    s.replace_events(&ms.id, "cal", 0, 1000, &[event("a", 10, 20), event("b", 500, 600), event("old", 900, 950)])
        .unwrap();
    // Refresh drops "old" (gone upstream) and keeps the rest.
    s.replace_events(&ms.id, "cal", 0, 1000, &[event("a", 10, 20), event("b", 500, 600)]).unwrap();
    let got = s.events_between(15, 550).unwrap();
    assert_eq!(got.iter().map(|e| e.event.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    assert_eq!(got[0].event.attendees[0].response, Response::Tentative);
    assert!(s.events_between(900, 1000).unwrap().is_empty());

    // Removing the calendar removes its events.
    s.replace_calendars(&ms.id, &[]).unwrap();
    assert!(s.events_between(0, 1000).unwrap().is_empty());
    assert!(s.calendars(&ms.id).unwrap().is_empty());
}

#[test]
fn bodies_roundtrip_and_cascade() {
    let (mut s, ms, _) = setup();
    s.apply_mail(&ms.id, "f", &[MailChange::Upsert(msg("a", "t", 1, &["in-ms"]))], "1").unwrap();
    assert_eq!(s.body(&ms.id, "a").unwrap(), None);
    let body = StoredBody {
        html: Some("<p>hi</p>".into()),
        text: None,
        has_remote_content: true,
        origin: Some(crate::identity::Origin {
            delivered_to: Some("ms@example.com".into()),
            auth: Some(crate::identity::Auth { spf: Some("pass".into()), ..Default::default() }),
            ..Default::default()
        }),
        in_reply_to: Some("<x@y>".into()),
        references: vec!["<w@y>".into(), "<x@y>".into()],
        attachments: vec![
            StoredAttachment {
                meta: AttachmentMeta { id: "1".into(), name: "a.pdf".into(), size: Some(10), ..Default::default() },
                local_path: None,
            },
            StoredAttachment {
                meta: AttachmentMeta {
                    id: "2".into(),
                    name: "logo.png".into(),
                    content_id: Some("logo".into()),
                    is_inline: true,
                    ..Default::default()
                },
                local_path: Some("/tmp/logo.png".into()),
            },
        ],
        fetched_at: 5,
        invite: Some(Invite {
            uid: Some("u1".into()),
            title: "Sync".into(),
            start_at: 10,
            end_at: 20,
            ..Default::default()
        }),
    };
    assert!(s.save_body(&ms.id, "a", &body).unwrap());
    assert_eq!(s.body(&ms.id, "a").unwrap().unwrap(), body);
    s.set_attachment_path(&ms.id, "a", "1", "/tmp/a.pdf").unwrap();
    assert_eq!(s.attachments(&ms.id, "a").unwrap()[0].local_path.as_deref(), Some("/tmp/a.pdf"));
    assert!(!s.save_body(&ms.id, "missing", &body).unwrap());

    s.apply_mail(&ms.id, "f", &[MailChange::Delete { id: "a".into() }], "2").unwrap();
    assert!(s.attachments(&ms.id, "a").unwrap().is_empty());
}

fn gmail_with_labels() -> (Store, Account) {
    let mut s = Store::open_in_memory().unwrap();
    let g = account(ProviderKind::Google, "me@gmail.com");
    s.upsert_account(&g).unwrap();
    let label =
        |id: &str, role| Folder { id: id.into(), name: id.into(), parent_id: None, role, kind: FolderKind::Label };
    s.replace_folders(
        &g.id,
        &[
            label("INBOX", Some(FolderRole::Inbox)),
            label("STARRED", Some(FolderRole::Starred)),
            label("TRASH", Some(FolderRole::Trash)),
            label("L1", None),
        ],
    )
    .unwrap();
    s.apply_mail(&g.id, "mail", &[MailChange::Upsert(msg("a", "t", 1, &["INBOX"]))], "1").unwrap();
    (s, g)
}

fn state(s: &Store, account: &Account, id: &str) -> MessageState {
    s.message_states(&account.id, &[id.to_string()]).unwrap().remove(0)
}

#[test]
fn optimistic_ops_apply_and_survive_sync_until_confirmed() {
    let (mut s, g) = gmail_with_labels();
    let archive = MailOp::Relabel { ids: vec!["a".into()], add: vec![], remove: vec!["INBOX".into()] };
    let id = s.enqueue(&g.id, &archive).unwrap();
    assert!(s.list_messages(&ListQuery::unified(FolderRole::Inbox, 5)).unwrap().is_empty());

    // A sync page that still shows the old server state must not resurrect it.
    s.apply_mail(&g.id, "mail", &[MailChange::Upsert(msg("a", "t", 1, &["INBOX"]))], "2").unwrap();
    assert!(s.list_messages(&ListQuery::unified(FolderRole::Inbox, 5)).unwrap().is_empty());

    let due = s.due_ops(&g.id, now_ms()).unwrap();
    assert_eq!((due.len(), due[0].id, &due[0].op), (1, id, &archive));
    assert_eq!(due[0].snapshot[0].folder_ids, ["INBOX"]);

    s.mark_op_done(id).unwrap();
    assert!(s.due_ops(&g.id, now_ms()).unwrap().is_empty());
    assert!(!s.cancel_pending_op(id).unwrap(), "already sent");
}

#[test]
fn cancel_before_send_restores_state() {
    let (mut s, g) = gmail_with_labels();
    let id = s.enqueue(&g.id, &MailOp::SetFlagged { ids: vec!["a".into()], flagged: true }).unwrap();
    let st = state(&s, &g, "a");
    assert!(st.is_flagged);
    assert_eq!(st.folder_ids, ["INBOX", "STARRED"], "gmail star label follows the flag");

    assert!(s.cancel_pending_op(id).unwrap());
    let st = state(&s, &g, "a");
    assert!(!st.is_flagged);
    assert_eq!(st.folder_ids, ["INBOX"]);
    assert!(s.outbox_entry(id).unwrap().is_none());
}

#[test]
fn rollback_and_retry_bookkeeping() {
    let (mut s, g) = gmail_with_labels();
    let id = s.enqueue(&g.id, &MailOp::Trash { ids: vec!["a".into()] }).unwrap();
    assert_eq!(state(&s, &g, "a").folder_ids, ["TRASH"]);
    s.mark_op_retry(id, "503", now_ms() + 60_000).unwrap();
    assert!(s.due_ops(&g.id, now_ms()).unwrap().is_empty(), "backing off");
    assert!(!s.cancel_pending_op(id).unwrap(), "may have reached the server");
    s.rollback_op(id).unwrap();
    assert_eq!(state(&s, &g, "a").folder_ids, ["INBOX"]);
}

#[test]
fn report_moves_locally_and_round_trips_through_the_outbox() {
    let (mut s, ms, _) = setup();
    s.replace_folders(&ms.id, &[folder("in", Some(FolderRole::Inbox)), folder("junk", Some(FolderRole::Spam))])
        .unwrap();
    s.apply_mail(&ms.id, "in", &[MailChange::Upsert(msg("r", "tr", 1, &["in"]))], "1").unwrap();
    let op = MailOp::Report { ids: vec!["r".into()], report: SpamReport::Phishing, folder_id: "junk".into() };
    let id = s.enqueue(&ms.id, &op).unwrap();
    assert_eq!(state(&s, &ms, "r").folder_ids, ["junk"]);
    assert!(s.list_messages(&ListQuery::unified(FolderRole::Inbox, 5)).unwrap().iter().all(|m| m.header.id != "r"));
    assert_eq!(s.due_ops(&ms.id, now_ms()).unwrap().last().unwrap().op, op, "payload survives serde");
    assert!(s.cancel_pending_op(id).unwrap());
    assert_eq!(state(&s, &ms, "r").folder_ids, ["in"]);
}

#[test]
fn undo_ops_invert_changes() {
    let orig =
        vec![MessageState { id: "a".into(), is_read: false, is_flagged: false, folder_ids: vec!["INBOX".into()] }];
    let trashed =
        vec![MessageState { id: "a".into(), is_read: true, is_flagged: false, folder_ids: vec!["TRASH".into()] }];
    assert_eq!(
        undo_ops(true, Some("TRASH"), &orig, &trashed),
        vec![
            MailOp::SetRead { ids: vec!["a".into()], read: false },
            MailOp::Untrash { ids: vec!["a".into()] },
            MailOp::Relabel { ids: vec!["a".into()], add: vec!["INBOX".into()], remove: vec![] },
        ]
    );
    let moved =
        vec![MessageState { id: "a".into(), is_read: false, is_flagged: false, folder_ids: vec!["arch".into()] }];
    assert_eq!(
        undo_ops(false, Some("del"), &orig, &moved),
        vec![MailOp::MoveTo { ids: vec!["a".into()], folder_id: "INBOX".into() }]
    );
    assert!(undo_ops(false, None, &orig, &orig).is_empty());
}

#[test]
fn drafts_roundtrip_and_follow_account_deletion() {
    let (s, ms, g) = setup();
    let from = crate::compose::Sender { account_id: g.id.clone(), address: Address::new(None, "me@gmail.com") };
    let mut d = crate::compose::Draft::new(from, 5);
    d.subject = "Hi".into();
    s.save_draft(&d).unwrap();
    assert_eq!(s.draft(&d.id).unwrap().unwrap(), d);

    // Switching From to another account re-files the draft under it.
    d.switch_from(crate::compose::Sender { account_id: ms.id.clone(), address: Address::new(None, "me@work.com") });
    s.save_draft(&d).unwrap();
    s.delete_account(&g.id).unwrap();
    assert_eq!(s.drafts().unwrap(), vec![d.clone()]);
    s.delete_draft(&d.id).unwrap();
    assert!(s.drafts().unwrap().is_empty());
}

#[test]
fn single_message_includes_folders() {
    let (mut s, _, g) = setup();
    s.apply_mail(&g.id, "mail", &[MailChange::Upsert(msg("b", "t", 1, &["INBOX", "STARRED"]))], "1").unwrap();
    let h = s.message(&g.id, "b").unwrap().unwrap();
    let mut folders = h.folder_ids.clone();
    folders.sort();
    assert_eq!(folders, ["INBOX", "STARRED"]);
    assert!(s.message(&g.id, "nope").unwrap().is_none());
}

#[test]
fn incremental_event_changes_and_single_event_writes() {
    let (mut s, ms, _) = setup();
    let cal = Calendar { id: "cal".into(), name: "Work".into(), color: None, is_primary: true, can_edit: true };
    s.replace_calendars(&ms.id, std::slice::from_ref(&cal)).unwrap();
    let mut inst = event("s_1", 30, 40);
    inst.series_id = Some("s".into());
    s.apply_events(
        &ms.id,
        "cal",
        "cal:cal",
        &[CalendarChange::Upsert(event("a", 10, 20)), CalendarChange::Upsert(inst)],
        true,
        Some("tok"),
    )
    .unwrap();
    assert_eq!(s.cursor(&ms.id, "cal:cal").unwrap().as_deref(), Some("tok"));
    // Deleting a series removes its instances.
    s.apply_events(&ms.id, "cal", "cal:cal", &[CalendarChange::Delete { id: "s".into() }], false, None).unwrap();
    assert_eq!(s.events_between(0, 100).unwrap().len(), 1);
    assert_eq!(s.cursor(&ms.id, "cal:cal").unwrap().as_deref(), Some("tok"), "no cursor = keep the old one");

    s.set_my_response(&ms.id, "cal", "a", Response::Declined).unwrap();
    assert_eq!(s.event(&ms.id, "cal", "a").unwrap().unwrap().my_response, Response::Declined);
    let mut b = event("b", 50, 60);
    b.cancelled = true;
    s.upsert_event(&ms.id, &b).unwrap();
    assert!(s.event(&ms.id, "cal", "b").unwrap().unwrap().cancelled);
    assert_eq!(s.events_between(0, 100).unwrap().len(), 1, "cancelled events are hidden");
    s.delete_event(&ms.id, "cal", "a").unwrap();
    assert!(s.event(&ms.id, "cal", "a").unwrap().is_none());

    // A calendar that disappears takes its cursor with it.
    s.replace_calendars(&ms.id, &[]).unwrap();
    assert_eq!(s.cursor(&ms.id, "cal:cal").unwrap(), None);
}

#[test]
fn search_query_combines_operators() {
    use crate::search::parse;
    let (mut s, ms, g) = setup();
    let mut a = msg("a", "t1", 1_000, &["in-ms"]);
    a.subject = "Invoice for October".into();
    a.has_attachments = true;
    let mut b = msg("b", "t2", 2_000, &["INBOX"]);
    b.subject = "Invoice reminder".into();
    b.is_read = true;
    let mut c = msg("c", "t3", 3_000, &["proj"]);
    c.subject = "Lunch".into();
    c.is_flagged = true;
    s.apply_mail(&ms.id, "f", &[MailChange::Upsert(a), MailChange::Upsert(c)], "1").unwrap();
    s.apply_mail(&g.id, "mail", &[MailChange::Upsert(b)], "1").unwrap();

    let ids = |q: &str| s.search_query(&parse(q), 50).unwrap().into_iter().map(|r| r.header.id).collect::<Vec<_>>();
    assert_eq!(ids("invoice"), ["b", "a"]);
    assert_eq!(ids("invoice has:attachment"), ["a"]);
    assert_eq!(ids("invoice is:unread"), ["a"]);
    assert_eq!(ids("is:flagged"), ["c"]);
    assert_eq!(ids("from:sender a"), ["a"], "from matches name");
    assert_eq!(ids("from:b@example"), ["b"]);
    assert_eq!(ids("in:inbox invoice"), ["b", "a"]);
    assert_eq!(ids("in:proj"), ["c"], "folder by name, any case");
    assert!(ids("in:nowhere").is_empty());
    assert_eq!(ids("account:gmail invoice"), ["b"]);
    assert_eq!(ids("after:1970-01-01 before:1970-01-01"), Vec::<String>::new());
    assert!(ids("").is_empty());
}

#[test]
fn search_scope_narrows_and_counts_the_rest() {
    use crate::search::{Elsewhere, FolderCount, SearchScope, parse};
    let (mut s, ms, g) = setup();
    let inv = |id: &str, at: i64, folders: &[&str]| {
        let mut m = msg(id, id, at, folders);
        m.subject = format!("Invoice {id}");
        m
    };
    let up = |m| MailChange::Upsert(m);
    s.apply_mail(
        &ms.id,
        "f",
        &[up(inv("a", 1, &["in-ms"])), up(inv("b", 2, &["proj"])), up(inv("c", 3, &["proj"]))],
        "1",
    )
    .unwrap();
    s.apply_mail(&g.id, "mail", &[up(inv("d", 4, &["INBOX", "STARRED"])), up(inv("e", 5, &["STARRED"]))], "1").unwrap();
    let q = parse("invoice");
    let ids = |scope: &SearchScope| {
        s.search_scoped(&q, scope, 50).unwrap().into_iter().map(|r| r.header.id).collect::<Vec<_>>()
    };
    let ms_inbox = SearchScope::Folder { account_id: ms.id.clone(), folder_id: "in-ms".into() };
    assert_eq!(ids(&SearchScope::All), ["e", "d", "c", "b", "a"]);
    assert_eq!(ids(&ms_inbox), ["a"]);
    assert_eq!(ids(&SearchScope::Account(g.id.clone())), ["e", "d"]);
    assert_eq!(ids(&SearchScope::Role(FolderRole::Inbox)), ["d", "a"]);
    assert_eq!(s.search_scoped(&parse("invoice in:proj"), &ms_inbox, 50).unwrap().len(), 0, "operators narrow further");

    // Starred is a label, not a place: e counts but names no folder.
    let fc = |role, name: &str, count| FolderCount { role, name: name.into(), count };
    assert_eq!(
        s.search_elsewhere(&q, &ms_inbox).unwrap(),
        Elsewhere { total: 4, folders: vec![fc(None, "PROJ", 2), fc(Some(FolderRole::Inbox), "INBOX", 1)] }
    );
    assert_eq!(s.search_elsewhere(&q, &SearchScope::Role(FolderRole::Inbox)).unwrap().total, 3);
    assert_eq!(s.search_elsewhere(&q, &SearchScope::All).unwrap(), Elsewhere::default());
    assert_eq!(s.search_elsewhere(&parse("nothing"), &ms_inbox).unwrap(), Elsewhere::default());
}

#[test]
fn found_messages_are_stored_without_touching_cursors() {
    let (mut s, ms, _) = setup();
    s.apply_found(&ms.id, &[MailChange::Upsert(msg("old", "t", 5, &["in-ms"]))]).unwrap();
    assert_eq!(s.search("Subject old", 5).unwrap().len(), 1);
    assert_eq!(s.cursor(&ms.id, "f").unwrap(), None);
}

#[test]
fn contact_suggestions_rank_sent_recent_and_skip_me() {
    let (mut s, ms, g) = setup();
    let day = 86_400_000;
    let now = 100 * day;
    let incoming = |id: &str, name: &str, email: &str, at: i64| MessageHeader {
        id: id.into(),
        thread_id: id.into(),
        from: Some(Address::new(Some(name), email)),
        to: vec![Address::new(None, "me@work.com")],
        received_at: at,
        folder_ids: vec!["in-ms".into()],
        ..Default::default()
    };
    let sent = |id: &str, to: Vec<Address>, cc: Vec<Address>, at: i64| MessageHeader {
        id: id.into(),
        thread_id: id.into(),
        from: Some(Address::new(Some("Me"), "Me@Work.com")),
        to,
        cc,
        received_at: at,
        folder_ids: vec!["proj".into()],
        ..Default::default()
    };
    s.apply_mail(
        &ms.id,
        "f",
        &[
            // Dana wrote twice, long ago; old name first, newer name later.
            MailChange::Upsert(incoming("1", "D. Cohen", "dana@x.co.il", day)),
            MailChange::Upsert(incoming("2", "Dana Cohen", "DANA@x.co.il", 2 * day)),
            // David: I wrote to him once, recently. Sent mail outranks received.
            MailChange::Upsert(sent("3", vec![Address::new(Some("David Levi"), "david@y.com")], vec![], 99 * day)),
            // Hebrew name, matched by its second word.
            MailChange::Upsert(sent("4", vec![], vec![Address::new(Some("דנה כהן"), "dana.k@z.io")], 50 * day)),
        ],
        "1",
    )
    .unwrap();
    s.apply_mail(&g.id, "mail", &[MailChange::Upsert(incoming("5", "Daniel", "daniel@q.org", 98 * day))], "1").unwrap();

    let mine = vec!["me@work.com".to_string(), "me@gmail.com".to_string()];
    let emails =
        |q: &str| s.suggest_contacts(q, &mine, 10, now).unwrap().into_iter().map(|a| a.email).collect::<Vec<_>>();
    assert_eq!(emails("da")[0], "david@y.com", "sent + recent ranks first");
    assert!(emails("da").contains(&"daniel@q.org".to_string()), "across accounts");
    let dana = s.suggest_contacts("cohen", &mine, 10, now).unwrap();
    assert_eq!(dana.iter().filter(|a| a.email == "dana@x.co.il").count(), 1, "deduped case-insensitively");
    assert_eq!(dana.iter().find(|a| a.email == "dana@x.co.il").unwrap().name.as_deref(), Some("Dana Cohen"));
    assert_eq!(emails("כהן"), ["dana.k@z.io"]);
    assert!(emails("me").is_empty(), "own addresses never suggested");
    assert!(emails("").is_empty());
    assert_eq!(emails("levi"), ["david@y.com"]);
    assert!(emails("avid").is_empty(), "prefix match only");
    assert_eq!(s.suggest_contacts("d", &mine, 2, now).unwrap().len(), 2);
}

#[test]
fn invites_resolve_to_local_events() {
    let (mut s, ms, g) = setup();
    let cal = Calendar { id: "cal".into(), name: "C".into(), color: None, is_primary: true, can_edit: true };
    s.replace_calendars(&ms.id, std::slice::from_ref(&cal)).unwrap();
    s.replace_calendars(&g.id, std::slice::from_ref(&cal)).unwrap();
    let mut weekly1 = event("w1", 1_000, 2_000);
    weekly1.series_id = Some("S".into());
    weekly1.ical_uid = Some("uid-weekly".into());
    let mut weekly2 = event("w2", 9_000, 10_000);
    weekly2.series_id = Some("S".into());
    weekly2.ical_uid = Some("uid-weekly".into());
    s.replace_events(&ms.id, "cal", 0, 100_000, &[weekly1, weekly2]).unwrap();
    let mut other = event("x", 5_000, 6_000);
    other.ical_uid = Some("uid-other".into());
    s.replace_events(&g.id, "cal", 0, 100_000, &[other]).unwrap();

    let inv = |uid: Option<&str>, event_id: Option<&str>, start| Invite {
        uid: uid.map(String::from),
        event_id: event_id.map(String::from),
        start_at: start,
        ..Default::default()
    };
    let id = |r: Option<EventRow>| r.map(|e| e.event.id);
    // Graph: provider id of the series master -> nearest instance.
    assert_eq!(id(s.invite_event(&ms.id, &inv(None, Some("S"), 8_500)).unwrap()).as_deref(), Some("w2"));
    // Gmail: UID within the account, nearest instance.
    assert_eq!(id(s.invite_event(&ms.id, &inv(Some("uid-weekly"), None, 0)).unwrap()).as_deref(), Some("w1"));
    // UID only known in another account.
    assert_eq!(id(s.invite_event(&ms.id, &inv(Some("uid-other"), None, 0)).unwrap()).as_deref(), Some("x"));
    assert!(s.invite_event(&ms.id, &inv(Some("nope"), None, 0)).unwrap().is_none());
    assert_eq!(
        s.events_between(0, 100_000).unwrap().iter().find(|e| e.event.id == "x").unwrap().event.ical_uid.as_deref(),
        Some("uid-other")
    );
}

#[test]
fn flag_and_retain_changes() {
    let (mut s, ms, _) = setup();
    s.apply_mail(
        &ms.id,
        "f",
        &[
            MailChange::Upsert(msg("a", "t", 100, &["in-ms"])),
            MailChange::Upsert(msg("b", "t", 200, &["in-ms"])),
            MailChange::Upsert(msg("old", "t", 5, &["in-ms"])),
        ],
        "1",
    )
    .unwrap();
    s.apply_mail(
        &ms.id,
        "f",
        &[
            MailChange::Flags { id: "a".into(), is_read: true, is_flagged: true },
            MailChange::Flags { id: "missing".into(), is_read: true, is_flagged: false },
            // Server now has only "a" in the window since 50: "b" is gone, "old" is outside the window.
            MailChange::Retain { folder_id: "in-ms".into(), ids: vec!["a".into()], since: 50 },
        ],
        "2",
    )
    .unwrap();
    let ids: Vec<String> =
        s.list_messages(&ListQuery::unified(FolderRole::Inbox, 10)).unwrap().into_iter().map(|r| r.header.id).collect();
    assert_eq!(ids, ["a", "old"]);
    let a = s.message(&ms.id, "a").unwrap().unwrap();
    assert!(a.is_read && a.is_flagged);
}

/// Trigger-maintained counters must equal a fresh recount.
fn assert_counts_consistent(s: &Store) {
    let recount: Vec<(String, String, i64, i64)> = s
        .conn
        .prepare(
            "SELECT mf.account_id, mf.folder_id, COUNT(*), SUM(NOT m.is_read)
             FROM message_folders mf JOIN messages m ON m.pk = mf.message_pk
             GROUP BY mf.account_id, mf.folder_id ORDER BY 1, 2",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    let kept: Vec<(String, String, i64, i64)> = s
        .conn
        .prepare("SELECT account_id, folder_id, total, unread FROM folder_counts WHERE total > 0 ORDER BY 1, 2")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(kept, recount);
    let negative: i64 =
        s.conn.query_row("SELECT COUNT(*) FROM folder_counts WHERE total < 0 OR unread < 0", [], |r| r.get(0)).unwrap();
    assert_eq!(negative, 0);
}

#[test]
fn folder_counters_track_every_mutation() {
    let (mut s, ms, g) = setup();
    let mut read = msg("r", "t", 3, &["in-ms"]);
    read.is_read = true;
    s.apply_mail(
        &ms.id,
        "f",
        &[
            MailChange::Upsert(msg("a", "t", 1, &["in-ms"])),
            MailChange::Upsert(msg("b", "t", 2, &["proj"])),
            MailChange::Upsert(read),
        ],
        "1",
    )
    .unwrap();
    s.apply_mail(&g.id, "mail", &[MailChange::Upsert(msg("c", "u", 4, &["INBOX", "STARRED"]))], "1").unwrap();
    assert_counts_consistent(&s);
    // Move, read flip via re-upsert, optimistic ops, rollback, delete, unlink.
    s.apply_mail(&ms.id, "f", &[MailChange::Upsert(msg("a", "t", 1, &["proj"]))], "2").unwrap();
    let id = s.enqueue(&ms.id, &MailOp::SetRead { ids: vec!["b".into()], read: true }).unwrap();
    assert_counts_consistent(&s);
    s.rollback_op(id).unwrap();
    s.enqueue(&g.id, &MailOp::Relabel { ids: vec!["c".into()], add: vec![], remove: vec!["INBOX".into()] }).unwrap();
    s.apply_mail(&ms.id, "f", &[MailChange::Unlink { id: "r".into(), folder_id: "in-ms".into() }], "3").unwrap();
    s.apply_mail(&g.id, "mail", &[MailChange::Delete { id: "c".into() }], "2").unwrap();
    assert_counts_consistent(&s);
    let proj = s.folders(&ms.id).unwrap().into_iter().find(|f| f.folder.id == "proj").unwrap();
    assert_eq!((proj.total, proj.unread), (2, 2));
    s.delete_account(&ms.id).unwrap();
    assert_counts_consistent(&s);
}

#[test]
fn threads_found_past_a_long_thread_and_paged_with_before() {
    let (mut s, ms, g) = setup();
    // One 200-message thread newer than everything else fills the first chunk.
    let mut changes: Vec<MailChange> =
        (0..200).map(|i| MailChange::Upsert(msg(&format!("long{i}"), "long", 10_000 + i, &["in-ms"]))).collect();
    changes.push(MailChange::Upsert(msg("x", "short", 500, &["in-ms"])));
    s.apply_mail(&ms.id, "f", &changes, "1").unwrap();
    s.apply_mail(&g.id, "mail", &[MailChange::Upsert(msg("y", "gm", 700, &["INBOX"]))], "1").unwrap();

    let t = s.list_threads(&ListQuery::unified(FolderRole::Inbox, 3)).unwrap();
    assert_eq!(t.iter().map(|t| t.thread_id.as_str()).collect::<Vec<_>>(), ["long", "gm", "short"]);
    assert_eq!(t[0].message_count, 200);
    assert_eq!(t[0].latest.id, "long199");

    let page = ListQuery { before: Some(700), ..ListQuery::unified(FolderRole::Inbox, 5) };
    let t = s.list_threads(&page).unwrap();
    assert_eq!(t.iter().map(|t| t.thread_id.as_str()).collect::<Vec<_>>(), ["short"]);

    let m = s.list_messages(&ListQuery::unified(FolderRole::Inbox, 3)).unwrap();
    assert_eq!(m.iter().map(|m| m.header.id.as_str()).collect::<Vec<_>>(), ["long199", "long198", "long197"]);
}

#[test]
fn draft_server_reference_roundtrips() {
    let (s, _, g) = setup();
    let from = crate::compose::Sender { account_id: g.id.clone(), address: Address::new(None, "me@gmail.com") };
    let mut d = crate::compose::Draft::new(from, 5);
    d.body_html = "<div dir=\"auto\">hi</div>".into();
    d.server = Some(crate::compose::ServerDraft { account_id: g.id.clone(), id: "r-123".into() });
    s.save_draft(&d).unwrap();
    let back = s.draft(&d.id).unwrap().unwrap();
    assert_eq!(back.server, d.server);
    assert_eq!(back.full_html(), "<div dir=\"auto\">hi</div>");
}

#[test]
fn services_and_zoom_meetings_round_trip() {
    let (store, a, _) = setup();
    assert!(store.service("zoom").unwrap().is_none());
    let svc =
        ConnectedService { id: "zoom".into(), email: "me@x.com".into(), display_name: "Me".into(), connected_at: 5 };
    store.upsert_service(&svc).unwrap();
    assert_eq!(store.service("zoom").unwrap(), Some(svc));
    let link = ZoomMeetingLink { meeting_id: "123".into(), join_url: "https://zoom.us/j/123".into() };
    store.set_zoom_meeting(&a.id, "ev1", &link).unwrap();
    assert_eq!(store.zoom_meeting(&a.id, "ev1").unwrap(), Some(link));
    store.delete_zoom_meeting(&a.id, "ev1").unwrap();
    assert!(store.zoom_meeting(&a.id, "ev1").unwrap().is_none());
    store.delete_service("zoom").unwrap();
    assert!(store.service("zoom").unwrap().is_none());
}

#[test]
fn patch_changes_state_and_keeps_the_header() {
    let (mut s, ms, _) = setup();
    let mut h = msg("a", "t", 100, &["in-ms"]);
    h.subject = "Quarterly".into();
    s.apply_mail(&ms.id, "f", &[MailChange::Upsert(h)], "1").unwrap();
    s.apply_mail(&ms.id, "f", &[MailChange::Patch { id: "a".into(), is_read: Some(true), is_flagged: None }], "2")
        .unwrap();
    let got = s.message(&ms.id, "a").unwrap().unwrap();
    assert_eq!((got.subject.as_str(), got.received_at, got.thread_id.as_str()), ("Quarterly", 100, "t"));
    assert!(got.is_read);
}

#[test]
fn icloud_addresses_are_detected_from_own_mail_and_overridable() {
    let mut s = Store::open_in_memory().unwrap();
    let ic = account(ProviderKind::ICloud, "me@gmail.com");
    let other = account(ProviderKind::Google, "me@gmail.com");
    s.upsert_account(&ic).unwrap();
    s.upsert_account(&other).unwrap();
    s.replace_folders(
        &ic.id,
        &[
            folder("INBOX", Some(FolderRole::Inbox)),
            folder("Sent Messages", Some(FolderRole::Sent)),
            folder("Junk", Some(FolderRole::Spam)),
            folder("Lists", None),
        ],
    )
    .unwrap();
    s.replace_folders(&other.id, &[folder("INBOX", Some(FolderRole::Inbox))]).unwrap();
    let mail = |id: &str, from: &str, to: &[&str], cc: &[&str], folder: &str| {
        let mut m = msg(id, id, 100, &[folder]);
        m.from = Some(Address::new(None, from));
        m.to = to.iter().map(|e| Address::new(None, e)).collect();
        m.cc = cc.iter().map(|e| Address::new(None, e)).collect();
        MailChange::Upsert(m)
    };
    let changes = [
        // Sent: From is ours, whatever the recipients.
        mail("s1", "Me@iCloud.com", &["a@x.io", "friend@icloud.com"], &[], "Sent Messages"),
        mail("s2", "me@icloud.com", &["a@x.io"], &[], "Sent Messages"),
        mail("s3", "me@gmail.com", &["a@x.io"], &[], "Sent Messages"),
        // Incoming to one address: an alias.
        mail("i1", "a@x.io", &["alias@me.com"], &[], "INBOX"),
        mail("i2", "a@x.io", &[], &["old@mac.com"], "Lists"),
        // Group mail: other people's iCloud addresses don't count.
        mail("i3", "a@x.io", &["me@icloud.com", "pal@icloud.com"], &["mate@icloud.com"], "INBOX"),
        // Spam to random addresses doesn't count.
        mail("j1", "spam@x.io", &["random@icloud.com"], &[], "Junk"),
        // Not an iCloud domain.
        mail("i4", "a@x.io", &["me@gmail.com"], &[], "INBOX"),
    ];
    s.apply_mail(&ic.id, "mail", &changes, "1").unwrap();
    // Another account's mail never leaks in.
    s.apply_mail(&other.id, "mail", &[mail("g1", "a@x.io", &["someone@icloud.com"], &[], "INBOX")], "1").unwrap();

    assert_eq!(s.detected_icloud_addresses(&ic.id).unwrap(), ["me@icloud.com", "alias@me.com", "old@mac.com"]);
    // Any domain, for the engine to confirm (an iCloud+ custom domain).
    let all = s.send_address_candidates(&ic.id).unwrap();
    assert!(all.contains(&"me@gmail.com".to_string()) && all.contains(&"alias@me.com".to_string()));
    assert!(!all.contains(&"random@icloud.com".to_string()) && !all.contains(&"pal@icloud.com".to_string()));
    assert_eq!(s.detected_icloud_addresses(&other.id).unwrap(), ["someone@icloud.com"]);

    s.add_send_address(&ic.id, " Custom@Domain.dev ", 1).unwrap();
    s.remove_send_address(&ic.id, "old@mac.com", 2).unwrap();
    s.add_send_address(&ic.id, "x@icloud.com", 3).unwrap();
    s.remove_send_address(&ic.id, "x@icloud.com", 4).unwrap();
    assert_eq!(
        s.send_address_overrides(&ic.id).unwrap(),
        [
            ("custom@domain.dev".to_string(), false),
            ("old@mac.com".to_string(), true),
            ("x@icloud.com".to_string(), true)
        ]
    );
    s.add_send_address(&ic.id, "old@mac.com", 5).unwrap();
    assert_eq!(s.send_address_overrides(&ic.id).unwrap().last(), Some(&("old@mac.com".to_string(), false)));

    // A detected custom-domain address doesn't undo a removal.
    s.remove_send_address(&ic.id, "help@shop.dev", 6).unwrap();
    s.note_send_address(&ic.id, "help@shop.dev", 7).unwrap();
    s.note_send_address(&ic.id, "new@shop.dev", 8).unwrap();
    let o = s.send_address_overrides(&ic.id).unwrap();
    assert!(o.contains(&("help@shop.dev".to_string(), true)) && o.contains(&("new@shop.dev".to_string(), false)));

    assert_eq!(s.main_address(&ic.id).unwrap(), None);
    s.set_main_address(&ic.id, Some(" Custom@Domain.dev ")).unwrap();
    s.set_main_address(&ic.id, Some("new@shop.dev")).unwrap();
    assert_eq!(s.main_address(&ic.id).unwrap().as_deref(), Some("new@shop.dev"));
    s.set_main_address(&ic.id, None).unwrap();
    assert_eq!(s.main_address(&ic.id).unwrap(), None);
    s.set_main_address(&ic.id, Some("new@shop.dev")).unwrap();

    s.delete_account(&ic.id).unwrap();
    assert!(s.send_address_overrides(&ic.id).unwrap().is_empty(), "go with the account");
    assert_eq!(s.main_address(&ic.id).unwrap(), None, "go with the account");
}

#[test]
fn purge_waits_out_undo_then_drops_the_message() {
    let (mut s, g) = gmail_with_labels();
    let purge = MailOp::Purge { ids: vec!["a".into()] };
    let id = s.enqueue(&g.id, &purge).unwrap();
    assert!(state(&s, &g, "a").folder_ids.is_empty(), "gone from every list");
    assert!(s.due_ops(&g.id, now_ms()).unwrap().is_empty(), "held for the undo grace");
    assert!(s.cancel_pending_op(id).unwrap(), "undo still works");
    assert_eq!(state(&s, &g, "a").folder_ids, ["INBOX"]);

    let id = s.enqueue(&g.id, &purge).unwrap();
    let due = s.due_ops(&g.id, now_ms() + super::PURGE_GRACE_MS).unwrap();
    assert_eq!(due[0].id, id);
    s.mark_op_done(id).unwrap();
    assert!(s.message(&g.id, "a").unwrap().is_none(), "deleted locally once the server took it");
}

#[test]
fn folder_message_ids_lists_a_folder() {
    let (s, g) = gmail_with_labels();
    assert_eq!(s.folder_message_ids(&g.id, "INBOX").unwrap(), ["a"]);
    assert!(s.folder_message_ids(&g.id, "TRASH").unwrap().is_empty());
}
