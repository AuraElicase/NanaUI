//! Fixture-only bounded sample storage. No diagnostics, scheduling or QoS changes.
use std::time::Instant;

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
    pub allocator_enabled: bool,
    pub execution_enabled: bool,
}

impl Options {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, &'static str> {
        let mut options = Self {
            enabled: false,
            direct_only: false,
            warmup: 30,
            measured: 240,
            allocator_enabled: true,
            execution_enabled: false,
        };
        let mut override_count = false;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--timing-samples" => options.enabled = true,
                "--execution-samples" => options.execution_enabled = true,
                "--direct-only" => options.direct_only = true,
                "--allocator-observation" => {
                    options.allocator_enabled = match args.next().as_deref() {
                        Some("on") => true,
                        Some("off") => false,
                        _ => return Err("allocator observation must be on or off"),
                    };
                    override_count = true;
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
            return Err("timing count and allocator observation overrides require --direct-only");
        }
        if options.execution_enabled && !(options.enabled && options.direct_only) {
            return Err("execution samples require --timing-samples --direct-only");
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
    pub execution_samples: Option<Vec<ExecutionSample>>,
    pub missing_execution_samples: usize,
    pub dropped_execution_samples: usize,
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
            execution_samples: None,
            missing_execution_samples: 0,
            dropped_execution_samples: 0,
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
        self.record_observed(
            presented_ordinal,
            warmup,
            included,
            wall_ms,
            thread_cpu_ms,
            None,
        );
    }

