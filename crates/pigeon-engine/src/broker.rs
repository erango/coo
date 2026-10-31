//! The app's side of the sign-in broker (`pigeon_core::broker`): answers the
//! `coo` helper with access tokens from the app's own token sources, so an
//! AI agent never needs Keychain access.

use std::sync::Arc;
use std::time::Instant;

use futures::future::BoxFuture;
use pigeon_core::broker::{self, Failure, Handler, Request, Response};
use pigeon_core::model::ProviderKind;

use crate::Pigeon;

/// Something the person should see in Coo after an agent's request.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Notice {
    /// A helper was turned away (usually started before Coo updated).
    Refused,
    /// An agent needed this account, and its sign-in has expired.
    SignInRequired(String),
}

pub type Notify = Arc<dyn Fn(Notice) + Send + Sync>;

struct AppBroker {
    app: Arc<Pigeon>,
    notify: Option<Notify>,
}

impl AppBroker {
    fn account(&self, id: &str) -> Option<pigeon_core::model::Account> {
        self.app.accounts().ok()?.into_iter().find(|a| a.id == id)
    }

    /// Keychain reads block (and can wait on securityd): keep them off the
    /// async workers so one slow read never stalls other requests.
    async fn read_secret(&self, account_id: String) -> pigeon_core::Result<Option<String>> {
        let app = self.app.clone();
        tokio::task::spawn_blocking(move || app.tokens.get(&account_id))
            .await
            .map_err(|e| pigeon_core::Error::Other(e.to_string()))?
    }

    async fn answer(&self, req: Request) -> Response {
        let account = match &req {
            Request::AccessToken { account } | Request::Secret { account } | Request::Has { account } => {
                Some(account.clone())
            }
            _ => None,
        };
        let resp = self.answer_inner(req).await;
        if resp.failure == Some(Failure::SignInRequired)
            && let (Some(notify), Some(account)) = (&self.notify, account)
        {
            notify(Notice::SignInRequired(account));
        }
        resp
    }

    async fn answer_inner(&self, req: Request) -> Response {
        match req {
            Request::Ping => Response::pong(),
            Request::Has { account } => match self.account(&account) {
                None => Response::has(false),
                Some(a) => match self.read_secret(a.id).await {
                    Ok(secret) => Response::has(secret.is_some()),
                    Err(e) => broker::failure_for(&e),
                },
            },
            // The connected Zoom account (not a mail account).
            Request::AccessToken { account } if account == pigeon_core::auth::ZOOM_TOKEN_KEY => {
                match self.app.zoom_tokens() {
                    Ok(src) => match src.access_token_with_expiry().await {
                        Ok((token, expires_at)) => {
                            Response::token(token, expires_at.saturating_duration_since(Instant::now()))
                        }
                        Err(e) => broker::failure_for(&e),
                    },
                    Err(e) => broker::failure_for(&e),
                }
            }
            Request::AccessToken { account } => {
                let Some(a) = self.account(&account) else {
                    return Response::fail(Failure::UnknownAccount, format!("{account} isn't signed in to Coo"));
                };
                if a.provider == ProviderKind::ICloud {
                    return Response::fail(Failure::Denied, "iCloud signs in with a password, not a token");
                }
                match self.app.token_source(&a) {
                    Ok(src) => match src.access_token_with_expiry().await {
                        Ok((token, expires_at)) => {
                            Response::token(token, expires_at.saturating_duration_since(Instant::now()))
                        }
                        Err(e) => broker::failure_for(&e),
                    },
                    Err(e) => broker::failure_for(&e),
                }
            }
            // Only iCloud's app-specific password is shared: IMAP, SMTP and
            // CalDAV log in with it directly. OAuth refresh tokens never leave.
            Request::Secret { account } => match self.account(&account) {
                None => Response::fail(Failure::UnknownAccount, format!("{account} isn't signed in to Coo")),
                Some(a) if a.provider != ProviderKind::ICloud => {
                    Response::fail(Failure::Denied, "this account shares access tokens only")
                }
                Some(a) => match self.read_secret(a.id.clone()).await {
                    Ok(Some(secret)) => Response { ok: true, token: Some(secret), ..Default::default() },
                    Ok(None) => Response::fail(Failure::SignInRequired, format!("{} needs to sign in again", a.email)),
                    Err(e) => broker::failure_for(&e),
                },
            },
        }
    }
}

impl Handler for AppBroker {
    fn handle(&self, req: Request) -> BoxFuture<'_, Response> {
        Box::pin(self.answer(req))
    }

    fn refused(&self) {
        if let Some(notify) = &self.notify {
            notify(Notice::Refused);
        }
    }
}

/// Start sharing sign-ins with the helper (the app calls this once). Only
/// for the Keychain: with the file store the helper reads the file itself.
pub fn start(app: Arc<Pigeon>, notify: Option<Notify>) {
    if app.tokens.kind() != "keychain" || app.tokens.broker().is_some() {
        return;
    }
    // Debug builds only: seed a sign-in written by the app itself, so an
    // end-to-end test never needs real accounts or a Keychain prompt.
    #[cfg(debug_assertions)]
    if let Some((account, secret)) =
        std::env::var("PIGEON_DEBUG_SEED_TOKEN").ok().as_deref().and_then(|v| v.split_once('='))
        && let Err(e) = if secret.is_empty() { app.tokens.delete(account) } else { app.tokens.set(account, secret) }
    {
        tracing::warn!(error = %e, "couldn't seed the debug sign-in");
    }
    let socket = broker::socket_path(&app.data_dir);
    tokio::spawn(async move {
        if let Err(e) = broker::serve(socket, Arc::new(AppBroker { app, notify })).await {
            tracing::warn!(error = %e, "sign-in broker stopped");
        }
    });
}

