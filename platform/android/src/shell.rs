//! Android window geometry: the Primary region the Runtime document fills.
//!
//! GameActivity lays the surface out edge to edge, under the status bar,
//! the navigation bar and the soft keyboard. Primary is what is left once
//! those bands are taken off: the Runtime document pads its root by them, so
//! its content sits in Primary while the theme background still covers the
//! whole surface. Touches outside Primary stay with the system.

use nana_ui_core::PhysicalRect;

/// Android mdpi baseline: `densityDpi == 160` → scale factor `1.0`.
pub const ANDROID_MDPI_DPI: u32 = 160;

/// Fallback when [`scale_factor_from_density_dpi`] has no usable density.
///
/// Matches the historical stub default (xhdpi / 320dpi). Host CI without an
/// Android configuration keeps this value until `resize` injects a density.
pub const DEFAULT_ANDROID_SCALE_FACTOR: f32 = 2.0;

/// Derive window scale from Android `Configuration` densityDpi.
///
/// `Some(dpi)` → `dpi / 160` (clamped to ≥ 0.25). `None`, `0`, or non-finite
/// results fall back to [`DEFAULT_ANDROID_SCALE_FACTOR`].
pub fn scale_factor_from_density_dpi(density_dpi: Option<u32>) -> f32 {
    match density_dpi {
        Some(dpi) if dpi > 0 => {
            let scale = dpi as f32 / ANDROID_MDPI_DPI as f32;
            if scale.is_finite() && scale > 0.0 {
                scale.max(0.25)
            } else {
                DEFAULT_ANDROID_SCALE_FACTOR
            }
        }
        _ => DEFAULT_ANDROID_SCALE_FACTOR,
    }
}

/// System bands over the edge-to-edge surface, in physical px.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SystemInsets {
    /// Status bar and display cutout.
    pub top: u32,
    /// Navigation bar, or the soft keyboard while it is taller.
    pub bottom: u32,
}

/// The part of a `physical_size` surface below `insets.top` and above
/// `insets.bottom`. Never empty: insets that cover the surface leave one row.
pub fn primary_rect(physical_size: (u32, u32), insets: SystemInsets) -> PhysicalRect {
    let (width, height) = (physical_size.0.max(1), physical_size.1.max(1));
    let top = insets.top.min(height - 1);
    let bottom = insets.bottom.min(height - top - 1);
    PhysicalRect {
        x: 0,
        y: top,
        width,
        height: height - top - bottom,
    }
}

/// Window size, scale and system insets; Primary derives from them.
#[derive(Debug, Clone)]
pub struct AndroidShellStub {
    physical_size: (u32, u32),
    scale_factor: f32,
    insets: SystemInsets,
}

impl Default for AndroidShellStub {
    fn default() -> Self {
        Self::new()
    }
}

impl AndroidShellStub {
    pub fn new() -> Self {
        Self {
            physical_size: (720, 1280),
            scale_factor: DEFAULT_ANDROID_SCALE_FACTOR,
            insets: SystemInsets::default(),
        }
    }

    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    /// The surface size: NativeWindow, wgpu framebuffer and MotionEvent
    /// share this pixel space.
    pub fn physical_size(&self) -> (u32, u32) {
        self.physical_size
    }

    pub fn insets(&self) -> SystemInsets {
        self.insets
    }

    /// Record the native window size (**physical** px) and scale.
    pub fn resize(&mut self, physical_width: u32, physical_height: u32, scale_factor: f32) {
        self.physical_size = (physical_width.max(1), physical_height.max(1));
        self.scale_factor = scale_factor.max(0.25);
    }

    pub fn set_insets(&mut self, insets: SystemInsets) {
        self.insets = insets;
    }

    /// The surface below the status bar and above the navigation bar or soft
    /// keyboard. Never empty: insets that cover the window leave one row.
    pub fn primary(&self) -> PhysicalRect {
        primary_rect(self.physical_size, self.insets)
    }

    /// Whether Nana [`nana_ui::runtime::DesktopShell`] rendering is wired on this target.
    pub const fn desktop_shell_available() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slot_input::pointer_in_slot;

    #[test]
    fn scale_factor_from_density_dpi_uses_mdpi_baseline() {
        assert!((scale_factor_from_density_dpi(Some(160)) - 1.0).abs() < f32::EPSILON);
        assert!((scale_factor_from_density_dpi(Some(320)) - 2.0).abs() < f32::EPSILON);
        assert!((scale_factor_from_density_dpi(Some(480)) - 3.0).abs() < f32::EPSILON);
        assert!((scale_factor_from_density_dpi(Some(213)) - (213.0 / 160.0)).abs() < 0.001);
    }

    #[test]
    fn scale_factor_from_density_dpi_falls_back_when_missing() {
        assert!(
            (scale_factor_from_density_dpi(None) - DEFAULT_ANDROID_SCALE_FACTOR).abs()
                < f32::EPSILON
        );
        assert!(
            (scale_factor_from_density_dpi(Some(0)) - DEFAULT_ANDROID_SCALE_FACTOR).abs()
                < f32::EPSILON
        );
    }

    #[test]
    fn primary_is_the_whole_window_without_insets() {
        let mut shell = AndroidShellStub::new();
        shell.resize(1080, 1920, 2.0);
        assert_eq!(shell.physical_size(), (1080, 1920));
        assert_eq!(
            shell.primary(),
            PhysicalRect {
                x: 0,
                y: 0,
                width: 1080,
                height: 1920,
            }
        );
        assert!(!AndroidShellStub::desktop_shell_available());
    }

    #[test]
    fn primary_sits_below_the_status_bar_and_above_the_navigation_bar() {
        let mut shell = AndroidShellStub::new();
        shell.resize(1080, 1920, 2.0);
        shell.set_insets(SystemInsets {
            top: 63,
            bottom: 126,
        });
        let primary = shell.primary();
        assert_eq!((primary.x, primary.y), (0, 63));
        assert_eq!(primary.width, 1080);
        assert_eq!(primary.y + primary.height, 1920 - 126);
        assert!(!pointer_in_slot(primary, 540.0, 10.0), "status bar");
        assert!(!pointer_in_slot(primary, 540.0, 1900.0), "navigation bar");
        assert!(pointer_in_slot(primary, 540.0, 960.0));
    }

    #[test]
    fn primary_never_collapses_under_oversized_insets() {
        let mut shell = AndroidShellStub::new();
        shell.resize(1080, 200, 2.0);
        shell.set_insets(SystemInsets {
            top: 150,
            bottom: 900,
        });
        let primary = shell.primary();
        assert_eq!(primary.height, 1);
        assert!(primary.y + primary.height <= 200);
    }

    #[test]
    fn density_changes_scale_but_not_the_physical_window() {
        for dpi in [160u32, 320, 480] {
            let scale = scale_factor_from_density_dpi(Some(dpi));
            let mut shell = AndroidShellStub::new();
            shell.resize(1080, 1920, scale);
            assert_eq!(shell.physical_size(), (1080, 1920));
            assert!((shell.scale_factor() - scale).abs() < f32::EPSILON);
            assert_eq!(shell.primary().height, 1920);
        }
    }
}
