//! Fixture-only bounded sample storage. No diagnostics, scheduling or QoS changes.
use std::time::Instant;

/// Cold endpoint observations only. A task role is not a thread-group id,
/// coalition membership, CPU residency or a frequency observation.
#[derive(serde::Serialize)]
pub struct NativeProcessSnapshot {
    pid: u32,
    thread_id: Option<u64>,
    thread_id_error: Option<i32>,
    task_role: Option<i32>,
    task_policy_get_error: Option<i32>,
    task_role_default: Option<bool>,
}

pub fn native_process_snapshot() -> NativeProcessSnapshot {
    let result = NativeProcessSnapshot {
        pid: std::process::id(),
        thread_id: None,
        thread_id_error: None,
        task_role: None,
        task_policy_get_error: None,
        task_role_default: None,
    };
    #[cfg(target_os = "macos")]
    let result = {
        let mut result = result;
        // macOS SDK mach/task_policy.h: TASK_CATEGORY_POLICY=1, one integer_t
        // role; mach_msg_type_number_t and mach_port_t are natural_t/u32.
        unsafe extern "C" {
            static mach_task_self_: u32;
            fn task_policy_get(
                task: u32,
                flavor: u32,
                policy: *mut i32,
                count: *mut u32,
                default: *mut i32,
            ) -> i32;
            fn pthread_threadid_np(thread: *mut std::ffi::c_void, id: *mut u64) -> i32;
        }
        let (mut role, mut count, mut default) = (0, 1, 0);
        let error =
            unsafe { task_policy_get(mach_task_self_, 1, &mut role, &mut count, &mut default) };
        result.task_policy_get_error = Some(error);
        result.task_role_default = (error == 0).then_some(default != 0);
        result.task_role = (error == 0 && count == 1 && default == 0).then_some(role);
        let mut id = 0;
        let error = unsafe { pthread_threadid_np(std::ptr::null_mut(), &mut id) };
        result.thread_id_error = Some(error);
        result.thread_id = (error == 0).then_some(id);
        result
    };
    result
}

/// CLOCK_THREAD_CPUTIME_ID measures this thread's CPU execution, excluding
/// scheduling/drawable waits. It is not a GPU execution timer.
pub fn thread_cpu_time() -> Option<std::time::Duration> {
    #[cfg(target_os = "macos")]
    {
        mac_thread_clock::read()
            .ok()
            .map(|value| std::time::Duration::new(value.seconds, value.nanos))
    }
    #[cfg(not(target_os = "macos"))]
    None
}

#[cfg(target_os = "macos")]
mod mac_thread_clock {
    use std::ffi::{c_int, c_long, c_uint};

    // Apple SDK _time.h: clockid_t is a C enum with unsigned int ABI;
    // _types/_timespec.h: struct timespec has two long fields on macOS.
    #[repr(C)]
    struct Timespec {
        seconds: c_long,
        nanos: c_long,
    }
    unsafe extern "C" {
        fn clock_gettime(clock: c_uint, value: *mut Timespec) -> c_int;
        fn __error() -> *mut c_int;
    }

    pub(super) struct CpuTime {
        pub seconds: u64,
        pub nanos: u32,
    }

    pub(super) fn errno() -> i32 {
        unsafe { *__error() }
    }

    pub(super) fn read() -> Result<CpuTime, (&'static str, Option<i32>)> {
        let mut value = Timespec {
            seconds: 0,
            nanos: 0,
        };
        if unsafe { clock_gettime(16, &mut value) } != 0 {
            return Err(("cpu16-clock-failed", Some(errno())));
        }
        if value.seconds < 0 || !(0..1_000_000_000).contains(&value.nanos) {
            return Err(("invalid-cpu16-timespec", None));
        }
        Ok(CpuTime {
            seconds: value.seconds as u64,
            nanos: value.nanos as u32,
        })
    }
}

#[derive(Clone, Copy)]
pub struct Options {
    pub enabled: bool,
    pub direct_only: bool,
    pub warmup: usize,
    pub measured: usize,
}

