use core::error::Error;

pub struct CallInterface {}

impl CallInterface {
    pub async fn run(room_name: &str, full_sid: &[u8]) -> Result<(), Box<dyn Error>> {
        println!("Connecting to {} using sid: {:?}...", room_name, full_sid);

        Ok(())
    }
}
