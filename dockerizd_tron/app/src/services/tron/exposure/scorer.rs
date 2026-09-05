const HOP_DECAY: f64 = 0.75;
const SERVICE_MEDIATION_FACTOR: f64 = 0.35;

pub fn edge_exposure_score(
    parent_score: f64,
    amount_share: f64,
    time_weight: f64,
    service_mediated: bool,
) -> f64 {
    let amount_weight = (0.25 + 0.75 * amount_share.clamp(0.0, 1.0).sqrt()).clamp(0.25, 1.0);
    let service_weight = if service_mediated {
        SERVICE_MEDIATION_FACTOR
    } else {
        1.0
    };

    (parent_score.clamp(0.0, 1.0)
        * HOP_DECAY
        * amount_weight
        * time_weight.clamp(0.0, 1.0)
        * service_weight)
        .clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::edge_exposure_score;

    #[test]
    fn applies_one_decay_factor_per_edge() {
        assert!((edge_exposure_score(1.0, 1.0, 1.0, false) - 0.75).abs() < f64::EPSILON);
        assert!((edge_exposure_score(0.75, 1.0, 1.0, false) - 0.5625).abs() < f64::EPSILON);
    }

    #[test]
    fn service_mediation_reduces_exposure() {
        let direct = edge_exposure_score(1.0, 1.0, 1.0, false);
        let mediated = edge_exposure_score(1.0, 1.0, 1.0, true);

        assert!(mediated < direct);
    }
}