impl Options {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, &'static str> {
        let mut options = Self {
            enabled: false,
            direct_only: false,
            warmup: 30,
            measured: 240,
        };
        let mut override_count = false;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--timing-samples" => options.enabled = true,
                "--direct-only" => options.direct_only = true,
                "--execution-samples"
                | "--focus-schedule"
                | "--task-category-policy"
                | "--allocator-observation" => {
                    return Err(
                        "research CLI moved to archive/docs-notes/performance-data/issue242-r5-2026-10-15/research-fixture-source; use the archived frozen binary, not this maintained fixture",
                    );
                }
                "--warmup-frames" | "--measured-frames" => {
                    let value = args
                        .next()
                        .ok_or("timing frame count needs a value")?
                        .parse::<usize>()
                        .map_err(|_| "timing frame count must be an integer")?;
                    if arg == "--warmup-frames" {
                        if !(30..=2400).contains(&value) {
                            return Err("warmup frames must be 30..=2400");
                        }
                        options.warmup = value;
                    } else {
                        if !(240..=2400).contains(&value) {
                            return Err("measured frames must be 240..=2400");
                        }
                        options.measured = value;
                    }
                    override_count = true;
                }
                _ => {}
            }
        }
        if override_count && !options.direct_only {
            return Err("timing frame count overrides require --direct-only");
        }
        Ok(options)
    }
}

#[derive(serde::Serialize)]
pub struct Sample {
    pub prepare_ordinal: usize,
    pub presented_ordinal: usize,
    pub warmup: bool,
    pub included_in_measurement: bool,
    pub prepare_since_start_ms: f64,
    pub prepare_interval_ms: Option<f64>,
    pub wall_ms: f64,
    pub thread_cpu_ms: Option<f64>,
    pub wall_minus_thread_cpu_ms: Option<f64>,
}

#[derive(serde::Serialize)]
pub struct Phase {
    pub samples: Vec<Sample>,
    pub dropped_samples: usize,
    #[serde(skip)]
    first_prepare: Option<Instant>,
    #[serde(skip)]
    previous_prepare: Option<Instant>,
    #[serde(skip)]
    prepare_ordinal: usize,
    #[serde(skip)]
    since_start_ms: f64,
    #[serde(skip)]
    interval_ms: Option<f64>,
}

impl Phase {
    fn new(capacity: usize) -> Self {
        Self {
            samples: Vec::with_capacity(capacity),
            dropped_samples: 0,
            first_prepare: None,
            previous_prepare: None,
            prepare_ordinal: 0,
            since_start_ms: 0.0,
            interval_ms: None,
        }
    }

    pub fn prepare(&mut self, at: Instant) {
        let first = *self.first_prepare.get_or_insert(at);
        self.since_start_ms = at.duration_since(first).as_secs_f64() * 1000.0;
        self.interval_ms = self
            .previous_prepare
            .map(|previous| at.duration_since(previous).as_secs_f64() * 1000.0);
        self.previous_prepare = Some(at);
        self.prepare_ordinal += 1;
    }

    pub fn record(
        &mut self,
        presented_ordinal: usize,
        warmup: bool,
        included: bool,
        wall_ms: f64,
        thread_cpu_ms: Option<f64>,
    ) {
        if self.samples.len() == self.samples.capacity() {
            self.dropped_samples += 1;
            return;
        }
        self.samples.push(Sample {
            prepare_ordinal: self.prepare_ordinal,
            presented_ordinal,
            warmup,
            included_in_measurement: included,
            prepare_since_start_ms: self.since_start_ms,
            prepare_interval_ms: self.interval_ms,
            wall_ms,
            thread_cpu_ms,
            wall_minus_thread_cpu_ms: thread_cpu_ms.map(|cpu| wall_ms - cpu),
        });
    }
}

#[derive(serde::Serialize)]
pub struct Series {
    pub phases: [Phase; 6],
    pub main_thread_qos: Option<Qos>,
}

