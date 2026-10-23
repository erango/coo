//! Hidden developer tools: a synthetic large store and hot-path benchmarks.
//!
//! ```sh
//! PIGEON_DATA_DIR=/tmp/coo-perf coo dev seed --messages 100000 --accounts 5 --events 3000
//! PIGEON_DATA_DIR=/tmp/coo-perf coo dev bench
//! ```

use std::time::{Duration, Instant};

use pigeon_core::Result;
use pigeon_core::model::*;
use pigeon_core::search;
use pigeon_core::store::{FolderSel, ListQuery, Store};

/// xorshift64*: deterministic, dependency-free.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }
    fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len() as u64) as usize]
    }
}

const EN_WORDS: &[&str] = &[
    "invoice",
    "meeting",
    "roadmap",
    "review",
    "quarterly",
    "budget",
    "design",
    "launch",
    "update",
    "contract",
    "hiring",
    "offsite",
    "travel",
    "report",
    "proposal",
    "feedback",
    "schedule",
    "release",
    "customer",
    "support",
    "order",
    "shipping",
    "renewal",
    "agenda",
    "notes",
    "draft",
    "follow-up",
    "question",
    "approval",
    "deadline",
];
const HE_WORDS: &[&str] = &[
    "חשבונית",
    "פגישה",
    "הזמנה",
    "משלוח",
    "עדכון",
    "תקציב",
    "סיכום",
    "שאלה",
    "אישור",
    "דוח",
    "לקוח",
    "חוזה",
    "תודה",
    "מחר",
    "שבוע",
    "בקשה",
];
const FIRST: &[&str] = &[
    "Dana", "Yossi", "Noa", "Priya", "Jonah", "Lena", "Theo", "Ava", "Sam", "Marcus", "Maya", "Omer", "Tal", "Ruth",
    "Daniel", "Sara", "דנה", "יוסי", "נועה", "עומר",
];
const LAST: &[&str] =
    &["Cohen", "Levi", "Raman", "Weiss", "Fischer", "Park", "Chen", "Lee", "Mizrahi", "Peretz", "כהן", "לוי"];

fn sentence(r: &mut Rng, words: usize) -> String {
    let hebrew = r.chance(25);
    let pool = if hebrew { HE_WORDS } else { EN_WORDS };
    (0..words).map(|_| *r.pick(pool)).collect::<Vec<_>>().join(" ")
}

fn person(r: &mut Rng, i: usize) -> Address {
    let name = format!("{} {}", r.pick(FIRST), r.pick(LAST));
    Address::new(Some(&name), &format!("person{i}@example{}.com", i % 40))
}

