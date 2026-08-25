{
  description = "GPUI desktop shell for codex-gui";

  nixConfig = {
    extra-substituters = [
      "https://oranc.li7g.com/ghcr.io/SuperKenVery/codex-gui-nix-cache"
    ];
    extra-trusted-public-keys = [
      "codex-gui-oranc-1:M5KXc8rneTuIystz2Z53emJGdiByGib+qNBva4PS2d0="
    ];
  };

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    bundlers = {
      url = "github:NixOS/bundlers";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      rust-overlay,
      bundlers,
      ...
    }:
    let
      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      cargoPackage = builtins.fromTOML (builtins.readFile ./Cargo.toml);
      version = cargoPackage.package.version;
      perSystem =
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
          lib = pkgs.lib;
          rustToolchain = pkgs.rust-bin.stable.latest.default.override {
            extensions = [ "rust-src" ];
          };
          craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
          source = craneLib.cleanCargoSource ./.;
          dependencyDummySrc = craneLib.mkDummySrc {
            src = source;
            extraDummyScript = ''
              # codex-code-mode-host is a git dependency which imports the API
              # of this local [patch] replacement. Keep this one local crate's
              # implementation available while Crane stubs the application.
              rm -rf "$out/crates/codex-code-mode-runtime-quickjs/src"
              cp -R \
                ${./crates/codex-code-mode-runtime-quickjs/src} \
                "$out/crates/codex-code-mode-runtime-quickjs/src"
            '';
          };
          linuxRuntimeLibs = with pkgs; [
            libglvnd
            vulkan-loader
            wayland
          ];
          desktopItem = pkgs.makeDesktopItem {
            name = "codex-gui";
            desktopName = "Codex GUI";
            comment = "Native desktop client for Codex";
            exec = "codex-gui";
            icon = "codex-gui";
            categories = [ "Development" ];
            terminal = false;
          };
          commonArgs = {
            pname = "codex-gui";
            inherit version;
            src = source;
            strictDeps = true;
            nativeBuildInputs =
              with pkgs;
              [
                cmake
                pkg-config
              ]
              ++ lib.optionals pkgs.stdenv.isDarwin [
                pkgs.llvmPackages.lld
              ];
            buildInputs =
              lib.optionals pkgs.stdenv.isDarwin [
                pkgs.apple-sdk
                pkgs.libiconv
              ]
              ++ (with pkgs; [
                openssl
              ])
              ++ lib.optionals pkgs.stdenv.isLinux (
                with pkgs;
                [
                  fontconfig
                  freetype
                  libglvnd
                  libx11
                  libxcb
                  libxkbcommon
                  vulkan-loader
                  wayland
                ]
              );
            NIX_LDFLAGS = lib.optionalString pkgs.stdenv.isLinux "-rpath ${lib.makeLibraryPath linuxRuntimeLibs}";
            postInstall = ''
              # Both executables are runtime components and must remain in the
              # same directory in every package format.
              test -x "$out/bin/codex-gui"
              test -x "$out/bin/codex-code-mode-host"

              install -Dm644 \
                ${desktopItem}/share/applications/codex-gui.desktop \
                "$out/share/applications/codex-gui.desktop"
              install -Dm644 \
                ${./packaging/codex-gui.svg} \
                "$out/share/icons/hicolor/scalable/apps/codex-gui.svg"
            '';
            meta = {
              description = "Native desktop GUI for Codex";
              homepage = "https://github.com/SuperKenVery/codex-gui";
              mainProgram = "codex-gui";
              platforms = lib.platforms.linux ++ lib.platforms.darwin;
            };
          };
          cargoArtifacts = craneLib.buildDepsOnly (
            commonArgs
            // {
              dummySrc = dependencyDummySrc;
              # This derivation only exports Cargo artifacts; the final
              # executable assertions belong to buildPackage below.
              postInstall = "";
            }
          );
          codex-gui = craneLib.buildPackage (
            commonArgs
            // {
              inherit cargoArtifacts;
            }
          );

          macosInfoPlist = pkgs.writeText "Info.plist" ''
            <?xml version="1.0" encoding="UTF-8"?>
            <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "https://www.apple.com/DTDs/PropertyList-1.0.dtd">
            <plist version="1.0">
            <dict>
              <key>CFBundleDisplayName</key>
              <string>Codex GUI</string>
              <key>CFBundleExecutable</key>
              <string>codex-gui</string>
              <key>CFBundleIdentifier</key>
              <string>io.github.superkenvery.codex-gui</string>
              <key>CFBundleInfoDictionaryVersion</key>
              <string>6.0</string>
              <key>CFBundleName</key>
              <string>Codex GUI</string>
              <key>CFBundlePackageType</key>
              <string>APPL</string>
              <key>CFBundleShortVersionString</key>
              <string>${version}</string>
              <key>CFBundleVersion</key>
              <string>${version}</string>
              <key>LSMinimumSystemVersion</key>
              <string>12.0</string>
              <key>NSHighResolutionCapable</key>
              <true/>
              <key>NSPrincipalClass</key>
              <string>NSApplication</string>
            </dict>
            </plist>
          '';

          macosApp =
            pkgs.runCommand "codex-gui-${version}-macos-app"
              {
                nativeBuildInputs = [ pkgs.macdylibbundler ];
              }
              ''
                app="$out/Codex GUI.app"
                mkdir -p "$app/Contents/MacOS" "$app/Contents/Frameworks" "$app/Contents/Resources"
                cp ${codex-gui}/bin/codex-gui "$app/Contents/MacOS/codex-gui"
                cp ${codex-gui}/bin/codex-code-mode-host "$app/Contents/MacOS/codex-code-mode-host"
                cp ${macosInfoPlist} "$app/Contents/Info.plist"
                chmod 755 "$app/Contents/MacOS/codex-gui" "$app/Contents/MacOS/codex-code-mode-host"

                dylibbundler -od -b \
                  -x "$app/Contents/MacOS/codex-gui" \
                  -d "$app/Contents/Frameworks" \
                  -p @executable_path/../Frameworks/
                dylibbundler -od -b \
                  -x "$app/Contents/MacOS/codex-code-mode-host" \
                  -d "$app/Contents/Frameworks" \
                  -p @executable_path/../Frameworks/

                /usr/bin/codesign --force --deep --sign - "$app"
              '';

          macosArchive =
            pkgs.runCommand "codex-gui-${version}-macos-${pkgs.stdenv.hostPlatform.uname.processor}"
              {
                nativeBuildInputs = [ pkgs.zip ];
              }
              ''
                mkdir -p "$out"
                cd ${macosApp}
                zip -r -9 "$out/codex-gui-${version}-macos-${pkgs.stdenv.hostPlatform.uname.processor}.zip" "Codex GUI.app"
              '';

          linuxPackages = lib.optionalAttrs pkgs.stdenv.isLinux {
            appimage = bundlers.bundlers.${system}.toAppImage codex-gui;
            deb = bundlers.bundlers.${system}.toDEB codex-gui;
            rpm = bundlers.bundlers.${system}.toRPM codex-gui;
          };
          darwinPackages = lib.optionalAttrs pkgs.stdenv.isDarwin {
            macos-app = macosApp;
            macos-archive = macosArchive;
          };
        in
        {
          packages = {
            default = codex-gui;
            inherit codex-gui;
          }
          // linuxPackages
          // darwinPackages;

          apps.default = {
            type = "app";
            program = "${codex-gui}/bin/codex-gui";
          };

          checks = {
            clippy = craneLib.cargoClippy (
              commonArgs
              // {
                inherit cargoArtifacts;
                cargoClippyExtraArgs = "--all-targets -- --deny warnings";
              }
            );
            fmt = craneLib.cargoFmt {
              src = commonArgs.src;
            };
          };

          devShells.default = craneLib.devShell {
            checks = self.checks.${system};
            packages = if pkgs.stdenv.isLinux then (with pkgs; [ mangohud ]) else [ ];
            env = {
              RUST_BACKTRACE = "1";
              # Keep panic backtraces without making every recoverable anyhow
              # error capture a stack trace. GPUI creates recoverable errors in
              # hot rendering paths, where lib backtraces make scrolling jank.
              RUST_LIB_BACKTRACE = "0";
            }
            // lib.optionalAttrs pkgs.stdenv.isLinux {
              LD_LIBRARY_PATH = lib.makeLibraryPath linuxRuntimeLibs;
            };
          };
        };
    in
    {
      packages = forAllSystems (system: (perSystem system).packages);
      apps = forAllSystems (system: (perSystem system).apps);
      checks = forAllSystems (system: (perSystem system).checks);
      devShells = forAllSystems (system: (perSystem system).devShells);
    };
}
