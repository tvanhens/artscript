use crate::color;
use crate::value::{Col, Coord, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ease {
    Smooth,
    Linear,
    In,
    Out,
    InOut,
    Hold,
}

pub fn ease_named(name: &str) -> Option<Ease> {
    Some(match name {
        "linear" => Ease::Linear,
        "in" => Ease::In,
        "out" => Ease::Out,
        "in_out" => Ease::InOut,
        "hold" => Ease::Hold,
        _ => return None,
    })
}

fn apply_ease(ease: Ease, s: f64) -> f64 {
    let s = s.clamp(0.0, 1.0);
    match ease {
        Ease::Smooth | Ease::Linear => s,
        Ease::In => s * s,
        Ease::Out => 1.0 - (1.0 - s) * (1.0 - s),
        Ease::InOut => {
            if s < 0.5 {
                2.0 * s * s
            } else {
                1.0 - (-2.0 * s + 2.0).powi(2) / 2.0
            }
        }
        Ease::Hold => 0.0,
    }
}

fn phrase(v: &Value) -> String {
    match v {
        Value::Percent(_) => "a percent".into(),
        Value::Ramp(_) => "a ramp".into(),
        Value::Gradient(_) => "a gradient".into(),
        Value::FieldPaint(..) => "a paint".into(),
        Value::Field(_) | Value::FieldMask(..) => "a field".into(),
        Value::Str(_) => "a string".into(),
        Value::Ref(_) => "a node reference".into(),
        other => other.kind().to_string(),
    }
}

fn stepped(v: &Value) -> bool {
    matches!(v, Value::Bool(_) | Value::Ident(_) | Value::Component(_))
}

fn numeric_coord(c: Coord) -> Result<f64, String> {
    match c {
        Coord::Num(n) => Ok(n),
        Coord::Pct(_) => Err("cannot animate a percent yet".into()),
    }
}

pub fn sample_track(values: &[Value], at: &[f64], eases: &[Ease], u: f64) -> Result<Value, String> {
    if values.is_empty() {
        return Err("`key` needs at least one value".into());
    }
    if at.len() != values.len() {
        return Err(format!(
            "`at:` has {} value(s) for {} key(s)",
            at.len(),
            values.len()
        ));
    }
    if eases.len() != values.len().saturating_sub(1) {
        return Err(format!(
            "`ease:` has {} value(s) for {} segment(s)",
            eases.len(),
            values.len().saturating_sub(1)
        ));
    }
    for (i, t) in at.iter().enumerate() {
        if !(0.0..=1.0).contains(t) || !t.is_finite() {
            return Err(format!(
                "`at:` value {} is outside 0..1",
                crate::value::num(*t)
            ));
        }
        if i > 0 && t < &at[i - 1] {
            return Err("`at:` values must be in order".into());
        }
    }
    let first = &values[0];
    for v in values {
        if phrase(v) != phrase(first) {
            return Err(format!("`key` mixes {} and {}", phrase(first), phrase(v)));
        }
        if !stepped(v)
            && !matches!(
                v,
                Value::Num(_) | Value::Vec2(..) | Value::Size(..) | Value::Color(_)
            )
        {
            return Err(format!("cannot animate {} yet", phrase(v)));
        }
    }
    if values.len() == 1 || u < at[0] {
        return Ok(first.clone());
    }
    if u >= at[at.len() - 1] {
        return Ok(values[values.len() - 1].clone());
    }
    let mut seg = values.len() - 2;
    for k in 0..values.len() - 1 {
        if u < at[k + 1] || k + 1 == values.len() - 1 {
            seg = k;
            break;
        }
    }
    let left = &values[seg];
    let right = &values[seg + 1];
    let width = at[seg + 1] - at[seg];
    if width <= f64::EPSILON || stepped(left) || eases[seg] == Ease::Hold {
        return Ok(left.clone());
    }
    let s = apply_ease(eases[seg], (u - at[seg]) / width);
    if eases[seg] == Ease::Smooth {
        let p = |i: isize| &values[i.clamp(0, values.len() as isize - 1) as usize];
        return cr_value(
            p(seg as isize - 1),
            p(seg as isize),
            p(seg as isize + 1),
            p(seg as isize + 2),
            s,
        );
    }
    lerp_value(left, right, s)
}

fn cr(p0: f64, p1: f64, p2: f64, p3: f64, s: f64) -> f64 {
    let s2 = s * s;
    let s3 = s2 * s;
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * s
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * s2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * s3)
}

