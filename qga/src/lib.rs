#![allow(non_snake_case, non_camel_case_types)]
#![doc(html_root_url = "https://docs.rs/qapi-qga/0.12.0")]

include!(concat!(env!("OUT_DIR"), "/qga.rs"));

use std::{io, str, fmt, error};
use serde::{Deserialize, Serialize};

pub trait QgaCommand: qapi_spec::Command { }
impl<'a, T: QgaCommand> QgaCommand for &'a T { }
impl<'a, T: QgaCommand> QgaCommand for &'a mut T { }

#[derive(Copy, Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GuestShutdownMode {
    Halt,
    Powerdown,
    Reboot,
}

impl GuestExecStatus {
    pub fn result(self) -> Result<Self, Self> {
        if self.exited {
            if self.exitcode != Some(0) || self.signal.is_some() {
                Err(self)
            } else {
                Ok(self)
            }
        } else {
            Ok(self)
        }
    }

    fn message(&self) -> String {
        let (err0, err1) = if let Some(Ok(data)) = self.err_data.as_ref().map(|s| str::from_utf8(s)) {
            (": ", data)
        } else {
            ("", "")
        };

        let sig = if let Some(signal) = self.signal {
            format!(" (terminated by signal {})", signal)
        } else {
            Default::default()
        };

        if let Some(code) = self.exitcode {
            format!("guest process exited with code {}{}{}{}", code, sig, err0, err1)
        } else if self.exited {
            format!("guest process exited{}{}{}", sig, err0, err1)
        } else {
            format!("guest process is still running")
        }
    }
}

impl fmt::Display for GuestExecStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl error::Error for GuestExecStatus {
    fn description(&self) -> &str {
        "guest process exit status"
    }
}

impl From<GuestExecStatus> for io::Error {
    fn from(s: GuestExecStatus) -> Self {
        io::Error::new(io::ErrorKind::Other, s.to_string())
    }
}
