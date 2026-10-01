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
  nix.settings = {
    substituters = [ "https://zerium.cachix.org" ];
    trusted-public-keys = [ "zerium.cachix.org-1:H6/69zhx0y/NO0jKNrqp2TTTVOFPyIyuZNJy/TX5IlU=" ];
  };

  environment.systemPackages = [
    inputs.zerium.packages.${pkgs.stdenv.hostPlatform.system}.default
  ];
}
```

## Status

Under active development: all features, including the project file format, are subject to breaking changes.

## Fonts

The UI uses bundled [Inter 4.1](https://github.com/rsms/inter/releases/tag/v4.1)
as its primary font, with regular, medium, semibold, bold, and italic faces.
Inter's license is included in `assets/inter/OFL.txt` and distributed packages.
Missing CJK glyphs use regional fonts, including
Yu Gothic UI / Meiryo on Windows and Noto Sans CJK JP / Noto Sans JP on Linux.
Install a Japanese font on Linux if none is available; Zerium does not bundle CJK fonts.

CJK fallback families are selected solely from the system locale, including
regional tags such as `ja-JP`, `ko-KR`, and `zh-Hant-TW`.
Text items try explicitly chosen font families in order, then use `sans-serif`.
Missing glyphs in that family are handled by the font system's regional fallbacks.

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

To develop against sibling `wgpui` and `wgpui-component` checkouts, create a local
`.cargo/config.toml` (ignored by Git):

```toml
[patch."https://github.com/1Step621/WGPUI"]
gpui-ce = { path = "../wgpui" }

[patch."https://github.com/1Step621/WGPUI-Component"]
ui = { path = "../wgpui-component/crates/ui" }
```

After publishing library changes, update the Git dependency revisions and regenerate
`Cargo.lock` without the local overrides before packaging a standalone release.
