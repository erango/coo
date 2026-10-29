//! Dragging an attachment out of Coo onto Finder (a folder, the desktop)
//! saves it there. The web view can't hand a file to Finder, so the bridge
//! reports the drag and this starts a native one carrying a file promise:
//! Finder asks for the file on drop, and the engine downloads it straight
//! to the place it names.
//!
//! Messages drag the same way (`start_mail`): each is a promised .eml file,
//! so Finder gets the original. Inside Coo the drag isn't the web view's,
//! so the source follows the pointer itself: it has the page highlight the
//! folder or composer under it, and on release over the window, drop there
//! (`__pigeonMailDragAt` / `__pigeonMailDropAt` in the bridge).

use serde::Deserialize;

/// What the bridge sends when an attachment chip is dragged.
#[derive(Debug, Clone, Deserialize)]
pub struct Drag {
    pub account_id: String,
    pub message_id: String,
    pub attachment_id: String,
    pub name: String,
}

/// A message dragged out as a .eml file.
#[derive(Debug, Clone)]
pub struct MailFile {
    pub account_id: String,
    pub message_id: String,
    pub subject: String,
}

/// JS for the main window's page, from the drag source (main thread).
pub fn main_js() -> &'static tokio::sync::broadcast::Sender<String> {
    static TX: std::sync::OnceLock<tokio::sync::broadcast::Sender<String>> = std::sync::OnceLock::new();
    TX.get_or_init(|| tokio::sync::broadcast::channel(64).0)
}

