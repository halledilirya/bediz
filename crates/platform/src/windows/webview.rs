use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Controller, ICoreWebView2Settings3, ICoreWebView2Settings5, ICoreWebView2Settings6,
};
use windows::core::{Interface, Result};

pub fn make_webview_an_app(controller: &ICoreWebView2Controller) -> Result<()> {
    let settings = unsafe { controller.CoreWebView2()?.Settings()? };
    unsafe {
        settings
            .cast::<ICoreWebView2Settings3>()?
            .SetAreBrowserAcceleratorKeysEnabled(false)?;
        settings
            .cast::<ICoreWebView2Settings5>()?
            .SetIsPinchZoomEnabled(false)?;
        settings
            .cast::<ICoreWebView2Settings6>()?
            .SetIsSwipeNavigationEnabled(false)
    }
}