/// Fill the store at `path` with a realistic large mailbox.
pub fn seed(path: &std::path::Path, messages: usize, accounts: usize, events: usize) -> Result<()> {
    let started = Instant::now();
    let mut store = Store::open(path)?;
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    let now = pigeon_core::now_ms();
    let people: Vec<Address> = (0..500).map(|i| person(&mut r, i)).collect();

    let mut accts = Vec::new();
    for a in 0..accounts {
        let google = a % 2 == 1;
        let provider = if google { ProviderKind::Google } else { ProviderKind::Microsoft };
        let email = format!("me{a}@{}", if google { "gmail.com" } else { "work.example" });
        let account =
            Account { id: Account::make_id(provider, &email), provider, email, display_name: format!("Me {a}") };
        store.upsert_account(&account)?;
        let kind = if google { FolderKind::Label } else { FolderKind::Folder };
        let mut folders = vec![
            Folder { id: "inbox".into(), name: "Inbox".into(), parent_id: None, role: Some(FolderRole::Inbox), kind },
            Folder { id: "sent".into(), name: "Sent".into(), parent_id: None, role: Some(FolderRole::Sent), kind },
            Folder {
                id: "archive".into(),
                name: "Archive".into(),
                parent_id: None,
                role: Some(FolderRole::Archive),
                kind,
            },
            Folder { id: "trash".into(), name: "Trash".into(), parent_id: None, role: Some(FolderRole::Trash), kind },
        ];
        if google {
            folders.push(Folder {
                id: "starred".into(),
                name: "Starred".into(),
                parent_id: None,
                role: Some(FolderRole::Starred),
                kind,
            });
        }
        for f in 0..10 {
            folders.push(Folder {
                id: format!("f{f}"),
                name: format!("Project {f}"),
                parent_id: None,
                role: None,
                kind,
            });
        }
        store.replace_folders(&account.id, &folders)?;
        accts.push((account, google));
    }

    // Two years of mail, threads of 1..20, newest last so cursors look natural.
    let span: i64 = 2 * 365 * 86_400_000;
    let mut written = 0;
    let mut thread_no = 0;
    let mut page = Vec::with_capacity(1000);
    while written < messages {
        let (account, google) = r.pick(&accts).clone();
        thread_no += 1;
        let len = 1 + (r.below(20) * r.below(20) / 20) as usize; // skewed toward short threads
        let start = now - r.below(span as u64) as i64;
        let n = 2 + r.below(5) as usize;
        let subject = sentence(&mut r, n);
        let user_folder = format!("f{}", r.below(10));
        for k in 0..len.min(messages - written) {
            let from = r.pick(&people).clone();
            let at = start + (k as i64) * (r.below(6 * 3_600_000) as i64 + 60_000);
            let mut folder_ids = Vec::new();
            let place = r.below(100);
            if google {
                if place < 45 {
                    folder_ids.push("inbox".to_string());
                }
                if r.chance(30) {
                    folder_ids.push(user_folder.clone());
                }
                if place >= 90 {
                    folder_ids.push("sent".into());
                }
            } else {
                folder_ids.push(match place {
                    0..45 => "inbox".into(),
                    45..80 => "archive".into(),
                    80..90 => "sent".into(),
                    90..93 => "trash".into(),
                    _ => user_folder.clone(),
                });
            }
            let flagged = r.chance(5);
            if google && flagged {
                folder_ids.push("starred".into());
            }
            page.push(MailChange::Upsert(MessageHeader {
                id: format!("t{thread_no}m{k}"),
                thread_id: format!("t{thread_no}"),
                internet_message_id: Some(format!("<t{thread_no}m{k}@seed>")),
                subject: if k == 0 { subject.clone() } else { format!("Re: {subject}") },
                from: Some(from),
                to: vec![Address::new(None, &account.email), r.pick(&people).clone()],
                cc: if r.chance(20) { vec![r.pick(&people).clone()] } else { vec![] },
                snippet: sentence(&mut r, 12),
                received_at: at,
                is_read: !r.chance(20),
                is_flagged: flagged,
                has_attachments: r.chance(15),
                is_draft: false,
                folder_ids,
            }));
            written += 1;
            if page.len() == 1000 {
                store.apply_found(&account.id, &page)?;
                page.clear();
            }
        }
        if !page.is_empty() {
            store.apply_found(&account.id, &page)?;
            page.clear();
        }
    }

    for (account, _) in &accts {
        let cal = Calendar { id: "cal".into(), name: "Calendar".into(), color: None, is_primary: true, can_edit: true };
        store.replace_calendars(&account.id, std::slice::from_ref(&cal))?;
        let per = events / accts.len().max(1);
        let evs: Vec<Event> = (0..per)
            .map(|i| {
                let start = now - 365 * 86_400_000 + r.below(2 * 365 * 86_400_000) as i64;
                Event {
                    id: format!("e{i}"),
                    calendar_id: "cal".into(),
                    title: sentence(&mut r, 3),
                    start_at: start,
                    end_at: start + (30 + r.below(90) as i64) * 60_000,
                    attendees: (0..r.below(6))
                        .map(|_| Attendee {
                            address: r.pick(&people).clone(),
                            response: Response::Accepted,
                            optional: false,
                        })
                        .collect(),
                    ..Default::default()
                }
            })
            .collect();
        store.replace_events(&account.id, "cal", i64::MIN / 2, i64::MAX / 2, &evs)?;
    }
    eprintln!(
        "seeded {written} messages, {} accounts, {events} events in {:.1}s",
        accts.len(),
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

struct Timing {
    name: String,
    median: Duration,
    p95: Duration,
    rows: usize,
}

fn time(name: &str, runs: usize, mut f: impl FnMut() -> Result<usize>) -> Result<Timing> {
    let rows = f()?; // warm-up (and row count)
    let mut samples: Vec<Duration> = (0..runs)
        .map(|_| {
            let t = Instant::now();
            let _ = f();
            t.elapsed()
        })
        .collect();
    samples.sort();
    Ok(Timing {
        name: name.to_string(),
        median: samples[samples.len() / 2],
        p95: samples[(samples.len() * 95 / 100).min(samples.len() - 1)],
        rows,
    })
}

/// Time the store's hot paths. Prints a table (ms).
pub fn bench(path: &std::path::Path, runs: usize) -> Result<()> {
    let mut out = Vec::new();
    out.push(time("Store::open (existing db)", runs.min(20), || {
        Store::open(path)?;
        Ok(0)
    })?);
    let mut store = Store::open(path)?;
    let accounts = store.accounts()?;
    let first =
        accounts.first().cloned().ok_or_else(|| pigeon_core::Error::Other("empty store: run dev seed".into()))?;
    let unified = |limit| ListQuery::unified(FolderRole::Inbox, limit);
    let folder_q = |limit| ListQuery {
        account_id: Some(first.id.clone()),
        folder: FolderSel::Id("archive".into()),
        limit,
        before: None,
    };
    for limit in [50, 300] {
        out.push(time(&format!("list_threads unified inbox {limit}"), runs, || {
            Ok(store.list_threads(&unified(limit))?.len())
        })?);
        out.push(time(&format!("list_messages unified inbox {limit}"), runs, || {
            Ok(store.list_messages(&unified(limit))?.len())
        })?);
        out.push(time(&format!("list_threads big folder {limit}"), runs, || {
            Ok(store.list_threads(&folder_q(limit))?.len())
        })?);
        out.push(time(&format!("list_messages big folder {limit}"), runs, || {
            Ok(store.list_messages(&folder_q(limit))?.len())
        })?);
    }
    let older = store.list_messages(&unified(300))?.last().map(|m| m.header.received_at);
    out.push(time("list_messages unified page 2 (before:)", runs, || {
        Ok(store.list_messages(&ListQuery { before: older, ..unified(50) })?.len())
    })?);
    for q in ["invoice", "from:dana is:unread", "has:attachment after:2026-01-01 report", "חשבונית"] {
        let parsed = search::parse(q);
        out.push(time(&format!("search_query {q:?}"), runs, || Ok(store.search_query(&parsed, 50)?.len()))?);
    }
    let big_thread = store.list_threads(&unified(300))?.into_iter().max_by_key(|t| t.message_count);
    if let Some(t) = big_thread {
        out.push(time(&format!("thread_messages ({} msgs)", t.message_count), runs, || {
            Ok(store.thread_messages(&t.account_id, &t.thread_id)?.len())
        })?);
    }
    out.push(time("folders() all accounts", runs, || {
        let mut n = 0;
        for a in &accounts {
            n += store.folders(&a.id)?.len();
        }
        Ok(n)
    })?);
    let now = pigeon_core::now_ms();
    out.push(time("events_between one week", runs, || Ok(store.events_between(now, now + 7 * 86_400_000)?.len()))?);
    // What the service's publish() reads on every change.
    out.push(time("publish snapshot (accounts+folders+list300+next-up+week)", runs, || {
        let accts = store.accounts()?;
        let mut n = 0;
        for a in &accts {
            n += store.folders(&a.id)?.len();
        }
        n += store.list_threads(&unified(300))?.len();
        n += store.events_between(now, now + 12 * 3_600_000)?.len();
        n += store.events_between(now, now + 7 * 86_400_000)?.len();
        for a in &accts {
            n += store.calendars(&a.id)?.len();
        }
        Ok(n)
    })?);
    // A 100-change sync page: re-upsert existing messages unchanged.
    let page: Vec<MailChange> = store
        .list_messages(&ListQuery { account_id: Some(first.id.clone()), ..unified(100) })?
        .into_iter()
        .map(|m| {
            let mut h = m.header;
            h.folder_ids = vec!["inbox".into()];
            MailChange::Upsert(h)
        })
        .collect();
    out.push(time("apply_mail 100-change page", runs.min(30), || {
        store.apply_mail(&first.id, "bench", &page, "c")?;
        Ok(page.len())
    })?);

    println!("{:<58} {:>9} {:>9} {:>6}", "operation", "median ms", "p95 ms", "rows");
    for t in out {
        println!(
            "{:<58} {:>9.2} {:>9.2} {:>6}",
            t.name,
            t.median.as_secs_f64() * 1000.0,
            t.p95.as_secs_f64() * 1000.0,
            t.rows
        );
    }
    Ok(())
}
