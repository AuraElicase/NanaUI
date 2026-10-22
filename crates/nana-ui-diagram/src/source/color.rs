//! CSS colours authors write in `style` and `classDef`.

/// Straight-alpha sRGB in `0..=1`, or `None` for anything unreadable.
pub fn parse(value: &str) -> Option<[f32; 4]> {
    let value = value.trim().trim_end_matches(";").trim();
    let lower = value.to_ascii_lowercase();
    if let Some(hex) = lower.strip_prefix('#') {
        return parse_hex(hex);
    }
    if let Some(args) = lower
        .strip_prefix("rgba(")
        .or_else(|| lower.strip_prefix("rgb("))
        .and_then(|rest| rest.strip_suffix(')'))
    {
        let parts: Vec<&str> = args
            .split([',', ' ', '/'])
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect();
        if parts.len() < 3 {
            return None;
        }
        let channel = |p: &str| -> Option<f32> {
            if let Some(pct) = p.strip_suffix('%') {
                pct.parse::<f32>().ok().map(|v| v / 100.0)
            } else {
                p.parse::<f32>().ok().map(|v| v / 255.0)
            }
        };
        let alpha = match parts.get(3) {
            Some(p) => match p.strip_suffix('%') {
                Some(pct) => pct.parse::<f32>().ok()? / 100.0,
                None => p.parse::<f32>().ok()?,
            },
            None => 1.0,
        };
        let c = [
            channel(parts[0])?,
            channel(parts[1])?,
            channel(parts[2])?,
            alpha,
        ];
        return c
            .iter()
            .all(|v| v.is_finite())
            .then(|| c.map(|v| v.clamp(0.0, 1.0)));
    }
    named(&lower)
}

fn parse_hex(hex: &str) -> Option<[f32; 4]> {
    let digit = |c: u8| -> Option<f32> { (c as char).to_digit(16).map(|d| d as f32) };
    let b = hex.as_bytes();
    match b.len() {
        3 | 4 => {
            let mut out = [1.0; 4];
            for (i, &c) in b.iter().enumerate() {
                out[i] = digit(c)? * 17.0 / 255.0;
            }
            Some(out)
        }
        6 | 8 => {
            let mut out = [1.0; 4];
            for i in 0..b.len() / 2 {
                out[i] = (digit(b[2 * i])? * 16.0 + digit(b[2 * i + 1])?) / 255.0;
            }
            Some(out)
        }
        _ => None,
    }
}

fn named(name: &str) -> Option<[f32; 4]> {
    let rgb =
        |r: u8, g: u8, b: u8| Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]);
    match name {
        "transparent" | "none" => Some([0.0; 4]),
        "black" => rgb(0, 0, 0),
        "white" => rgb(255, 255, 255),
        "red" => rgb(255, 0, 0),
        "green" => rgb(0, 128, 0),
        "lime" => rgb(0, 255, 0),
        "blue" => rgb(0, 0, 255),
        "yellow" => rgb(255, 255, 0),
        "orange" => rgb(255, 165, 0),
        "purple" => rgb(128, 0, 128),
        "pink" => rgb(255, 192, 203),
        "gray" | "grey" => rgb(128, 128, 128),
        "lightgray" | "lightgrey" => rgb(211, 211, 211),
        "darkgray" | "darkgrey" => rgb(169, 169, 169),
        "silver" => rgb(192, 192, 192),
        "brown" => rgb(165, 42, 42),
        "cyan" | "aqua" => rgb(0, 255, 255),
        "magenta" | "fuchsia" => rgb(255, 0, 255),
        "navy" => rgb(0, 0, 128),
        "teal" => rgb(0, 128, 128),
        "olive" => rgb(128, 128, 0),
        "maroon" => rgb(128, 0, 0),
        "gold" => rgb(255, 215, 0),
        "coral" => rgb(255, 127, 80),
        "salmon" => rgb(250, 128, 114),
        "violet" => rgb(238, 130, 238),
        "indigo" => rgb(75, 0, 130),
        "lightblue" => rgb(173, 216, 230),
        "lightgreen" => rgb(144, 238, 144),
        "darkgreen" => rgb(0, 100, 0),
        "darkblue" => rgb(0, 0, 139),
        "darkred" => rgb(139, 0, 0),
        "skyblue" => rgb(135, 206, 235),
        "steelblue" => rgb(70, 130, 180),
        "tomato" => rgb(255, 99, 71),
        "khaki" => rgb(240, 230, 140),
        "beige" => rgb(245, 245, 220),
        "ivory" => rgb(255, 255, 240),
        "lavender" => rgb(230, 230, 250),
        "crimson" => rgb(220, 20, 60),
        "turquoise" => rgb(64, 224, 208),
        "chocolate" => rgb(210, 105, 30),
        "tan" => rgb(210, 180, 140),
        _ => None,
    }
}

/// Relative luminance, for picking a readable text colour on an author fill.
pub fn luminance(c: [f32; 4]) -> f32 {
    let lin = |v: f32| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_hex_functional_and_named_colours() {
        assert_eq!(parse("#fff"), Some([1.0; 4]));
        assert_eq!(
            parse("#ff000080").map(|c| (c[3] * 255.0).round()),
            Some(128.0)
        );
        assert_eq!(parse("rgb(255, 0, 0)"), Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(parse("rgba(0,0,255,0.5)"), Some([0.0, 0.0, 1.0, 0.5]));
        assert_eq!(parse("Red"), Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(parse("#ggg"), None);
    }
}
