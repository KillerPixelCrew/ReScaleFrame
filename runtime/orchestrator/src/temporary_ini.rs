// SPDX-License-Identifier: MIT
use std::{io, os::windows::ffi::OsStrExt, path::PathBuf};

pub(crate) struct TemporaryIni {
    path: PathBuf,
    wide: Vec<u16>,
}

impl TemporaryIni {
    pub(crate) fn new(fixture: &str, name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("rsf-{fixture}-{}-{name}.ini", std::process::id()));
        if let Err(err) = std::fs::remove_file(&path) {
            assert_eq!(err.kind(), io::ErrorKind::NotFound, "{}", path.display());
        }
        let wide = path.as_os_str().encode_wide().chain([0]).collect();
        Self { path, wide }
    }

    pub(crate) fn wide(&self) -> &[u16] {
        &self.wide
    }

    pub(crate) fn directory_wide(&self) -> Vec<u16> {
        self.path
            .parent()
            .expect("temporary INI has a directory")
            .as_os_str()
            .encode_wide()
            .chain([0])
            .collect()
    }

    pub(crate) fn contents(&self) -> io::Result<Option<String>> {
        match std::fs::read(&self.path) {
            Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl Drop for TemporaryIni {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
