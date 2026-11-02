//! The composer's formatting toolbar. Markup only: the bridge's rich text
//! script (`richtext`) runs the buttons by their `data-fmt` command, keeps
//! the editor's selection while they're clicked, and sets `aria-pressed`
//! from the caret (bold on in bold text, and so on).

use dioxus::prelude::*;

use super::Icon;
use crate::richtext::tip;

/// The buttons in groups (name, [(command, icon, label)]): history, text,
/// color, paragraph, insert, clear. `size`, `color`, `highlight`, `align`,
/// `link` and `emoji` open a popover.
/// (command, icon, label).
type Button = (&'static str, &'static str, &'static str);

const GROUPS: &[(&str, &[Button])] = &[
    ("History", &[("undo", "undo", "Undo"), ("redo", "redo", "Redo")]),
    (
        "Text",
        &[
            ("size", "fmt-size", "Text size"),
            ("bold", "fmt-bold", "Bold"),
            ("italic", "fmt-italic", "Italic"),
            ("underline", "fmt-underline", "Underline"),
            ("strikeThrough", "fmt-strike", "Strikethrough"),
        ],
    ),
    ("Color", &[("color", "fmt-color", "Text color"), ("highlight", "fmt-highlight", "Highlight")]),
    (
        "Paragraph",
        &[
            ("insertUnorderedList", "list", "Bulleted list"),
            ("insertOrderedList", "list-ordered", "Numbered list"),
            ("outdent", "outdent", "Decrease indent"),
            ("indent", "indent", "Increase indent"),
            ("align", "align-left", "Paragraph: alignment, direction, spacing"),
            ("dirLtr", "dir-ltr", "Left to right"),
            ("dirRtl", "dir-rtl", "Right to left"),
        ],
    ),
    (
        "Insert",
        &[
            ("quote", "quote", "Quote"),
            ("code", "code", "Inline code"),
            ("codeBlock", "code-block", "Code block"),
            ("link", "link", "Link"),
            ("emoji", "emoji", "Emoji"),
        ],
    ),
    ("Clear", &[("removeFormat", "fmt-clear", "Clear formatting")]),
];

#[component]
pub fn FormatBar(editor: String) -> Element {
    rsx! {
        div { class: "fmt-bar", role: "toolbar", "aria-label": "Formatting", "data-for": "{editor}",
            for (name, buttons) in GROUPS.iter() {
                div { class: "fmt-bar__group", role: "group", "aria-label": *name, key: "{name}", "data-group": *name,
                    for (cmd, icon, label) in buttons.iter() {
                        {
                            let popup = matches!(*cmd, "size" | "color" | "highlight" | "align" | "link" | "emoji");
                            let title = match *cmd {
                                "size" => "Text size (⇧⌘> / ⇧⌘<)".to_string(),
                                "emoji" => "Emoji (all of them: ⌃⌘Space)".to_string(),
                                "dirLtr" => "Left to right paragraph (⌃ + left ⇧)".to_string(),
                                "dirRtl" => "Right to left paragraph (⌃ + right ⇧)".to_string(),
                                "color" | "highlight" | "align" => label.to_string(),
                                _ => tip(cmd),
                            };
                            rsx! {
                                button { class: "icon-btn icon-btn--sm", r#type: "button", key: "{cmd}",
                                    "data-fmt": *cmd, title: "{title}", "aria-label": *label,
                                    "aria-haspopup": popup.then_some("true"),
                                    // 18 px like the reader toolbar's: a 24-unit icon drawn at
                                    // 14 px blurred on 1x displays.
                                    Icon { name: *icon, class: "icon--lg" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
