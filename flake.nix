{
  description = "Rust project";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
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

      pkgsFor = forAllSystems (
        system:
        import nixpkgs {
          inherit system;
          overlays = [
            rust-overlay.overlays.default
          ];
        }
      );
    in
    {
      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor.${system};
          isLinux = pkgs.stdenv.hostPlatform.isLinux;

          rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

          linuxLibraries = [
            pkgs.alsa-lib
            pkgs.dbus
            pkgs.fontconfig
            pkgs.libx11
            pkgs.libxkbcommon
            pkgs.vulkan-loader
            pkgs.wayland
          ];
        in
        {
          default = pkgs.mkShell {
            strictDeps = true;

            nativeBuildInputs = [
              rustToolchain
              pkgs.pkg-config
              pkgs.rustPlatform.bindgenHook
            ];

            buildInputs = [ pkgs.ffmpeg ] ++ lib.optionals isLinux linuxLibraries;
            LD_LIBRARY_PATH = lib.optionalString isLinux (lib.makeLibraryPath linuxLibraries);
          };
        }
      );
    };
}
