//! Google Drive attachments: giving a Google account Drive access
//! (incremental consent for `drive.file`), the browser picker, and fetching
//! picked files at send time. Any account can send them; the Drive access
//! belongs to the Google account the files were picked with.

use pigeon_core::auth::{self, OAuthApp};
use pigeon_core::drive::{DRIVE_FILE_SCOPE, DriveFile, app_id, has_scope};
use pigeon_core::model::{Account, ProviderKind};
use pigeon_core::{Error, Result};
use pigeon_google::drive::{Drive, granted_scopes};
use pigeon_google::picker::{self, PickOutcome, PickerConfig};

use crate::Pigeon;

pub const NOT_IN_BUILD: &str = "Google Drive isn't set up in this build.";

/// Whether this build can open the Drive picker (it needs an API key).
pub fn drive_available() -> bool {
    auth::google_api_key().is_some()
}

impl Pigeon {
    fn google_account(&self, account_id: &str) -> Result<Account> {
        let account = self.account(account_id)?;
        if account.provider != ProviderKind::Google {
            return Err(Error::Other("Google Drive needs a Google account".into()));
        }
        Ok(account)
    }

    fn drive(&self, account_id: &str) -> Result<Drive> {
        let account = self.google_account(account_id)?;
        Ok(Drive::new(self.token_source(&account)?))
    }

    /// Whether the account's sign-in may open picked Drive files. Accounts
    /// that signed in before Coo recorded scopes are asked about once.
    pub async fn drive_ready(&self, account_id: &str) -> Result<bool> {
        let account = self.google_account(account_id)?;
        if let Some(scopes) = self.store().account_scopes(&account.id)? {
            return Ok(has_scope(&scopes, DRIVE_FILE_SCOPE));
        }
        let token = self.token_source(&account)?.access_token().await?;
        let scopes = granted_scopes(&token).await?;
        self.store().set_account_scopes(&account.id, &scopes)?;
        Ok(has_scope(&scopes, DRIVE_FILE_SCOPE))
    }

    /// Ask Google (in the browser) to add Drive access to an existing
    /// Google account, keeping everything it already allows. The sign-in
    /// must be the same account; its new refresh token replaces the old.
    pub async fn connect_drive(&self, account_id: &str, on_url: impl FnOnce(&str)) -> Result<()> {
        let account = self.google_account(account_id)?;
        let app = OAuthApp::from_env(ProviderKind::Google)?.google_add_scopes(&account.email);
        let token = auth::authorize(&app, on_url).await?;
        let (email, _) = pigeon_google::whoami(&token.access_token).await?;
        if !email.eq_ignore_ascii_case(&account.email) {
            return Err(Error::Auth(format!(
                "That was a different Google account. Choose {} to let Coo open its Drive files.",
                account.email
            )));
        }
        let scopes = token.scope.clone().unwrap_or_default();
        if !has_scope(&scopes, DRIVE_FILE_SCOPE) {
            return Err(Error::Auth(
                "Google Drive access wasn't allowed. Try again and tick the Google Drive box.".into(),
            ));
        }
        let refresh =
            token.refresh_token.as_deref().ok_or_else(|| Error::Auth("Google returned no refresh token".into()))?;
        self.tokens.set(&account.id, refresh)?;
        self.store().set_account_scopes(&account.id, &scopes)?;
        // Fresh token source + backend, so nothing keeps using an access
        // token minted before Drive was allowed.
        self.backends.lock().unwrap_or_else(|p| p.into_inner()).remove(&account.id);
        self.sources.lock().unwrap_or_else(|p| p.into_inner()).remove(&account.id);
        Ok(())
    }

    /// Open the Google Picker in the browser for `account_id` and wait for
    /// the user's pick. `nickname` is shown on the page with the address.
    pub async fn drive_pick(
        &self,
        account_id: &str,
        nickname: Option<&str>,
        on_url: impl FnOnce(&str),
    ) -> Result<PickOutcome> {
        let api_key = auth::google_api_key().ok_or_else(|| Error::Other(NOT_IN_BUILD.into()))?;
        let client = OAuthApp::from_env(ProviderKind::Google)?;
        let app_id = app_id(&client.client_id)
            .ok_or_else(|| Error::Other("The Google client id has no project number".into()))?
            .to_string();
        let account = self.google_account(account_id)?;
        let access_token = self.token_source(&account)?.access_token().await?;
        let cfg = PickerConfig {
            access_token,
            api_key,
            app_id,
            email: account.email.clone(),
            nickname: nickname.map(str::trim).filter(|n| !n.is_empty()).map(str::to_string),
        };
        let outcome = picker::pick(&cfg, on_url).await?;
        let PickOutcome::Picked(mut files) = outcome else { return Ok(PickOutcome::Cancelled) };
        self.fill_links(account_id, &mut files).await;
        Ok(PickOutcome::Picked(files))
    }

    /// The picker normally reports each file's link; ask Drive for any
    /// that's missing (best effort: without one the file can't be linked).
    async fn fill_links(&self, account_id: &str, files: &mut [DriveFile]) {
        let Ok(drive) = self.drive(account_id) else { return };
        for f in files.iter_mut().filter(|f| f.url.is_none()) {
            match drive.metadata(&f.id).await {
                Ok(meta) => {
                    f.size_bytes = f.size_bytes.or(meta.size_bytes());
                    f.url = meta.web_view_link;
                }
                Err(e) => tracing::debug!(file = f.id, error = %e, "no Drive link for a picked file"),
            }
        }
    }

    /// A picked file's bytes (or its export), in memory.
    pub async fn drive_bytes(&self, account_id: &str, file_id: &str, export_mime: Option<&str>) -> Result<Vec<u8>> {
        tracing::debug!(file = file_id, export = export_mime.is_some(), "fetching a Drive attachment");
        self.drive(account_id)?.content(file_id, export_mime).await
    }

    /// Let anyone with the link view these files.
    pub async fn drive_share(&self, account_id: &str, file_ids: &[String]) -> Result<()> {
        let drive = self.drive(account_id)?;
        for id in file_ids {
            drive.share_with_anyone(id).await?;
        }
        Ok(())
    }
}
