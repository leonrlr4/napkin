//! `bin/math.js`.

use std::cell::Cell;
use std::hash::{BuildHasher, Hasher, RandomState};

use crate::js::to_int32;

/// rough.js `Random`. The state is a JS number, so a falsy seed (0 or NaN) makes every
/// draw fall back to `Math.random`.
#[derive(Clone, Debug)]
pub struct Random {
    seed: f64,
}

impl Random {
    pub fn new(seed: f64) -> Self {
        Random { seed }
    }

    #[expect(
        clippy::should_implement_trait,
        reason = "keeps rough.js's name; not an iterator"
    )]
    pub fn next(&mut self) -> f64 {
        if self.seed != 0.0 && !self.seed.is_nan() {
            let state = to_int32(self.seed).wrapping_mul(48271);
            self.seed = f64::from(state);
            f64::from(state & 0x7FFF_FFFF) / 2_147_483_648.0
        } else {
            math_random()
        }
    }
}

/// Stand-in for `Math.random`, reached only where rough.js itself calls it (seed 0, the
/// dots filler, hachure skip offsets without a randomizer). Not reproducible, by design.
pub fn math_random() -> f64 {
    thread_local! {
        static SOURCE: (RandomState, Cell<u64>) = (RandomState::new(), const { Cell::new(0) });
    }
    SOURCE.with(|(state, counter)| {
        let n = counter.get();
        counter.set(n.wrapping_add(1));
        let mut hasher = state.build_hasher();
        hasher.write_u64(n);
        (hasher.finish() >> 11) as f64 / (1u64 << 53) as f64
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_zero_is_not_reproducible() {
        let a: Vec<f64> = (0..4).map(|_| Random::new(0.0).next()).collect();
        assert!(a.windows(2).any(|w| w[0] != w[1]), "{a:?}");
        assert!(a.iter().all(|x| (0.0..1.0).contains(x)));
    }
}
