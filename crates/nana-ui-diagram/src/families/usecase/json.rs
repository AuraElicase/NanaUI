//! A strict JSON object flattened to table rows: `address.city`,
//! `items[0].name`, empty containers as one row.

const MAX_DEPTH: usize = 32;

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Null,
    Bool(bool),
    Number(String),
    Text(String),
    Array(Vec<Value>),
    /// Properties in source order; a repeated key keeps its first place
    /// and takes the last value.
    Object(Vec<(String, Value)>),
}

struct Reader<'s> {
    s: &'s [u8],
    text: &'s str,
    i: usize,
}

impl Reader<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        self.ws();
        if self.s.get(self.i) == Some(&c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn value(&mut self, depth: usize) -> Option<Value> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.ws();
        match *self.s.get(self.i)? {
            b'{' => {
                self.i += 1;
                let mut props: Vec<(String, Value)> = Vec::new();
                if self.eat(b'}') {
                    return Some(Value::Object(props));
                }
                loop {
                    self.ws();
                    let key = self.string()?;
                    if !self.eat(b':') {
                        return None;
                    }
                    let value = self.value(depth + 1)?;
                    match props.iter_mut().find(|(k, _)| *k == key) {
                        Some(slot) => slot.1 = value,
                        None => props.push((key, value)),
                    }
                    if !self.eat(b',') {
                        return self.eat(b'}').then_some(Value::Object(props));
                    }
                }
            }
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                if self.eat(b']') {
                    return Some(Value::Array(items));
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    if !self.eat(b',') {
                        return self.eat(b']').then_some(Value::Array(items));
                    }
                }
            }
            b'"' => self.string().map(Value::Text),
            b't' => self.word("true", Value::Bool(true)),
            b'f' => self.word("false", Value::Bool(false)),
            b'n' => self.word("null", Value::Null),
            b'-' | b'0'..=b'9' => {
                let start = self.i;
                while self.i < self.s.len()
                    && matches!(
                        self.s[self.i],
                        b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'
                    )
                {
                    self.i += 1;
                }
                let n = &self.text[start..self.i];
                n.parse::<f64>().ok().filter(|v| v.is_finite())?;
                Some(Value::Number(n.to_string()))
            }
            _ => None,
        }
    }

    fn word(&mut self, w: &str, v: Value) -> Option<Value> {
        if self.s[self.i..].starts_with(w.as_bytes()) {
            self.i += w.len();
            Some(v)
        } else {
            None
        }
    }

    fn string(&mut self) -> Option<String> {
        if self.s.get(self.i) != Some(&b'"') {
            return None;
        }
        self.i += 1;
        let mut out = String::new();
        loop {
            let rest = self.text.get(self.i..)?;
            let c = rest.chars().next()?;
            self.i += c.len_utf8();
            match c {
                '"' => return Some(out),
                '\\' => {
                    let e = self.text.get(self.i..)?.chars().next()?;
                    self.i += e.len_utf8();
                    match e {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' | 'b' | 'f' => {}
                        'u' => {
                            let hex = self.text.get(self.i..self.i + 4)?;
                            self.i += 4;
                            let code = u32::from_str_radix(hex, 16).ok()?;
                            out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                        }
                        other => out.push(other),
                    }
                }
                c => out.push(c),
            }
        }
    }
}

/// Rows of a JSON object, or `None` when `text` is not one. Stops adding
/// rows past `max`.
pub(super) fn rows(text: &str, max: usize) -> Option<Vec<(String, String)>> {
    let mut r = Reader {
        s: text.as_bytes(),
        text,
        i: 0,
    };
    let value = r.value(0)?;
    r.ws();
    if r.i != r.s.len() || !matches!(value, Value::Object(_)) {
        return None;
    }
    let mut out = Vec::new();
    flatten(&value, String::new(), &mut out, max);
    Some(out)
}

fn flatten(value: &Value, path: String, out: &mut Vec<(String, String)>, max: usize) {
    if out.len() >= max {
        return;
    }
    let leaf = |v: String, out: &mut Vec<(String, String)>| out.push((path.clone(), v));
    match value {
        Value::Null => leaf("null".into(), out),
        Value::Bool(b) => leaf(b.to_string(), out),
        Value::Number(n) => leaf(n.clone(), out),
        Value::Text(t) => leaf(t.clone(), out),
        Value::Array(items) if items.is_empty() => leaf("[]".into(), out),
        Value::Object(props) if props.is_empty() && !path.is_empty() => leaf("{}".into(), out),
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                // Arrays of scalars list their values under one key.
                let scalar = !matches!(item, Value::Array(_) | Value::Object(_));
                let p = match (scalar, i) {
                    (false, _) => format!("{path}[{i}]"),
                    (true, 0) => path.clone(),
                    (true, _) => String::new(),
                };
                flatten(item, p, out, max);
            }
        }
        Value::Object(props) => {
            for (k, v) in props {
                let p = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                flatten(v, p, out, max);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flattens_in_source_order() {
        let rows = rows(
            r#"{ "2": "second", "1": "first", "on": true, "n": 3, "x": null, "colors": ["Red", "Green"],
               "address": { "city": "Oslo" }, "items": [{ "name": "Book" }], "e": {}, "a": [], "2": "again" }"#,
            100,
        )
        .unwrap();
        let keys: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "2",
                "1",
                "on",
                "n",
                "x",
                "colors",
                "",
                "address.city",
                "items[0].name",
                "e",
                "a"
            ]
        );
        assert_eq!(rows[0].1, "again");
        assert_eq!(rows[6].1, "Green");
        assert!(super::rows("[1]", 10).is_none());
        assert!(super::rows("{\"a\": }", 10).is_none());
        assert!(super::rows(&"[".repeat(100), 10).is_none());
    }
}
