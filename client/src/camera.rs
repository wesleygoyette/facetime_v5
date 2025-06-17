use opencv::{
    core::{
        CV_8UC1, LogLevel, Mat, MatExprTraitConst, MatTraitConst, get_log_level, set_log_level,
    },
    videoio::{CAP_ANY, VideoCapture, VideoCaptureTrait, VideoCaptureTraitConst},
};
use std::error::Error;
use std::fs::File;
use std::os::unix::io::AsRawFd;
use std::result::Result;
use strum::IntoEnumIterator;
use tokio::time::Instant;

pub const MAX_USER_CAMERAS: i32 = 10;

#[cfg(windows)]
use std::os::windows::io::AsRawHandle;

#[cfg(unix)]
use libc::{STDERR_FILENO, dup2};
#[cfg(unix)]
use std::io::stderr;

use crate::{
    ascii_converter::{HEIGHT, WIDTH},
    frame_generator::{CameraTestMode, FrameGenerator},
};

pub struct Camera {
    capture: CameraCapture,
    frame: Mat,
    start_time: Instant,
}

enum CameraCapture {
    Real(VideoCapture),
    Test(CameraTestMode),
}

impl Camera {
    pub fn new(camera_index: i32) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let frame = Mat::default();
        let start_time = Instant::now();

        for (idx, mode) in CameraTestMode::iter().enumerate() {
            if camera_index == MAX_USER_CAMERAS + idx as i32 {
                return Ok(Self {
                    capture: CameraCapture::Test(mode),
                    frame,
                    start_time,
                });
            }
        }

        let cam = VideoCapture::new(camera_index, CAP_ANY)?;
        if !cam.is_opened()? {
            return Err(format!("Could not open camera at index {}", camera_index).into());
        }

        Ok(Self {
            capture: CameraCapture::Real(cam),
            frame,
            start_time,
        })
    }

    pub async fn get_frame(&mut self) -> Result<&Mat, Box<dyn Error + Send + Sync>> {
        match &mut self.capture {
            CameraCapture::Real(video_capture) => {
                video_capture.read(&mut self.frame)?;

                if self.frame.empty() {
                    return Err("Empty frame captured".into());
                }

                Ok(&self.frame)
            }
            CameraCapture::Test(mode) => {
                let mut output = Mat::zeros(HEIGHT, WIDTH, CV_8UC1)?.to_mat()?;
                let time = self.start_time.elapsed().as_millis() as i32 / 70;

                FrameGenerator::generate_frame(mode, time, &mut output)?;

                self.frame = output;

                Ok(&self.frame)
            }
        }
    }

    pub fn list_available_cameras() -> Vec<String> {
        let prev_log_level = get_log_level().unwrap_or(LogLevel::LOG_LEVEL_INFO);
        let _ = set_log_level(LogLevel::LOG_LEVEL_SILENT);

        let available = silence_stderr(|| {
            let mut available = vec!["0".to_string()];

            for i in 1..MAX_USER_CAMERAS {
                if let Ok(cam) = VideoCapture::new(i, CAP_ANY) {
                    if cam.is_opened().unwrap_or(false) {
                        available.push(format!("{}", i));
                    }
                }
            }

            for idx in 0..CameraTestMode::iter().len() {
                available.push((MAX_USER_CAMERAS + idx as i32).to_string());
            }

            available
        });

        let _ = set_log_level(prev_log_level);
        available
    }

    pub fn is_valid_camera_name(camera: &String) -> bool {
        let camera_index = match camera.parse::<i32>() {
            Ok(idx) if idx < 0 => return false,
            Ok(idx) => idx,
            Err(_) => return false,
        };

        if camera_index == 0
            || (camera_index >= MAX_USER_CAMERAS
                && camera_index < MAX_USER_CAMERAS + CameraTestMode::iter().len() as i32)
        {
            return true;
        }

        Camera::list_available_cameras().contains(camera)
    }
}

#[cfg(unix)]
fn silence_stderr<F: FnOnce() -> T, T>(f: F) -> T {
    let devnull = File::open("/dev/null").unwrap();
    let stderr_fd = stderr().as_raw_fd();
    let old_fd = unsafe { libc::dup(stderr_fd) };
    unsafe {
        dup2(devnull.as_raw_fd(), STDERR_FILENO);
    }
    let result = f();
    unsafe {
        dup2(old_fd, STDERR_FILENO);
        libc::close(old_fd);
    }
    result
}

#[cfg(windows)]
fn silence_stderr<F: FnOnce() -> T, T>(f: F) -> T {
    use std::fs::OpenOptions;
    use std::ptr;
    use winapi::um::fileapi::CreateFileA;
    use winapi::um::handleapi::{CloseHandle, DuplicateHandle};
    use winapi::um::processenv::GetStdHandle;
    use winapi::um::processthreadsapi::GetCurrentProcess;
    use winapi::um::winbase::{DUPLICATE_SAME_ACCESS, STD_ERROR_HANDLE};
    use winapi::um::winnt::{GENERIC_WRITE, HANDLE};

    unsafe {
        let h_proc = GetCurrentProcess();
        let mut old_handle: HANDLE = ptr::null_mut();
        let stderr_handle = GetStdHandle(STD_ERROR_HANDLE);
        DuplicateHandle(
            h_proc,
            stderr_handle,
            h_proc,
            &mut old_handle,
            0,
            1,
            DUPLICATE_SAME_ACCESS,
        );

        let null_handle = CreateFileA(
            b"NUL\0".as_ptr() as _,
            GENERIC_WRITE,
            0,
            ptr::null_mut(),
            3,
            0,
            ptr::null_mut(),
        );

        DuplicateHandle(
            h_proc,
            null_handle,
            h_proc,
            &mut stderr_handle as *mut _,
            0,
            1,
            DUPLICATE_SAME_ACCESS,
        );

        let result = f();

        DuplicateHandle(
            h_proc,
            old_handle,
            h_proc,
            &mut stderr_handle as *mut _,
            0,
            1,
            DUPLICATE_SAME_ACCESS,
        );
        CloseHandle(old_handle);
        CloseHandle(null_handle);

        result
    }
}
