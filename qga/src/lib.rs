#![allow(non_snake_case, non_camel_case_types)]

extern crate qapi_spec as qapi;
extern crate serde;
#[macro_use]
extern crate serde_derive;

include!(concat!(env!("OUT_DIR"), "/qga.rs"));

use std::{io, str};

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
}

impl From<GuestExecStatus> for io::Error {
    fn from(s: GuestExecStatus) -> Self {
        let (err0, err1) = if let Some(Ok(data)) = s.err_data.as_ref().map(|s| str::from_utf8(s)) {
            (": ", data)
        } else {
            ("", "")
        };

        let sig = if let Some(signal) = s.signal {
            format!(" (terminated by signal {})", signal)
        } else {
            Default::default()
        };

        let msg = if let Some(code) = s.exitcode {
            format!("process exited with code {}{}{}{}", code, sig, err0, err1)
        } else if s.exited {
            format!("process exited{}{}{}", sig, err0, err1)
        } else {
            panic!("a running process is not an error")
        };

        io::Error::new(io::ErrorKind::Other, msg)
    }
}
