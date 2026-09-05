pub fn risk_level(risk_percent: u8) -> &'static str {
    match risk_percent {
        0..=24 => "LOW",
        25..=59 => "MEDIUM",
        60..=84 => "HIGH",
        _ => "CRITICAL",
    }
}

pub fn ratio(numerator: f32, denominator: f32) -> f32 {
    if denominator <= 0.0 {
        0.0
    } else {
        numerator / denominator
    }
}

pub fn clamp01(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}
