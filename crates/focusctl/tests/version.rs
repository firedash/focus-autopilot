// SPDX-License-Identifier: Apache-2.0
//! `focusctl --version` prints the Cargo package version (the one a release is tagged with).

use std::process::Command;

#[test]
fn version_flag_prints_the_package_version() {
    for flag in ["--version", "-V"] {
        let out = Command::new(env!("CARGO_BIN_EXE_focusctl"))
            .arg(flag)
            .output()
            .unwrap();
        assert!(out.status.success(), "{flag}: {out:?}");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim_end(),
            concat!("focusctl ", env!("CARGO_PKG_VERSION"))
        );
    }
}
