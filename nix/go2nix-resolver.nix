{ writeShellApplication, coreutils, nix, pluginFile }:
writeShellApplication {
  name = "iets-go2nix-resolver";
  runtimeInputs = [ coreutils ];
  text = ''
    input=$(mktemp)
    trap 'rm -f "$input"' EXIT
    cat > "$input"
    ${nix}/bin/nix-instantiate \
      --option plugin-files ${pluginFile} \
      --eval --strict --json --impure \
      --expr '{ path }: builtins.resolveGoPackages (builtins.fromJSON (builtins.readFile path))' \
      --argstr path "$input"
  '';
}
