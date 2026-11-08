use super::MediaError;
use libwebrtc::rtp_parameters::{
    DegradationPreference, Priority, RtpEncodingParameters, RtpParameters,
};
use libwebrtc::stats::RtcStats;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ViewerState {
    #[default]
    Watching,
    Preview,
}

impl ViewerState {
    pub fn parse(value: &str) -> Self {
        if value == "preview" {
            Self::Preview
        } else {
            Self::Watching
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Watching => "watching",
            Self::Preview => "preview",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct VideoProfile {
    pub scale: f64,
    pub bitrate: u64,
    pub fps: f64,
}

impl VideoProfile {
    pub fn for_quality(
        quality: &str,
        step: usize,
        viewer: ViewerState,
        controller: bool,
    ) -> Result<Option<Self>, MediaError> {
        let quality = match quality {
            "camera" => return Ok(None),
            "high" => 0,
            "medium" => 1,
            "low" => 2,
            _ => return Err(MediaError::InvalidSignal),
        };
        let ladder = [
            (1.0, [6_000_000, 3_000_000, 2_500_000], [60.0, 60.0, 30.0]),
            (1.0, [3_500_000, 2_200_000, 1_500_000], [60.0, 60.0, 30.0]),
            (1.5, [2_000_000, 1_400_000, 1_000_000], [60.0, 60.0, 30.0]),
            (2.0, [1_200_000, 800_000, 600_000], [30.0, 30.0, 24.0]),
        ];
        let (scale, bitrates, fps) = ladder[step.min(ladder.len() - 1)];
        let mut profile = if viewer == ViewerState::Preview {
            Self {
                scale: 4.0,
                bitrate: 150_000,
                fps: 2.0,
            }
        } else {
            Self {
                scale,
                bitrate: bitrates[quality],
                fps: fps[quality],
            }
        };
        if controller {
            profile.scale = 1.0;
            profile.fps = 60.0;
        }
        Ok(Some(profile))
    }

    pub fn apply(self, parameters: &mut RtpParameters) {
        if parameters.encodings.is_empty() {
            parameters.encodings.push(RtpEncodingParameters::default());
        }
        parameters.set_degradation_preference(DegradationPreference::MaintainFramerate);
        let first = &mut parameters.encodings[0];
        first.scale_resolution_down_by = Some(self.scale);
        first.max_bitrate = Some(self.bitrate);
        first.max_framerate = Some(self.fps);
        first.priority = Priority::High;
    }
}

#[derive(Default)]
pub(crate) struct AdaptiveVideo {
    pub step: usize,
    baseline: Option<f64>,
    breaches: u8,
    clean: u8,
    last_packets: Option<(String, i64, u64)>,
    pub applied: Option<VideoProfile>,
}

impl AdaptiveVideo {
    pub fn observe(&mut self, loss: f64, rtt: f64) {
        if !loss.is_finite() || !(0.0..=1.0).contains(&loss) || !rtt.is_finite() || rtt < 0.0 {
            return;
        }
        let baseline = match self.baseline {
            Some(previous) if rtt <= 0.0 || rtt >= previous => previous * 0.98 + rtt * 0.02,
            _ => rtt,
        };
        if rtt > 0.0 {
            self.baseline = Some(baseline);
        }
        let excess = if rtt > 0.0 && baseline > 0.0 {
            (rtt - baseline).max(0.0)
        } else {
            0.0
        };
        let desired = if loss > 0.05 || excess > 0.20 {
            3
        } else if loss > 0.02 || excess > 0.12 {
            2
        } else if loss > 0.008 || excess > 0.06 {
            1
        } else {
            0
        };
        if desired > self.step {
            self.clean = 0;
            self.breaches += 1;
            if self.breaches >= 2 {
                self.step = (self.step + 1).min(3);
                self.breaches = 0;
            }
        } else {
            self.breaches = 0;
            if desired < self.step {
                self.clean += 1;
                if self.clean >= 3 {
                    self.step -= 1;
                    self.clean = 0;
                }
            } else {
                self.clean = 0;
            }
        }
    }

    pub fn stats(&mut self, reports: &[RtcStats]) {
        let outbound = reports.iter().find_map(|report| match report {
            RtcStats::OutboundRtp(report) if report.stream.kind == "video" => Some(report),
            _ => None,
        });
        let Some(outbound) = outbound else {
            return;
        };
        let remote = reports.iter().find_map(|report| match report {
            RtcStats::RemoteInboundRtp(report)
                if report.stream.kind == "video"
                    && (report.remote_inbound.local_id == outbound.rtc.id
                        || report.stream.ssrc == outbound.stream.ssrc) =>
            {
                Some(report)
            }
            _ => None,
        });
        let Some(remote) = remote else {
            return;
        };
        let lost = remote.received.packets_lost;
        let sent = outbound.sent.packets_sent;
        let delta = self
            .last_packets
            .as_ref()
            .filter(|(id, _, _)| id == &outbound.rtc.id)
            .map(|(_, previous_lost, previous_sent)| {
                let lost = lost.saturating_sub(*previous_lost).max(0) as f64;
                let sent = sent.saturating_sub(*previous_sent) as f64;
                if sent > 0.0 {
                    (lost / (sent + lost)).min(1.0)
                } else {
                    0.0
                }
            })
            .unwrap_or(0.0);
        self.last_packets = Some((outbound.rtc.id.clone(), lost, sent));
        let reported = remote.remote_inbound.fraction_lost;
        let loss = if reported.is_finite() && (0.0..=1.0).contains(&reported) {
            reported
        } else {
            delta
        };
        self.observe(loss, remote.remote_inbound.round_trip_time);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_ladder_preview_camera_and_controller_profiles_are_distinct_from_initial_limits() {
        for (quality, base, last) in [
            ("low", 2_500_000, 600_000),
            ("medium", 3_000_000, 800_000),
            ("high", 6_000_000, 1_200_000),
        ] {
            assert_eq!(
                VideoProfile::for_quality(quality, 0, ViewerState::Watching, false)
                    .unwrap()
                    .unwrap()
                    .bitrate,
                base
            );
            assert_eq!(
                VideoProfile::for_quality(quality, usize::MAX, ViewerState::Watching, false)
                    .unwrap()
                    .unwrap()
                    .bitrate,
                last
            );
            let preview = VideoProfile::for_quality(quality, 3, ViewerState::Preview, false)
                .unwrap()
                .unwrap();
            assert_eq!(
                preview,
                VideoProfile {
                    scale: 4.0,
                    bitrate: 150_000,
                    fps: 2.0
                }
            );
            let controller = VideoProfile::for_quality(quality, 3, ViewerState::Preview, true)
                .unwrap()
                .unwrap();
            assert_eq!(
                (controller.scale, controller.fps, controller.bitrate),
                (1.0, 60.0, 150_000)
            );
        }
        assert!(
            VideoProfile::for_quality("camera", 3, ViewerState::Preview, true)
                .unwrap()
                .is_none()
        );
        assert!(VideoProfile::for_quality("invalid", 0, ViewerState::Watching, false).is_err());
    }

    #[test]
    fn loss_breaches_drop_one_rung_and_three_clean_samples_recover_one_rung() {
        let mut adaptive = AdaptiveVideo::default();
        for step in 1..=3 {
            adaptive.observe(0.1, 0.02);
            assert_eq!(adaptive.step, step - 1);
            adaptive.observe(0.1, 0.02);
            assert_eq!(adaptive.step, step);
        }
        for step in (0..3).rev() {
            for _ in 0..2 {
                adaptive.observe(0.0, 0.02);
                assert_eq!(adaptive.step, step + 1);
            }
            adaptive.observe(0.0, 0.02);
            assert_eq!(adaptive.step, step);
        }
    }

    #[test]
    fn rtt_baseline_hysteresis_and_invalid_samples_do_not_create_unbounded_steps() {
        let mut adaptive = AdaptiveVideo::default();
        adaptive.observe(0.0, 0.02);
        adaptive.observe(0.0, 0.25);
        adaptive.observe(0.0, 0.25);
        assert_eq!(adaptive.step, 1);
        for (loss, rtt) in [
            (f64::NAN, 1.0),
            (f64::INFINITY, 1.0),
            (0.0, f64::NAN),
            (0.0, -1.0),
            (-0.1, 0.0),
            (1.1, 0.0),
        ] {
            adaptive.observe(loss, rtt);
        }
        assert_eq!(adaptive.step, 1);
    }

    #[test]
    fn typed_stats_correlate_video_ssrc_handle_counter_reset_and_ignore_unrelated_audio() {
        use libwebrtc::stats::{OutboundRtpStats, RemoteInboundRtpStats};
        let mut outbound = OutboundRtpStats::default();
        outbound.rtc.id = "video-original".into();
        outbound.stream.kind = "video".into();
        outbound.stream.ssrc = 42;
        outbound.sent.packets_sent = 100;
        let mut remote = RemoteInboundRtpStats::default();
        remote.stream.kind = "video".into();
        remote.stream.ssrc = 42;
        remote.remote_inbound.round_trip_time = 0.02;
        remote.remote_inbound.fraction_lost = f64::NAN;
        let mut adaptive = AdaptiveVideo::default();
        for lost in [0, 100, 200] {
            remote.received.packets_lost = lost;
            outbound.sent.packets_sent += 10;
            adaptive.stats(&[
                RtcStats::OutboundRtp(outbound.clone()),
                RtcStats::RemoteInboundRtp(remote.clone()),
            ]);
        }
        assert_eq!(adaptive.step, 1);
        outbound.rtc.id = "video-replaced".into();
        outbound.sent.packets_sent = 0;
        remote.received.packets_lost = i64::MIN;
        adaptive.stats(&[
            RtcStats::OutboundRtp(outbound.clone()),
            RtcStats::RemoteInboundRtp(remote.clone()),
        ]);
        assert_eq!(adaptive.step, 1);
        remote.stream.kind = "audio".into();
        remote.remote_inbound.fraction_lost = 1.0;
        for _ in 0..5 {
            adaptive.stats(&[
                RtcStats::OutboundRtp(outbound.clone()),
                RtcStats::RemoteInboundRtp(remote.clone()),
            ]);
        }
        assert_eq!(adaptive.step, 1);
    }

    #[test]
    fn applying_a_profile_keeps_encoding_identity_activity_and_unrelated_encodings() {
        let mut params = RtpParameters::default();
        params.encodings = vec![
            RtpEncodingParameters {
                active: false,
                rid: "retained".into(),
                ..Default::default()
            },
            RtpEncodingParameters {
                max_bitrate: Some(123),
                ..Default::default()
            },
        ];
        VideoProfile::for_quality("high", 2, ViewerState::Watching, false)
            .unwrap()
            .unwrap()
            .apply(&mut params);
        assert!(!params.encodings[0].active);
        assert_eq!(params.encodings[0].rid, "retained");
        assert_eq!(params.encodings[1].max_bitrate, Some(123));
        assert_eq!(params.encodings[0].scale_resolution_down_by, Some(1.5));
    }
}
