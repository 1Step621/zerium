# Zerium

A video editor with zero limits.

## Concept

<img align="right" src="assets/zerium.svg" width="180" alt="Zerium icon">

- Beginner-friendly features and extensible plugins, like [AviUtl](https://spring-fragrance.mints.ne.jp/aviutl/)
- Rich effects and an intuitive interface for advanced editing
- Advanced scene features that enhance work reusability
- High-speed, real-time rendering using the GPU
- Cross-platform support
- Open source and free forever!

## Install

Available formats include MSI, DMG, AppImage, DEB, RPM, ELF binaries, and Nix packages.

### Nix

Run Zerium directly with:

```sh
nix run github:1Step621/zerium
```

For NixOS or nix-darwin, add Zerium as a flake input:

```nix
{
  inputs.zerium.url = "github:1Step621/zerium";
}
```

Then add it to your system packages:

```nix
{ inputs, pkgs, ... }:
{
  environment.systemPackages = [
    inputs.zerium.packages.${pkgs.stdenv.hostPlatform.system}.default
  ];
}
```

## Status

Under active development: all features, including the project file format, are subject to breaking changes.

## Acknowledgements

- [AviUtl](https://spring-fragrance.mints.ne.jp/aviutl/)
  - A video editing software developed by KEN-kun, embraced by countless creators across Japanese internet culture.
  - Huge respect for offering it for free and making it accessible to beginners while remaining powerful enough for advanced editing.
- [Adachi Rei](https://mechanicalgirl.jp/adachi-rei/)
  - A **cute** synthesized voice character created by [missile](https://x.com/missile_39) at Mechanical Girl!
  - The word "Rei" means "zero" in Japanese, which inspired the name "Zerium".

## Contributing

Contributions of all kinds are welcome! Feel free to open an issue or submit a PR - AI-generated code is welcome, too.

Use `direnv` to enter the shared Rust environment:

```sh
direnv allow
```