impl Series {
    pub fn new(options: Options) -> Self {
        Self {
            phases: std::array::from_fn(|phase| {
                Phase::new(if phase == 0 {
                    options.warmup + options.measured
                } else if phase == 5 || options.direct_only {
                    0
                } else {
                    30 + 60 + 64
                })
            }),
            main_thread_qos: observe_qos(),
        }
    }
}

#[derive(Clone, Copy, serde::Serialize)]
pub struct Qos {
    pub requested_class: u32,
    pub class_name: &'static str,
    pub relative_priority: i32,
}

#[cfg(target_os = "macos")]
fn observe_qos() -> Option<Qos> {
    // Apple SDK pthread/qos.h: int pthread_get_qos_class_np(pthread_t,
    // qos_class_t *restrict, int *restrict). sys/qos.h defines unsigned int
    // classes. This observes requested QoS only, not effective priority/core.
    unsafe extern "C" {
        fn pthread_self() -> *mut std::ffi::c_void;
        fn pthread_get_qos_class_np(
            thread: *mut std::ffi::c_void,
            class: *mut u32,
            relative_priority: *mut i32,
        ) -> i32;
    }
    let mut class = 0;
    let mut relative_priority = 0;
    if unsafe { pthread_get_qos_class_np(pthread_self(), &mut class, &mut relative_priority) } != 0
    {
        return None;
    }
    Some(Qos {
        requested_class: class,
        relative_priority,
        class_name: match class {
            0x21 => "user-interactive",
            0x19 => "user-initiated",
            0x15 => "default",
            0x11 => "utility",
            0x09 => "background",
            0 => "unspecified",
            _ => "unknown",
        },
    })
}

#[cfg(not(target_os = "macos"))]
fn observe_qos() -> Option<Qos> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn options(args: &[&str]) -> Result<Options, &'static str> {
        Options::parse(args.iter().map(|v| (*v).into()))
    }

    #[test]
    fn timing_overrides_require_explicit_direct_scope_and_bounded_counts() {
        let defaults = options(&[]).unwrap();
        assert!(!defaults.enabled && !defaults.direct_only);
        assert_eq!((defaults.warmup, defaults.measured), (30, 240));
        assert!(options(&["--warmup-frames", "60"]).is_err());
        assert!(options(&["--direct-only", "--measured-frames", "239"]).is_err());
        assert!(options(&["--direct-only", "--warmup-frames", "2401"]).is_err());
        let long = options(&[
            "--direct-only",
            "--timing-samples",
            "--warmup-frames",
            "2400",
            "--measured-frames",
            "2400",
        ])
        .unwrap();
        assert!(long.enabled && long.direct_only);
        assert_eq!((long.warmup, long.measured), (2400, 2400));
    }

    #[test]
    fn successful_frames_preserve_prepare_gaps_and_missing_prepare_attempts_without_growth() {
        let mut phase = Phase::new(2);
        let at = Instant::now();
        phase.prepare(at);
        phase.record(1, true, false, 1.0, None);
        phase.prepare(at + Duration::from_millis(8)); // Unpresented attempt.
        phase.prepare(at + Duration::from_millis(24));
        phase.record(2, false, true, 2.0, Some(1.0));
        assert_eq!(phase.samples[0].prepare_interval_ms, None);
        assert_eq!(phase.samples[1].prepare_interval_ms, Some(16.0));
        assert_eq!(phase.samples[1].prepare_since_start_ms, 24.0);
        assert_eq!(phase.samples[1].prepare_ordinal, 3);
        phase.record(3, false, true, 2.0, None);
        assert_eq!(phase.dropped_samples, 1);
        assert_eq!(phase.samples.capacity(), 2);
        assert_eq!(phase.samples.len(), 2);
    }

    #[test]
    fn research_switches_fail_with_explicit_archive_migration() {
        for flag in [
            "--execution-samples",
            "--focus-schedule",
            "--task-category-policy",
            "--allocator-observation",
        ] {
            let error = options(&["--direct-only", "--timing-samples", flag])
                .err()
                .unwrap();
            assert!(
                error.contains("research CLI moved to archive/"),
                "{flag}: {error}"
            );
        }
    }
}
