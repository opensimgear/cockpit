//! Windows app notifications for live profile progress. The caller must invoke
//! this synchronous OS API on a blocking worker. Unsupported or suppressed
//! delivery is an error so the caller can display its native window instead.
//!
//! App identity is supplied by Cockpit's Tauri identifier/installer. This module
//! never borrows another application's AUMID or changes Windows preferences.
//! `Show` acceptance is not evidence that a person saw a banner; Windows owns
//! placement, lifetime and Notification Center. We additionally check documented
//! notification policy, DND mode and fullscreen/presentation suppression.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

#[derive(Default)]
struct Lifecycle {
    active: AtomicBool,
    dismissed: AtomicBool,
    cancel_requested: AtomicBool,
    can_cancel: AtomicBool,
    failed: Mutex<Option<String>>,
}

impl Lifecycle {
    fn new(can_cancel: bool) -> Self {
        Self { active: AtomicBool::new(true), can_cancel: AtomicBool::new(can_cancel), ..Self::default() }
    }

    fn request_cancel(&self, arguments: &str, expected: &str, callback: &dyn Fn()) {
        if self.active.load(Ordering::Acquire)
            && arguments == expected
            && self.can_cancel.swap(false, Ordering::AcqRel)
        {
            self.cancel_requested.store(true, Ordering::Release);
            callback();
        }
    }

    fn failure(&self) -> Result<(), String> {
        if let Some(error) = self.failed.lock().map_err(|error| error.to_string())?.as_ref() {
            Err(error.clone())
        } else {
            Ok(())
        }
    }

    fn should_update(&self) -> Result<bool, String> {
        if !self.active.load(Ordering::Acquire) { return Ok(false); }
        // A delayed OS Failed event remains authoritative even if the preceding
        // update could not find the toast or a dismissal callback also arrived.
        self.failure()?;
        Ok(!self.dismissed.load(Ordering::Acquire))
    }

    fn missing_notification(&self) -> Result<(), String> {
        self.failure()?;
        if self.dismissed.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err("Windows did not retain the profile progress notification; continuing in Cockpit's notification window.".into())
        }
    }
}

fn notification_keys(id: &str) -> (String, String) {
    use sha2::{Digest, Sha256};
    let digest = format!("{:x}", Sha256::digest(id.as_bytes()));
    // Windows limits each to 16 characters. Together the pair retains 128 bits
    // of operation identity, without truncating similar operation-id prefixes.
    (digest[..16].into(), digest[16..32].into())
}

fn xml_escape(value: &str) -> String {
    let mut result = String::new();
    for character in value.chars() {
        match character {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' => result.push_str("&quot;"),
            '\'' => result.push_str("&apos;"),
            '\t' | '\n' | '\r' => result.push(character),
            character if !character.is_control() => result.push(character),
            _ => result.push('\u{fffd}'),
        }
    }
    result
}

