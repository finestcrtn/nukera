use super::{NetworkDriver, Platform};
use anyhow::{anyhow, Result};
use crate::config::AppConfig;

pub struct Driver;

impl NetworkDriver for Driver {
    fn enable(
        &mut self,
        _rt: &tokio::runtime::Runtime,
        _cfg: &AppConfig,
        _tun_fd: Option<i32>,
    ) -> Result<()> {
        Err(anyhow!("unsupported platform (no NetworkDriver)"))
    }
    fn disable(&mut self, _rt: &tokio::runtime::Runtime) -> Result<()> {
        Ok(())
    }
}

pub struct StubPlatform;
impl Platform for StubPlatform {
    fn name(&self) -> &'static str {
        "stub"
    }
    fn setup(&self) -> Result<()> {
        Ok(())
    }
    fn teardown(&self) -> Result<()> {
        Ok(())
    }
}

pub fn platform() -> Box<dyn Platform> {
    Box::new(StubPlatform)
}

pub fn driver() -> Driver {
    Driver
}