/// In the main window: run the drag source's JS there.
pub fn use_main_js() {
    use dioxus::prelude::*;
    use_hook(|| {
        spawn(async {
            let mut rx = main_js().subscribe();
            loop {
                match rx.recv().await {
                    Ok(js) => {
                        document::eval(&js);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        })
    });
}

/// Start a native drag of messages as .eml files (from a list row).
pub fn start_mail(files: Vec<MailFile>) {
    if !files.is_empty() {
        mac::start_mail(files);
    }
}

/// Start the native drag for the bridge's `attach-drag` message (JSON).
pub fn start(json: &str) {
    match serde_json::from_str::<Drag>(json) {
        Ok(d) => mac::start(d),
        Err(e) => tracing::warn!(error = %e, "bad attachment drag"),
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::cell::RefCell;
    use std::sync::OnceLock;
    use std::time::Duration;

    use objc2::rc::Retained;
    use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
    use objc2_app_kit::{
        NSApplication, NSDragOperation, NSDraggingContext, NSDraggingItem, NSDraggingSession, NSDraggingSource,
        NSEvent, NSEventModifierFlags, NSEventType, NSFilePromiseProvider, NSFilePromiseProviderDelegate, NSWindow,
        NSWorkspace,
    };
    use objc2_foundation::{NSArray, NSError, NSOperationQueue, NSPoint, NSRect, NSSize, NSString, NSURL};
    use pigeon_engine::service::Cmd;

    use super::{Drag, MailFile};

    /// How long Finder's drop may wait on the download.
    const WRITE_TIMEOUT: Duration = Duration::from_secs(300);

    define_class!(
        // Writes the promised file: off the main thread, on `queue()`.
        #[unsafe(super(NSObject))]
        #[name = "CooAttachmentPromise"]
        #[ivars = Drag]
        struct Promise;

        unsafe impl NSObjectProtocol for Promise {}

        unsafe impl NSFilePromiseProviderDelegate for Promise {
            #[unsafe(method_id(filePromiseProvider:fileNameForType:))]
            fn file_name(&self, _p: &NSFilePromiseProvider, _t: &NSString) -> Retained<NSString> {
                NSString::from_str(&safe_name(&self.ivars().name))
            }

            #[unsafe(method(filePromiseProvider:writePromiseToURL:completionHandler:))]
            fn write(&self, _p: &NSFilePromiseProvider, url: &NSURL, done: &block2::DynBlock<dyn Fn(*mut NSError)>) {
                let ok = url.path().is_some_and(|path| save(self.ivars(), std::path::Path::new(&path.to_string())));
                if ok {
                    done.call((std::ptr::null_mut(),));
                } else {
                    let error = NSError::new(-1, &NSString::from_str("CooAttachment"));
                    done.call((Retained::as_ptr(&error).cast_mut(),));
                }
            }

            #[unsafe(method_id(operationQueueForFilePromiseProvider:))]
            fn queue(&self, _p: &NSFilePromiseProvider) -> Retained<NSOperationQueue> {
                background_queue()
            }
        }
    );

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "CooAttachmentDragSource"]
        struct Source;

        unsafe impl NSObjectProtocol for Source {}

        unsafe impl NSDraggingSource for Source {
            #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
            fn mask(&self, _s: &NSDraggingSession, context: NSDraggingContext) -> NSDragOperation {
                if context == NSDraggingContext::OutsideApplication {
                    NSDragOperation::Copy
                } else {
                    NSDragOperation::None
                }
            }
        }
    );

    fn background_queue() -> Retained<NSOperationQueue> {
        struct Queue(Retained<NSOperationQueue>);
        // SAFETY: NSOperationQueue is thread-safe.
        unsafe impl Send for Queue {}
        unsafe impl Sync for Queue {}
        static QUEUE: OnceLock<Queue> = OnceLock::new();
        QUEUE.get_or_init(|| Queue(NSOperationQueue::new())).0.clone()
    }

    thread_local! {
        // The provider holds its delegate weakly: keep the last drag's alive.
        static LIVE: RefCell<Option<(Retained<Promise>, Retained<Source>)>> = const { RefCell::new(None) };
    }

    /// No folders or Finder-hidden names.
    fn safe_name(name: &str) -> String {
        let cleaned: String =
            name.chars().map(|c| if matches!(c, '/' | ':' | '\\') || c.is_control() { '-' } else { c }).collect();
        let cleaned = cleaned.trim().trim_start_matches('.').trim();
        if cleaned.is_empty() { "Attachment".into() } else { cleaned.into() }
    }

    /// Download into `path`'s folder (under its name, or "name 2" when
    /// that's taken), waiting for the engine.
    fn save(d: &Drag, path: &std::path::Path) -> bool {
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else { return false };
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        crate::send_cmd(Cmd::SaveAttachments {
            account_id: d.account_id.clone(),
            message_id: d.message_id.clone(),
            files: vec![(d.attachment_id.clone(), name.to_string_lossy().into_owned())],
            dest: dir.to_path_buf(),
            into_folder: true,
            done: Some(tx),
        });
        rx.recv_timeout(WRITE_TIMEOUT).unwrap_or(false)
    }

    define_class!(
        // A message's promised .eml file.
        #[unsafe(super(NSObject))]
        #[name = "CooMailPromise"]
        #[ivars = MailFile]
        struct MailPromise;

        unsafe impl NSObjectProtocol for MailPromise {}

        unsafe impl NSFilePromiseProviderDelegate for MailPromise {
            #[unsafe(method_id(filePromiseProvider:fileNameForType:))]
            fn file_name(&self, _p: &NSFilePromiseProvider, _t: &NSString) -> Retained<NSString> {
                NSString::from_str(&pigeon_engine::service::eml_name(&self.ivars().subject))
            }

            #[unsafe(method(filePromiseProvider:writePromiseToURL:completionHandler:))]
            fn write(&self, _p: &NSFilePromiseProvider, url: &NSURL, done: &block2::DynBlock<dyn Fn(*mut NSError)>) {
                let ok =
                    url.path().is_some_and(|path| save_mail(self.ivars(), std::path::Path::new(&path.to_string())));
                if ok {
                    done.call((std::ptr::null_mut(),));
                } else {
                    let error = NSError::new(-1, &NSString::from_str("CooMail"));
                    done.call((Retained::as_ptr(&error).cast_mut(),));
                }
            }

            #[unsafe(method_id(operationQueueForFilePromiseProvider:))]
            fn queue(&self, _p: &NSFilePromiseProvider) -> Retained<NSOperationQueue> {
                background_queue()
            }
        }
    );

    define_class!(
        // Copies out to Finder; inside Coo, follows the pointer for the page.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "CooMailDragSource"]
        #[ivars = Retained<NSWindow>]
        struct MailSource;

        unsafe impl NSObjectProtocol for MailSource {}

        unsafe impl NSDraggingSource for MailSource {
            #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
            fn mask(&self, _s: &NSDraggingSession, context: NSDraggingContext) -> NSDragOperation {
                if context == NSDraggingContext::OutsideApplication {
                    NSDragOperation::Copy
                } else {
                    NSDragOperation::None
                }
            }

            #[unsafe(method(draggingSession:movedToPoint:))]
            fn moved(&self, _s: &NSDraggingSession, at: NSPoint) {
                if let Some((x, y)) = page_point(self.ivars(), at) {
                    let _ = super::main_js().send(format!("window.__pigeonMailDragAt && window.__pigeonMailDragAt({x},{y})"));
                }
            }

            #[unsafe(method(draggingSession:endedAtPoint:operation:))]
            fn ended(&self, _s: &NSDraggingSession, at: NSPoint, op: NSDragOperation) {
                // Not taken outside (Finder): a drop inside Coo, if over it.
                if op == NSDragOperation::None
                    && let Some((x, y)) = page_point(self.ivars(), at)
                {
                    let _ = super::main_js().send(format!("window.__pigeonMailDropAt && window.__pigeonMailDropAt({x},{y})"));
                }
                let _ = super::main_js().send("window.__pigeonMailDragEnd && window.__pigeonMailDragEnd()".into());
            }
        }
    );

    /// A message drag's promises and source, kept alive while it runs.
    type MailDrag = (Vec<Retained<MailPromise>>, Retained<MailSource>);

    thread_local! {
        static LIVE_MAIL: RefCell<Option<MailDrag>> = const { RefCell::new(None) };
    }

    /// A screen point as the page's viewport coordinates (None: outside the
    /// window's content).
    fn page_point(window: &NSWindow, screen: NSPoint) -> Option<(f64, f64)> {
        let p = window.convertPointFromScreen(screen);
        let size = window.contentView()?.frame().size;
        let (x, y) = (p.x, size.height - p.y);
        (x >= 0.0 && y >= 0.0 && x <= size.width && y <= size.height).then_some((x, y))
    }

    fn save_mail(m: &MailFile, path: &std::path::Path) -> bool {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        crate::send_cmd(Cmd::SaveMessages {
            messages: vec![(m.account_id.clone(), m.message_id.clone(), m.subject.clone())],
            dest: path.to_path_buf(),
            into_folder: false,
            done: Some(tx),
        });
        rx.recv_timeout(WRITE_TIMEOUT).unwrap_or(false)
    }

    pub fn start_mail(files: Vec<MailFile>) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        if NSEvent::pressedMouseButtons() & 1 == 0 {
            let _ = super::main_js().send("window.__pigeonMailDragEnd && window.__pigeonMailDragEnd()".into());
            return;
        }
        let app = NSApplication::sharedApplication(mtm);
        let Some(window) = app.keyWindow() else { return };
        let Some(view) = window.contentView() else { return };
        let at = window.mouseLocationOutsideOfEventStream();
        let event = app
            .currentEvent()
            .filter(|e| matches!(e.r#type(), NSEventType::LeftMouseDragged | NSEventType::LeftMouseDown))
            .or_else(|| {
                NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                    NSEventType::LeftMouseDragged,
                    at,
                    NSEventModifierFlags::empty(),
                    0.0,
                    window.windowNumber(),
                    None,
                    0,
                    1,
                    1.0,
                )
            });
        let Some(event) = event else { return };
        #[allow(deprecated)]
        let icon = NSWorkspace::sharedWorkspace().iconForFileType(&NSString::from_str("eml"));
        let local = view.convertPoint_fromView(at, None);
        let mut promises = Vec::new();
        let mut items = Vec::new();
        for (i, f) in files.into_iter().enumerate() {
            let promise: Retained<MailPromise> = {
                let this = MailPromise::alloc().set_ivars(f);
                // SAFETY: NSObject's init on a freshly allocated instance.
                unsafe { msg_send![super(this), init] }
            };
            let provider = NSFilePromiseProvider::initWithFileType_delegate(
                NSFilePromiseProvider::alloc(),
                &NSString::from_str("public.data"),
                ProtocolObject::from_ref(&*promise),
            );
            let item =
                NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), ProtocolObject::from_ref(&*provider));
            let off = 4.0 * i.min(3) as f64;
            let frame = NSRect::new(NSPoint::new(local.x - 16.0 + off, local.y - 16.0 - off), NSSize::new(32.0, 32.0));
            // SAFETY: NSImage is a valid drag image.
            unsafe { item.setDraggingFrame_contents(frame, Some(&icon)) };
            items.push(item);
            promises.push(promise);
        }
        let source: Retained<MailSource> = {
            let this = MailSource::alloc(mtm).set_ivars(window.clone());
            // SAFETY: as above.
            unsafe { msg_send![super(this), init] }
        };
        let session = view.beginDraggingSessionWithItems_event_source(
            &NSArray::from_retained_slice(&items),
            &event,
            ProtocolObject::from_ref(&*source),
        );
        // Dropped inside Coo (or nowhere): no flying back to the row.
        session.setAnimatesToStartingPositionsOnCancelOrFail(false);
        LIVE_MAIL.with(|l| *l.borrow_mut() = Some((promises, source)));
    }

    pub fn start(d: Drag) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        // Only while the button is still held (the message can trail the mouse).
        if NSEvent::pressedMouseButtons() & 1 == 0 {
            return;
        }
        let app = NSApplication::sharedApplication(mtm);
        let Some(window) = app.keyWindow() else { return };
        let Some(view) = window.contentView() else { return };
        let at = window.mouseLocationOutsideOfEventStream();
        let event = app
            .currentEvent()
            .filter(|e| matches!(e.r#type(), NSEventType::LeftMouseDragged | NSEventType::LeftMouseDown));
        let event = event.or_else(|| {
            NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                NSEventType::LeftMouseDragged,
                at,
                NSEventModifierFlags::empty(),
                0.0,
                window.windowNumber(),
                None,
                0,
                1,
                1.0,
            )
        });
        let Some(event) = event else { return };

        let ext =
            std::path::Path::new(&d.name).extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
        let promise: Retained<Promise> = {
            let this = Promise::alloc().set_ivars(d);
            // SAFETY: NSObject's init on a freshly allocated instance.
            unsafe { msg_send![super(this), init] }
        };
        let source: Retained<Source> = {
            let this = Source::alloc(mtm).set_ivars(());
            // SAFETY: as above.
            unsafe { msg_send![super(this), init] }
        };
        let provider = NSFilePromiseProvider::initWithFileType_delegate(
            NSFilePromiseProvider::alloc(),
            &NSString::from_str("public.data"),
            ProtocolObject::from_ref(&*promise),
        );
        let item =
            NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), ProtocolObject::from_ref(&*provider));
        // The extension's Finder icon (the content-type variant needs the
        // UniformTypeIdentifiers bindings for just this).
        #[allow(deprecated)]
        let icon = NSWorkspace::sharedWorkspace().iconForFileType(&NSString::from_str(&ext));
        let local = view.convertPoint_fromView(at, None);
        let frame = NSRect::new(NSPoint::new(local.x - 16.0, local.y - 16.0), NSSize::new(32.0, 32.0));
        // SAFETY: NSImage is a valid drag image.
        unsafe { item.setDraggingFrame_contents(frame, Some(&icon)) };
        view.beginDraggingSessionWithItems_event_source(
            &NSArray::from_retained_slice(&[item]),
            &event,
            ProtocolObject::from_ref(&*source),
        );
        LIVE.with(|l| *l.borrow_mut() = Some((promise, source)));
    }
}

#[cfg(not(target_os = "macos"))]
mod mac {
    pub fn start(_: super::Drag) {}
    pub fn start_mail(_: Vec<super::MailFile>) {}
}