fn toast_xml(id: &str, can_cancel: bool) -> String {
    let action = if can_cancel {
        format!(r#"<actions><action content="Cancel" arguments="{}" activationType="foreground"/></actions>"#, xml_escape(&format!("cancel:{id}")))
    } else { String::new() };
    format!(r#"<toast duration="long" launch="{}"><visual><binding template="ToastGeneric"><text>{{title}}</text><text>{{profile}}</text><progress value="indeterminate" valueStringOverride="" status="{{message}}"/></binding></visual>{action}<audio silent="true"/></toast>"#, xml_escape(&format!("progress:{id}")))
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use windows::{
        core::{HSTRING, IInspectable, Interface},
        Data::Xml::Dom::XmlDocument,
        Foundation::TypedEventHandler,
        UI::Notifications::{
            NotificationData, NotificationSetting, NotificationUpdateResult,
            ToastActivatedEventArgs, ToastDismissalReason, ToastDismissedEventArgs,
            ToastFailedEventArgs, ToastNotification, ToastNotificationManager,
            ToastNotificationMode, ToastNotifier,
        },
        Win32::{
            System::{
                Com::{CoIncrementMTAUsage, CoDecrementMTAUsage, CO_MTA_USAGE_COOKIE},
                WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
            },
            UI::Shell::{SHQueryUserNotificationState, QUNS_ACCEPTS_NOTIFICATIONS},
        },
    };

    struct Apartment(bool);

    // The documented MTA usage cookie may be released on another thread. Store
    // its opaque value as an integer so no raw pointer is exposed or dereferenced.
    // This keeps callbacks/RPC resources alive between blocking worker calls.
    struct MtaUsage(usize);
    impl MtaUsage {
        fn acquire() -> Result<Self, String> {
            unsafe { CoIncrementMTAUsage() }.map(|cookie| Self(cookie.0 as usize))
                .map_err(|error| format!("Cannot retain the Windows notification runtime: {error}"))
        }
    }
    impl Drop for MtaUsage {
        fn drop(&mut self) { let _ = unsafe { CoDecrementMTAUsage(CO_MTA_USAGE_COOKIE(self.0 as *mut core::ffi::c_void)) }; }
    }

    impl Apartment {
        fn initialize() -> Result<Self, String> {
            // These WinRT objects are agile (Send + Sync). Each worker initializes
            // its apartment before calling them, balancing successful S_OK/S_FALSE
            // initialization. An already initialized STA is also usable.
            match unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
                Ok(()) => Ok(Self(true)),
                Err(error) if error.code().0 == 0x80010106u32 as i32 => Ok(Self(false)),
                Err(error) => Err(format!("Windows notification runtime is unavailable: {error}")),
            }
        }
    }
    impl Drop for Apartment {
        fn drop(&mut self) { if self.0 { unsafe { RoUninitialize() }; } }
    }

    fn permitted(notifier: &ToastNotifier) -> Result<(), String> {
        let setting = notifier.Setting().map_err(|error| format!("Cannot read Windows notification permission: {error}"))?;
        if setting != NotificationSetting::Enabled {
            return Err(format!("Windows notifications are disabled for Cockpit ({setting:?})."));
        }
        // This public DND API requires Windows build 23504+. If unavailable we
        // cannot establish banner eligibility, so prefer the visible fallback.
        let mode = ToastNotificationManager::GetDefault().and_then(|manager| manager.NotificationMode())
            .map_err(|error| format!("Cannot verify Windows Do Not Disturb mode: {error}"))?;
        if mode != ToastNotificationMode::Unrestricted {
            return Err("Windows Do Not Disturb is suppressing ordinary notifications.".into());
        }
        let state = unsafe { SHQueryUserNotificationState() }
            .map_err(|error| format!("Cannot verify Windows notification visibility: {error}"))?;
        if state != QUNS_ACCEPTS_NOTIFICATIONS {
            return Err(format!("Windows is suppressing notification banners in the current session ({state:?})."));
        }
        Ok(())
    }

    fn data(title: &str, profile: &str, message: &str, sequence: u32) -> Result<NotificationData, String> {
        let data = NotificationData::new().map_err(|error| error.to_string())?;
        let values = data.Values().map_err(|error| error.to_string())?;
        for (key, value) in [("title", title), ("profile", profile), ("message", message)] {
            // Keep the OS payload bounded while preserving complete messages in
            // Cockpit's own status view. Data binding treats these as plain text.
            let text: String = value.chars().take(512).collect();
            values.Insert(&HSTRING::from(key), &HSTRING::from(text)).map_err(|error| error.to_string())?;
        }
        data.SetSequenceNumber(sequence).map_err(|error| error.to_string())?;
        Ok(data)
    }

    struct Toast {
        notification: ToastNotification,
        activated: i64,
        dismissed: i64,
        failed: i64,
        callbacks_live: Arc<AtomicBool>,
    }

    impl Toast {
        fn create(id: &str, tag: &str, group: &str, title: &str, profile: &str, message: &str, can_cancel: bool, sequence: u32, silent_replacement: bool, lifecycle: Arc<Lifecycle>, on_cancel: Arc<dyn Fn() + Send + Sync>) -> Result<Self, String> {
            let document = XmlDocument::new().map_err(|error| error.to_string())?;
            document.LoadXml(&HSTRING::from(toast_xml(id, can_cancel))).map_err(|error| error.to_string())?;
            let notification = ToastNotification::CreateToastNotification(&document).map_err(|error| error.to_string())?;
            notification.SetTag(&HSTRING::from(tag)).map_err(|error| error.to_string())?;
            notification.SetGroup(&HSTRING::from(group)).map_err(|error| error.to_string())?;
            notification.SetSuppressPopup(silent_replacement).map_err(|error| error.to_string())?;
            notification.SetData(&data(title, profile, message, sequence)?).map_err(|error| error.to_string())?;
            let callbacks_live = Arc::new(AtomicBool::new(true));
            let live = callbacks_live.clone();
            let state = lifecycle.clone();
            let expected = format!("cancel:{id}");
            let activated = notification.Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(move |_, args| {
                if live.load(Ordering::Acquire) {
                    if let Some(args) = args.as_ref() {
                        if let Ok(arguments) = args.cast::<ToastActivatedEventArgs>().and_then(|args| args.Arguments()) {
                            state.request_cancel(&arguments.to_string(), &expected, on_cancel.as_ref());
                        }
                    }
                }
                Ok(())
            })).map_err(|error| error.to_string())?;
            let live = callbacks_live.clone();
            let state = lifecycle.clone();
            let dismissed = match notification.Dismissed(&TypedEventHandler::<ToastNotification, ToastDismissedEventArgs>::new(move |_, args| {
                if live.load(Ordering::Acquire) && !state.cancel_requested.load(Ordering::Acquire) && args.as_ref().and_then(|args| args.Reason().ok()) == Some(ToastDismissalReason::UserCanceled) {
                    state.dismissed.store(true, Ordering::Release);
                }
                Ok(())
            })) {
                Ok(token) => token,
                Err(error) => { let _ = notification.RemoveActivated(activated); return Err(error.to_string()); }
            };
            let live = callbacks_live.clone();
            let state = lifecycle;
            let failed = match notification.Failed(&TypedEventHandler::<ToastNotification, ToastFailedEventArgs>::new(move |_, args| {
                if live.load(Ordering::Acquire) {
                    let code = args.as_ref().and_then(|args| args.ErrorCode().ok());
                    *state.failed.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(format!("Windows rejected the profile notification ({code:?})."));
                }
                Ok(())
            })) {
                Ok(token) => token,
                Err(error) => { let _ = notification.RemoveActivated(activated); let _ = notification.RemoveDismissed(dismissed); return Err(error.to_string()); }
            };
            Ok(Self { notification, activated, dismissed, failed, callbacks_live })
        }
    }

    impl Drop for Toast {
        fn drop(&mut self) {
            self.callbacks_live.store(false, Ordering::Release);
            if let Ok(_apartment) = Apartment::initialize() {
                let _ = self.notification.RemoveActivated(self.activated);
                let _ = self.notification.RemoveDismissed(self.dismissed);
                let _ = self.notification.RemoveFailed(self.failed);
            }
        }
    }

    pub(crate) struct SystemNotification {
        notifier: ToastNotifier,
        toast: Toast,
        app_id: String,
        id: String,
        tag: String,
        group: String,
        sequence: u32,
        can_cancel: bool,
        lifecycle: Arc<Lifecycle>,
        on_cancel: Arc<dyn Fn() + Send + Sync>,
        last_content: (String, String, String),
        retained: bool,
        // Declared last so the MTA remains alive until all interfaces drop.
        _mta_usage: MtaUsage,
    }

    impl SystemNotification {
        pub(crate) fn show(app_id: &str, id: &str, title: &str, profile: &str, message: &str, can_cancel: bool, on_cancel: Arc<dyn Fn() + Send + Sync>) -> Result<Self, String> {
            let mta_usage = MtaUsage::acquire()?;
            let _apartment = Apartment::initialize()?;
            if app_id.is_empty() || app_id.len() > 128 || id.is_empty() || id.len() > 128 {
                return Err("Invalid application or operation identity for Windows notifications.".into());
            }
            let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id))
                .map_err(|error| format!("Cockpit's Windows notification identity is unavailable: {error}"))?;
            permitted(&notifier)?;
            let (tag, group) = notification_keys(id);
            let lifecycle = Arc::new(Lifecycle::new(can_cancel));
            let toast = Toast::create(id, &tag, &group, title, profile, message, can_cancel, 1, false, lifecycle.clone(), on_cancel.clone())?;
            notifier.Show(&toast.notification).map_err(|error| format!("Cannot show Windows profile notification: {error}"))?;
            let result = Self { notifier, toast, app_id: app_id.into(), id: id.into(), tag, group, sequence: 1, can_cancel, lifecycle, on_cancel, last_content: (title.into(), profile.into(), message.into()), retained: false, _mta_usage: mta_usage };
            result.lifecycle.failure()?;
            Ok(result)
        }

        pub(crate) fn update(&mut self, title: &str, profile: &str, message: &str, can_cancel: bool) -> Result<(), String> {
            if !self.lifecycle.should_update()? { return Ok(()); }
            let _apartment = Apartment::initialize()?;
            self.lifecycle.failure()?;
            permitted(&self.notifier)?;
            if self.lifecycle.cancel_requested.load(Ordering::Acquire) {
                // Activating an OS action can remove its toast. Continue cleanup
                // (or a failed-cancel retry) in the persistent native window.
                return Err("The Windows Cancel action was requested; continuing profile progress in Cockpit's notification window.".into());
            }
            // Confirm one successful OS update after Show, even for unchanged
            // content. Show returning S_OK does not prove the toast was retained.
            if self.retained && can_cancel == self.can_cancel && self.last_content.0 == title && self.last_content.1 == profile && self.last_content.2 == message {
                return Ok(());
            }
            self.sequence = self.sequence.checked_add(1).ok_or("Windows notification update sequence exhausted.")?;
            self.lifecycle.can_cancel.store(can_cancel, Ordering::Release);
            if can_cancel != self.can_cancel {
                // Actions cannot be data-bound. Replace the same operation's
                // layout silently when cancelability changes, without a new alert.
                let replacement = Toast::create(&self.id, &self.tag, &self.group, title, profile, message, can_cancel, self.sequence, true, self.lifecycle.clone(), self.on_cancel.clone())?;
                self.toast.callbacks_live.store(false, Ordering::Release);
                self.notifier.Show(&replacement.notification).map_err(|error| format!("Cannot update Windows profile notification: {error}"))?;
                self.toast = replacement;
                self.can_cancel = can_cancel;
                self.retained = false;
            } else {
                let result = self.notifier.UpdateWithTagAndGroup(&data(title, profile, message, self.sequence)?, &HSTRING::from(self.tag.as_str()), &HSTRING::from(self.group.as_str()))
                    .map_err(|error| format!("Cannot update Windows profile notification: {error}"))?;
                if result == NotificationUpdateResult::NotificationNotFound {
                    // Notification Center can remove an item shortly before its
                    // UserCanceled callback arrives. Give that callback at most
                    // 50 ms on this blocking worker; absence alone is not proof
                    // of a user dismissal (an unregistered app can be dropped).
                    for _ in 0..5 {
                        self.lifecycle.failure()?;
                        if self.lifecycle.dismissed.load(Ordering::Acquire) { return Ok(()); }
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    return self.lifecycle.missing_notification();
                }
                if result != NotificationUpdateResult::Succeeded { return Err(format!("Windows could not update profile progress ({result:?}).")); }
                self.retained = true;
            }
            self.last_content = (title.into(), profile.into(), message.into());
            self.lifecycle.failure()
        }

        pub(crate) fn close(&mut self) {
            self.lifecycle.active.store(false, Ordering::Release);
            self.lifecycle.can_cancel.store(false, Ordering::Release);
            self.toast.callbacks_live.store(false, Ordering::Release);
            if let Ok(_apartment) = Apartment::initialize() {
                let _ = self.notifier.Hide(&self.toast.notification);
                // Hide only removes the banner on desktop Windows. Remove this
                // operation's history entry too, so completed work leaves no
                // stale Cancel action behind in Notification Center.
                let _ = ToastNotificationManager::History().and_then(|history| history.RemoveGroupedTagWithId(&HSTRING::from(self.tag.as_str()), &HSTRING::from(self.group.as_str()), &HSTRING::from(self.app_id.as_str())));
            }
        }
    }

    impl Drop for SystemNotification { fn drop(&mut self) { self.close(); } }
}

