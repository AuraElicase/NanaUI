//! Parse and layout timings over synthetic graphs, as JSON lines.

use std::time::Instant;

use nana_ui_diagram::{DiagramMetrics, FixedMeasure, Limits, layout, parse};

fn chain(n: usize) -> String {
    let mut s = String::from("flowchart TD\n");
    for i in 0..n {
        s.push_str(&format!("n{i}[Node {i}] --> n{}[Node {}]\n", i + 1, i + 1));
    }
    s
}

fn fan(n: usize) -> String {
    let mut s = String::from("flowchart LR\n");
    for i in 0..n {
        s.push_str(&format!("hub --> leaf{i}[Leaf {i}]\n"));
    }
    s
}

fn dag(n: usize, seed: u64) -> String {
    let mut s = String::from("flowchart TD\n");
    let mut x = seed;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for i in 1..n {
        let parents = 1 + (next() % 2) as usize;
        for _ in 0..parents {
            let p = (next() as usize) % i;
            s.push_str(&format!("n{p}[Task {p}] -->|e| n{i}[Task {i}]\n"));
        }
    }
    s
}

fn clusters(groups: usize, per: usize) -> String {
    let mut s = String::from("flowchart LR\n");
    for g in 0..groups {
        s.push_str(&format!("subgraph g{g}[Group {g}]\n"));
        for i in 0..per {
            s.push_str(&format!("g{g}n{i} --> g{g}n{}\n", i + 1));
        }
        s.push_str("end\n");
        if g > 0 {
            s.push_str(&format!("g{}n{per} --> g{g}n0\n", g - 1));
        }
    }
    s
}

fn main() {
    let cases = [
        ("chain-100", chain(100)),
        ("chain-150", chain(150)),
        ("fan-200", fan(200)),
        ("dag-50", dag(50, 7)),
        ("dag-300", dag(300, 11)),
        ("dag-600", dag(600, 13)),
        ("clusters-20x8", clusters(20, 8)),
    ];
    // Timing, not acceptance: a wide synthetic graph may pass the canvas
    // bound a document would enforce.
    let limits = Limits {
        canvas: 1.0e6,
        ..Limits::DEFAULT
    };
    let metrics = DiagramMetrics::DEFAULT;
    for (name, source) in cases {
        let runs = 5;
        let mut parse_us = Vec::new();
        let mut layout_us = Vec::new();
        let mut items = 0;
        for _ in 0..runs {
            let t = Instant::now();
            let parsed = match parse(&source, &limits) {
                Ok(parsed) => parsed,
                Err(error) => {
                    println!("{{\"case\":\"{name}\",\"error\":\"{error}\"}}");
                    break;
                }
            };
            parse_us.push(t.elapsed().as_micros());
            let t = Instant::now();
            let scene = match layout(&parsed, &FixedMeasure, &metrics, &limits) {
                Ok(scene) => scene,
                Err(error) => {
                    println!("{{\"case\":\"{name}\",\"error\":\"{error}\"}}");
                    break;
                }
            };
            layout_us.push(t.elapsed().as_micros());
            items = scene.items.len();
        }
        if layout_us.len() < runs {
            continue;
        }
        parse_us.sort_unstable();
        layout_us.sort_unstable();
        println!(
            "{{\"case\":\"{name}\",\"parse_us\":{},\"layout_us\":{},\"items\":{items}}}",
            parse_us[runs / 2],
            layout_us[runs / 2]
        );
    }
}
