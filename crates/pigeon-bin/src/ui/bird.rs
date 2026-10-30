//! The Coo pigeon, the app's recurring character: one drawing (the app
//! icon's bird, facing left) in a few poses, for empty states and small
//! moments of delight. Colors come from the theme: the scarf is the accent,
//! beak and feet the warning color, so every palette dresses it.
//!
//! Use `Bird { pose: Pose::Awake }` anywhere; size it with CSS on `.bird`
//! (or `style`), and animate it with `class` (app.css has `bird--*`
//! motions: breathe, startle, peck, shrug). The wing is its own group
//! (`.bird__wing`) and the whole body another (`.bird__body`), so motions
//! can move either. Motion lives in app.css and stops under Reduce Motion.
//! Small tokens-only helpers for other delights live here too: the flag
//! flutter (`FlagIcon`) and the count that pops on its way to zero
//! (`CountBadge`).

use dioxus::prelude::*;

use super::Icon;

/// What the pigeon is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pose {
    /// Eyes closed (inbox zero).
    Sleeping,
    /// Eyes open (empty reader, onboarding).
    Awake,
    /// Leaning to peck at three dots (loading); pair with `bird--peck`.
    Pecking,
    /// Peering through a magnifier (no results).
    Magnifier,
    /// Sitting back, one foot tucked (a free day; the calendar doesn't use
    /// it yet).
    #[allow(dead_code)]
    Lounging,
    /// Asleep in a nightcap (late at night).
    Nightcap,
    /// Beak open, eyes shut (early morning).
    Yawn,
}

impl Pose {
    /// A short name for CSS (`bird--<name>` is set on the svg).
    pub fn name(self) -> &'static str {
        match self {
            Pose::Sleeping => "sleeping",
            Pose::Awake => "awake",
            Pose::Pecking => "pecking",
            Pose::Magnifier => "magnifier",
            Pose::Lounging => "lounging",
            Pose::Nightcap => "nightcap",
            Pose::Yawn => "yawn",
        }
    }
}

const BODY: &str = r##"<path d="M22 9C28 9 30 14 31 20C36 22 48 26 60 33L58 38C50 42 40 46 30 45C20 44 13 38 13 30C13 25 15 22 16 20C14 18 13 15 14 13C15 10 18 9 22 9Z" fill="#EDEEF0" stroke="var(--line-strong)" stroke-width="1"/>"##;
const WING: &str =
    r##"<g class="bird__wing"><path d="M30 26C38 27 48 30 56 35C46 38 36 38 30 34Z" fill="#B9C3CF"/></g>"##;
const SCARF: &str = r##"<path class="bird__scarf" d="M16 20C18 24 22 26 27 25" fill="none" stroke="var(--accent)" stroke-width="3" stroke-linecap="round"/>"##;
const BEAK: &str = r##"<path d="M14 13.8L8 16.4L14 17.6Z" fill="var(--warning)"/>"##;
const BEAK_OPEN: &str = r##"<path d="M14 15L9.4 16.8L14 17Z" fill="#1F3A5C" opacity="0.6"/><path d="M14 12.2L7.4 12.6L14 15Z" fill="var(--warning)"/><path d="M14 17L8.4 20.6L14 18.6Z" fill="var(--warning)"/>"##;
const EYE_OPEN: &str = r##"<circle cx="19.5" cy="14" r="1.5" fill="#1F3A5C"/>"##;
const EYE_SHUT: &str = r##"<path d="M17.6 14.4Q19.5 16 21.4 14.4" fill="none" stroke="#1F3A5C" stroke-width="1.2" stroke-linecap="round"/>"##;
const FEET: &str =
    r##"<path d="M28 45V54M35 45V54" stroke="var(--warning)" stroke-width="2.4" stroke-linecap="round"/>"##;
const FEET_TUCKED: &str =
    r##"<path d="M28 45V54M35 45L39 49" stroke="var(--warning)" stroke-width="2.4" stroke-linecap="round"/>"##;
const MAGNIFIER: &str = r##"<g class="bird__lens"><circle cx="16" cy="13.5" r="6.5" fill="color-mix(in srgb, var(--accent) 14%, transparent)" stroke="var(--text-2)" stroke-width="1.7"/><path d="M11.4 18.2L5 25" stroke="var(--text-2)" stroke-width="2.4" stroke-linecap="round"/></g>"##;
const NIGHTCAP: &str = r##"<path d="M13 12.5C14.5 6 23 3.5 28.5 7.5C32 10 35 12.5 37.5 16C33.5 14.5 30.5 13.2 27.5 12Z" fill="color-mix(in srgb, var(--accent) 70%, #ffffff)" stroke="color-mix(in srgb, var(--accent) 80%, #000000)" stroke-width="0.8"/><path d="M13 12.5C17 10.8 23 10.6 27.5 12" fill="none" stroke="#FFFFFF" stroke-width="1.6" stroke-linecap="round"/><circle cx="38.2" cy="16.6" r="2.2" fill="#FFFFFF" stroke="color-mix(in srgb, var(--accent) 80%, #000000)" stroke-width="0.8"/>"##;
const DOTS: &str = r##"<g class="bird__crumbs"><circle cx="-6" cy="53" r="1.6" fill="var(--text-3)"/><circle cx="0" cy="53" r="1.6" fill="var(--text-3)"/><circle cx="6" cy="53" r="1.6" fill="var(--text-3)"/></g>"##;

