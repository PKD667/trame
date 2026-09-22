//! Device-arch detection for the cuda-oxide backend.

use std::env;
use std::process::Command;

pub fn detect_arch() -> Option<String> {
    if env::var_os("CUDA_OXIDE_TARGET").is_some() || env::var_os("CUDA_OXIDE_DEVICE_ARCH").is_some()
    {
        return None;
    }
    let output = Command::new("nvidia-smi")
        .args(["--query-gpu=compute_cap", "--format=csv,noheader"])
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    parse_arch(&String::from_utf8_lossy(&output.stdout))
}

pub fn parse_arch(output: &str) -> Option<String> {
    let (major, minor) = output.lines().next()?.trim().split_once('.')?;
    let major: u32 = major.parse().ok()?;
    let minor: u32 = minor.parse().ok()?;
    Some(if major >= 9 {
        format!("sm_{major}{minor}a")
    } else {
        format!("sm_{major}{minor}")
    })
}
