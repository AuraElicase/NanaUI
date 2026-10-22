//! Renders Mermaid fixtures through NativeMarkdown and the real scene and GPU
//! path, light and dark, at 1× and 2×, one PNG each.
//!
//! `cargo run -p nana-ui-devtools --features runtime-agent,nana-ui/components
//! --example diagram-probe -- <output-dir> [fixture.mmd | dir]...`
//!
//! Without fixture arguments it renders every `.mmd` under
//! `crates/nana-ui-runtime/tests/fixtures/mermaid` and
//! `crates/nana-ui-diagram/tests/fixtures`.
use nana_ui::runtime::*;
use nana_ui_devtools::{
    agent::RuntimeAgentSession,
    offscreen::{self, OffscreenSnapshots, Size},
};
use std::path::{Path, PathBuf};

const WIDTH: u32 = 900;

fn fixtures(args: &[String]) -> Vec<PathBuf> {
    let roots: Vec<PathBuf> = if args.is_empty() {
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        vec![
            crates.join("nana-ui-runtime/tests/fixtures/mermaid"),
            crates.join("nana-ui-diagram/tests/fixtures"),
        ]
    } else {
        args.iter().map(PathBuf::from).collect()
    };
    let mut out = Vec::new();
    for root in roots {
        if root.is_file() {
            out.push(root);
            continue;
        }
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "mmd") {
                    out.push(path);
                }
            }
        }
    }
    out.sort();
    out
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let output = PathBuf::from(
        args.first()
            .cloned()
            .unwrap_or_else(|| "target/diagram-probe".into()),
    );
    std::fs::create_dir_all(&output)?;
    let mut gpu = OffscreenSnapshots::new()?;
    for fixture in fixtures(args.get(1..).unwrap_or_default()) {
        let source = std::fs::read_to_string(&fixture)?;
        let stem = fixture
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("diagram");
        let family = fixture
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("mermaid");
        let markdown_source = format!("```mermaid\n{}\n```", source.trim_end());
        for (theme_name, theme) in [
            ("light", nana_ui::ThemeAppearance::Light),
            ("dark", nana_ui::ThemeAppearance::Dark),
        ] {
            // Lay out once to learn the height the document needs.
            let id = DocumentId::new(7).unwrap();
            let build = |height: u32,
                         scale: f32|
             -> Result<
                (RuntimeAgentSession, StableNodeId),
                Box<dyn std::error::Error>,
            > {
                let mut document = RuntimeDocument::new(id);
                let cx = document.context_mut();
                cx.set_preset_theme(theme)?;
                let root = cx.create_component(id, Stack::column(0.0).padding(16.0))?;
                let body = cx
                    .create_detached_component(id, NativeMarkdown::from_source(&markdown_source))?;
                cx.append_child(root, body)?;
                let session = RuntimeAgentSession::new_scaled(document, WIDTH, height, scale)?;
                Ok((session, body.stable_id()))
            };
            let (mut probe, body) = build(4000, 1.0)?;
            probe.flush()?;
            let content = probe
                .document()
                .context()
                .world()
                .layout_box(body)
                .map_or(400.0, |b| b.height);
            let height = (content + 32.0).ceil().clamp(64.0, 4000.0) as u32;
            for scale in [1.0f32, 2.0] {
                let (mut session, _) = build(height, scale)?;
                session.flush()?;
                let renderers = gpu.default_gpu_renderers();
                let size = Size::new(
                    (WIDTH as f32 * scale) as u32,
                    (height as f32 * scale) as u32,
                );
                let pixels = gpu.paint_layers_scaled(
                    &[(session.document().scene(), true)],
                    size,
                    scale,
                    session.clear_color(),
                    None,
                    Some(&renderers),
                )?;
                let name = format!("{family}-{stem}-{theme_name}@{scale}x.png");
                offscreen::write_painter_png(&output.join(&name), size, &pixels)?;
                println!("{name}");
            }
        }
    }
    Ok(())
}
