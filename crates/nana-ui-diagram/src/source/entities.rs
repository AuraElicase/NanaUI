//! Mermaid's `#name;` / `#123;` entity codes and the common HTML entities.

/// Replace entity codes with the characters they name.
pub fn decode(text: &str) -> String {
    if !text.contains(['#', '&']) {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(['#', '&']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if let Some(end) = tail[1..].find(';').map(|e| e + 1)
            && end <= 12
            && let Some(c) = entity(&tail[1..end])
        {
            out.push(c);
            rest = &tail[end + 1..];
            continue;
        }
        out.push(tail.chars().next().unwrap_or('#'));
        rest = &tail[1..];
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    if let Some(num) = name
        .strip_prefix('#')
        .or(Some(name))
        .filter(|n| n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty())
    {
        return num.parse::<u32>().ok().and_then(char::from_u32);
    }
    if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix('x'))
        && !hex.is_empty()
        && hex.chars().all(|c| c.is_ascii_hexdigit())
    {
        return u32::from_str_radix(hex, 16).ok().and_then(char::from_u32);
    }
    Some(match name {
        "quot" => '"',
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "semi" => ';',
        "colon" => ':',
        "num" => '#',
        "copy" => '©',
        "reg" => '®',
        "hearts" => '♥',
        "rarr" => '→',
        "larr" => '←',
        "uarr" => '↑',
        "darr" => '↓',
        "harr" => '↔',
        "infin" => '∞',
        "deg" => '°',
        "plusmn" => '±',
        "times" => '×',
        "divide" => '÷',
        "hellip" => '…',
        "mdash" => '—',
        "ndash" => '–',
        "middot" => '·',
        "bull" => '•',
        "check" => '✓',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_mermaid_and_html_entities() {
        assert_eq!(decode("A #quot;quoted#quot;"), "A \"quoted\"");
        assert_eq!(decode("#9829; &amp; &lt;b&gt;"), "♥ & <b>");
        assert_eq!(decode("issue #12 stays"), "issue #12 stays");
        assert_eq!(decode("#x41;"), "A");
    }
}