#[cfg(target_os = "windows")]
pub(crate) use platform::SystemNotification;

#[cfg(not(target_os = "windows"))]
pub(crate) struct SystemNotification;

#[cfg(not(target_os = "windows"))]
impl SystemNotification {
    pub(crate) fn show(_app_id: &str, _id: &str, _title: &str, _profile: &str, _message: &str, _can_cancel: bool, _on_cancel: Arc<dyn Fn() + Send + Sync>) -> Result<Self, String> { Err("System progress notifications are unavailable on this operating system.".into()) }
    pub(crate) fn update(&mut self, _title: &str, _profile: &str, _message: &str, _can_cancel: bool) -> Result<(), String> { Err("System progress notifications are unavailable on this operating system.".into()) }
    pub(crate) fn close(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn xml_is_inert_and_escapes_the_exact_operation_identity() {
        let xml = toast_xml("op\"/><evil>&'\u{0001}", true);
        assert!(!xml.contains("<evil>"));
        assert!(xml.contains("cancel:op&quot;/&gt;&lt;evil&gt;&amp;&apos;\u{fffd}"));
        assert!(xml.contains("value=\"indeterminate\""));
        assert!(xml.contains("<audio silent=\"true\"/>"));
        assert!(!toast_xml("operation", false).contains("<action "));
    }

    #[test]
    fn operation_tags_are_bounded_and_preserve_differences_after_a_shared_prefix() {
        let first = notification_keys("vehicle-switch-aaaaaaaaaaaaaaaaaaa-1");
        let second = notification_keys("vehicle-switch-aaaaaaaaaaaaaaaaaaa-2");
        assert_ne!(first, second);
        assert_eq!(first, notification_keys("vehicle-switch-aaaaaaaaaaaaaaaaaaa-1"));
        for key in [first.0, first.1, second.0, second.1] { assert_eq!(key.len(), 16); assert!(key.bytes().all(|byte| byte.is_ascii_hexdigit())); }
    }

    #[test]
    fn cancellation_requires_the_exact_live_operation_and_is_delivered_once() {
        let state = Lifecycle::new(true);
        let calls = AtomicUsize::new(0);
        let callback = || { calls.fetch_add(1, Ordering::SeqCst); };
        state.request_cancel("cancel:old", "cancel:current", &callback);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        state.request_cancel("cancel:current", "cancel:current", &callback);
        state.request_cancel("cancel:current", "cancel:current", &callback);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(state.cancel_requested.load(Ordering::Acquire));
        assert!(!state.dismissed.load(Ordering::Acquire));
        let retired = Lifecycle::new(true);
        retired.active.store(false, Ordering::Release);
        retired.request_cancel("cancel:old", "cancel:old", &callback);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let switching = Lifecycle::new(false);
        switching.request_cancel("cancel:switch", "cancel:switch", &callback);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn an_unretained_toast_falls_back_and_late_os_failures_cannot_be_hidden_as_dismissal() {
        let state = Lifecycle::new(false);
        assert!(state.missing_notification().is_err());
        assert!(!state.dismissed.load(Ordering::Acquire));
        assert!(state.should_update().unwrap());
        *state.failed.lock().unwrap() = Some("Windows rejected the notification identity".into());
        assert!(state.should_update().unwrap_err().contains("identity"));
        // Even if a dismissal event races with Failed, the actual API failure
        // remains visible so the router can select its dependable fallback.
        state.dismissed.store(true, Ordering::Release);
        assert!(state.should_update().is_err());
        assert!(state.missing_notification().is_err());
        let user_dismissed = Lifecycle::new(false);
        user_dismissed.dismissed.store(true, Ordering::Release);
        assert!(!user_dismissed.should_update().unwrap());
        assert!(user_dismissed.missing_notification().is_ok());
    }
}