/// Whether a running Coo answers on the broker socket (for `coo doctor`).
pub fn ping(app: &Pigeon) -> Result<String, String> {
    let socket = broker::socket_path(&app.data_dir);
    match broker::call(&socket, &Request::Ping, std::time::Duration::from_secs(3)) {
        Ok(r) if r.ok => Ok(r.version.unwrap_or_default()),
        Ok(r) => Err(r.error.unwrap_or_else(|| "refused".into())),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pigeon_core::model::Account;
    use pigeon_core::tokens::TokenStore;

    /// The app's broker over a real socket, asked by a helper-side store,
    /// exactly as `coo mcp` does (file store stands in for the Keychain).
    #[tokio::test]
    async fn helper_reads_sign_ins_through_the_app() {
        let dir = std::env::temp_dir().join(format!("coo-appbroker-{}", pigeon_core::auth::random_token(4)));
        std::fs::create_dir_all(&dir).unwrap();
        let app = Arc::new(Pigeon::open_with(dir.clone(), TokenStore::File(dir.join("tokens.json"))).unwrap());
        let icloud = Account {
            id: Account::make_id(ProviderKind::ICloud, "a@icloud.com"),
            provider: ProviderKind::ICloud,
            email: "a@icloud.com".into(),
            display_name: "a".into(),
        };
        let google = Account {
            id: Account::make_id(ProviderKind::Google, "b@gmail.com"),
            provider: ProviderKind::Google,
            email: "b@gmail.com".into(),
            display_name: "b".into(),
        };
        for a in [&icloud, &google] {
            app.store().upsert_account(a).unwrap();
        }
        app.tokens.set(&icloud.id, "app-specific-pw").unwrap();
        app.tokens.set(&google.id, "refresh-token-never-shared").unwrap();

        let socket = broker::socket_path(&dir);
        tokio::spawn(broker::serve(socket.clone(), Arc::new(AppBroker { app: app.clone(), notify: None })));
        for _ in 0..50 {
            if socket.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }

        let helper = TokenStore::Brokered { client: broker::Client::new(&dir, false), fallback: None };
        let (icloud_id, google_id) = (icloud.id.clone(), google.id.clone());
        let (pw, has, secret, token, unknown) = tokio::task::spawn_blocking(move || {
            let client = broker::Client::new(&dir, false);
            (
                helper.get(&icloud_id).unwrap(),
                helper.has(&google_id).unwrap(),
                helper.get(&google_id),
                client.call(&Request::AccessToken { account: icloud_id.clone() }).unwrap(),
                helper.get("google:nobody@x.com").unwrap(),
            )
        })
        .await
        .unwrap();
        assert_eq!(pw.as_deref(), Some("app-specific-pw"), "iCloud password is brokered");
        assert!(has);
        assert!(secret.is_err(), "OAuth refresh tokens never leave the app");
        assert_eq!(token.failure, Some(Failure::Denied), "iCloud has no access token");
        assert_eq!(unknown, None);
        std::fs::remove_dir_all(app.data_dir.clone()).ok();
    }

    /// An agent hitting an expired sign-in is reported to the app, so Coo
    /// can ask the person to sign in again instead of looking fine.
    #[tokio::test]
    async fn expired_sign_ins_are_reported_to_the_app() {
        let dir = std::env::temp_dir().join(format!("coo-notice-{}", pigeon_core::auth::random_token(4)));
        std::fs::create_dir_all(&dir).unwrap();
        let app = Arc::new(Pigeon::open_with(dir.clone(), TokenStore::File(dir.join("tokens.json"))).unwrap());
        let icloud = Account {
            id: Account::make_id(ProviderKind::ICloud, "a@icloud.com"),
            provider: ProviderKind::ICloud,
            email: "a@icloud.com".into(),
            display_name: "a".into(),
        };
        app.store().upsert_account(&icloud).unwrap();
        let seen: Arc<std::sync::Mutex<Vec<Notice>>> = Default::default();
        let sink = seen.clone();
        let broker = AppBroker { app: app.clone(), notify: Some(Arc::new(move |n| sink.lock().unwrap().push(n))) };
        // No password stored: the agent is told to sign in again, and so is the app.
        let r = broker.answer(Request::Secret { account: icloud.id.clone() }).await;
        assert_eq!(r.failure, Some(Failure::SignInRequired));
        broker.refused();
        assert_eq!(*seen.lock().unwrap(), vec![Notice::SignInRequired(icloud.id.clone()), Notice::Refused]);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn without_the_app_the_agent_is_told_to_open_coo() {
        let dir = std::env::temp_dir().join(format!("coo-noapp-{}", pigeon_core::auth::random_token(4)));
        std::fs::create_dir_all(&dir).unwrap();
        let helper = TokenStore::Brokered { client: broker::Client::new(&dir, false), fallback: None };
        let err = helper.get("icloud:a@icloud.com").unwrap_err().to_string();
        assert!(err.contains("Open Coo"), "{err}");
        std::fs::remove_dir_all(dir).ok();
    }
}
