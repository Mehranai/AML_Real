use anyhow::{Result, ensure};
pub const SUPPLY: u64 = 2_100_000_000_000_000;
/// Tight per-output bounds under conservation, with fees included among all
/// other destinations. No FIFO, haircut estimate, common ownership or coin ID.
pub fn allocate(lower: u64, upper: u64, total_input: u64, output: u64) -> Result<(u64, u64)> {
    ensure!(
        lower <= upper && upper <= total_input && output <= total_input && total_input <= SUPPLY,
        "invalid UTXO allocation bounds"
    );
    Ok((
        lower.saturating_sub(total_input - output),
        upper.min(output),
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fee_mixed_inputs_and_single_satoshi_are_exact() {
        assert_eq!(allocate(100, 100, 100, 99).unwrap(), (99, 99));
        assert_eq!(allocate(50, 50, 100, 50).unwrap(), (0, 50));
        assert_eq!(allocate(80, 80, 100, 70).unwrap(), (50, 70));
        assert_eq!(allocate(1, 1, 1, 1).unwrap(), (1, 1));
        assert_eq!(
            allocate(SUPPLY, SUPPLY, SUPPLY, SUPPLY - 1).unwrap(),
            (SUPPLY - 1, SUPPLY - 1)
        );
        assert!(allocate(2, 1, 3, 1).is_err());
    }
    #[test]
    fn small_integer_allocations_match_all_feasible_assignments() {
        for total in 0..16 {
            for low in 0..=total {
                for high in low..=total {
                    for out in 0..=total {
                        let possible = (low..=high)
                            .flat_map(|tagged| {
                                (0..=out)
                                    .filter(move |x| *x <= tagged && tagged - *x <= total - out)
                            })
                            .collect::<Vec<_>>();
                        let got = allocate(low, high, total, out).unwrap();
                        assert_eq!(
                            got,
                            (
                                *possible.iter().min().unwrap(),
                                *possible.iter().max().unwrap()
                            )
                        );
                    }
                }
            }
        }
    }
}
