pub fn cubic_bezier(progress: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let progress = progress.clamp(0.0, 1.0);
    let sample = |t: f32, a: f32, b: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    let mut lower = 0.0;
    let mut upper = 1.0;
    for _ in 0..24 {
        let midpoint = (lower + upper) / 2.0;
        if sample(midpoint, x1, x2) < progress {
            lower = midpoint;
        } else {
            upper = midpoint;
        }
    }
    sample((lower + upper) / 2.0, y1, y2)
}

pub fn updater_phase(elapsed_ms: u64, installing: bool, reduced_motion: bool) -> f32 {
    if reduced_motion {
        return 0.5;
    }
    let duration = if installing { 1100 } else { 1400 };
    let progress = (elapsed_ms % duration) as f32 / duration as f32;
    if installing {
        cubic_bezier(progress, 0.16, 1.0, 0.3, 1.0)
    } else {
        progress
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bezier_preserves_endpoints_and_css_reference_midpoint() {
        assert!(cubic_bezier(0.0, 0.16, 1.0, 0.3, 1.0) < 0.00001);
        assert!((cubic_bezier(1.0, 0.16, 1.0, 0.3, 1.0) - 1.0).abs() < 0.00001);
        assert!((cubic_bezier(0.5, 0.16, 1.0, 0.3, 1.0) - 0.971779).abs() < 0.0001);
        assert_eq!(updater_phase(700, false, false), 0.5);
        assert_eq!(updater_phase(1400, false, false), 0.0);
        assert_eq!(updater_phase(123, true, true), 0.5);
    }
}
