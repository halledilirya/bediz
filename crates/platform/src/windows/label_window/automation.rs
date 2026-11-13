//! Presents the label window to UI Automation as a tooltip, so screen readers hear each notice as
//! it opens, changes and closes.

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering, fence};

use windows::Win32::Foundation::{E_NOINTERFACE, E_POINTER, HWND, LPARAM, LRESULT, S_OK, WPARAM};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    IRawElementProviderSimple, IRawElementProviderSimple_Vtbl, IWindowProvider,
    IWindowProvider_Impl, NotificationKind_ActionAborted, NotificationKind_ActionCompleted,
    NotificationProcessing_ImportantMostRecent, ProviderOptions,
    ProviderOptions_ServerSideProvider, ProviderOptions_UseComThreading, UIA_ControlTypePropertyId,
    UIA_E_INVALIDOPERATION, UIA_EVENT_ID, UIA_PATTERN_ID, UIA_PROPERTY_ID,
    UIA_ToolTipClosedEventId, UIA_ToolTipControlTypeId, UIA_ToolTipOpenedEventId,
    UIA_Window_WindowClosedEventId, UIA_Window_WindowOpenedEventId, UIA_WindowPatternId,
    UiaClientsAreListening, UiaDisconnectProvider, UiaHostProviderFromHwnd,
    UiaRaiseAutomationEvent, UiaRaiseNotificationEvent, UiaReturnRawElementProvider,
    WindowInteractionState, WindowInteractionState_Running, WindowVisualState,
    WindowVisualState_Normal,
};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
use windows::core::{BOOL, BSTR, Error, GUID, HRESULT, IUnknown, IUnknown_Vtbl, Interface, Result};
use windows_core::implement;

thread_local! {
    static PROVIDERS: RefCell<Vec<(HWND, IRawElementProviderSimple)>> =
        const { RefCell::new(Vec::new()) };
}

pub struct Announcer(IRawElementProviderSimple);

impl Announcer {
    pub fn attach(window: HWND) -> Self {
        let provider = provider(window);
        PROVIDERS.with(|providers| providers.borrow_mut().push((window, provider.clone())));
        Self(provider)
    }

    pub fn opened(&self, text: &str, warning: bool) -> Result<()> {
        opened(self, text, warning)
    }

    pub fn replaced(&self, text: &str, warning: bool) -> Result<()> {
        replaced(self, text, warning)
    }

    pub fn closed(&self) -> Result<()> {
        closed(self)
    }
}

trait Events {
    fn listening(&self) -> bool;
    fn raise(&self, event: UIA_EVENT_ID) -> Result<()>;
    fn notify(&self, text: &str, warning: bool) -> Result<()>;
}

impl Events for Announcer {
    fn listening(&self) -> bool {
        unsafe { UiaClientsAreListening() }.as_bool()
    }

    fn raise(&self, event: UIA_EVENT_ID) -> Result<()> {
        unsafe { UiaRaiseAutomationEvent(&self.0, event) }
    }

    fn notify(&self, text: &str, warning: bool) -> Result<()> {
        let kind = if warning {
            NotificationKind_ActionAborted
        } else {
            NotificationKind_ActionCompleted
        };
        unsafe {
            UiaRaiseNotificationEvent(
                &self.0,
                kind,
                NotificationProcessing_ImportantMostRecent,
                &BSTR::from(text),
                &BSTR::from("Bediz.Label"),
            )
        }
    }
}

fn opened(events: &impl Events, text: &str, warning: bool) -> Result<()> {
    if !events.listening() {
        return Ok(());
    }
    [
        events.raise(UIA_ToolTipOpenedEventId),
        events.raise(UIA_Window_WindowOpenedEventId),
        events.notify(text, warning),
    ]
    .into_iter()
    .collect()
}

fn replaced(events: &impl Events, text: &str, warning: bool) -> Result<()> {
    if !events.listening() {
        return Ok(());
    }
    events.notify(text, warning)
}

fn closed(events: &impl Events) -> Result<()> {
    if !events.listening() {
        return Ok(());
    }
    [
        events.raise(UIA_ToolTipClosedEventId),
        events.raise(UIA_Window_WindowClosedEventId),
    ]
    .into_iter()
    .collect()
}

pub fn answer(window: HWND, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    let provider = PROVIDERS.with(|providers| {
        providers
            .borrow()
            .iter()
            .find(|(owner, _)| *owner == window)
            .map(|(_, provider)| provider.clone())
    })?;
    Some(unsafe { UiaReturnRawElementProvider(window, wparam, lparam, &provider) })
}