/// The drawing for `pose`, and the viewBox that fits it.
fn drawing(pose: Pose) -> (&'static str, String) {
    let parts: Vec<&str> = match pose {
        Pose::Sleeping => vec![BODY, WING, SCARF, BEAK, EYE_SHUT, FEET],
        Pose::Awake => vec![BODY, WING, SCARF, BEAK, EYE_OPEN, FEET],
        Pose::Pecking => vec![BODY, WING, SCARF, BEAK, EYE_OPEN, FEET],
        Pose::Magnifier => vec![BODY, WING, SCARF, BEAK, EYE_OPEN, FEET, MAGNIFIER],
        Pose::Lounging => vec![BODY, WING, SCARF, BEAK, EYE_SHUT, FEET_TUCKED],
        Pose::Nightcap => vec![BODY, WING, SCARF, BEAK, EYE_SHUT, NIGHTCAP, FEET],
        Pose::Yawn => vec![BODY, WING, SCARF, BEAK_OPEN, EYE_SHUT, FEET],
    };
    let body = format!("<g class=\"bird__body\">{}</g>", parts.concat());
    match pose {
        // Room on the left for the crumbs it pecks at (outside the body
        // group, so they stay put while it bobs).
        Pose::Pecking => ("-10 4 74 56", format!("{body}{DOTS}")),
        Pose::Magnifier => ("-2 4 66 56", body),
        _ => ("4 4 60 56", body),
    }
}

/// The pigeon. `class` adds motion (`bird--breathe`, `bird--startle`,
/// `bird--peck`, `bird--shrug`); `style` sizes it where CSS doesn't.
#[component]
pub fn Bird(pose: Pose, #[props(default)] class: Option<String>, #[props(default)] style: Option<String>) -> Element {
    let (view_box, inner) = drawing(pose);
    let class = format!("bird bird--{} {}", pose.name(), class.unwrap_or_default());
    rsx! {
        svg { class: "{class}", view_box: "{view_box}", style: style.unwrap_or_default(), "aria-hidden": "true",
            dangerous_inner_html: inner,
        }
    }
}

/// How long the one-off motions below run (app.css matches).
const FLUTTER_MS: u64 = 700;
const POP_MS: u64 = 900;

/// The flag icon, which waves once when the message gets flagged and
/// lowers itself when unflagged (only on the change, never for a flag that
/// was already up when it appeared). Renders nothing while unflagged,
/// unless `always` (a toolbar button's icon).
#[component]
pub fn FlagIcon(
    flagged: bool,
    #[props(default = "row__flag")] class: &'static str,
    #[props(default)] always: bool,
) -> Element {
    let mut anim = use_signal(|| None::<&'static str>);
    let mut prev = use_signal(|| flagged);
    let mut epoch = use_signal(|| 0u64);
    use_effect(use_reactive!(|flagged| {
        if *prev.peek() == flagged {
            return;
        }
        prev.set(flagged);
        anim.set(Some(if flagged { "up" } else { "down" }));
        let e = *epoch.peek() + 1;
        epoch.set(e);
        spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(FLUTTER_MS)).await;
            if *epoch.peek() == e {
                anim.set(None);
            }
        });
    }));
    let a = *anim.read();
    if !flagged && !always && a != Some("down") {
        return rsx! {};
    }
    rsx! {
        span { class: "flag-wave", "data-flag": a,
            Icon { name: "flag", class: class }
        }
    }
}

/// An unread count. When it drops from something to zero, the old number
/// pops away and a small feather floats up instead of just vanishing.
#[component]
pub fn CountBadge(count: u32, #[props(default = "count")] class: &'static str) -> Element {
    let mut prev = use_signal(|| count);
    let mut popping = use_signal(|| None::<u32>);
    let mut epoch = use_signal(|| 0u64);
    use_effect(use_reactive!(|count| {
        let was = *prev.peek();
        if was == count {
            return;
        }
        prev.set(count);
        if count == 0 && was > 0 {
            popping.set(Some(was));
            let e = *epoch.peek() + 1;
            epoch.set(e);
            spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(POP_MS)).await;
                if *epoch.peek() == e {
                    popping.set(None);
                }
            });
        } else if count > 0 {
            popping.set(None);
        }
    }));
    if count > 0 {
        return rsx! { span { class: class, "{count}" } };
    }
    match *popping.read() {
        Some(n) => rsx! {
            span { class: "{class} count--zero", "aria-hidden": "true",
                "{n}"
                svg { class: "count__feather", view_box: "0 0 44 44",
                    dangerous_inner_html: FEATHER,
                }
            }
        },
        None => rsx! {},
    }
}

/// A small feather (empty Trash, a count reaching zero).
pub const FEATHER: &str = r##"<path d="M8 38C14 26 22 14 36 6C34 18 26 30 12 36Z" fill="#EDEEF0" stroke="#B9C3CF" stroke-width="1.4"/><path d="M8 38L30 12" stroke="#B9C3CF" stroke-width="1.2"/>"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pose_draws_the_whole_bird() {
        for pose in
            [Pose::Sleeping, Pose::Awake, Pose::Pecking, Pose::Magnifier, Pose::Lounging, Pose::Nightcap, Pose::Yawn]
        {
            let (vb, inner) = drawing(pose);
            assert_eq!(vb.split(' ').count(), 4, "{pose:?}");
            assert!(
                inner.contains("bird__body") && inner.contains("bird__wing") && inner.contains("var(--accent)"),
                "{pose:?}"
            );
        }
        assert!(drawing(Pose::Pecking).1.contains("bird__crumbs"));
        assert!(drawing(Pose::Nightcap).1.contains("circle"));
    }
}
