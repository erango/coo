//! "Add account": one sheet for every provider. The user types an address,
//! the engine works out who handles its mail (`Cmd::DetectProvider`), and
//! the sheet offers that sign-in: the browser for Google and Microsoft, an
//! app-specific password for iCloud. Opened from Settings › Accounts, the
//! sidebar, ⌘K, and an iCloud account's "Sign in again"; onboarding embeds
//! the same flow (`use_flow` + `FlowView`).

use dioxus::prelude::*;
use pigeon_core::model::ProviderKind;
use pigeon_core::provider_detect::{self, Detection, provider_label};
use pigeon_engine::service::Cmd;

use super::Icon;
use crate::send_cmd;
use crate::state::{AppState, next_token};

/// Where the flow is.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Step {
    /// The address field, Continue, and the three providers.
    #[default]
    Start,
    /// Waiting for `Delta::ProviderDetected` with this token.
    Checking(u64),
    Detected(Detection),
    /// Pick a provider: Coo couldn't tell, or "Not Google?".
    Choose,
    /// The app-specific password form (ui/icloud.rs).
    ICloud,
}

/// What the sheet opens on.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Opening {
    pub email: String,
    pub step: Step,
    /// Press Continue straight away (debug hook).
    pub auto_continue: bool,
    /// Show the browser wait without starting a sign-in (debug hook).
    pub waiting: Option<ProviderKind>,
}

/// Open the sheet on its start step, `email` prefilled.
pub fn open(s: AppState, email: &str) {
    let mut slot = s.add_account;
    slot.set(Some(Opening { email: email.to_string(), ..Default::default() }));
}

/// Open the sheet on the iCloud password step (an iCloud account's
/// "Sign in again").
pub fn open_icloud(s: AppState, email: &str) {
    let mut slot = s.add_account;
    slot.set(Some(Opening { email: email.to_string(), step: Step::ICloud, ..Default::default() }));
}

/// Debug builds: `PIGEON_DEBUG_OPEN=add-account:<arg>`, where `<arg>` is
/// empty (start), `<email>` (continue with it), `choose|<email>`,
/// `icloud|<email>`, or `waiting|google|<email>`.
pub fn debug_opening(arg: &str) -> Opening {
    let parts: Vec<&str> = arg.split('|').collect();
    let email = |s: &str| s.to_string();
    match parts[..] {
        ["choose", e] => Opening { email: email(e), step: Step::Choose, ..Default::default() },
        ["icloud", e] => Opening { email: email(e), step: Step::ICloud, ..Default::default() },
        ["waiting", p, e] => Opening {
            email: email(e),
            waiting: ProviderKind::parse(p).or(Some(ProviderKind::Google)),
            ..Default::default()
        },
        _ => Opening { email: email(arg), auto_continue: !arg.is_empty(), ..Default::default() },
    }
}

/// The step a detection leads to.
pub fn step_for(d: Detection) -> Step {
    if d.host.provider().is_some() { Step::Detected(d) } else { Step::Choose }
}

/// The detected card's line: "Mail for acme.co.il is handled by Google."
pub fn detected_line(d: &Detection) -> String {
    let who = d.host.provider().map(provider_label).unwrap_or("another provider");
    format!("Mail for {} is handled by {who}.", d.domain)
}

/// The problem with an address typed on the start step, if any.
pub fn email_problem(email: &str) -> Option<&'static str> {
    if provider_detect::domain_of(email).is_some() {
        None
    } else {
        Some("Enter your full address, like name@example.com")
    }
}

/// "Continue" on a typed address: answered locally for the consumer domains
/// Coo knows (no "Checking…" flash), else the engine is asked.
fn known_step(email: &str) -> Option<Step> {
    let domain = provider_detect::domain_of(email)?;
    let host = provider_detect::by_domain(&domain)?;
    Some(step_for(Detection { domain, host }))
}

