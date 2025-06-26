use core::error::Error;

use crate::camera::{TEST_FRAME_HEIGHT, TEST_FRAME_WIDTH};
use opencv::core::{Mat, MatTrait};
use strum_macros::{Display, EnumIter};

#[derive(EnumIter, Display)]
pub enum CameraTestMode {
    #[strum(serialize = "spiral")]
    SpiralTunnel,
    #[strum(serialize = "vortex")]
    VortexGrid,
    #[strum(serialize = "diamond")]
    DiamondFlow,
}

pub struct FrameGenerator;

use opencv::{
    core::{CV_8UC3, Vec3b},
    prelude::*,
};
use std::f32::consts::PI;

impl FrameGenerator {
    pub fn generate_frame(
        mode: &CameraTestMode,
        time: i32,
        output: &mut Mat,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let cx = TEST_FRAME_WIDTH as f32 / 2.0;
        let cy = TEST_FRAME_HEIGHT as f32 / 2.0;
        let t = time as f32;

        if output.channels() != 3 || output.typ() != CV_8UC3 {
            *output =
                Mat::zeros(TEST_FRAME_HEIGHT as i32, TEST_FRAME_WIDTH as i32, CV_8UC3)?.to_mat()?;
        }

        for y in 0..TEST_FRAME_HEIGHT {
            for x in 0..TEST_FRAME_WIDTH {
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;

                let pixel = match mode {
                    CameraTestMode::SpiralTunnel => {
                        let dist = (dx * dx + dy * dy).sqrt();
                        let angle = dy.atan2(dx);
                        let normalized_dist = dist / (cx.min(cy) * 2.2);

                        let t1 = t * 0.08;
                        let t2 = t * 0.12;

                        let spiral_base = angle * 3.0 + normalized_dist * 8.0;
                        let spiral1 = ((spiral_base - t1).sin() + 1.0) * 0.6;
                        let spiral2 =
                            ((angle * 5.0 - normalized_dist * 6.0 + t2).cos() + 1.0) * 0.6;

                        let tunnel_freq = normalized_dist * 12.0;
                        let tunnel = ((tunnel_freq - t * 0.7).sin()
                            + (tunnel_freq * 0.7 - t * 0.5).cos())
                            * 0.6;
                        let tunnel = (tunnel + 1.2) * 0.5;

                        let pulse = ((normalized_dist * 5.0 + t * 0.15).sin() + 1.0) * 0.6;

                        let intensity = (spiral1 + spiral2) * 0.5 * tunnel * pulse;
                        let enhanced_intensity = (intensity.powf(0.6) + 0.2).clamp(0.0, 1.0);

                        let hue_offset = t * 0.02 + normalized_dist * 0.3;
                        let base_color = (enhanced_intensity * 280.0).clamp(0.0, 255.0) as u8;

                        let r = ((base_color as f32) * (0.7 + 0.3 * (angle + hue_offset).sin()))
                            .clamp(0.0, 255.0) as u8;
                        let g = ((base_color as f32)
                            * (0.7 + 0.3 * (angle + hue_offset + 2.094).sin()))
                        .clamp(0.0, 255.0) as u8;
                        let b = ((base_color as f32)
                            * (0.7 + 0.3 * (angle + hue_offset + 4.189).sin()))
                        .clamp(0.0, 255.0) as u8;

                        Vec3b::from([b, g, r])
                    }

                    CameraTestMode::VortexGrid => {
                        let dx_norm = dx / cx;
                        let dy_norm = dy / cy;
                        let r = (dx_norm * dx_norm + dy_norm * dy_norm).sqrt();
                        let angle = dy.atan2(dx);

                        let grid_scale = 8.0;
                        let time_offset1 = t * 0.1;
                        let time_offset2 = t * 0.08;

                        let rot_angle = t * 0.03;
                        let cos_rot = rot_angle.cos();
                        let sin_rot = rot_angle.sin();

                        let rx = dx_norm * cos_rot - dy_norm * sin_rot;
                        let ry = dx_norm * sin_rot + dy_norm * cos_rot;

                        let grid1 = ((rx * grid_scale + time_offset1).sin()
                            * (ry * grid_scale + time_offset1).cos())
                        .abs();
                        let grid2 = ((dx_norm * 10.0 - time_offset2).sin()
                            * (dy_norm * 10.0 - time_offset2).cos())
                        .abs();

                        let vortex1 = (angle * 2.0 + r * 4.0 + t * 0.04).sin().abs();
                        let vortex2 = (angle * 3.0 - r * 3.5 - t * 0.06).cos().abs();

                        let fade = (1.2 - r * 0.3).clamp(0.3, 1.0);
                        let combined = (grid1 + grid2) * (vortex1 + vortex2) * fade;
                        let val = (combined * 120.0 + 30.0).clamp(0.0, 255.0);

                        let hue = (angle + PI + t * 0.01) / (2.0 * PI);
                        let sat = 0.9;
                        let value = (val / 255.0 + 0.1).clamp(0.0, 1.0);

                        hsv_to_rgb(hue, sat, value)
                    }

                    CameraTestMode::DiamondFlow => {
                        let dx_abs = dx.abs();
                        let dy_abs = dy.abs();
                        let diamond_dist = dx_abs + dy_abs;
                        let max_dist = cx + cy;

                        let wave1 = ((diamond_dist * 0.08 - t * 0.3).sin() + 1.0) * 0.6;
                        let wave2 = ((diamond_dist * 0.15 - t * 0.5).cos() + 1.0) * 0.6;

                        let cross = ((dx * 0.2 + dy * 0.2 + t * 0.1).sin()
                            * (dx * 0.2 - dy * 0.2 - t * 0.12).cos())
                        .abs();

                        let edge_factor =
                            (1.2 - (diamond_dist / max_dist).powf(1.5)).clamp(0.2, 1.0);

                        let pattern = (wave1 + wave2) * 0.5 * cross * edge_factor;
                        let val = (pattern + 0.15).clamp(0.0, 1.0);

                        let hue = (wave1 * 0.4 + t * 0.008) % 1.0;
                        let sat = 0.9;
                        hsv_to_rgb(hue, sat, val)
                    }
                };

                *output.at_2d_mut::<Vec3b>(y, x)? = pixel;
            }
        }

        Ok(())
    }
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> Vec3b {
    let h = h * 6.0;
    let i = h.floor() as i32;
    let f = h - i as f32;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);

    let (r, g, b) = match i % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        5 | _ => (v, p, q),
    };

    Vec3b::from([(b * 255.0) as u8, (g * 255.0) as u8, (r * 255.0) as u8])
}
