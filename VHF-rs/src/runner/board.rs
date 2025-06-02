use crate::{Error, Result};
use nix::sys::stat::Mode;
use nix::unistd::{Gid, Group, User};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::Builder;
use std::thread::sleep;
use std::time::Duration;

const DEVICE_BASE_PATH: &str = "/sys/bus/usb/devices";
const SET_DEVICE_MODE: &str = "VHF/board_init/set_device_mode";

/// List all usb-devices whose product description are VHF.
///
/// bash equivalent: `grep -r VHF /sys/bus/usb/devices/*/product`, yielding
///   `/sys/bus/usb/devices/xxx`
pub fn find_device_by_sys() -> Result<Vec<PathBuf>> {
    Ok(fs::read_dir(DEVICE_BASE_PATH)
        .map_err(Error::Io)?
        .filter_map(|read_dir_entry| read_dir_entry.ok())
        .map(|dir_entry| dir_entry.path().as_path().join("product"))
        .filter(|p| p.is_file())
        .filter(|product_file| {
            fs::read_to_string(product_file.clone())
                .unwrap_or_default()
                .contains("VHF")
        })
        .map(|p| p.parent().unwrap().to_path_buf())
        .collect())
}

/// Asserts some expecatations with regards to the Cpp file.
fn assert_set_device_perms() -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let set_mode = Path::new(SET_DEVICE_MODE);

    let mut has_error: bool = false;

    if !set_mode.exists() {
        log::error!(
            "Compiled C file for setting VHF board could not be found! Run `make init` at root of git repository.",
        );
        return Err(Error::User);
    }

    let metadata = fs::symlink_metadata(set_mode).map_err(Error::Io)?;

    // Check 2: File owner
    if User::from_uid(metadata.uid().into())
        .map_err(Error::CIo)?
        .map(|u| u.name.to_ascii_uppercase())
        != Some(String::from("ROOT"))
    {
        log::error!(
            "Compiled C file for setting VHF board has wrong owner! Run `make init` at root of git repository."
        );
        has_error = true;
    }

    // Check 3: File group
    if Group::from_gid(metadata.gid().into())
        .map_err(Error::CIo)?
        .map(|u| u.name.to_ascii_uppercase())
        != Some(String::from("ROOT"))
    {
        log::error!(
            "Compiled C file for setting VHF board has wrong group! Run `make init` at root of git repository."
        );
        has_error = true;
    };

    const S_ENFMT: u32 = 0o2000; // From Python
    if (metadata.mode() & S_ENFMT) == 0 {
        log::error!(
            "Compiled C file for setting VHF board has wrong permissions bits! Run `make init` at root of git repository."
        );
        has_error = true;
    }

    if has_error {
        return Err(Error::User);
    }

    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
enum USBMode {
    ACM,
    Hybrid,
}

impl TryFrom<usize> for USBMode {
    type Error = Error;
    fn try_from(value: usize) -> std::result::Result<Self, Self::Error> {
        match value {
            1 => Ok(USBMode::ACM),
            2 => Ok(USBMode::Hybrid),
            _ => Err(Error::ParseUnrecognised("bConfig unrecognised".to_string())),
        }
    }
}

impl USBMode {
    /// Equivalent value in bConfig.
    #[inline]
    fn val(&self) -> usize {
        match self {
            USBMode::ACM => 1,
            USBMode::Hybrid => 2,
        }
    }
}

pub struct Board {
    /// DEVICE_BASE_PATH.join(usb_device_id)
    dev_id: PathBuf,
    pub board_id: String,
}

impl Board {
    /// Get a representation of VHF board.
    ///
    /// * dev_id: Path to VHF board as seen from within `/sys`.
    ///
    /// # Errors
    /// - User: Check if folder has been set to configured to expectation as done through Makefile
    /// initialisation.
    /// - Io: Spawn etc has issues.
    pub fn new(dev_id: PathBuf) -> Result<Self> {
        // Check Cpp file
        assert_set_device_perms()?;

        // Check full name of /sys/bus/..../product
        const EXPECTED_PRODUCT: &str = "VHF Processor";
        assert_eq!(
            fs::read_to_string(dev_id.clone().join("product"))
                .map_err(Error::Io)?
                .trim(),
            EXPECTED_PRODUCT
        );

        // Get id
        let board_id = fs::read_to_string(dev_id.clone().join("serial"))
            .map_err(Error::Io)?
            .as_str()
            .trim()
            .into();

        Ok(Self { dev_id, board_id })
    }

