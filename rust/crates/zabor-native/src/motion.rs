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

pub fn mic_thumb_spring(progress: f64, pop: bool) -> f64 {
    if progress <= 0.0 {
        return 0.0;
    }
    if progress >= 1.0 || !progress.is_finite() {
        return 1.0;
    }
    let (stiffness, damping, duration): (f64, f64, f64) = if pop {
        (120.0, 9.0, 1.8800000000000012)
    } else {
        (300.0, 14.0, 1.2800000000000007)
    };
    let w0 = stiffness.sqrt();
    let zeta = damping / (2.0 * w0);
    let wd = w0 * (1.0 - zeta * zeta).sqrt();
    let b = zeta * w0 / wd;
    let time = progress * duration;
    1.0 - (-time * zeta * w0).exp() * ((wd * time).cos() + b * (wd * time).sin())
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

    #[test]
    fn microphone_thumb_matches_frozen_anime_450_settling_curves() {
        let press = [
            0.0,
            1.0183190445004364,
            1.1600258787292927,
            0.9392871562951786,
            0.9952114406793986,
            1.0118417837746632,
            0.9965310786798368,
            0.9992767631461571,
            1.0008386106639988,
            0.9998183429237901,
            1.0,
        ];
        let pop = [
            0.0,
            0.945304220041468,
            1.198330421941768,
            0.9586562467407654,
            0.9741954151059311,
            1.0143031785425232,
            1.0010439190870688,
            0.997095427406035,
            1.0005607552455136,
            1.0003895069319237,
            1.0,
        ];
        for (pop, expected) in [(false, press), (true, pop)] {
            for (index, expected) in expected.into_iter().enumerate() {
                assert!((mic_thumb_spring(index as f64 / 10.0, pop) - expected).abs() < 1e-13);
            }
        }
    }

    #[test]
    fn microphone_thumb_clamps_time_but_preserves_the_reference_overshoot() {
        for pop in [false, true] {
            assert_eq!(mic_thumb_spring(-0.5, pop), 0.0);
            assert_eq!(mic_thumb_spring(1.5, pop), 1.0);
            assert_eq!(mic_thumb_spring(f64::NAN, pop), 1.0);
            assert_eq!(mic_thumb_spring(f64::NEG_INFINITY, pop), 0.0);
            assert_eq!(mic_thumb_spring(f64::INFINITY, pop), 1.0);
            let peak = (0..1000)
                .map(|index| mic_thumb_spring(index as f64 / 1000.0, pop))
                .fold(0.0_f64, f64::max);
            assert!(peak > 1.24 && peak < 1.25);
            assert!(mic_thumb_spring(0.3, pop) < 1.0);
        }
    }

    #[test]
    fn microphone_thumb_emits_full_reference_precision_and_slint_float_samples() {
        let curves: Vec<_> = [false, true].into_iter().map(|pop| {
            let duration = if pop { 1880 } else { 1280 };
            let samples: Vec<_> = (0..=duration).map(|elapsed| {
                let progress = f64::from(elapsed) / f64::from(duration);
                serde_json::json!({"elapsedMs":elapsed,"value":mic_thumb_spring(progress, pop),
                    "slintValue":mic_thumb_spring(f64::from(progress as f32), pop) as f32})
            }).collect();
            serde_json::json!({"kind":if pop {"pop"} else {"press"},"settlingDuration":duration,"samples":samples})
        }).collect();
        println!(
            "MIC_THUMB_SPRING_REFERENCE={}",
            serde_json::json!({"curves":curves})
        );
    }
}
