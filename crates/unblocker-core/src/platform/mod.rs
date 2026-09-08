use anyhow::Result;

use once_cell::sync::Lazy;
use parking_lot::Mutex as ParkMutex;

use crate::config::AppConfig;

pub mod linux;
pub use self::linux as imp;

#[cfg(not(target_os = "linux"))]
pub mod stub;
#[cfg(not(target_os = "linux"))]
pub use self::stub as imp;

pub trait Platform {
    fn name(&self) -> &'static str;
    fn setup(&self) -> Result<()>;
    fn teardown(&self) -> Result<()>;
}

pub fn current() -> Box<dyn Platform> {
    imp::platform()
}

/// Bypass driver — Linux: nfqws + nftables + `/etc/hosts` + in-process TG proxy.
pub trait NetworkDriver: Send {
    /// Bring the bypass up. `rt` is the FFI-facing tokio runtime; callers
    /// invoke this synchronously from outside a runtime.
    fn enable(
        &mut self,
        rt: &tokio::runtime::Runtime,
        cfg: &AppConfig,
    ) -> Result<()>;

    /// Tear the bypass down (stop TG proxy, kill nfqws, flush nft, hosts,
    /// state).
    fn disable(&mut self, rt: &tokio::runtime::Runtime) -> Result<()>;
}

static DRIVER: Lazy<ParkMutex<imp::Driver>> =
    Lazy::new(|| ParkMutex::new(imp::driver()));

/// Process-global bypass driver. `enable`/`disable` lock it for the duration
/// of the FFI call.
pub fn driver() -> &'static ParkMutex<imp::Driver> {
    &DRIVER
}