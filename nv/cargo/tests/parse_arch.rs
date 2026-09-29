use nv_cargo::parse_arch;

#[test]
fn formats_device_arch_hints() {
    assert_eq!(parse_arch("7.5\n"), Some("sm_75".into()));
    assert_eq!(parse_arch("9.0\n"), Some("sm_90a".into()));
    assert_eq!(parse_arch("not available\n"), None);
}

#[cfg(unix)]
#[test]
fn cargo_nv_refuses_invalid_identity_before_spawning_cargo() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    const UUID: &str = "GPU-01234567-89ab-cdef-0123-456789abcdef";
    let scratch = std::path::PathBuf::from("/home/pkd/code/agents/nerve-nv-20260929/scratch");
    fs::create_dir_all(&scratch).expect("create campaign scratch directory");
    let root = scratch.join(format!("cargo-nv-preflight-{}", std::process::id()));
    fs::create_dir(&root).expect("create private preflight harness");
    let bin = root.join("bin");
    fs::create_dir(&bin).expect("create stub command directory");
    let nvidia_smi = bin.join("nvidia-smi");
    fs::write(
        &nvidia_smi,
        "#!/bin/sh\n[ \"${NV_FAIL:-}\" != 1 ] || exit 7\nprintf '%s\\n' \"$NV_ROW\"\n",
    )
    .expect("write nvidia-smi stub");
    let cargo = bin.join("cargo");
    let spawned = root.join("cargo-spawned");
    fs::write(&cargo, format!("#!/bin/sh\n: >'{}'\n", spawned.display()))
        .expect("write cargo stub");
    for path in [&nvidia_smi, &cargo] {
        let mut permissions = fs::metadata(path)
            .expect("read stub metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("make stub executable");
    }
    let backend = root.join("backend.so");
    fs::write(&backend, b"test backend").expect("write private backend fixture");
    let backend = backend
        .canonicalize()
        .expect("canonicalize backend fixture");
    let binary = env!("CARGO_BIN_EXE_cargo-nv");

    let cases = [
        (
            "missing-backend",
            "/missing/backend.so",
            Some(UUID),
            None,
            Some(""),
            "cannot be canonicalized",
        ),
        (
            "unset-selection",
            backend.to_str().expect("backend utf8"),
            None,
            None,
            Some(""),
            "CUDA_VISIBLE_DEVICES is unset",
        ),
        (
            "index-selection",
            backend.to_str().expect("backend utf8"),
            Some("0"),
            None,
            Some(""),
            "not one GPU-<uuid> selection",
        ),
        (
            "unsupported-arch",
            backend.to_str().expect("backend utf8"),
            Some(UUID),
            None,
            Some(""),
            "sm_70 is the minimum",
        ),
        (
            "arch-mismatch",
            backend.to_str().expect("backend utf8"),
            Some(UUID),
            Some("sm_75"),
            Some(""),
            "disagrees with selected GPU",
        ),
        (
            "query-failure",
            backend.to_str().expect("backend utf8"),
            Some(UUID),
            None,
            Some("1"),
            "exited with exit status: 7",
        ),
        (
            "wrong-row",
            backend.to_str().expect("backend utf8"),
            Some(UUID),
            None,
            Some(""),
            "mismatched or malformed row",
        ),
        (
            "multiple-rows",
            backend.to_str().expect("backend utf8"),
            Some(UUID),
            None,
            Some(""),
            "multiple GPU rows",
        ),
    ];
    for (name, backend_value, visible, arch, fail, expected) in cases {
        if spawned.exists() {
            fs::remove_file(&spawned).expect("remove prior cargo-spawned marker");
        }
        let mut command = Command::new(binary);
        command
            .arg("build")
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()));
        command.env_remove("CUDA_OXIDE_TARGET");
        command.env("CUDA_OXIDE_BACKEND", backend_value);
        let row = match name {
            "unsupported-arch" => format!("{UUID}, 6.1"),
            "wrong-row" => "GPU-11234567-89ab-cdef-0123-456789abcdef, 8.0".into(),
            "multiple-rows" => format!("{UUID}, 8.0\n{UUID}, 8.0"),
            _ => format!("{UUID}, 8.0"),
        };
        command.env("NV_ROW", row);
        command.env("NV_FAIL", fail.expect("stub status input"));
        if let Some(visible) = visible {
            command.env("CUDA_VISIBLE_DEVICES", visible);
        } else {
            command.env_remove("CUDA_VISIBLE_DEVICES");
        }
        command.env_remove("CUDA_OXIDE_DEVICE_ARCH");
        if let Some(arch) = arch {
            command.env("CUDA_OXIDE_DEVICE_ARCH", arch);
        }
        let output = command.output().expect("run cargo-nv preflight harness");
        assert!(!output.status.success(), "{name} unexpectedly passed");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(expected), "{name}: {stderr}");
        assert!(!spawned.exists(), "{name} spawned cargo");
    }
    let evidence = root.join("evidence");
    fs::create_dir(&evidence).expect("create private identity evidence directory");
    let output = Command::new(binary)
        .arg("build")
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("CUDA_OXIDE_BACKEND", backend)
        .env("CUDA_VISIBLE_DEVICES", UUID)
        .env("NV_ROW", format!("{UUID}, 9.0"))
        .env("NV_FAIL", "")
        .env("NVMPI_MANIFEST_DIR", &evidence)
        .output()
        .expect("run valid cargo-nv identity harness");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(spawned.exists(), "valid identity did not invoke cargo");
    let identity = fs::read_to_string(evidence.join("cargo-nv-identity.log"))
        .expect("read campaign identity evidence");
    assert!(
        identity.starts_with("backend-sha256=") && identity.ends_with(" arch=sm_90a\n"),
        "{identity}"
    );
    fs::remove_dir_all(root).expect("remove private preflight harness");
}
