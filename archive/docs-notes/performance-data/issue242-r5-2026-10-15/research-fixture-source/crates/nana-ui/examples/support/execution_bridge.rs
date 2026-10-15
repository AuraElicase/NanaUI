//! Optional archive helper bridge. No loading, native calls or storage by default.
use serde::Serialize;

#[derive(Clone, Copy, Default, Serialize)]
#[repr(C)]
pub struct Context {
    pub version: u32,
    pub size: u32,
    pub mach_before: u64,
    pub mach_after: u64,
    pub tid: u64,
    pub app_active: i32,
    pub window_visible: i32,
    pub window_occluded: i32,
    pub window_key: i32,
    pub window_main: i32,
    pub thermal_state: i32,
    pub low_power: i32,
    pub frontmost_pid: i32,
    pub task_role: i32,
    pub task_role_status: i32,
}

#[derive(Default, Serialize)]
#[repr(C)]
pub struct Anchor {
    pub abi_version: u32,
    pub status: i32,
    pub anchor_id: u64,
    pub pid: u64,
    pub thread_id: u64,
    pub mach_before: u64,
    pub mach_after: u64,
    pub continuous_before: u64,
    pub continuous_after: u64,
    pub timebase_numer: u32,
    pub timebase_denom: u32,
    pub signpost_enabled: u32,
    pub reserved: u32,
}

#[derive(Serialize)]
pub struct ContextSample {
    pub presented_ordinal: usize,
    pub requested_state: i32,
    pub request_result: Option<i32>,
    pub before: Option<Context>,
    pub after: Option<Context>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Schedule {
    Passive,
    Abba,
    Baab,
}

impl Schedule {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, &'static str> {
        let mut args = args.into_iter();
        let mut result = Self::Passive;
        while let Some(arg) = args.next() {
            if arg == "--focus-schedule" {
                result = match args.next().as_deref() {
                    Some("passive") => Self::Passive,
                    Some("abba") => Self::Abba,
                    Some("baab") => Self::Baab,
                    _ => return Err("focus schedule must be passive, abba or baab"),
                };
            }
        }
        Ok(result)
    }

    pub fn requested(self, ordinal: usize, warmup: usize, measured: usize) -> i32 {
        if self == Self::Passive {
            return 0;
        }
        let block = ordinal.saturating_sub(warmup + 1) / (measured / 4).max(1);
        let active = matches!(block, 1 | 2) == (self == Self::Abba);
        if active { 1 } else { 2 }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use std::ffi::{CString, c_char, c_int, c_void};
    type Observe = unsafe extern "C" fn(*mut Context, usize) -> c_int;
    type Activate = unsafe extern "C" fn(i32, i32) -> c_int;
    type Restore = unsafe extern "C" fn() -> c_int;
    type Emit = unsafe extern "C" fn(u64, *mut Anchor, usize) -> c_int;
    unsafe extern "C" {
        fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    }
    pub struct Bridge {
        // Kept loaded to child exit. The fixture exits via process::exit.
        _handle: *mut c_void,
        observe: Observe,
        activate: Activate,
        emit: Emit,
        restore: Option<Restore>,
    }
    impl Bridge {
        pub fn load() -> Option<Self> {
            let path = CString::new(std::env::var("NANA_R5_HELPER").ok()?).ok()?;
            let handle = unsafe { dlopen(path.as_ptr(), 2) };
            if handle.is_null() {
                return None;
            }
            let observe = unsafe { dlsym(handle, c"nanaui_r5_context".as_ptr()) };
            let activate = unsafe { dlsym(handle, c"nanaui_r5_activation".as_ptr()) };
            let emit = unsafe { dlsym(handle, c"nanaui_r5_trace_anchor".as_ptr()) };
            if observe.is_null() || activate.is_null() || emit.is_null() {
                return None;
            }
            let restore = unsafe { dlsym(handle, c"nanaui_r5_restore".as_ptr()) };
            Some(Self {
                _handle: handle,
                observe: unsafe { std::mem::transmute::<*mut c_void, Observe>(observe) },
                activate: unsafe { std::mem::transmute::<*mut c_void, Activate>(activate) },
                emit: unsafe { std::mem::transmute::<*mut c_void, Emit>(emit) },
                restore: (!restore.is_null())
                    .then(|| unsafe { std::mem::transmute::<*mut c_void, Restore>(restore) }),
            })
        }
        pub fn category_supported(&self) -> bool {
            self.restore.is_some()
        }
        pub fn restore(&self) -> Option<i32> {
            self.restore.map(|restore| unsafe { restore() })
        }
        pub fn context(&self) -> Option<Context> {
            let mut result = Context::default();
            let status = unsafe { (self.observe)(&mut result, std::mem::size_of::<Context>()) };
            (status == 0
                && result.version == 1
                && result.size as usize == std::mem::size_of::<Context>()
                && result.mach_after >= result.mach_before)
                .then_some(result)
        }
        pub fn activate(&self, mode: i32, neutral_pid: i32) -> i32 {
            unsafe { (self.activate)(mode, neutral_pid) }
        }
        pub fn anchor(&self, id: u64) -> Anchor {
            let mut result = Anchor::default();
            let status = unsafe { (self.emit)(id, &mut result, std::mem::size_of::<Anchor>()) };
            if status != 0 {
                result.status = status;
            }
            result
        }
    }
}

#[cfg(target_os = "macos")]
pub use platform::Bridge;

#[cfg(not(target_os = "macos"))]
pub struct Bridge;
#[cfg(not(target_os = "macos"))]
impl Bridge {
    pub fn load() -> Option<Self> {
        None
    }
    pub fn category_supported(&self) -> bool {
        false
    }
    pub fn restore(&self) -> Option<i32> {
        None
    }
    pub fn context(&self) -> Option<Context> {
        None
    }
    pub fn activate(&self, _: i32, _: i32) -> i32 {
        -1
    }
    pub fn anchor(&self, _: u64) -> Anchor {
        Anchor {
            status: -1,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schedule_preserves_warmup_and_equal_four_blocks() {
        assert_eq!(std::mem::size_of::<Context>(), 72);
        assert_eq!(std::mem::size_of::<Anchor>(), 80);
        for n in 1_usize..=2640 {
            let block = n.saturating_sub(241_usize) / 600;
            let expected = if matches!(block, 1 | 2) { 1 } else { 2 };
            assert_eq!(Schedule::Abba.requested(n, 240, 2400), expected);
            assert_eq!(Schedule::Baab.requested(n, 240, 2400), 3 - expected);
            assert_eq!(Schedule::Passive.requested(n, 240, 2400), 0);
        }
        assert!(Schedule::parse(["--focus-schedule".into(), "other".into()]).is_err());
    }
}
