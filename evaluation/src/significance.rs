//! Exact one-sided tests and the power simulation of the large-spec reading (#333; the #5
//! decision's payback criteria).

/// The one-sided level of every large-spec test (#5 decision).
pub const LEVEL: f64 = 0.025;

/// The binomial coefficient, exact.
fn choose(n: u64, k: u64) -> u128 {
    if k > n {
        return 0;
    }
    (0..k).fold(1, |c, i| c * u128::from(n - i) / u128::from(i + 1))
}

/// Fisher's exact test, one-sided: the chance that `a` of `n` runs succeed or more, given
/// `a + b` successes among the `n + m` runs of both arms.
pub fn fisher(a: u64, n: u64, b: u64, m: u64) -> f64 {
    let k = a + b;
    let hits: u128 = (a..=k.min(n))
        .map(|x| choose(n, x) * choose(m, k - x))
        .sum();
    hits as f64 / choose(n + m, k) as f64
}

/// The Mann–Whitney U of `a` below `b`: the pairs in which `a`'s value is lower, ties as half.
pub fn wins(a: &[f64], b: &[f64]) -> f64 {
    let pair = |x: &f64, y: &f64| match () {
        _ if x < y => 1.0,
        _ if x == y => 0.5,
        _ => 0.0,
    };
    a.iter()
        .map(|x| b.iter().map(|y| pair(x, y)).sum::<f64>())
        .sum()
}

/// The exact one-sided permutation p that `a` ranks below `b`: the share of the ways to split
/// the pooled values into groups of their sizes whose U is at least the observed one.
pub fn rank_p(a: &[f64], b: &[f64]) -> f64 {
    let pooled: Vec<f64> = a.iter().chain(b).copied().collect();
    let observed = wins(a, b);
    let (mut hits, mut splits) = (0u64, 0u64);
    // shortcut: walks all 2^n subsets, instant at the batch's 16 runs; walk k-subsets past ~24.
    for mask in 0u64..1 << pooled.len() {
        if mask.count_ones() as usize != a.len() {
            continue;
        }
        let side = |inside: bool| -> Vec<f64> {
            let chosen = pooled.iter().enumerate();
            chosen
                .filter(|(i, _)| (mask >> i & 1 == 1) == inside)
                .map(|(_, v)| *v)
                .collect()
        };
        splits += 1;
        hits += u64::from(wins(&side(true), &side(false)) >= observed);
    }
    hits as f64 / splits as f64
}

/// Simulated trials per power figure; the figure is good to about half a point.
const POWER_TRIALS: u32 = 20_000;

/// The chance that the rank test passes at `LEVEL` for a real 20% advantage (one arm's values
/// 0.8 times the other's), simulated as for the #5 decision's table: log-normal values with
/// `sigma` in both arms, `n` and `m` runs, and a fixed seed, to two decimals.
pub fn power(n: usize, m: usize, sigma: f64) -> f64 {
    // Continuous values never tie, so p ≤ LEVEL exactly when U reaches the critical value of
    // U's distribution over every split of n + m distinct ranks.
    let mut null: Vec<usize> = (0u64..1 << (n + m))
        .filter(|s| s.count_ones() as usize == n)
        .map(|s| {
            let above = |i: usize| (i + 1..n + m).filter(|j| s >> j & 1 == 0).count();
            (0..n + m).filter(|i| s >> i & 1 == 1).map(above).sum()
        })
        .collect();
    null.sort_unstable_by(|a, b| b.cmp(a));
    let tail = |u: usize| null.partition_point(|&v| v >= u) as f64 / null.len() as f64;
    let Some(critical) = null
        .iter()
        .copied()
        .take_while(|&u| tail(u) <= LEVEL)
        .last()
    else {
        return 0.0;
    };
    // SplitMix64 and Box–Muller: deterministic, standard library only.
    let mut state: u64 = 205;
    let mut uniform = || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    };
    let mut normal =
        || (-2.0 * (1.0 - uniform()).ln()).sqrt() * (std::f64::consts::TAU * uniform()).cos();
    let mut passed = 0;
    for _ in 0..POWER_TRIALS {
        let a: Vec<f64> = (0..n).map(|_| 0.8f64.ln() + sigma * normal()).collect();
        let b: Vec<f64> = (0..m).map(|_| sigma * normal()).collect();
        passed += u32::from(wins(&a, &b) >= critical as f64);
    }
    (f64::from(passed) / f64::from(POWER_TRIALS) * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ties count as half a pair, and the permutation p counts every split of the pooled runs:
    /// of the six splits of {1, 2, ∞, ∞}, three give U ≥ 2.5.
    #[test]
    fn rank_test_counts_ties_as_half() {
        let inf = f64::INFINITY;
        assert_eq!(wins(&[1.0, inf], &[2.0, inf]), 2.5);
        assert_eq!(rank_p(&[1.0, inf], &[2.0, inf]), 0.5);
        assert_eq!(rank_p(&[1.0, 2.0], &[3.0, 4.0]), 1.0 / 6.0);
    }

    /// The #5 decision's power table at n = 8 (simulated, log-normal, equal σ, a real 20%
    /// advantage, the 2.5% rank rule): 98%, 76%, 52% and 35% at σ 0.10 to 0.25. This simulation
    /// gives 98%, 77%, 53% and 37%: within the table's own simulation noise.
    #[test]
    fn power_reproduces_the_fifth_decisions_table() {
        for (sigma, table) in [(0.10, 0.98), (0.15, 0.76), (0.20, 0.52), (0.25, 0.35)] {
            let power = power(8, 8, sigma);
            assert!((power - table).abs() < 0.025, "σ {sigma}: {power}");
        }
    }
}