    fn b_config_path(&self) -> PathBuf {
        self.dev_id.as_path().join("bConfigurationValue")
    }

    fn get_b_config(&self) -> Result<usize> {
        fs::read_to_string(self.b_config_path())
            .map_err(Error::Io)
            .and_then(|s| {
                s.as_str().trim().parse().map_err(|_| {
                    Error::ParseUnrecognised("bConfigPath contents unrecognised".to_string())
                })
            })
    }

    fn usb_mode(&self) -> Result<USBMode> {
        self.get_b_config().and_then(|v| v.try_into())
    }

    fn hotplug_path(&self) -> Result<PathBuf> {
        let iter = self
            .usb_mode()
            .map(|usb| match usb {
                USBMode::ACM => PathBuf::from("/dev/serial/by-id"),
                USBMode::Hybrid => PathBuf::from("/dev/ioboards/"),
            })?
            .read_dir();

        let found: Vec<_> = iter
            .map_err(Error::Io)?
            .filter_map(|rd| rd.ok())
            .filter(|file| {
                file.file_name()
                    .into_string()
                    .unwrap()
                    .contains(&self.board_id)
            })
            .collect();

        if found.len() > 1 {
            log::error!(
                "More than 1 hotplug_path found. Searched with {}",
                self.board_id
            );
            log::error!("Found: {:?}", found);
            return Err(Error::InternalInconsistency);
        } else if found.is_empty() {
            log::error!("board_id({}) not found in Path", self.board_id);
            return Err(Error::InternalInconsistency);
        }

        Ok(found[0].path())
    }

    fn interface_path(&self) -> Result<PathBuf> {
        self.hotplug_path()
            .and_then(|p| p.canonicalize().map_err(Error::Io))
    }

    /// Check if interface_path() has correct read/write perms.
    /// Returns as error if invalid.
    pub fn valid_interface_perms(&self) -> Result<()> {
        let interface = self.interface_path()?;
        let meta = interface.metadata().map_err(Error::Io)?;

        use std::os::linux::fs::MetadataExt;
        let gid = meta.st_gid();
        let stat_mode = meta.st_mode();
        let stat_mode = Mode::from_bits(stat_mode).unwrap();

        if gid != Gid::effective().as_raw()
            || !Mode::S_IRGRP.intersects(stat_mode)
            || !Mode::S_IWGRP.intersects(stat_mode)
        {
            log::error! {"interface {:?} does not have correct permissions. Driver installation error?", interface};
            return Err(Error::InternalInconsistency);
        }

        Ok(())
    }

    /// Sets VHF Board to USB ACM Mode.
    pub fn set_acm(&self) -> Result<()> {
        log::debug!("Attempting to set to ACM mode.");

        match self.usb_mode()? {
            USBMode::ACM => Ok(()),
            USBMode::Hybrid => {
                let set_mode = Command::new(SET_DEVICE_MODE)
                    .arg("set")
                    .arg(self.b_config_path().as_os_str())
                    .arg(USBMode::ACM.val().to_string())
                    .output()
                    .map_err(Error::Io)?;

                match set_mode.status.success() {
                    false => {
                        log::error!("set_device_mode error! status = {:?}", set_mode.status);
                        log::error!("stderr = {:?}", str::from_utf8(&set_mode.stderr));
                        Err(Error::InternalInconsistency)
                    }
                    true => {
                        sleep(Duration::new(0, 500_000_000));
                        assert_eq!(self.usb_mode()?, USBMode::ACM);
                        Ok(())
                    }
                }
            }
        }
    }

