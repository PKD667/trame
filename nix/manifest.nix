# The backend part of Cargo.toml, generated from the descriptors (src/backends/README.md, The manifest). Cargo needs a
# feature per backend and the optional dependencies of its backend declared in the manifest, and a manifest cannot be
# computed; so the lines it needs are written here, between the markers `# >>> backends:<region>` and `# <<< backends` of
# Cargo.toml, and `checks.manifest` fails when the committed file differs. `nix run .#manifest` rewrites it.
#   dependencies  the optional dependencies of every backend
#   features      one feature per backend, named for its directory, enabling that backend's dependencies; and `lock`,
#                 the union of them all, which keeps a program's Cargo.lock whole without selecting a backend
#   bins          the binaries a backend builds from its own sources
{ pkgs, self, tree }:
let
  inherit (tree) lib backendNames descriptors;
  cargo = n: { dependencies = { }; bins = [ ]; } // (descriptors.${n}.cargo or { });

  value = v:
    if builtins.isAttrs v then "{ ${lib.concatStringsSep ", " (lib.mapAttrsToList (k: x: "${k} = ${value x}") v)} }"
    else builtins.toJSON v;

  # The same dependency named by two backends must be declared the same way.
  dependencies = lib.zipAttrsWith
    (name: ds: assert lib.assertMsg (lib.all (d: d == lib.head ds) ds) "backends declare `${name}` differently"; lib.head ds)
    (map (n: (cargo n).dependencies) backendNames);
  deps = n: map (d: "dep:${d}") (builtins.attrNames (cargo n).dependencies);
  list = xs: "[${lib.concatMapStringsSep ", " builtins.toJSON xs}]";

  regions = {
    dependencies = lib.concatStrings (lib.mapAttrsToList (k: v: "${k} = ${value v}\n") dependencies);
    features =
      lib.concatMapStrings (n: "${n} = ${list (deps n)}\n") backendNames
      + "lock = ${list (lib.unique (lib.concatMap deps backendNames))}\n";
    bins = lib.concatMapStrings (n: lib.concatMapStrings (b: ''
      [[bin]]
      name = ${builtins.toJSON b.name}
      path = ${builtins.toJSON b.path}
      required-features = ${list [ n ]}

    '') (cargo n).bins) backendNames;
  };

  # A region's text reaches awk as a file named by the environment variable `region_<name>`.
  generated = pkgs.runCommand "Cargo.toml"
    ({ nativeBuildInputs = [ pkgs.gawk ]; } // lib.mapAttrs' (k: v: lib.nameValuePair "region_${k}" (pkgs.writeText "region-${k}" v)) regions)
    ''
      awk '
        /^# <<< backends/ { skipping = 0 }
        !skipping { print }
        /^# >>> backends:/ {
          name = $3; sub(/^backends:/, "", name)
          while ((getline line < ENVIRON["region_" name]) > 0) print line
          skipping = 1
        }
      ' ${self + "/Cargo.toml"} > $out
    '';
in
{ inherit generated regions; }
