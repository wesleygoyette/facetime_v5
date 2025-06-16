use core::error::Error;

use opencv::{
    core::{Mat, MatTraitConst},
    videoio::{CAP_ANY, VideoCapture, VideoCaptureTrait, VideoCaptureTraitConst},
};

pub struct Camera {
    cam: VideoCapture,
    frame: Mat,
}

impl Camera {
    pub fn new() -> Result<Self, Box<dyn Error + Send + Sync>> {
        let cam: VideoCapture = VideoCapture::new(0, CAP_ANY)?;

        if !cam.is_opened()? {
            return Err("Error: Could not open camera".into());
        }

        let frame = Mat::default();
        Ok(Self { cam, frame })
    }

    pub async fn get_frame(&mut self) -> Result<&Mat, Box<dyn Error + Send + Sync>> {
        self.cam.read(&mut self.frame)?;

        if self.frame.empty() {
            return Err("Empty frame captured".into());
        }

        Ok(&self.frame)
    }
}
