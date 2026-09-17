{
  description = "Zerium - A video editor with zero limits.";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
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

      cargoToml = fromTOML (builtins.readFile ./Cargo.toml);
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;

          rust = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

          rustPlatform = pkgs.makeRustPlatform {
            cargo = rust;
            rustc = rust;
          };

          zerium = rustPlatform.buildRustPackage {
            pname = cargoToml.package.name;
            version = cargoToml.package.version;

            src = ./.;
            cargoLock = {
              lockFile = ./Cargo.lock;
              allowBuiltinFetchGit = true;
            };
            strictDeps = true;
            doCheck = false;

            nativeBuildInputs = [
              pkgs.pkg-config
              pkgs.rustPlatform.bindgenHook
            ]
            ++ lib.optionals pkgs.stdenv.hostPlatform.isLinux [
              pkgs.autoPatchelfHook
            ];

            buildInputs = [
              pkgs.ffmpeg
            ]
            ++ lib.optionals pkgs.stdenv.hostPlatform.isLinux (linuxRuntimeDeps pkgs)
            ++ lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
              pkgs.libiconv
            ];

            installPhase = ''
              runHook preInstall

              install -Dm755 \
                target/${pkgs.stdenv.hostPlatform.rust.cargoShortTarget}/release/zerium \
                "$out/bin/zerium"
            ''
            + lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
              install -Dm644 packaging/linux/zerium.desktop \
                "$out/share/applications/zerium.desktop"

              install -Dm644 assets/zerium.png \
                "$out/share/icons/hicolor/256x256/apps/zerium.png"

              install -Dm644 assets/zerium.svg \
                "$out/share/icons/hicolor/scalable/apps/zerium.svg"

              install -Dm644 LICENSE \
                "$out/share/doc/zerium/copyright"
            ''
            + ''
              runHook postInstall
            '';

            meta = {
              description = cargoToml.package.description;
              license = lib.licenses.gpl3Plus;
              mainProgram = "zerium";
              platforms = systems;
            };
          };
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
            inputsFrom = [ self.packages.${system}.zerium ];
            packages = [ rust ];
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
