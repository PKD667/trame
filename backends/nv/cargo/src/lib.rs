//! Device-arch detection for the cuda-oxide backend.

use std::env;
use std::process::Command;

pub fn detect_arch() -> Result<String, String> {
    let selected = env::var("CUDA_VISIBLE_DEVICES")
        .map_err(|_| "CUDA_VISIBLE_DEVICES is unset; expected one GPU-<uuid>".to_string())?;
    validate_uuid(&selected)?;

    let output = Command::new("nvidia-smi")
        .arg(format!("--id={selected}"))
        .args(["--query-gpu=uuid,compute_cap", "--format=csv,noheader"])
        .output()
        .map_err(|error| format!("could not execute nvidia-smi for {selected}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "nvidia-smi query for {selected} exited with {}",
            output.status
        ));
    }
    let stdout = String::from_utf8(output.stdout).map_err(|error| {
        format!("nvidia-smi query for {selected} returned non-UTF-8 output: {error}")
    })?;
    let arch = parse_device_output(&stdout, &selected)?;

    for (name, value) in [
        ("CUDA_OXIDE_TARGET", env::var("CUDA_OXIDE_TARGET")),
        ("CUDA_OXIDE_DEVICE_ARCH", env::var("CUDA_OXIDE_DEVICE_ARCH")),
    ] {
        if let Ok(value) = value {
            if value != arch {
                return Err(format!(
                    "{name}={value} disagrees with selected GPU {selected} architecture {arch}"
                ));
            }
        }
    }
    Ok(arch)
}

fn validate_uuid(value: &str) -> Result<(), String> {
    let Some(uuid) = value.strip_prefix("GPU-") else {
        return Err(format!(
            "CUDA_VISIBLE_DEVICES={value:?} is not one GPU-<uuid> selection"
        ));
    };
    if uuid.len() != 36
        || !uuid.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
    {
        return Err(format!(
            "CUDA_VISIBLE_DEVICES={value:?} is not one well-formed GPU UUID"
        ));
    }
    Ok(())
}

fn parse_device_output(output: &str, selected: &str) -> Result<String, String> {
    let mut rows = output.lines();
    let row = rows
        .next()
        .ok_or_else(|| format!("nvidia-smi query for {selected} returned no GPU row"))?;
    if rows.next().is_some() {
        return Err(format!(
            "nvidia-smi query for {selected} returned multiple GPU rows"
        ));
    }
    let mut fields = row.split(',').map(str::trim);
    let uuid = fields
        .next()
        .ok_or_else(|| format!("nvidia-smi query for {selected} returned a malformed row"))?;
    let capability = fields
        .next()
        .ok_or_else(|| format!("nvidia-smi query for {selected} returned a malformed row"))?;
    if fields.next().is_some() || uuid != selected {
        return Err(format!(
            "nvidia-smi query for {selected} returned mismatched or malformed row: {row:?}"
        ));
    }
    let arch = parse_capability(capability).ok_or_else(|| {
        format!("selected GPU {selected} has invalid compute capability {capability:?}")
    })?;
    let major = capability
        .split_once('.')
        .and_then(|(major, _)| major.parse::<u32>().ok())
        .ok_or_else(|| {
            format!("selected GPU {selected} has invalid compute capability {capability:?}")
        })?;
    if major < 7 {
        return Err(format!(
            "selected GPU {selected} has unsupported compute capability {capability}; sm_70 is the minimum"
        ));
    }
    Ok(arch)
}

fn parse_capability(capability: &str) -> Option<String> {
    let (major, minor) = capability.split_once('.')?;
    if major.is_empty() || minor.is_empty() || minor.contains('.') {
        return None;
    }
    let major: u32 = major.parse().ok()?;
    let minor: u32 = minor.parse().ok()?;
    Some(if major >= 9 {
        format!("sm_{major}{minor}a")
    } else {
        format!("sm_{major}{minor}")
    })
}

pub fn parse_arch(output: &str) -> Option<String> {
    let mut lines = output.lines();
    let capability = lines.next()?.trim();
    if lines.next().is_some() {
        return None;
    }
    parse_capability(capability)
}

#[cfg(test)]
mod tests {
    use super::{parse_device_output, validate_uuid};

    const UUID: &str = "GPU-01234567-89ab-cdef-0123-456789abcdef";

    #[test]
    fn accepts_one_matching_volta_or_newer_device() {
        assert_eq!(
            parse_device_output(&format!("{UUID}, 7.0\n"), UUID),
            Ok("sm_70".into())
        );
    }

    #[test]
    fn rejects_invalid_selection_forms() {
        for value in [
            "",
            "0",
            "GPU-01234567-89ab-cdef-0123-456789abcdef,GPU-11234567-89ab-cdef-0123-456789abcdef",
            "GPU-not-a-uuid",
        ] {
            assert!(validate_uuid(value).is_err(), "accepted {value:?}");
        }
    }

    #[test]
    fn rejects_missing_duplicate_mismatched_and_unsupported_device_rows() {
        assert!(
            parse_device_output("", UUID)
                .unwrap_err()
                .contains("no GPU row")
        );
        assert!(
            parse_device_output(&format!("{UUID}, 7.0\n{UUID}, 7.0\n"), UUID)
                .unwrap_err()
                .contains("multiple GPU rows")
        );
        assert!(
            parse_device_output("GPU-11234567-89ab-cdef-0123-456789abcdef, 8.0\n", UUID)
                .unwrap_err()
                .contains("mismatched")
        );
        assert!(
            parse_device_output(&format!("{UUID}, 6.1\n"), UUID)
                .unwrap_err()
                .contains("sm_70 is the minimum")
        );
    }
}