pub fn detach(window: HWND) {
    let provider = PROVIDERS.with(|providers| {
        let mut providers = providers.borrow_mut();
        let index = providers.iter().position(|(owner, _)| *owner == window)?;
        Some(providers.swap_remove(index).1)
    });
    let Some(provider) = provider else {
        return;
    };
    unsafe {
        UiaReturnRawElementProvider(
            window,
            WPARAM(0),
            LPARAM(0),
            None::<&IRawElementProviderSimple>,
        );
        let _ = UiaDisconnectProvider(&provider);
    }
}

// Written by hand: the generated vtable cannot answer S_OK with a null pattern, as UI Automation requires.
#[repr(C)]
struct Provider {
    vtable: &'static IRawElementProviderSimple_Vtbl,
    count: AtomicU32,
    window: HWND,
    pattern: IUnknown,
}

static PROVIDER: IRawElementProviderSimple_Vtbl = IRawElementProviderSimple_Vtbl {
    base__: IUnknown_Vtbl {
        QueryInterface: query_interface,
        AddRef: add_ref,
        Release: release,
    },
    ProviderOptions: provider_options,
    GetPatternProvider: pattern_provider,
    GetPropertyValue: property_value,
    HostRawElementProvider: host_provider,
};

fn provider(window: HWND) -> IRawElementProviderSimple {
    let pattern: IWindowProvider = WindowPattern { window }.into();
    let provider = Box::new(Provider {
        vtable: &PROVIDER,
        count: AtomicU32::new(1),
        window,
        pattern: pattern.into(),
    });
    unsafe { IRawElementProviderSimple::from_raw(Box::into_raw(provider).cast()) }
}

unsafe fn provider_of<'a>(this: *mut c_void) -> &'a Provider {
    unsafe { &*this.cast::<Provider>() }
}

unsafe extern "system" fn query_interface(
    this: *mut c_void,
    iid: *const GUID,
    interface: *mut *mut c_void,
) -> HRESULT {
    if interface.is_null() {
        return E_POINTER;
    }
    unsafe { interface.write(std::ptr::null_mut()) };
    if iid.is_null() {
        return E_POINTER;
    }
    let iid = unsafe { iid.read() };
    if iid != IUnknown::IID && iid != IRawElementProviderSimple::IID {
        return E_NOINTERFACE;
    }
    unsafe {
        add_ref(this);
        interface.write(this);
    }
    S_OK
}

unsafe extern "system" fn add_ref(this: *mut c_void) -> u32 {
    let previous = unsafe { provider_of(this) }
        .count
        .fetch_add(1, Ordering::Relaxed);
    previous.saturating_add(1)
}

unsafe extern "system" fn release(this: *mut c_void) -> u32 {
    let previous = unsafe { provider_of(this) }
        .count
        .fetch_sub(1, Ordering::Release);
    if previous == 1 {
        fence(Ordering::Acquire);
        drop(unsafe { Box::from_raw(this.cast::<Provider>()) });
    }
    previous.saturating_sub(1)
}

unsafe extern "system" fn provider_options(
    _: *mut c_void,
    options: *mut ProviderOptions,
) -> HRESULT {
    if options.is_null() {
        return E_POINTER;
    }
    unsafe { options.write(ProviderOptions_ServerSideProvider | ProviderOptions_UseComThreading) };
    S_OK
}

unsafe extern "system" fn pattern_provider(
    this: *mut c_void,
    pattern: UIA_PATTERN_ID,
    found: *mut *mut c_void,
) -> HRESULT {
    if found.is_null() {
        return E_POINTER;
    }
    let supported = (pattern == UIA_WindowPatternId)
        .then(|| unsafe { provider_of(this) }.pattern.clone().into_raw());
    unsafe { found.write(supported.unwrap_or(std::ptr::null_mut())) };
    S_OK
}

unsafe extern "system" fn property_value(
    _: *mut c_void,
    property: UIA_PROPERTY_ID,
    value: *mut VARIANT,
) -> HRESULT {
    if value.is_null() {
        return E_POINTER;
    }
    let known = if property == UIA_ControlTypePropertyId {
        VARIANT::from(UIA_ToolTipControlTypeId.0)
    } else {
        VARIANT::default()
    };
    unsafe { value.write(known) };
    S_OK
}

unsafe extern "system" fn host_provider(this: *mut c_void, host: *mut *mut c_void) -> HRESULT {
    if host.is_null() {
        return E_POINTER;
    }
    unsafe { host.write(std::ptr::null_mut()) };
    match unsafe { UiaHostProviderFromHwnd(provider_of(this).window) } {
        Ok(provider) => {
            unsafe { host.write(provider.into_raw()) };
            S_OK
        }
        Err(error) => error.code(),
    }
}

