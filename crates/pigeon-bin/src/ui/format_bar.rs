//! The composer's formatting toolbar. Markup only: the bridge's rich text
//! script (`richtext`) runs the buttons by their `data-fmt` command, keeps
//! the editor's selection while they're clicked, and sets `aria-pressed`
//! from the caret (bold on in bold text, and so on).

use dioxus::prelude::*;

use super::Icon;
use crate::richtext::tip;

/// (command, icon, label). `None` is a separator. `size`, `color`,
/// `highlight`, `align`, `link` and `emoji` open a popover.
const BUTTONS: &[Option<(&str, &str, &str)>] = &[
    Some(("undo", "undo", "Undo")),
    Some(("redo", "redo", "Redo")),
    None,
    Some(("size", "fmt-size", "Text size")),
    Some(("bold", "fmt-bold", "Bold")),
    Some(("italic", "fmt-italic", "Italic")),
    Some(("underline", "fmt-underline", "Underline")),
    Some(("strikeThrough", "fmt-strike", "Strikethrough")),
    Some(("color", "fmt-color", "Text color")),
    Some(("highlight", "fmt-highlight", "Highlight")),
    None,
    Some(("insertUnorderedList", "list", "Bulleted list")),
    Some(("insertOrderedList", "list-ordered", "Numbered list")),
    Some(("outdent", "outdent", "Decrease indent")),
    Some(("indent", "indent", "Increase indent")),
    Some(("align", "align-left", "Paragraph: alignment, direction, spacing")),
    None,
    Some(("quote", "quote", "Quote")),
    Some(("code", "code", "Inline code")),
    Some(("codeBlock", "code-block", "Code block")),
    Some(("link", "link", "Link")),
    Some(("emoji", "emoji", "Emoji")),
    Some(("removeFormat", "fmt-clear", "Clear formatting")),
];

#[component]
pub fn FormatBar(editor: String) -> Element {
    rsx! {
        div { class: "fmt-bar", role: "toolbar", "aria-label": "Formatting", "data-for": "{editor}",
            for (i, b) in BUTTONS.iter().enumerate() {
                match b {
                    None => rsx! { span { class: "fmt-bar__sep", key: "{i}" } },
                    Some((cmd, icon, label)) => {
                        let popup = matches!(*cmd, "size" | "color" | "highlight" | "align" | "link" | "emoji");
                        let title = if *cmd == "size" { "Text size (⇧⌘> / ⇧⌘<)".to_string() }
                            else if *cmd == "emoji" { "Emoji (all of them: ⌃⌘Space)".to_string() }
                            else if matches!(*cmd, "color" | "highlight" | "align") { label.to_string() }
                            else { tip(cmd) };
                        rsx! {
                            button { class: "icon-btn icon-btn--sm", r#type: "button", key: "{i}",
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
