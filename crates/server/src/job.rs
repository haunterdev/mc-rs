//! Every process the server starts (cloudflared, one viewer per browser player) ends with it,
//! however it ends. `kill_on_drop` covers Ctrl+C and a normal exit; closing the console window
//! kills the server without dropping anything, which left the quick tunnels running with no server
//! behind them. The server puts itself in a job object that kills its members when the last
//! handle closes, and the children it starts join that job.

/// Puts this process in a kill-on-close job; false when Windows refused (the children then end
/// only through `kill_on_drop`).
#[cfg(windows)]
pub fn bind_children() -> bool {
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: plain Win32 calls on a handle created here; the handle is left open on purpose, so
    // the job lives exactly as long as this process.
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return false;
        }
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let set = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        set != 0 && AssignProcessToJobObject(job, GetCurrentProcess()) != 0
    }
}

#[cfg(not(windows))]
pub fn bind_children() -> bool {
    true
}
