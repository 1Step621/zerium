{
  description = "Zerium - A video editor with zero limits.";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      nixpkgs,
      crane,
      rust-overlay,
      ...
    }:
    let
      inherit (nixpkgs) lib;

      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      forAllSystems = lib.genAttrs systems;

      pkgsFor =
        system:
        import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };

      linuxRuntimeDeps = pkgs: [
        pkgs.alsa-lib
        pkgs.dbus
        pkgs.fontconfig
        pkgs.freetype
        pkgs.libx11
        pkgs.libxcb
        pkgs.libxkbcommon
        pkgs.vulkan-loader
        pkgs.wayland
      ];

      nativeBuildDeps =
        pkgs:
        [
          pkgs.pkg-config
          pkgs.makeWrapper
        ]
        ++ lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
          pkgs.rustPlatform.bindgenHook
          pkgs.swift
          pkgs.swiftpm
        ];

      # apple-cf builds a Swift static library, but Cargo performs the final
      # link with clang rather than swiftc. Clang needs both the Swift toolchain
      # libraries and the Swift overlays shipped in the selected macOS SDK.
      darwinRustFlags =
        pkgs:
        let
          swiftLibDir = "${pkgs.swift}/${pkgs.swift.swiftLibSubdir}";
          sdkSwiftLibDir = "${pkgs.apple-sdk_26.sdkroot}/usr/lib/swift";
          swiftRuntimeDir = "${pkgs.swiftPackages.stdlib}/lib";
        in
        "-L native=${swiftLibDir} -L native=${sdkSwiftLibDir} -C link-arg=-Wl,-rpath,${swiftRuntimeDir}";

      gstreamerPlugins = pkgs: [
        pkgs.gst_all_1.gst-plugins-base
        pkgs.gst_all_1.gst-plugins-good
        pkgs.gst_all_1.gst-plugins-bad
        pkgs.gst_all_1.gst-plugins-ugly
        pkgs.gst_all_1.gst-libav
      ];

      # Core elements (filesrc, queue, typefind) live in the library output.
      gstreamerPluginPath = pkgs:
        lib.makeSearchPath "lib/gstreamer-1.0" (
          [ pkgs.gst_all_1.gstreamer.out ] ++ gstreamerPlugins pkgs
        );

      buildDeps =
        pkgs:
        [ pkgs.gst_all_1.gstreamer ]
        ++ gstreamerPlugins pkgs
        ++ lib.optionals pkgs.stdenv.hostPlatform.isLinux (linuxRuntimeDeps pkgs)
        ++ lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
          pkgs.libiconv
          # Swift 6.2 uses SDK 26; Cargo's final clang link must use it too.
          pkgs.apple-sdk_26
        ];

      workspaceToml = fromTOML (builtins.readFile ./Cargo.toml);
      cargoToml = fromTOML (builtins.readFile ./crates/zerium/Cargo.toml);
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;

          rustToolchain = p: p.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

          craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

          commonArgs = {
            src = ./.;

            cargoVendorDir = craneLib.vendorCargoDeps {
              src = ./.;
              overrideVendorGitCheckout =
                packages: drv:
                if
                  lib.any (p: lib.hasPrefix "git+https://github.com/software-mansion/smelter?" p.source) packages
                then
                  drv.overrideAttrs (_: {
                    src = fetchGit {
                      url = "https://github.com/software-mansion/smelter";
                      rev = lib.last (lib.splitString "#" (builtins.head packages).source);
                      allRefs = true;
                      submodules = false;
                    };
                  })
                else
                  drv;
            };

            pname = cargoToml.package.name;
            version = workspaceToml.workspace.package.version;

            strictDeps = true;
            doCheck = false;
            env = lib.optionalAttrs pkgs.stdenv.hostPlatform.isDarwin {
              RUSTFLAGS = darwinRustFlags pkgs;
            };
            cargoExtraArgs = "--no-default-features";

            nativeBuildInputs = nativeBuildDeps pkgs;
            buildInputs = buildDeps pkgs;
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          zerium = craneLib.buildPackage (
            commonArgs
            // {
              inherit cargoArtifacts;

              nativeBuildInputs =
                commonArgs.nativeBuildInputs
                ++ lib.optionals pkgs.stdenv.hostPlatform.isLinux [
                  pkgs.autoPatchelfHook
                ];

              runtimeDependencies = lib.optionals pkgs.stdenv.hostPlatform.isLinux (linuxRuntimeDeps pkgs);

              postFixup = ''
                wrapProgram "$out/bin/zerium" --prefix GST_PLUGIN_SYSTEM_PATH_1_0 : "${gstreamerPluginPath pkgs}"
              '';

              postInstall = lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
                install -Dm644 packaging/linux/zerium.desktop \
                  "$out/share/applications/zerium.desktop"

                install -Dm644 packaging/linux/zerium.xml \
                  "$out/share/mime/packages/zerium.xml"

                install -Dm644 assets/zerium.png \
                  "$out/share/icons/hicolor/256x256/apps/zerium.png"

                install -Dm644 assets/zerium.svg \
                  "$out/share/icons/hicolor/scalable/apps/zerium.svg"

                install -Dm644 LICENSE \
                  "$out/share/doc/zerium/copyright"

                install -Dm644 assets/inter/OFL.txt \
                  "$out/share/doc/zerium/Inter-OFL.txt"
              '';

              meta = {
                description = cargoToml.package.description;
                license = lib.licenses.gpl3Plus;
                mainProgram = "zerium";
                platforms = systems;
              };
            }
          );
        in
        {
          inherit zerium;
          default = zerium;
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          rust = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        in
        {
          default = pkgs.mkShell {
            packages = [ rust ];
            nativeBuildInputs = nativeBuildDeps pkgs;
            buildInputs = buildDeps pkgs;
            env = lib.optionalAttrs pkgs.stdenv.hostPlatform.isDarwin {
              RUSTFLAGS = darwinRustFlags pkgs;
            };
            GST_PLUGIN_SYSTEM_PATH_1_0 = gstreamerPluginPath pkgs;
            LD_LIBRARY_PATH = lib.optionalString pkgs.stdenv.hostPlatform.isLinux (
              lib.makeLibraryPath (linuxRuntimeDeps pkgs)
            );
          };
        }
      );
    };

  nixConfig = {
    extra-substituters = [ "https://zerium.cachix.org" ];
    extra-trusted-public-keys = [ "zerium.cachix.org-1:H6/69zhx0y/NO0jKNrqp2TTTVOFPyIyuZNJy/TX5IlU=" ];
  };
}
