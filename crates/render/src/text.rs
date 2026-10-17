//! Number and id formatting shared by the SVG writer. Deterministic, no trailing zeros.

pub fn fmt_num(v: f32) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let rounded = (v as f64 * 1000.0).round() / 1000.0;
    if rounded == 0.0 {
        return "0".into();
    }
    let mut s = format!("{rounded:.3}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s
}

/// XML Name-ish id. Dots in node paths stay, so `hero.body` remains readable.
/// `~` becomes `-`, so `circle~1` (`circle-1`) does not collide with an author name `circle_1`.
pub fn xml_id(path: &str) -> String {
    let mut s: String = path
        .chars()
        .map(|c| {
            if c == '~' {
                '-'
            } else if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        s.push('n');
    }
    if s.starts_with(|c: char| c.is_ascii_digit() || c == '.' || c == '-') {
        s.insert(0, 'n');
    }
    s
}

pub fn base64(data: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= data.len() {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | data[i + 2] as u32;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
        out.push(TABLE[(n & 63) as usize] as char);
        i += 3;
    }
    let rest = data.len() - i;
    if rest == 1 {
        let n = (data[i] as u32) << 16;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rest == 2 {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8);
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
        out.push('=');
    }
    out
}
