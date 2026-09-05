pub fn combine_confidence(base: f32, supporting: impl IntoIterator<Item = f32>) -> f32 {
    let mut remaining_uncertainty = 1.0 - base.clamp(0.0, 1.0);

    for confidence in supporting {
        remaining_uncertainty *= 1.0 - confidence.clamp(0.0, 1.0);
    }

    (1.0 - remaining_uncertainty).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agreeing_evidence_increases_but_never_exceeds_one() {
        let combined = combine_confidence(0.8, [0.6, 0.5]);

        assert!(combined > 0.8);
        assert!(combined <= 1.0);
    }
}
