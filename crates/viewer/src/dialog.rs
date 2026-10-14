//! A native message box. The shareable build has no console, so a startup error needs a window
//! of its own. Off Windows it falls back to stderr.

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    #[link(name = "user32")]
    extern "system" {
        pub fn MessageBoxW(hwnd: *mut c_void, text: *const u16, caption: *const u16, kind: u32) -> i32;
    }

    pub const MB_ICONERROR: u32 = 0x10;

    pub fn show(text: &str, kind: u32) -> i32 {
        let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
        let (text, caption) = (wide(text), wide("mc-rs"));
        unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), kind) }
    }
}

/// An error the user has to read before the process exits.
pub fn error(text: &str) {
    #[cfg(windows)]
    win::show(text, win::MB_ICONERROR);
    #[cfg(not(windows))]
    eprintln!("mc-rs viewer: {text}");
}