/// The flow's state, shared by the sheet and onboarding. Copy: signals.
#[derive(Clone, Copy)]
pub struct Flow {
    s: AppState,
    pub email: Signal<String>,
    pub step: Signal<Step>,
    /// A sign-in under way: the browser (Google, Microsoft) or the iCloud
    /// password check.
    pub waiting: Signal<Option<ProviderKind>>,
    /// The last sign-in's error, shown in the sheet.
    pub error: Signal<Option<String>>,
    /// Typed address checked (show its problem).
    pub touched: Signal<bool>,
    /// The sign-in in progress: (first toast id that can be its error,
    /// `signed_in` count before it).
    attempt: Signal<Option<(u64, u64)>>,
}

/// The flow's signals and the effects that move it along: detection
/// answers, and a finished or failed sign-in (`on_done` on success, after
/// which the flow starts over).
pub fn use_flow(opening: Opening, on_done: Callback<()>) -> Flow {
    let s = use_context::<AppState>();
    let opening = use_hook(|| opening);
    let mut flow = Flow {
        s,
        email: use_signal(|| opening.email.clone()),
        step: use_signal(|| opening.step.clone()),
        waiting: use_signal(|| opening.waiting),
        error: use_signal(|| None),
        touched: use_signal(|| false),
        attempt: use_signal(|| None),
    };
    use_hook(move || {
        if opening.auto_continue {
            flow.submit_email();
        }
    });
    // The engine's answer for the address being checked.
    use_effect(move || {
        let Some((token, result)) = s.detected.read().clone() else { return };
        if *flow.step.peek() == Step::Checking(token) {
            flow.step.set(step_for(result));
        }
    });
    // A sign-in finished (close / start over) or failed (say why).
    use_effect(move || {
        let done = *s.signed_in.read();
        let toasts = s.toasts.read();
        let Some((first_toast, before)) = *flow.attempt.peek() else { return };
        if done > before {
            flow.attempt.set(None);
            flow.waiting.set(None);
            flow.reset();
            on_done.call(());
        } else if let Some(t) = toasts.iter().rev().find(|t| t.error && t.id >= first_toast) {
            flow.error.set(Some(t.message.clone()));
            flow.attempt.set(None);
            flow.waiting.set(None);
        }
    });
    flow
}

// Props equality: the same signals are the same flow.
impl PartialEq for Flow {
    fn eq(&self, other: &Self) -> bool {
        self.email == other.email && self.step == other.step && self.attempt == other.attempt
    }
}

impl Flow {
    /// Back to an empty start step.
    pub fn reset(mut self) {
        self.email.set(String::new());
        self.step.set(Step::Start);
        self.touched.set(false);
        self.error.set(None);
    }

    /// The typed address, when it's a full one (the login hint).
    fn hint(&self) -> Option<String> {
        let e = self.email.peek().trim().to_string();
        provider_detect::domain_of(&e).map(|_| e)
    }

    /// Continue on the start step: detect the provider.
    pub fn submit_email(mut self) {
        self.touched.set(true);
        let email = self.email.peek().trim().to_string();
        if email_problem(&email).is_some() {
            return;
        }
        self.error.set(None);
        if let Some(step) = known_step(&email) {
            return self.step.set(step);
        }
        let token = next_token();
        self.step.set(Step::Checking(token));
        send_cmd(Cmd::DetectProvider { email, token });
    }

    /// Enter / the primary button, whatever the step.
    pub fn submit(self) {
        match self.step.peek().clone() {
            Step::Start => self.submit_email(),
            Step::Detected(d) => {
                if let Some(p) = d.host.provider() {
                    self.sign_in(p);
                }
            }
            // The iCloud form and the provider list have their own buttons.
            Step::Checking(_) | Step::Choose | Step::ICloud => {}
        }
    }

    /// Start a sign-in: the browser for Google / Microsoft, the password
    /// step for iCloud.
    pub fn sign_in(mut self, p: ProviderKind) {
        self.error.set(None);
        if p == ProviderKind::ICloud {
            return self.step.set(Step::ICloud);
        }
        if self.waiting.peek().is_some() {
            return;
        }
        self.begin_attempt(p);
        let mut si = self.s.signing_in;
        si.set(true);
        send_cmd(Cmd::Login { provider: p, login_hint: self.hint() });
    }

