use core::error::Error;

use crate::ascii_converter::{HEIGHT, WIDTH};
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

impl FrameGenerator {
    pub fn generate_frame(
        mode: &CameraTestMode,
        time: i32,
        output: &mut Mat,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let cx = WIDTH as f32 / 2.0;
        let cy = HEIGHT as f32 / 2.0;
        let t = time as f32;

        match mode {
            CameraTestMode::SpiralTunnel => {
                for y in 0..HEIGHT {
                    for x in 0..WIDTH {
                        let t = t;
                        let dx = x as f32 - cx;
                        let dy = y as f32 - cy;
                        let dist = (dx * dx + dy * dy).sqrt();
                        let angle = dy.atan2(dx) + t * 0.07;
                        let wave = ((dist * 0.1 - t * 0.5).sin() + 1.0) * 0.5;
                        let spiral = ((angle + dist * 0.03).cos() + 1.0) * 0.5;
                        let val = (wave * spiral * 255.0) as u8;
                        *output.at_2d_mut::<u8>(y, x)? = val;
                    }
                }
            }

            CameraTestMode::VortexGrid => {
                for y in 0..HEIGHT {
                    for x in 0..WIDTH {
                        let dx = (x as f32 - cx) / cx;
                        let dy = (y as f32 - cy) / cy;
                        let r = (dx * dx + dy * dy).sqrt();
                        let angle = dy.atan2(dx);

                        let grid_x = ((dx * 10.0 + t * 0.1).sin()).abs();
                        let grid_y = ((dy * 10.0 + t * 0.1).cos()).abs();

                        let vortex = (angle + t * 0.02).sin().abs();
                        let val =
                            ((1.0 - r) * (grid_x + grid_y + vortex) * 85.0).clamp(0.0, 255.0) as u8;

                        *output.at_2d_mut::<u8>(y, x)? = val;
                    }
                }
            }

            CameraTestMode::DiamondFlow => {
                for y in 0..HEIGHT {
                    for x in 0..WIDTH {
                        let dx = (x as f32 - cx).abs();
                        let dy = (y as f32 - cy).abs();
                        let diamond_dist = dx + dy;

                        let wave = ((diamond_dist * 0.15 - t * 0.4).sin() + 1.0) * 0.5;
                        let fade = (1.0 - diamond_dist / (cx + cy)).powf(1.3).clamp(0.0, 1.0);

                        let val = (wave * fade * 255.0).clamp(0.0, 255.0) as u8;
                        *output.at_2d_mut::<u8>(y, x)? = val;
                    }
                }
            }
        }

        Ok(())
    }
}
