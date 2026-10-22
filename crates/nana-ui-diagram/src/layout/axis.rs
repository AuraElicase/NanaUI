//! Scales for the chart-like families: linear value axes with round ticks
//! (the chart crate's ECharts-compatible steps) and band axes for
//! categories.

use nana_ui_charts::scale::{LinearTicks, format_number, nice_linear};

/// A linear value scale mapped onto a pixel range.
#[derive(Debug, Clone, PartialEq)]
pub struct LinearScale {
    pub ticks: LinearTicks,
    pub range: [f32; 2],
}

impl LinearScale {
    /// Round ticks covering `min..=max`, about `count` intervals.
    pub fn nice(min: f64, max: f64, count: usize, range: [f32; 2]) -> Self {
        Self {
            ticks: nice_linear(min, max, count, None, None),
            range,
        }
    }

    /// Exactly `min..=max`, ticks inside it.
    pub fn fixed(min: f64, max: f64, count: usize, range: [f32; 2]) -> Self {
        Self {
            ticks: nice_linear(min, max, count, Some(min), Some(max)),
            range,
        }
    }

    pub fn map(&self, value: f64) -> f32 {
        let span = self.ticks.max - self.ticks.min;
        let t = if span.abs() < f64::EPSILON {
            0.5
        } else {
            (value - self.ticks.min) / span
        };
        self.range[0] + (self.range[1] - self.range[0]) * t as f32
    }

    pub fn label(&self, value: f64) -> String {
        format_number(value, self.ticks.step)
    }
}

/// Evenly spaced category bands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandScale {
    pub count: usize,
    pub range: [f32; 2],
    /// Fraction of each band left empty, split between both sides.
    pub padding: f32,
}

impl BandScale {
    pub fn band(&self) -> f32 {
        if self.count == 0 {
            return 0.0;
        }
        (self.range[1] - self.range[0]) / self.count as f32
    }

    /// The start and width of band `i`'s filled part.
    pub fn slot(&self, i: usize) -> (f32, f32) {
        let band = self.band();
        let inner = band * (1.0 - self.padding.clamp(0.0, 0.95));
        (
            self.range[0] + band * i as f32 + (band - inner) / 2.0,
            inner,
        )
    }

    pub fn center(&self, i: usize) -> f32 {
        let band = self.band();
        self.range[0] + band * (i as f32 + 0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_scales_round_and_map() {
        let s = LinearScale::nice(0.0, 97.0, 5, [0.0, 100.0]);
        assert_eq!(s.ticks.max, 100.0);
        assert_eq!(s.map(50.0), 50.0);
        let b = BandScale {
            count: 4,
            range: [0.0, 100.0],
            padding: 0.2,
        };
        assert_eq!(b.center(1), 37.5);
        assert!((b.slot(0).1 - 20.0).abs() < 1e-4);
    }
}
