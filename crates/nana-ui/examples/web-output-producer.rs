//! Dedicated Windows DX12 producer for examples/web-output-bridge.
//! Stdout carries handles/metadata only; the product path never reads pixels.
#[cfg(not(all(windows, feature = "native-export")))]
fn main() {
    println!(
        "{}",
        serde_json::json!({"type":"summary","status":"unsupported","reason":"Windows DX12 native-export is required","evidence":null})
    );
    std::process::exit(2);
}

#[cfg(all(windows, feature = "native-export"))]
#[path = "web_output/producer.rs"]
mod producer;

#[cfg(all(windows, feature = "native-export"))]
fn main() {
    if let Err(error) = producer::run() {
        let unsupported = error
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::Unsupported);
        println!(
            "{}",
            serde_json::json!({"type":"summary","status":if unsupported {"unsupported"} else {"error"},"error":error.to_string(),"evidence":null})
        );
        std::process::exit(if unsupported { 2 } else { 1 });
    }
}