#[implement(IWindowProvider)]
struct WindowPattern {
    window: HWND,
}

impl IWindowProvider_Impl for WindowPattern_Impl {
    fn SetVisualState(&self, state: WindowVisualState) -> Result<()> {
        if state == WindowVisualState_Normal {
            Ok(())
        } else {
            Err(Error::from_hresult(HRESULT(UIA_E_INVALIDOPERATION as i32)))
        }
    }

    fn Close(&self) -> Result<()> {
        unsafe { PostMessageW(Some(self.window), WM_CLOSE, WPARAM(0), LPARAM(0)) }
    }

    fn WaitForInputIdle(&self, _: i32) -> Result<BOOL> {
        Ok(true.into())
    }

    fn CanMaximize(&self) -> Result<BOOL> {
        Ok(false.into())
    }

    fn CanMinimize(&self) -> Result<BOOL> {
        Ok(false.into())
    }

    fn IsModal(&self) -> Result<BOOL> {
        Ok(false.into())
    }

    fn WindowVisualState(&self) -> Result<WindowVisualState> {
        Ok(WindowVisualState_Normal)
    }

    fn WindowInteractionState(&self) -> Result<WindowInteractionState> {
        Ok(WindowInteractionState_Running)
    }

    fn IsTopmost(&self) -> Result<BOOL> {
        Ok(true.into())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use windows::Win32::Foundation::E_FAIL;
    use windows::Win32::System::Variant::{VT_EMPTY, VT_I4};
    use windows::Win32::UI::Accessibility::{
        UIA_InvokePatternId, UIA_NamePropertyId, UIA_TextPatternId, WindowVisualState_Maximized,
        WindowVisualState_Minimized,
    };

    use super::*;

    #[derive(Default)]
    struct Recorded {
        listening: bool,
        failing: Option<UIA_EVENT_ID>,
        raised: RefCell<Vec<String>>,
    }

    impl Recorded {
        fn listening() -> Self {
            Self {
                listening: true,
                ..Self::default()
            }
        }

        fn raised(&self) -> Vec<String> {
            self.raised.borrow().clone()
        }
    }

    impl Events for Recorded {
        fn listening(&self) -> bool {
            self.listening
        }

        fn raise(&self, event: UIA_EVENT_ID) -> Result<()> {
            let name = [
                (UIA_ToolTipOpenedEventId, "tooltip opened"),
                (UIA_ToolTipClosedEventId, "tooltip closed"),
                (UIA_Window_WindowOpenedEventId, "window opened"),
                (UIA_Window_WindowClosedEventId, "window closed"),
            ]
            .into_iter()
            .find_map(|(known, name)| (known == event).then_some(name))
            .unwrap_or("other");
            self.raised.borrow_mut().push(name.to_owned());
            if self.failing == Some(event) {
                return Err(E_FAIL.into());
            }
            Ok(())
        }

        fn notify(&self, text: &str, warning: bool) -> Result<()> {
            let kind = if warning { "aborted" } else { "completed" };
            self.raised.borrow_mut().push(format!("{kind}: {text}"));
            Ok(())
        }
    }

    #[test]
    fn a_label_that_appears_opens_as_a_tooltip_and_window_then_says_its_text() {
        let events = Recorded::listening();
        opened(&events, "Not saved: the disk is full", true).unwrap();
        assert_eq!(
            events.raised(),
            [
                "tooltip opened",
                "window opened",
                "aborted: Not saved: the disk is full"
            ]
        );
    }

    #[test]
    fn a_label_replaced_while_shown_only_says_its_new_text() {
        let events = Recorded::listening();
        replaced(&events, "Copied", false).unwrap();
        assert_eq!(events.raised(), ["completed: Copied"]);
    }

    #[test]
    fn a_label_that_goes_closes_as_a_tooltip_and_window() {
        let events = Recorded::listening();
        closed(&events).unwrap();
        assert_eq!(events.raised(), ["tooltip closed", "window closed"]);
    }

    #[test]
    fn nothing_is_raised_while_no_client_listens() {
        let events = Recorded::default();
        opened(&events, "Copied", false).unwrap();
        replaced(&events, "Saved", false).unwrap();
        closed(&events).unwrap();
        assert!(events.raised().is_empty());
    }

    #[test]
    fn a_failed_event_is_reported_and_the_rest_still_go_out() {
        let events = Recorded {
            failing: Some(UIA_ToolTipOpenedEventId),
            ..Recorded::listening()
        };
        let failure = opened(&events, "Copied", false).unwrap_err();
        assert_eq!(failure.code(), E_FAIL);
        assert_eq!(
            events.raised(),
            ["tooltip opened", "window opened", "completed: Copied"]
        );
    }

    fn label() -> IRawElementProviderSimple {
        provider(HWND::default())
    }

    fn pattern(label: &IRawElementProviderSimple, id: UIA_PATTERN_ID) -> *mut c_void {
        let mut found = std::ptr::dangling_mut::<c_void>();
        let code = unsafe { (label.vtable().GetPatternProvider)(label.as_raw(), id, &mut found) };
        assert_eq!(code, S_OK);
        found
    }

    fn property(label: &IRawElementProviderSimple, id: UIA_PROPERTY_ID) -> VARIANT {
        let mut value = VARIANT::default();
        let code = unsafe { (label.vtable().GetPropertyValue)(label.as_raw(), id, &mut value) };
        assert_eq!(code, S_OK);
        value
    }

    #[test]
    fn a_pattern_it_lacks_comes_back_as_success_with_a_null_pointer() {
        let label = label();
        for id in [UIA_InvokePatternId, UIA_TextPatternId] {
            assert!(pattern(&label, id).is_null());
        }
    }

    #[test]
    fn the_window_pattern_comes_back_as_its_own_object() {
        let label = label();
        let found = pattern(&label, UIA_WindowPatternId);
        assert!(!found.is_null());
        let window = unsafe { IUnknown::from_raw(found) };
        assert!(window.cast::<IWindowProvider>().is_ok());
    }

    #[test]
    fn it_answers_only_for_itself_and_clears_the_slot_otherwise() {
        let label = label();
        assert!(label.cast::<IUnknown>().is_ok());
        assert!(label.cast::<IRawElementProviderSimple>().is_ok());
        let mut found = std::ptr::dangling_mut::<c_void>();
        let code = unsafe {
            (label.vtable().base__.QueryInterface)(
                label.as_raw(),
                &IWindowProvider::IID,
                &mut found,
            )
        };
        assert_eq!(code, E_NOINTERFACE);
        assert!(found.is_null());
    }

    #[test]
    fn missing_out_pointers_are_refused() {
        let label = label();
        let raw = label.as_raw();
        let vtable = label.vtable();
        unsafe {
            assert_eq!(
                (vtable.ProviderOptions)(raw, std::ptr::null_mut()),
                E_POINTER
            );
            assert_eq!(
                (vtable.GetPatternProvider)(raw, UIA_WindowPatternId, std::ptr::null_mut()),
                E_POINTER
            );
            assert_eq!(
                (vtable.GetPropertyValue)(raw, UIA_ControlTypePropertyId, std::ptr::null_mut()),
                E_POINTER
            );
            assert_eq!(
                (vtable.HostRawElementProvider)(raw, std::ptr::null_mut()),
                E_POINTER
            );
        }
    }

    #[test]
    fn it_is_a_server_side_tooltip_whose_name_comes_from_the_window() {
        let label = label();
        let mut options = ProviderOptions(0);
        let code = unsafe { (label.vtable().ProviderOptions)(label.as_raw(), &mut options) };
        assert_eq!(code, S_OK);
        assert_eq!(
            options,
            ProviderOptions_ServerSideProvider | ProviderOptions_UseComThreading
        );
        let kind = property(&label, UIA_ControlTypePropertyId);
        assert_eq!(kind.vt(), VT_I4);
        assert_eq!(i32::try_from(&kind).unwrap(), UIA_ToolTipControlTypeId.0);
        assert_eq!(property(&label, UIA_NamePropertyId).vt(), VT_EMPTY);
    }

    #[test]
    fn the_label_stays_a_plain_topmost_window() {
        let window: IWindowProvider = WindowPattern {
            window: HWND::default(),
        }
        .into();
        unsafe {
            assert!(window.SetVisualState(WindowVisualState_Normal).is_ok());
            for state in [WindowVisualState_Maximized, WindowVisualState_Minimized] {
                let refused = window.SetVisualState(state).unwrap_err();
                assert_eq!(refused.code(), HRESULT(UIA_E_INVALIDOPERATION as i32));
            }
            assert!(!window.CanMaximize().unwrap().as_bool());
            assert!(!window.CanMinimize().unwrap().as_bool());
            assert!(!window.IsModal().unwrap().as_bool());
            assert!(window.IsTopmost().unwrap().as_bool());
            assert_eq!(
                window.WindowVisualState().unwrap(),
                WindowVisualState_Normal
            );
        }
    }
}