    /// Mark a sign-in as under way (errors toasted after now are its own).
    pub fn begin_attempt(mut self, p: ProviderKind) {
        let next_toast = self.s.toasts.peek().iter().map(|t| t.id + 1).max().unwrap_or(0);
        self.attempt.set(Some((next_toast, *self.s.signed_in.peek())));
        self.waiting.set(Some(p));
    }

    /// Stop waiting for the browser (the sign-in may still finish later).
    pub fn cancel_wait(mut self) {
        self.attempt.set(None);
        self.waiting.set(None);
        let mut si = self.s.signing_in;
        si.set(false);
    }

    /// Back to the start step, keeping the address.
    pub fn back(mut self) {
        self.error.set(None);
        self.step.set(Step::Start);
    }

    /// Waiting on the browser (not the iCloud check).
    pub fn browser_wait(&self) -> Option<ProviderKind> {
        self.waiting.read().filter(|p| *p != ProviderKind::ICloud)
    }
}

/// The sheet over the app (`s.add_account` is Some).
#[component]
pub fn AddAccountSheet() -> Element {
    let s = use_context::<AppState>();
    let mut slot = s.add_account;
    let opening = slot.peek().clone().unwrap_or_default();
    let close = use_callback(move |()| slot.set(None));
    let flow = use_flow(opening, close);
    let mut close_now = move || {
        flow.cancel_wait();
        slot.set(None);
    };
    rsx! {
        div { class: "update-scrim add-account-scrim", onclick: move |_| close_now(),
            FlowView { flow, embedded: false, on_close: move |_| close_now() }
        }
    }
}

/// A provider's logo at `px` pixels.
#[component]
fn Logo(kind: ProviderKind, #[props(default = 16)] px: u32) -> Element {
    rsx! {
        span { class: "provider-logo", style: "--logo:{px}px", "aria-hidden": "true",
            dangerous_inner_html: super::sidebar::provider_svg(kind) }
    }
}

/// Focus an element once it's on screen.
async fn focus(e: MountedEvent) {
    let _ = e.set_focus(true).await;
}

/// The flow's content: the sheet's dialog, or `embedded` in onboarding
/// (no title, no close; onboarding shows the browser wait itself).
#[component]
pub fn FlowView(flow: Flow, embedded: bool, #[props(default)] on_close: EventHandler<()>) -> Element {
    let step = flow.step.read().clone();
    let waiting = flow.browser_wait();
    let class = if embedded { "add-account add-account--embedded" } else { "dialog add-account" };
    let label = if step == Step::ICloud { "Add an iCloud account" } else { "Add an account" };
    rsx! {
        form { class, role: if embedded { "group" } else { "dialog" }, "aria-label": label,
            onclick: move |e| e.stop_propagation(),
            onsubmit: move |e| { e.prevent_default(); flow.submit(); },
            onkeydown: move |e| if e.key() == Key::Escape && !embedded { e.prevent_default(); on_close.call(()) },
            if step == Step::ICloud {
                super::icloud::ICloudStep { flow }
            } else if let (Some(p), false) = (waiting, embedded) {
                BrowserWait { flow, provider: p }
            } else {
                AddressSteps { flow, embedded, step }
            }
        }
    }
}

/// "Finish signing in in your browser".
#[component]
fn BrowserWait(flow: Flow, provider: ProviderKind) -> Element {
    let name = provider_label(provider);
    rsx! {
        div { class: "add-account__wait",
            span { class: "add-account__wait-icon", Icon { name: "browser" } }
            div { class: "add-account__title", "Finish signing in in your browser" }
            p { class: "add-account__sub", "Coo opened {name}'s sign-in page. Come back here when it says you're done." }
            if provider == ProviderKind::Google {
                p { class: "add-account__hint", "If Google says it hasn't verified Coo, click Advanced, then Go to Coo." }
            }
            button { class: "btn btn--secondary", r#type: "button", onmounted: focus,
                onclick: move |_| flow.cancel_wait(), "Cancel" }
        }
    }
}