fn cr_value(p0: &Value, p1: &Value, p2: &Value, p3: &Value, s: f64) -> Result<Value, String> {
    let scalar = |a: &Value, b: &Value, c: &Value, d: &Value| -> Result<f64, String> {
        match (a, b, c, d) {
            (Value::Num(a), Value::Num(b), Value::Num(c), Value::Num(d)) => {
                Ok(cr(*a, *b, *c, *d, s))
            }
            _ => Err(format!("cannot animate {} yet", phrase(b))),
        }
    };
    match p1 {
        Value::Num(_) => Ok(Value::Num(scalar(p0, p1, p2, p3)?)),
        Value::Vec2(..) | Value::Size(..) => {
            let comp = |f: fn(&Value) -> Result<f64, String>| -> Result<f64, String> {
                Ok(cr(f(p0)?, f(p1)?, f(p2)?, f(p3)?, s))
            };
            let x = |v: &Value| numeric_coord(v.pair().ok_or_else(|| phrase(v))?.0);
            let y = |v: &Value| numeric_coord(v.pair().ok_or_else(|| phrase(v))?.1);
            let (x, y) = (comp(x)?, comp(y)?);
            let pair = (Coord::Num(x), Coord::Num(y));
            Ok(if matches!(p1, Value::Size(..)) {
                Value::Size(pair.0, pair.1)
            } else {
                Value::Vec2(pair.0, pair.1)
            })
        }
        Value::Color(c) => {
            let labs = [p0, p1, p2, p3].map(color_of);
            let l = cr(labs[0].0, labs[1].0, labs[2].0, labs[3].0, s);
            let a = cr(labs[0].1, labs[1].1, labs[2].1, labs[3].1, s);
            let b = cr(labs[0].2, labs[1].2, labs[2].2, labs[3].2, s);
            let alpha = cr(labs[0].3, labs[1].3, labs[2].3, labs[3].3, s);
            let _ = c;
            Ok(Value::Color(Col::plain(color::from_oklab(l, a, b, alpha))))
        }
        other => Err(format!("cannot animate {} yet", phrase(other))),
    }
}

fn color_of(v: &Value) -> (f64, f64, f64, f64) {
    match v {
        Value::Color(c) => {
            let [l, a, b] = color::oklab(c.rgba);
            (l, a, b, c.rgba.a)
        }
        _ => (0.0, 0.0, 0.0, 1.0),
    }
}

fn lerp_value(a: &Value, b: &Value, s: f64) -> Result<Value, String> {
    let n = |x: f64, y: f64| x + (y - x) * s;
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => Ok(Value::Num(n(*x, *y))),
        (Value::Vec2(..) | Value::Size(..), Value::Vec2(..) | Value::Size(..)) => {
            let (ax, ay) = a.pair().ok_or_else(|| phrase(a))?;
            let (bx, by) = b.pair().ok_or_else(|| phrase(b))?;
            let pair = (
                Coord::Num(n(numeric_coord(ax)?, numeric_coord(bx)?)),
                Coord::Num(n(numeric_coord(ay)?, numeric_coord(by)?)),
            );
            Ok(if matches!(a, Value::Size(..)) {
                Value::Size(pair.0, pair.1)
            } else {
                Value::Vec2(pair.0, pair.1)
            })
        }
        (Value::Color(x), Value::Color(y)) => {
            let [l0, a0, b0] = color::oklab(x.rgba);
            let [l1, a1, b1] = color::oklab(y.rgba);
            Ok(Value::Color(Col::plain(color::from_oklab(
                n(l0, l1),
                n(a0, a1),
                n(b0, b1),
                n(x.rgba.a, y.rgba.a),
            ))))
        }
        _ => Err(format!("cannot animate {} yet", phrase(a))),
    }
}

pub fn even_at(n: usize) -> Vec<f64> {
    if n <= 1 {
        return vec![0.0; n];
    }
    (0..n).map(|i| i as f64 / (n - 1) as f64).collect()
}

pub fn frame_delays(frame_count: usize, dur: f64) -> Result<Vec<u16>, &'static str> {
    if frame_count == 0 {
        return Ok(Vec::new());
    }
    let total = (dur * 100.0).round().max(0.0) as u64;
    let n = frame_count as u64;
    let mut acc = 0u64;
    let mut delays = Vec::with_capacity(frame_count);
    for _ in 0..frame_count {
        acc += total;
        let d = acc / n;
        acc %= n;
        delays.push(d as u16);
    }
    if dur > 0.0 && delays.contains(&0) {
        return Err("a gif has more frames than centiseconds");
    }
    Ok(delays)
}

