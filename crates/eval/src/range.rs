//! Bound policy for `a..b`. This file is the only place that compares a range's ends.

use std::cmp::Ordering;
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
        let mut n = 0u64;
        let mut prev = self.lo;
        std::iter::from_fn(move || {
            let x = self.lo + n as f64;
            // An offset that does not move forward is infinity or a lost integer, so stop.
            if n > 0 && x.partial_cmp(&prev) != Some(Ordering::Greater) {
                return None;
            }
            if !self.contains(x) {
                return None;
            }
            prev = x;
            n += 1;
            Some(x)
        })
    }

    pub(crate) fn scale(self, op: Arith, k: f64) -> Range {
        match op {
            // Leave a negative `k` reversed. `contains` then matches nothing, same as a descending literal.
            Arith::Mul => Range {
                lo: self.lo * k,
                hi: self.hi * k,
            },
            Arith::Div => Range {
                lo: self.lo / k,
                hi: self.hi / k,
            },
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
            assert!(
                steps.iter().all(|x| r.contains(*x)),
                "{lo}..{hi} -> {steps:?}"
            );
            let next = lo + steps.len() as f64;
            if steps.last().is_some_and(|last| next > *last) {
                assert!(!r.contains(next), "{lo}..{hi} still contains {next}");
            }
        }

        let third = 1.0 / 3.0;
        let drifted = Range::new(third, third + 2.0);
        assert_eq!(
            drifted.steps().collect::<Vec<_>>(),
            vec![third, third + 1.0]
        );

        let nan_lo = Range::new(f64::NAN, 1.0);
        assert!(!nan_lo.contains(0.0));
        assert!(!nan_lo.contains(f64::NAN));
        assert!(nan_lo.steps().next().is_none());
        assert!(Range::new(0.0, f64::NAN).steps().next().is_none());
        assert_ne!(
            Range::new(f64::NAN, f64::NAN),
            Range::new(f64::NAN, f64::NAN)
        );
    }
}
