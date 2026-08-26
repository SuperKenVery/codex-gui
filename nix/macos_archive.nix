{
  macosApp,
  pkgs,
  version,
}:
pkgs.runCommand "codex-gui-${version}-macos-${pkgs.stdenv.hostPlatform.uname.processor}"
  {
    nativeBuildInputs = [ pkgs.zip ];
  }
  ''
    mkdir -p "$out"
    cd ${macosApp}
    zip -r -9 "$out/codex-gui-${version}-macos-${pkgs.stdenv.hostPlatform.uname.processor}.zip" "Codex GUI.app"
  ''
