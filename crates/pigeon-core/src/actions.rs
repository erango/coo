//! Outbox replay: send queued ops in order, back off on transient failures,
//! roll back what the server rejects.

use std::sync::Mutex;

use crate::model::MailOp;
use crate::provider::MailActions;
use crate::store::Store;
use crate::sync::lock;
use crate::{Error, Result, now_ms};

const MAX_ATTEMPTS: u32 = 8;
const BASE_BACKOFF_MS: i64 = 5_000;
const MAX_BACKOFF_MS: i64 = 10 * 60_000;

#[derive(Debug, Default)]
pub struct FlushReport {
    pub sent: usize,
    /// Will be retried later.
    pub deferred: usize,
    /// Rejected by the server and rolled back locally (surface as a toast).
    pub rejected: Vec<(MailOp, String)>,
    /// Flushing stopped because the account needs to sign in again.
    pub needs_sign_in: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Retry,
    Reject,
    SignIn,
}

fn classify(e: &Error) -> Outcome {
    match e {
        Error::SignInRequired(_) => Outcome::SignIn,
        Error::Http(_) => Outcome::Retry,
        _ => match e.status() {
            Some(401) => Outcome::SignIn,
            Some(408 | 429 | 500..=599) => Outcome::Retry,
            _ => Outcome::Reject,
        },
    }
}

fn backoff_ms(attempts: u32) -> i64 {
    (BASE_BACKOFF_MS << attempts.min(10)).min(MAX_BACKOFF_MS)
}

/// Send due ops for one account, oldest first. A transient failure stops the
/// account's queue (later ops may depend on earlier ones).
pub async fn flush_outbox<P: MailActions>(p: &P, account_id: &str, store: &Mutex<Store>) -> Result<FlushReport> {
    let mut report = FlushReport::default();
    let due = lock(store).due_ops(account_id, now_ms())?;
    let mut iter = due.into_iter();
    for entry in iter.by_ref() {
        match p.apply(&entry.op).await {
            Ok(()) => {
                lock(store).mark_op_done(entry.id)?;
                report.sent += 1;
            }
            Err(e) => match classify(&e) {
                Outcome::SignIn => {
                    report.needs_sign_in = true;
                    report.deferred += 1;
                    break;
                }
                Outcome::Retry if entry.attempts + 1 < MAX_ATTEMPTS => {
                    tracing::warn!(account_id, op = entry.id, error = %e, "op failed; will retry");
                    lock(store).mark_op_retry(entry.id, &e.to_string(), now_ms() + backoff_ms(entry.attempts))?;
                    report.deferred += 1;
                    break;
                }
                _ => {
                    tracing::error!(account_id, op = entry.id, error = %e, "op rejected; rolling back");
                    lock(store).rollback_op(entry.id)?;
                    report.rejected.push((entry.op, e.to_string()));
                }
            },
        }
    }
    report.deferred += iter.count();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::model::*;
    use crate::store::ListQuery;

    /// Fails the n-th call (1-based) with the given status.
    struct Fake {
        calls: AtomicUsize,
        fail_on: usize,
        status: u16,
    }

    impl MailActions for Fake {
        async fn apply(&self, _: &MailOp) -> Result<()> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            if n == self.fail_on {
                return Err(Error::Api { status: self.status, url: "x".into(), body: String::new() });
            }
            Ok(())
        }
    }

    fn store() -> (Mutex<Store>, String) {
        let mut s = Store::open_in_memory().unwrap();
        let acct = Account {
            id: "microsoft:a@b.c".into(),
            provider: ProviderKind::Microsoft,
            email: "a@b.c".into(),
            display_name: String::new(),
        };
        s.upsert_account(&acct).unwrap();
        let folder =
            |id: &str, role| Folder { id: id.into(), name: id.into(), parent_id: None, role, kind: FolderKind::Folder };
        s.replace_folders(
            &acct.id,
            &[folder("in", Some(FolderRole::Inbox)), folder("arch", Some(FolderRole::Archive))],
        )
        .unwrap();
        let h = MessageHeader {
            id: "m".into(),
            thread_id: "t".into(),
            folder_ids: vec!["in".into()],
            ..Default::default()
        };
        s.apply_mail(&acct.id, "f", &[MailChange::Upsert(h)], "c").unwrap();
        (Mutex::new(s), acct.id)
    }

    fn inbox_len(store: &Mutex<Store>) -> usize {
        store.lock().unwrap().list_messages(&ListQuery::unified(FolderRole::Inbox, 5)).unwrap().len()
    }

    #[tokio::test]
    async fn rejected_op_rolls_back_and_later_ops_continue() {
        let (store, acct) = store();
        let archive = MailOp::MoveTo { ids: vec!["m".into()], folder_id: "arch".into() };
        store.lock().unwrap().enqueue(&acct, &archive).unwrap();
        store.lock().unwrap().enqueue(&acct, &MailOp::SetRead { ids: vec!["m".into()], read: true }).unwrap();
        assert_eq!(inbox_len(&store), 0);

        let p = Fake { calls: AtomicUsize::new(0), fail_on: 1, status: 400 };
        let r = flush_outbox(&p, &acct, &store).await.unwrap();
        assert_eq!((r.sent, r.rejected.len(), r.deferred), (1, 1, 0));
        assert_eq!(inbox_len(&store), 1, "archive rolled back");
        let st = store.lock().unwrap().message_states(&acct, &["m".into()]).unwrap();
        assert!(st[0].is_read, "later op kept");
    }

    #[tokio::test]
    async fn transient_failure_defers_the_queue() {
        let (store, acct) = store();
        for read in [true, false] {
            store.lock().unwrap().enqueue(&acct, &MailOp::SetRead { ids: vec!["m".into()], read }).unwrap();
        }
        let p = Fake { calls: AtomicUsize::new(0), fail_on: 1, status: 503 };
        let r = flush_outbox(&p, &acct, &store).await.unwrap();
        assert_eq!((r.sent, r.deferred), (0, 2));
        assert_eq!(p.calls.load(Ordering::SeqCst), 1, "stops at the first transient failure");
        assert!(store.lock().unwrap().due_ops(&acct, now_ms()).unwrap().is_empty(), "backing off");
    }

    #[test]
    fn classification_and_backoff() {
        let api = |status| Error::Api { status, url: String::new(), body: String::new() };
        assert_eq!(classify(&api(429)), Outcome::Retry);
        assert_eq!(classify(&api(503)), Outcome::Retry);
        assert_eq!(classify(&api(400)), Outcome::Reject);
        assert_eq!(classify(&api(401)), Outcome::SignIn);
        assert_eq!(classify(&Error::SignInRequired("x".into())), Outcome::SignIn);
        assert_eq!(backoff_ms(0), 5_000);
        assert_eq!(backoff_ms(20), MAX_BACKOFF_MS);
    }
}