pub fn frame_count(dur: f64, fps: f64) -> usize {
    if dur <= 0.0 || fps <= 0.0 {
        return 0;
    }
    let mut n = 0usize;
    while (n as f64) / fps < dur {
        n += 1;
        if n > 100_000 {
            break;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgba;

    fn nums(xs: &[f64]) -> Vec<Value> {
        xs.iter().copied().map(Value::Num).collect()
    }

    fn smooth(n: usize) -> Vec<Ease> {
        vec![Ease::Smooth; n.saturating_sub(1)]
    }

    #[test]
    fn two_point_smooth_matches_linear_midpoint() {
        let v = sample_track(&nums(&[0.0, 10.0]), &even_at(2), &smooth(2), 0.5).unwrap();
        assert_eq!(v.num(), Some(5.0));
    }

    #[test]
    fn three_point_catmull_at_quarter() {
        let v = sample_track(&nums(&[0.0, 10.0, 0.0]), &even_at(3), &smooth(3), 0.25).unwrap();
        assert!(
            (v.num().unwrap() - 5.625).abs() < 1e-9,
            "{}",
            v.num().unwrap()
        );
    }

    #[test]
    fn easings_and_hold_and_step() {
        let at = even_at(2);
        let vals = nums(&[0.0, 10.0]);
        let one = |e| sample_track(&vals, &at, &[e], 0.5).unwrap().num().unwrap();
        assert!((one(Ease::Linear) - 5.0).abs() < 1e-9);
        assert!((one(Ease::In) - 2.5).abs() < 1e-9);
        assert!((one(Ease::Out) - 7.5).abs() < 1e-9);
        assert!((one(Ease::InOut) - 5.0).abs() < 1e-9);
        assert_eq!(one(Ease::Hold), 0.0);
        let words = vec![Value::Ident("red".into()), Value::Ident("blue".into())];
        let stepped = sample_track(&words, &at, &smooth(2), 0.9).unwrap();
        assert_eq!(stepped.word(), Some("red"));
        let landed = sample_track(&words, &at, &smooth(2), 1.0).unwrap();
        assert_eq!(landed.word(), Some("blue"));
    }

    #[test]
    fn colors_move_in_oklab_and_lists_do_not() {
        let cols = vec![
            Value::Color(Col::plain(Rgba::hex("ff0000").unwrap())),
            Value::Color(Col::plain(Rgba::hex("0000ff").unwrap())),
        ];
        let mid = sample_track(&cols, &even_at(2), &[Ease::Linear], 0.5).unwrap();
        let Value::Color(c) = mid else {
            panic!("color")
        };
        assert!(c.rgba.r > 0.0 && c.rgba.b > 0.0 && c.rgba.g < c.rgba.r);
        let err = sample_track(
            &[Value::List(vec![]), Value::List(vec![])],
            &even_at(2),
            &smooth(2),
            0.5,
        )
        .unwrap_err();
        assert!(err.contains("cannot animate a list"), "{err}");
        let pct = sample_track(
            &[Value::Percent(0.1), Value::Percent(0.2)],
            &even_at(2),
            &smooth(2),
            0.5,
        )
        .unwrap_err();
        assert!(pct.contains("a percent"), "{pct}");
    }

    #[test]
    fn twenty_four_fps_alternates_four_and_five() {
        let d = frame_delays(24, 1.0).unwrap();
        assert_eq!(d.iter().map(|n| *n as u32).sum::<u32>(), 100);
        assert!(d.iter().all(|n| *n == 4 || *n == 5));
        assert_eq!(&d[..6], &[4, 4, 4, 4, 4, 5]);
        assert_eq!(frame_delays(5, 0.2).unwrap(), vec![4, 4, 4, 4, 4]);
        assert_eq!(frame_count(0.2, 24.0), 5);
        assert_eq!(frame_count(2.0, 24.0), 48);
    }

    #[test]
    fn a_duplicated_start_time_uses_the_later_key() {
        let jumped =
            sample_track(&nums(&[0.0, 1.0, 2.0]), &[0.0, 0.0, 1.0], &smooth(3), 0.0).unwrap();
        assert_eq!(jumped.num(), Some(1.0));
        let rest = sample_track(&nums(&[0.0, 10.0]), &[0.0, 1.0], &smooth(2), 0.0).unwrap();
        assert_eq!(rest.num(), Some(0.0));
        let before = sample_track(&nums(&[4.0, 10.0]), &[0.2, 1.0], &smooth(2), 0.1).unwrap();
        assert_eq!(before.num(), Some(4.0));
    }

    #[test]
    fn more_frames_than_centiseconds_is_an_error() {
        assert_eq!(
            frame_delays(120, 1.0).unwrap_err(),
            "a gif has more frames than centiseconds"
        );
        assert_eq!(
            frame_delays(101, 1.0).unwrap_err(),
            "a gif has more frames than centiseconds"
        );
        let fitted = frame_delays(100, 1.0).unwrap();
        assert_eq!(fitted.len(), 100);
        assert_eq!(fitted.iter().map(|n| *n as u32).sum::<u32>(), 100);
    }
}
