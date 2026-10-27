# Cargo for the validate checks (experiments/validate.nix): the vendored crates of the root and of
# rejections/, and the offline cargo configuration that uses them. Nothing is fetched from the network.
{ pkgs }:
{
  # The crates of the root Cargo.lock, vendored: the checks below build offline.
  vendor = pkgs.rustPlatform.importCargoLock { lockFile = ../Cargo.lock; };
  vendorRejections = pkgs.rustPlatform.importCargoLock { lockFile = ../rejections/Cargo.lock; };
  # Cargo's offline source (the vendored crates, nothing from the network), and a copy of the source tree
  # to build in: the checks write into the tree (run.sh rewrites its manifest), the store does not allow it.
  cargoHomeWith = vendorDir: src: dir: ''
    export CARGO_HOME="$TMPDIR/cargo-home"
    mkdir -p "$CARGO_HOME"
    cat > "$CARGO_HOME/config.toml" <<EOF
    [source.crates-io]
    replace-with = "vendored-sources"
    [source.vendored-sources]
    directory = "${vendorDir}"
    EOF
    cp -r ${src} "$TMPDIR/src"
    chmod -R u+w "$TMPDIR/src"
    cd "$TMPDIR/src/${dir}"
    export CARGO_TARGET_DIR="$TMPDIR/target"
  '';
}