/// Start, Checking, Detected and Choose: the address field and what follows.
#[component]
fn AddressSteps(flow: Flow, embedded: bool, step: Step) -> Element {
    let mut email = flow.email;
    let mut step_w = flow.step;
    let mut error_w = flow.error;
    let problem = (*flow.touched.read()).then(|| email_problem(&email.read())).flatten();
    let domain = provider_detect::domain_of(&email.read()).unwrap_or_else(|| "this address".into());
    let error = flow.error.read().clone();
    rsx! {
        if !embedded {
            div { class: "add-account__head",
                h1 { class: "add-account__title", "Add an account" }
                if step != Step::Choose {
                    p { class: "add-account__sub", "Coo works with Google, Microsoft 365 and iCloud mail and calendars." }
                }
            }
        }
        label { class: "add-account__field",
            "Email address"
            input { class: "input", r#type: "email", autocomplete: "email", spellcheck: "false",
                placeholder: "name@example.com", value: "{email}", dir: "auto",
                "aria-invalid": "{problem.is_some()}",
                onmounted: move |e| async move { if *flow.step.peek() == Step::Start { focus(e).await } },
                oninput: move |e| {
                    email.set(e.value());
                    error_w.set(None);
                    if *step_w.peek() != Step::Start { step_w.set(Step::Start); }
                },
            }
            if let Some(p) = problem { span { class: "field__error", "{p}" } }
        }
        match step.clone() {
            Step::Start => rsx! {
                button { class: "btn btn--primary add-account__primary", r#type: "submit", "Continue" }
                div { class: "add-account__divider", span {} "or choose" span {} }
                div { class: "add-account__quick",
                    for p in [ProviderKind::Google, ProviderKind::Microsoft, ProviderKind::ICloud] {
                        button { class: "btn btn--secondary", r#type: "button", key: "{p.as_str()}",
                            onclick: move |_| flow.sign_in(p),
                            Logo { kind: p } "{provider_label(p)}"
                        }
                    }
                }
            },
            Step::Checking(_) => rsx! {
                button { class: "btn btn--primary add-account__primary", r#type: "button", disabled: true, "aria-busy": "true",
                    span { class: "add-account__spin", Icon { name: "sync", class: "icon--sm" } }
                    "Checking…"
                }
            },
            Step::Detected(d) => rsx! { Detected { flow, d } },
            Step::Choose => rsx! { Choose { flow, domain } },
            Step::ICloud => rsx! {},
        }
        if let Some(err) = error {
            div { class: "banner banner--error add-account__error", role: "alert",
                Icon { name: "alert", class: "icon--sm" } span { dir: "auto", "{err}" } }
        }
    }
}

#[component]
fn Detected(flow: Flow, d: Detection) -> Element {
    let Some(p) = d.host.provider() else { return rsx! {} };
    let name = provider_label(p);
    let line = detected_line(&d);
    let mut step = flow.step;
    let hint = match p {
        ProviderKind::ICloud => "iCloud uses an app-specific password you make at account.apple.com.".to_string(),
        _ => format!("Your browser opens {name}'s sign-in. Coo never sees your password."),
    };
    rsx! {
        div { class: "add-account__card", "data-provider": "{p.as_str()}",
            Logo { kind: p, px: 26 }
            div { class: "add-account__card-text",
                span { class: "add-account__card-name", "{d.host.name()}" }
                span { class: "add-account__card-line", "{line}" }
            }
        }
        button { class: "btn btn--primary add-account__primary", r#type: "submit", onmounted: focus,
            "Continue with {name}"
            if p != ProviderKind::ICloud { Icon { name: "popout", class: "icon--sm" } }
        }
        p { class: "add-account__hint",
            "{hint} "
            button { class: "link-btn", r#type: "button", onclick: move |_| step.set(Step::Choose),
                "Not {name}? Choose another provider" }
        }
        if d.host.may_be_apple_id() {
            p { class: "add-account__hint",
                button { class: "link-btn", r#type: "button", onclick: move |_| flow.sign_in(ProviderKind::ICloud),
                    "Signing in to iCloud with this Apple ID?" }
            }
        }
    }
}

#[component]
fn Choose(flow: Flow, domain: String) -> Element {
    let choices = [
        (ProviderKind::Google, "Google", "Gmail or Google Workspace"),
        (ProviderKind::Microsoft, "Microsoft", "Microsoft 365, Outlook.com"),
        (ProviderKind::ICloud, "iCloud", "Including custom domains on iCloud+"),
    ];
    rsx! {
        div { class: "add-account__question",
            span { class: "add-account__question-title", "Who handles mail for {domain}?" }
            span { class: "add-account__sub", "Coo couldn't tell from the domain. Pick where you sign in:" }
        }
        div { class: "add-account__choices",
            for (i, (p, name, sub)) in choices.into_iter().enumerate() {
                button { class: "provider-choice add-account__choice", r#type: "button", key: "{name}",
                    onmounted: move |e| async move { if i == 0 { focus(e).await } },
                    onclick: move |_| flow.sign_in(p),
                    Logo { kind: p, px: 18 }
                    span { class: "provider-choice__text",
                        span { class: "provider-choice__name", "{name}" }
                        span { class: "provider-choice__sub", "{sub}" }
                    }
                    Icon { name: "chev-right", class: "icon--sm" }
                }
            }
        }
        div { class: "add-account__note",
            b { "Typed a Gmail or other address you use as your Apple ID? " }
            "Coo picks Google for gmail.com; choose iCloud here instead, and Coo will ask for your @icloud.com address so you can send from it."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pigeon_core::provider_detect::MailHost;

    fn det(domain: &str, host: MailHost) -> Detection {
        Detection { domain: domain.into(), host }
    }

    #[test]
    fn detection_picks_the_step() {
        let d = det("acme.co.il", MailHost::GoogleWorkspace);
        assert_eq!(step_for(d.clone()), Step::Detected(d));
        assert_eq!(step_for(det("frostbyte.dev", MailHost::Unknown)), Step::Choose);
    }

    #[test]
    fn detected_card_names_the_company() {
        assert_eq!(
            detected_line(&det("acme.co.il", MailHost::GoogleWorkspace)),
            "Mail for acme.co.il is handled by Google."
        );
        assert_eq!(
            detected_line(&det("halyard.co", MailHost::Microsoft365)),
            "Mail for halyard.co is handled by Microsoft."
        );
        assert_eq!(
            detected_line(&det("ortiz.family", MailHost::ICloudCustomDomain)),
            "Mail for ortiz.family is handled by iCloud."
        );
    }

    #[test]
    fn consumer_domains_skip_the_lookup() {
        assert_eq!(known_step("me@gmail.com"), Some(Step::Detected(det("gmail.com", MailHost::Gmail))));
        assert_eq!(known_step("me@icloud.com"), Some(Step::Detected(det("icloud.com", MailHost::ICloud))));
        assert_eq!(known_step("dana@acme.co.il"), None);
        assert_eq!(known_step("dana"), None);
    }

    #[test]
    fn start_needs_a_full_address() {
        assert!(email_problem("dana@acme.co.il").is_none());
        assert!(email_problem("dana@acme").is_some());
        assert!(email_problem("").is_some());
    }

    #[test]
    fn debug_hook_opens_each_state() {
        assert_eq!(debug_opening(""), Opening::default());
        let o = debug_opening("dana@acme.co.il");
        assert!(o.auto_continue && o.email == "dana@acme.co.il" && o.step == Step::Start);
        assert_eq!(debug_opening("choose|eran@frostbyte.dev").step, Step::Choose);
        assert_eq!(debug_opening("icloud|sam@icloud.com").step, Step::ICloud);
        assert_eq!(debug_opening("waiting|microsoft|a@b.co").waiting, Some(ProviderKind::Microsoft));
    }
}