    /// Sets VHF Board to USB Hybrid Mode.
    pub fn set_hybrid(&self) -> Result<()> {
        log::debug!("Attempting to set to Hybrid mode.");

        match self.usb_mode()? {
            USBMode::Hybrid => Ok(()),
            USBMode::ACM => {
                let set_mode = Command::new(SET_DEVICE_MODE)
                    .arg("set")
                    .arg(self.b_config_path().as_os_str())
                    .arg(USBMode::ACM.val().to_string())
                    .output()
                    .map_err(Error::Io)?;

                match set_mode.status.success() {
                    false => {
                        log::error!("set_device_mode error! status = {:?}", set_mode.status);
                        log::error!("stderr = {:?}", str::from_utf8(&set_mode.stderr));
                        Err(Error::InternalInconsistency)
                    }
                    true => {
                        sleep(Duration::new(0, 500_000_000));
                        assert_eq!(self.usb_mode()?, USBMode::Hybrid);
                        Ok(())
                    }
                }
            }
        }
    }

    /// Toggles between ACM and Hybrid Mode on VHF Board.
    pub fn toggle_usb_mode(&self) -> Result<()> {
        match self.usb_mode()? {
            USBMode::ACM => self.set_hybrid(),
            USBMode::Hybrid => self.set_acm(),
        }
    }

    /// Check if a board is in use.
    /// Errors if Io error occurs.
    pub fn in_use(&self) -> Result<bool> {
        match super::fuser_used(&self.interface_path()?) {
            Ok(b) => Ok(b),
            Err(Error::User) => panic!("Board no longer found despite initialisation"),
            Err(e) => Err(e),
        }
    }

    /// ACM method of clearing FIFO queue.
    pub fn acm_clear(&self) -> Result<()> {
        self.set_acm()?;
        // Trying without Serial
        let mut vhf = fs::File::options()
            .write(true)
            .open(self.interface_path()?)
            .map_err(Error::Io)?;

        use std::io::Write;
        vhf.write(b"CONFIG 16\n").map_err(Error::Io)?;
        log::info!("Raised Clear");
        sleep(Duration::new(1, 0));

        vhf.write(b"CONFIG 0\n").map_err(Error::Io)?;
        vhf.write(b"SKIP\n").map_err(Error::Io)?;
        vhf.write(b"CLOCKINIT\nADCINIT\n").map_err(Error::Io)?;
        log::info!("Lowered Clear\nFIFO should be flushed!");

        Ok(())
    }

    /// Reading data out via hybrid mode.
    /// Use when ACM clearing is insufficient.
    pub fn hybrid_clear(&self) -> Result<()> {
        if self.in_use()? {
            return Err(Error::EngineRunning);
        } else if self.usb_mode()? != USBMode::Hybrid {
            return Err(Error::User);
        }

        (0..5).try_for_each(|_| {
            sleep(Duration::from_millis(100));
            let board = self.interface_path()?;
            let drain = Builder::new()
                .name("Drain VHF Board".to_string())
                .spawn(|| hybrid_drain(board))
                .map_err(Error::Io)?;
            log::debug!("Spawned drain = {:?}", &drain);
            match drain.join() {
                Ok(Ok(v)) => Ok(v),
                Ok(Err(e)) => {
                    log::warn!("An error occurred while trying to drain VHF");
                    log::warn!("{:?}", e);
                    Err(e)
                }
                Err(_) => {
                    log::warn!("Failed to spawn VHF drainer");
                    Err(Error::InternalInconsistency)
                }
            }
        })
    }
}

/// Function passed to process for spawning that tries to read out from FIFO queue.
fn hybrid_drain(board: PathBuf) -> Result<()> {
    use super::Config;
    use super::VHF;
    use super::fold::StreamFold;

    let stream_fold = StreamFold::none_default();

    // Define the appropriate configuration first.
    let config = Config {
        num_samples: 1,
        skip_num: 99,
        board,
        stream_fold: stream_fold.clone(),
        ..Config::default()
    };

    let mut vhf = VHF::new(&config, &stream_fold)?;
    vhf.start()?;

    sleep(Duration::from_millis(500));
    vhf.stop()?;

    Ok(())
}
