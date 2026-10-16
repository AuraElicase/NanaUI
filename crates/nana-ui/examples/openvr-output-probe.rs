//! Concrete DX12 -> D3D11 -> SteamVR overlay reference. No CPU pixel path.
//! Run on Windows with SteamVR running and an explicit SDK DLL path:
//! `--openvr-dll C:\\OpenVR\\bin\\win64\\openvr_api.dll --output report.json`.
//!
//! The default uses the documented DXGISharedHandle overlay path: one
//! persistent consumer texture updated only by atomic CopyResource. Its
//! identity never retires while streaming. `--immutable-snapshots` instead
//! publishes three immutable revisions then defers. Neither mode infers
//! compositor retirement from an API return or a local GPU fence.

#![recursion_limit = "256"]

#[cfg(windows)]
#[path = "support/openvr_dx11.rs"]
mod platform;

#[derive(Debug)]
pub(crate) enum ProbeError {
    Unsupported(String),
    Failed(String),
}

impl From<String> for ProbeError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

impl From<&str> for ProbeError {
    fn from(message: &str) -> Self {
        Self::Failed(message.into())
    }
}

fn report_result(result: Result<serde_json::Value, ProbeError>) -> (serde_json::Value, i32) {
    match result {
        Ok(report) => (report, 0),
        Err(ProbeError::Unsupported(reason)) => (
            serde_json::json!({"schema_version":1,"fixture":"openvr-native-output",
                "status":"unsupported","evidence":"unavailable","unsupported_reason":reason}),
            2,
        ),
        Err(ProbeError::Failed(error)) => (
            serde_json::json!({"schema_version":1,"fixture":"openvr-native-output",
                "status":"error","evidence":null,"error":error}),
            1,
        ),
    }
}

fn main() {
    #[cfg(windows)]
    let result = platform::run();
    #[cfg(not(windows))]
    let result: Result<serde_json::Value, ProbeError> = Err(ProbeError::Unsupported(
        "this concrete OpenVR reference requires Windows DX12, D3D11 and SteamVR".into(),
    ));
    let (report, code) = report_result(result);
    let mut args = std::env::args().skip(1);
    let mut path = None;
    while let Some(arg) = args.next() {
        if arg == "--output" {
            path = args.next();
        }
    }
    let json = serde_json::to_string_pretty(&report).expect("serialize OpenVR report");
    if let Some(path) = path
        && let Err(error) = std::fs::write(path, &json)
    {
        eprintln!("cannot write OpenVR report: {error}");
        std::process::exit(1);
    }
    println!("{json}");
    // The finite native consumer pool deliberately lives until process exit;
    // an external compositor's completion is not inferred from API returns.
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_faults_cannot_be_downgraded_by_capability_words_in_the_message() {
        for message in [
            "unsupported texture was unexpectedly submitted",
            "missing completion receipt",
            "GPU validation error",
        ] {
            let (report, code) = report_result(Err(message.into()));
            assert_eq!(code, 1);
            assert_eq!(report["status"], "error");
            assert_eq!(report["error"], message);
            assert!(report.get("unsupported_reason").is_none());
            assert!(report["evidence"].is_null());
        }
        let (report, code) = report_result(Err(ProbeError::Unsupported("no DX12 adapter".into())));
        assert_eq!(code, 2);
        assert_eq!(report["status"], "unsupported");
        assert!(report.get("frames_received").is_none());
    }
}