    pub fn record_observed(
        &mut self,
        presented_ordinal: usize,
        warmup: bool,
        included: bool,
        wall_ms: f64,
        thread_cpu_ms: Option<f64>,
        execution: Option<ExecutionInterval>,
    ) {
        if self.samples.len() == self.samples.capacity() {
            self.dropped_samples += 1;
            if self.execution_samples.is_some() {
                self.dropped_execution_samples += 1;
            }
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
        if let Some(samples) = &mut self.execution_samples {
            if samples.len() == samples.capacity() {
                self.dropped_execution_samples += 1;
                return;
            }
            if execution.as_ref().is_none_or(|value| value.status != "ok") {
                self.missing_execution_samples += 1;
            }
            samples.push(ExecutionSample {
                prepare_ordinal: self.prepare_ordinal,
                presented_ordinal,
                warmup,
                included_in_measurement: included,
                observation: execution,
            });
        }
    }

    pub fn execution_complete(&self) -> bool {
        self.execution_samples.as_ref().is_none_or(|samples| {
            samples.len() == self.samples.len()
                && self.missing_execution_samples == 0
                && self.dropped_execution_samples == 0
        })
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
                let mut value = Phase::new(if phase == 0 {
                    options.warmup + options.measured
                } else if phase == 5 || options.direct_only {
                    0
                } else {
                    30 + 60 + 64
                });
                if options.execution_enabled {
                    value.execution_samples = Some(Vec::with_capacity(value.samples.capacity()));
                }
                value
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

/// Fixture diagnostics only. None is unavailable, never an observed zero.
#[derive(Clone, Copy, serde::Serialize)]
pub struct Observation<T> {
    pub status: &'static str,
    pub reason: Option<&'static str>,
    pub error: Option<i32>,
    pub value: Option<T>,
}

impl<T> Observation<T> {
    fn ok(value: T) -> Self {
        Self {
            status: "ok",
            reason: None,
            error: None,
            value: Some(value),
        }
    }

    fn unavailable(reason: &'static str, error: Option<i32>) -> Self {
        Self {
            status: "unavailable",
            reason: Some(reason),
            error,
            value: None,
        }
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn observe_thread_cpu_time() -> Observation<u64> {
    match mac_thread_clock::read() {
        Ok(value) => match value
            .seconds
            .checked_mul(1_000_000_000)
            .and_then(|s| s.checked_add(u64::from(value.nanos)))
        {
            Some(ns) => Observation::ok(ns),
            None => Observation::unavailable("cpu16-overflow", None),
        },
        Err((reason, error)) => Observation::unavailable(reason, error),
    }
}

#[derive(Clone, Copy, serde::Serialize)]
pub struct Timebase {
    pub numerator: u32,
    pub denominator: u32,
}

/// Pinned diagnostic SPI ABI, not a framework or stable Apple API contract.
/// XNU resource_private.h THSC_TIME_CPI_PER_PERF_LEVEL=4, four u64 fields.
#[repr(C)]
#[derive(Clone, Copy, serde::Serialize)]
pub struct LevelCounter {
    pub instructions: u64,
    pub cycles: u64,
    pub user_mach: u64,
    pub system_mach: u64,
}

const _: [(); 32] = [(); std::mem::size_of::<LevelCounter>()];
const MAX_LEVELS: usize = 8;

#[derive(serde::Serialize)]
pub struct LevelDescription {
    pub level: usize,
    pub name: Observation<String>,
}

#[derive(serde::Serialize)]
pub struct ExecutionMetadata {
    pub status: &'static str,
    pub reason: Option<&'static str>,
    pub error: Option<i32>,
    pub process_id: u32,
    pub thread_id: Observation<u64>,
    pub clock: &'static str,
    pub timebase: Observation<Timebase>,
    pub levels: Vec<LevelDescription>,
    pub effective_qos: Observation<Qos>,
    pub abi_source: &'static str,
    pub scope: &'static str,
    pub snapshot_order: &'static str,
}

impl ExecutionMetadata {
    fn unavailable(reason: &'static str) -> Self {
        Self {
            status: "unavailable",
            reason: Some(reason),
            error: None,
            process_id: std::process::id(),
            thread_id: Observation::unavailable(reason, None),
            clock: "mach_absolute_time raw ticks; machine-boot epoch; not UTC or trace-relative ns",
            timebase: Observation::unavailable(reason, None),
            levels: Vec::new(),
            effective_qos: Observation::unavailable(
                "public-pthread-getter-reports-requested-qos-only",
                None,
            ),
            abi_source: "https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/sys/resource_private.h#L47-L84",
            scope: "opt-in diagnostic; cumulative thread execution attribution, not instantaneous core, migration order, frequency or GPU time; snapshots do not bracket an earlier Instant::now wall endpoint",
            snapshot_order: "mach-before, requested-qos, CPU16, kind4, mach-after; distinct observations, observer cost included",
        }
    }
}

#[derive(Clone, Copy, serde::Serialize)]
pub struct ClockAnchor {
    pub mach_ticks: Option<u64>,
    pub thread_id: Option<u64>,
}

#[derive(Clone, Copy, serde::Serialize)]
pub struct ExecutionSnapshot {
    pub status: &'static str,
    pub mach_before: Option<u64>,
    pub mach_after: Option<u64>,
    pub thread_id: Option<u64>,
    pub thread_cpu_ns: Observation<u64>,
    pub requested_qos: Observation<Qos>,
    pub effective_qos: Observation<Qos>,
    pub level_count: usize,
    pub levels: Observation<[Option<LevelCounter>; MAX_LEVELS]>,
}

impl ExecutionSnapshot {
    fn unavailable(reason: &'static str) -> Self {
        Self {
            status: "unavailable",
            mach_before: None,
            mach_after: None,
            thread_id: None,
            thread_cpu_ns: Observation::unavailable(reason, None),
            requested_qos: Observation::unavailable(reason, None),
            effective_qos: Observation::unavailable(
                "public-pthread-getter-reports-requested-qos-only",
                None,
            ),
            level_count: 0,
            levels: Observation::unavailable(reason, None),
        }
    }
}

#[derive(Clone, Copy, serde::Serialize)]
pub struct ExecutionInterval {
    pub status: &'static str,
    pub reason: Option<&'static str>,
    pub start: Option<ExecutionSnapshot>,
    pub end: ExecutionSnapshot,
    pub thread_cpu_ns: Option<u64>,
    pub level_deltas: Option<[Option<LevelCounter>; MAX_LEVELS]>,
    pub start_observer_ticks: Option<u64>,
    pub end_observer_ticks: Option<u64>,
}

#[derive(serde::Serialize)]
pub struct ExecutionSample {
    pub prepare_ordinal: usize,
    pub presented_ordinal: usize,
    pub warmup: bool,
    pub included_in_measurement: bool,
    pub observation: Option<ExecutionInterval>,
}

pub struct ExecutionObserver {
    pub metadata: ExecutionMetadata,
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    native: Option<mac_execution::Native>,
}

impl Default for ExecutionObserver {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionObserver {
    /// Cold, explicit opt-in initialization only. No scheduler/QoS mutation.
    pub fn new() -> Self {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            let (metadata, native) = mac_execution::initialize();
            Self { metadata, native }
        }
        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        Self {
            metadata: ExecutionMetadata::unavailable("requires-arm64-macos-kind4-diagnostic"),
        }
    }

    /// One raw clock read for an external sparse-marker or wall-endpoint bracket.
    /// Trace-relative timestamp calibration must be established independently.
    pub fn clock_anchor(&self) -> ClockAnchor {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        if let Some(native) = &self.native {
            return native.anchor();
        }
        ClockAnchor {
            mach_ticks: None,
            thread_id: None,
        }
    }

    pub fn snapshot(&self) -> ExecutionSnapshot {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        if let Some(native) = &self.native {
            return native.snapshot();
        }
        ExecutionSnapshot::unavailable(self.metadata.reason.unwrap_or("observer-unavailable"))
    }

    pub fn interval(
        &self,
        start: Option<ExecutionSnapshot>,
        end: ExecutionSnapshot,
    ) -> ExecutionInterval {
        checked_interval(
            start,
            end,
            self.metadata.thread_id.value,
            self.metadata.timebase.value,
        )
    }
}

fn checked_interval(
    start: Option<ExecutionSnapshot>,
    end: ExecutionSnapshot,
    owner: Option<u64>,
    timebase: Option<Timebase>,
) -> ExecutionInterval {
    let mut interval = ExecutionInterval {
        status: "unavailable",
        reason: Some("missing-start"),
        start,
        end,
        thread_cpu_ns: None,
        level_deltas: None,
        start_observer_ticks: start.and_then(|s| s.mach_after?.checked_sub(s.mach_before?)),
        end_observer_ticks: end
            .mach_after
            .and_then(|after| after.checked_sub(end.mach_before?)),
    };
    let Some(start) = start else { return interval };
    let validate = || -> Result<(u64, [Option<LevelCounter>; MAX_LEVELS]), &'static str> {
        if start.status != "ok" || end.status != "ok" {
            return Err("snapshot-unavailable");
        }
        if start.thread_cpu_ns.status != "ok"
            || end.thread_cpu_ns.status != "ok"
            || start.levels.status != "ok"
            || end.levels.status != "ok"
            || start.requested_qos.status != "ok"
            || end.requested_qos.status != "ok"
        {
            return Err("snapshot-observation-unavailable");
        }
        if owner.is_none() || start.thread_id != owner || end.thread_id != owner {
            return Err("thread-changed");
        }
        if timebase.is_none_or(|t| t.numerator == 0 || t.denominator == 0) {
            return Err("invalid-timebase");
        }
        match (
            start.mach_before,
            start.mach_after,
            end.mach_before,
            end.mach_after,
        ) {
            (Some(a), Some(b), Some(c), Some(d)) if a <= b && b <= c && c <= d => {}
            _ => return Err("clock-nonmonotonic-or-missing"),
        }
        if start.level_count == 0
            || start.level_count > MAX_LEVELS
            || start.level_count != end.level_count
        {
            return Err("level-count-changed");
        }
        let cpu = end
            .thread_cpu_ns
            .value
            .and_then(|v| v.checked_sub(start.thread_cpu_ns.value?))
            .filter(|v| *v > 0)
            .ok_or("cpu-clock-nonmonotonic-or-missing")?;
        let a = start.levels.value.ok_or("start-counters-missing")?;
        let b = end.levels.value.ok_or("end-counters-missing")?;
        let mut deltas = [None; MAX_LEVELS];
        let mut instructions = 0_u64;
        let mut cycles = 0_u64;
        let mut execution = 0_u64;
        for index in 0..start.level_count {
            let a = a[index].ok_or("start-level-missing")?;
            let b = b[index].ok_or("end-level-missing")?;
            let delta = LevelCounter {
                instructions: b
                    .instructions
                    .checked_sub(a.instructions)
                    .ok_or("nonmonotonic-instructions")?,
                cycles: b
                    .cycles
                    .checked_sub(a.cycles)
                    .ok_or("nonmonotonic-cycles")?,
                user_mach: b
                    .user_mach
                    .checked_sub(a.user_mach)
                    .ok_or("nonmonotonic-user-time")?,
                system_mach: b
                    .system_mach
                    .checked_sub(a.system_mach)
                    .ok_or("nonmonotonic-system-time")?,
            };
            instructions = instructions
                .checked_add(delta.instructions)
                .ok_or("instruction-overflow")?;
            cycles = cycles.checked_add(delta.cycles).ok_or("cycle-overflow")?;
            execution = execution
                .checked_add(delta.user_mach)
                .and_then(|n| n.checked_add(delta.system_mach))
                .ok_or("execution-time-overflow")?;
            deltas[index] = Some(delta);
        }
        if instructions == 0 || cycles == 0 || execution == 0 {
            return Err("hardware-counters-unavailable");
        }
        Ok((cpu, deltas))
    };
    match validate() {
        Ok((cpu, deltas)) => {
            interval.status = "ok";
            interval.reason = None;
            interval.thread_cpu_ns = Some(cpu);
            interval.level_deltas = Some(deltas);
        }
        Err(reason) => interval.reason = Some(reason),
    }
    interval
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod mac_execution {
    use super::*;
    use std::ffi::{CStr, CString, c_char, c_int, c_void};

    type CountsFn = unsafe extern "C" fn(u32, *mut c_void, usize) -> c_int;
    #[repr(C)]
    struct MachTimebase {
        numer: u32,
        denom: u32,
    }
    unsafe extern "C" {
        fn mach_absolute_time() -> u64;
        fn mach_timebase_info(info: *mut MachTimebase) -> c_int;
        fn pthread_main_np() -> c_int;
        fn pthread_self() -> *mut c_void;
        fn pthread_equal(a: *mut c_void, b: *mut c_void) -> c_int;
        fn pthread_threadid_np(thread: *mut c_void, id: *mut u64) -> c_int;
        fn pthread_get_qos_class_np(
            thread: *mut c_void,
            class: *mut u32,
            relative: *mut i32,
        ) -> c_int;
        fn sysctlbyname(
            name: *const c_char,
            value: *mut c_void,
            size: *mut usize,
            new: *mut c_void,
            new_size: usize,
        ) -> c_int;
        fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    }

    pub(super) struct Native {
        owner: usize,
        thread_id: u64,
        count: usize,
        counts: CountsFn,
    }

    pub(super) fn initialize() -> (ExecutionMetadata, Option<Native>) {
        let mut metadata = ExecutionMetadata::unavailable("initializing");
        if unsafe { pthread_main_np() } != 1 {
            metadata.reason = Some("initialization-requires-main-thread");
            return (metadata, None);
        }
        let owner = unsafe { pthread_self() };
        let mut id = 0;
        let rc = unsafe { pthread_threadid_np(owner, &mut id) };
        if rc != 0 || id == 0 {
            metadata.thread_id = Observation::unavailable("thread-id-failed", Some(rc));
            metadata.reason = Some("thread-id-failed");
            metadata.error = Some(rc);
            return (metadata, None);
        }
        metadata.thread_id = Observation::ok(id);
        let mut timebase = MachTimebase { numer: 0, denom: 0 };
        let rc = unsafe { mach_timebase_info(&mut timebase) };
        if rc != 0 || timebase.numer == 0 || timebase.denom == 0 {
            metadata.timebase = Observation::unavailable("invalid-mach-timebase", Some(rc));
            metadata.reason = Some("invalid-mach-timebase");
            metadata.error = Some(rc);
            return (metadata, None);
        }
        metadata.timebase = Observation::ok(Timebase {
            numerator: timebase.numer,
            denominator: timebase.denom,
        });
        let mut count = 0_u32;
        let mut size = std::mem::size_of::<u32>();
        let rc = unsafe {
            sysctlbyname(
                c"hw.nperflevels".as_ptr(),
                (&mut count as *mut u32).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0
            || size != std::mem::size_of::<u32>()
            || !(1..=MAX_LEVELS as u32).contains(&count)
        {
            metadata.reason = Some("invalid-perflevel-count");
            metadata.error = (rc != 0).then(mac_thread_clock::errno);
            return (metadata, None);
        }
        for level in 0..count as usize {
            let name = CString::new(format!("hw.perflevel{level}.name")).unwrap();
            let mut bytes = [0_u8; 64];
            let mut size = bytes.len();
            let rc = unsafe {
                sysctlbyname(
                    name.as_ptr(),
                    bytes.as_mut_ptr().cast(),
                    &mut size,
                    std::ptr::null_mut(),
                    0,
                )
            };
            let value = if rc != 0 {
                Observation::unavailable(
                    "perflevel-name-read-failed",
                    Some(mac_thread_clock::errno()),
                )
            } else if size == 0 || size > bytes.len() || bytes[size - 1] != 0 {
                Observation::unavailable("invalid-perflevel-name-size", None)
            } else {
                match CStr::from_bytes_with_nul(&bytes[..size])
                    .ok()
                    .and_then(|s| s.to_str().ok())
                {
                    Some(name) if !name.is_empty() => Observation::ok(name.to_owned()),
                    _ => Observation::unavailable("invalid-perflevel-name", None),
                }
            };
            metadata
                .levels
                .push(LevelDescription { level, name: value });
        }
        if metadata
            .levels
            .iter()
            .any(|level| level.name.value.is_none())
        {
            metadata.reason = Some("perflevel-names-unavailable");
            return (metadata, None);
        }
        // Darwin SDK dlfcn.h: RTLD_DEFAULT = (void *)-2. Lookup is cold only.
        let symbol = unsafe { dlsym((-2_isize) as *mut c_void, c"thread_selfcounts".as_ptr()) };
        if symbol.is_null() {
            metadata.reason = Some("kind4-symbol-unavailable");
            return (metadata, None);
        }
        let counts = unsafe { std::mem::transmute::<*mut c_void, CountsFn>(symbol) };
        metadata.status = "ok";
        metadata.reason = None;
        (
            metadata,
            Some(Native {
                owner: owner as usize,
                thread_id: id,
                count: count as usize,
                counts,
            }),
        )
    }

    impl Native {
        fn same_thread(&self) -> bool {
            unsafe { pthread_equal(self.owner as *mut c_void, pthread_self()) != 0 }
        }

        pub(super) fn anchor(&self) -> ClockAnchor {
            if !self.same_thread() {
                return ClockAnchor {
                    mach_ticks: None,
                    thread_id: None,
                };
            }
            ClockAnchor {
                mach_ticks: Some(unsafe { mach_absolute_time() }),
                thread_id: Some(self.thread_id),
            }
        }

        pub(super) fn snapshot(&self) -> ExecutionSnapshot {
            if !self.same_thread() {
                return ExecutionSnapshot::unavailable("wrong-thread");
            }
            let mut result = ExecutionSnapshot::unavailable("snapshot-failed");
            result.mach_before = Some(unsafe { mach_absolute_time() });
            result.thread_id = Some(self.thread_id);
            let mut class = 0;
            let mut relative_priority = 0;
            let rc = unsafe {
                pthread_get_qos_class_np(
                    self.owner as *mut c_void,
                    &mut class,
                    &mut relative_priority,
                )
            };
            result.requested_qos = if rc == 0 {
                Observation::ok(Qos {
                    requested_class: class,
                    class_name: qos_name(class),
                    relative_priority,
                })
            } else {
                Observation::unavailable("requested-qos-failed", Some(rc))
            };
            result.thread_cpu_ns = observe_thread_cpu_time();
            let sentinel = LevelCounter {
                instructions: u64::MAX,
                cycles: u64::MAX,
                user_mach: u64::MAX,
                system_mach: u64::MAX,
            };
            let mut raw = [sentinel; MAX_LEVELS];
            let rc = unsafe {
                (self.counts)(
                    4,
                    raw.as_mut_ptr().cast(),
                    self.count * std::mem::size_of::<LevelCounter>(),
                )
            };
            result.level_count = self.count;
            result.levels = if rc != 0 {
                Observation::unavailable("kind4-read-failed", Some(mac_thread_clock::errno()))
            } else if raw[..self.count].iter().any(|level| {
                [
                    level.instructions,
                    level.cycles,
                    level.user_mach,
                    level.system_mach,
                ]
                .contains(&u64::MAX)
            }) {
                Observation::unavailable("kind4-unwritten-or-invalid-field", None)
            } else {
                let mut levels = [None; MAX_LEVELS];
                for (index, level) in raw[..self.count].iter().enumerate() {
                    levels[index] = Some(*level);
                }
                Observation::ok(levels)
            };
            result.mach_after = Some(unsafe { mach_absolute_time() });
            if result.thread_cpu_ns.status == "ok"
                && result.requested_qos.status == "ok"
                && result.levels.status == "ok"
            {
                result.status = "ok";
            }
            result
        }
    }

    fn qos_name(class: u32) -> &'static str {
        match class {
            0x21 => "user-interactive",
            0x19 => "user-initiated",
            0x15 => "default",
            0x11 => "utility",
            0x09 => "background",
            0 => "unspecified",
            _ => "unknown",
        }
    }
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
        assert!(!defaults.execution_enabled);
        assert!(options(&["--execution-samples"]).is_err());
        assert!(options(&["--execution-samples", "--direct-only"]).is_err());
        assert!(
            options(&["--execution-samples", "--timing-samples", "--direct-only"])
                .unwrap()
                .execution_enabled
        );
        assert_eq!((defaults.warmup, defaults.measured), (30, 240));
        assert!(options(&["--warmup-frames", "60"]).is_err());
        assert!(defaults.allocator_enabled);
        assert!(options(&["--allocator-observation", "off"]).is_err());
        assert!(options(&["--direct-only", "--allocator-observation", "maybe"]).is_err());
        assert!(
            !options(&["--direct-only", "--allocator-observation", "off"])
                .unwrap()
                .allocator_enabled
        );
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

    fn synthetic_snapshot(end: bool) -> ExecutionSnapshot {
        let level = LevelCounter {
            instructions: if end { 110 } else { 100 },
            cycles: if end { 220 } else { 200 },
            user_mach: if end { 30 } else { 20 },
            system_mach: if end { 15 } else { 10 },
        };
        let mut levels = [None; MAX_LEVELS];
        levels[0] = Some(level);
        levels[1] = Some(LevelCounter {
            instructions: 0,
            cycles: 0,
            user_mach: 0,
            system_mach: 0,
        });
        ExecutionSnapshot {
            status: "ok",
            mach_before: Some(if end { 3 } else { 1 }),
            mach_after: Some(if end { 4 } else { 2 }),
            thread_id: Some(7),
            thread_cpu_ns: Observation::ok(if end { 1200 } else { 1000 }),
            requested_qos: Observation::ok(Qos {
                requested_class: 33,
                class_name: "user-interactive",
                relative_priority: 0,
            }),
            effective_qos: Observation::unavailable("requested-is-not-effective", None),
            level_count: 2,
            levels: Observation::ok(levels),
        }
    }

    fn interval(start: Option<ExecutionSnapshot>, end: ExecutionSnapshot) -> ExecutionInterval {
        checked_interval(
            start,
            end,
            Some(7),
            Some(Timebase {
                numerator: 125,
                denominator: 3,
            }),
        )
    }

    #[test]
    fn execution_delta_keeps_real_inactive_zero_distinct_from_unavailable_and_raw_clock_brackets() {
        let result = interval(Some(synthetic_snapshot(false)), synthetic_snapshot(true));
        assert_eq!(result.status, "ok");
        assert_eq!(result.thread_cpu_ns, Some(200));
        let levels = result.level_deltas.unwrap();
        assert_eq!(levels[0].unwrap().instructions, 10);
        assert_eq!(levels[0].unwrap().user_mach, 10);
        assert_eq!(levels[1].unwrap().instructions, 0);
        assert!(levels[2..].iter().all(Option::is_none));
        assert_eq!(result.start_observer_ticks, Some(1));
        assert_eq!(result.end_observer_ticks, Some(1));
        assert!(result.start.unwrap().effective_qos.value.is_none());
    }

    #[test]
    fn execution_failure_never_publishes_substituted_counter_zero() {
        let start = synthetic_snapshot(false);
        let end = synthetic_snapshot(true);
        let mut cases = vec![interval(None, end)];
        let mut bad = end;
        bad.thread_id = Some(8);
        cases.push(interval(Some(start), bad));
        let mut bad = end;
        bad.mach_before = Some(0);
        cases.push(interval(Some(start), bad));
        let mut bad = end;
        bad.level_count = 3;
        cases.push(interval(Some(start), bad));
        let mut bad = end;
        bad.levels.value.as_mut().unwrap()[0]
            .as_mut()
            .unwrap()
            .instructions = 99;
        cases.push(interval(Some(start), bad));
        let mut bad = end;
        bad.levels.status = "unavailable";
        cases.push(interval(Some(start), bad));
        let mut bad = end;
        bad.thread_cpu_ns.value = None;
        cases.push(interval(Some(start), bad));
        cases.push(checked_interval(
            Some(start),
            end,
            Some(7),
            Some(Timebase {
                numerator: 125,
                denominator: 0,
            }),
        ));
        for case in cases {
            assert_eq!(case.status, "unavailable");
            assert!(case.reason.is_some());
            assert!(case.level_deltas.is_none() && case.thread_cpu_ns.is_none());
        }
    }

    #[test]
    fn execution_storage_is_opt_in_bounded_and_missing_sample_fails_completion() {
        let mut phase = Phase::new(1);
        assert!(phase.execution_samples.is_none());
        phase.prepare(Instant::now());
        phase.record(1, true, false, 1.0, None);
        assert!(phase.execution_samples.is_none());
        let mut observed = Phase::new(1);
        observed.execution_samples = Some(Vec::with_capacity(1));
        observed.prepare(Instant::now());
        observed.record_observed(1, true, false, 1.0, None, None);
        assert!(!observed.execution_complete());
        assert_eq!(observed.missing_execution_samples, 1);
        observed.record_observed(
            2,
            false,
            true,
            1.0,
            None,
            Some(interval(
                Some(synthetic_snapshot(false)),
                synthetic_snapshot(true),
            )),
        );
        assert_eq!(observed.dropped_execution_samples, 1);
        assert_eq!(observed.execution_samples.unwrap().capacity(), 1);
    }
}
