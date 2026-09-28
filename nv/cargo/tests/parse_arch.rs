use nv_cargo::parse_arch;

#[test]
fn formats_device_arch_hints() {
    assert_eq!(parse_arch("7.5\n"), Some("sm_75".into()));
    assert_eq!(parse_arch("9.0\n"), Some("sm_90a".into()));
    assert_eq!(parse_arch("not available\n"), None);
}
