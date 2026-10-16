//! Bound policy for `a..b`. This file is the only place that compares a range's ends.

use std::fmt;

use crate::value::{Arith, num};

/// The numbers `x` with `lo <= x < hi`. Empty when `hi <= lo`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Range {
    lo: f64,
    hi: f64,
}

impl Range {
    pub(crate) fn new(lo: f64, hi: f64) -> Range {
        Range { lo, hi }
    }

    pub(crate) fn contains(self, x: f64) -> bool {
        // NaN makes both comparisons false, so a NaN bound matches nothing.
        self.lo <= x && x < self.hi
    }

    pub(crate) fn steps(self) -> impl Iterator<Item = f64> {
        let mut x = self.lo;
        let mut more = true;
        std::iter::from_fn(move || {
            if !more || !self.contains(x) {
                return None;
            }
            let current = x;
            let next = x + 1.0;
            if next > x {
                x = next;
            } else {
                // `x + 1` did not increase (infinity or a NaN), so the walk must stop.
                more = false;
            }
            Some(current)
        })
    }

    pub(crate) fn scale(self, op: Arith, k: f64) -> Range {
        match op {
            // Leave a negative `k` reversed. `contains` then matches nothing, same as a descending literal.
            Arith::Mul => Range { lo: self.lo * k, hi: self.hi * k },
            Arith::Div => Range { lo: self.lo / k, hi: self.hi / k },
            Arith::Add | Arith::Sub => panic!("range scale is multiplication or division"),
        }
    }

    pub(crate) fn miss_note(self, x: f64) -> String {
        let note = format!("{} is outside {}", num(x), self);
        if x == self.hi {
            format!("{note} (the end is excluded)")
        } else {
            note
        }
    }
}

impl PartialEq for Range {
    fn eq(&self, other: &Range) -> bool {
        let empty = |r: &Range| r.hi <= r.lo;
        (empty(self) && empty(other)) || (self.lo == other.lo && self.hi == other.hi)
    }
}

impl fmt::Display for Range {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", num(self.lo), num(self.hi))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_stay_inside_contains() {
        let pairs = [(0.0, 3.0), (0.5, 3.2), (1.2, 2.2), (5.0, 5.0), (6.0, 2.0)];
        for (lo, hi) in pairs {
            let r = Range::new(lo, hi);
            let steps: Vec<f64> = r.steps().take(64).collect();
            assert!(steps.len() < 64, "{lo}..{hi} did not stop");
            assert!(steps.iter().all(|x| r.contains(*x)), "{lo}..{hi} -> {steps:?}");
            if let Some(last) = steps.last() {
                let next = last + 1.0;
                if next > *last {
                    assert!(!r.contains(next), "{lo}..{hi} still contains {next}");
                }
            }
        }

        let nan_lo = Range::new(f64::NAN, 1.0);
        assert!(!nan_lo.contains(0.0));
        assert!(!nan_lo.contains(f64::NAN));
        assert!(nan_lo.steps().next().is_none());
        assert!(Range::new(0.0, f64::NAN).steps().next().is_none());
        assert_ne!(Range::new(f64::NAN, f64::NAN), Range::new(f64::NAN, f64::NAN));
    }
}
