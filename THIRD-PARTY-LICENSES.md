# Third-Party Licenses

ctrl-tab itself is licensed under the [MIT License](./LICENSE).

It is built on the open-source components listed below. Each component remains under
its own license; the SPDX identifiers were verified against the metadata of the
installed packages (npm `package.json` `license` fields and crate `Cargo.toml`
`license` fields / shipped license files). For dual/multi-licensed components you may
use them under **any** of the listed licenses.

Full license texts ship with each dependency:

- npm packages — `node_modules/<package>/LICENSE`
- Rust crates — `~/.cargo/registry/src/.../<crate>/` (and the git checkout for
  `tauri-nspanel`, which ships `LICENSE_APACHE-2.0` and `LICENSE_MIT`)
- JetBrains Mono font — bundled in this repo at
  [`licenses/JetBrainsMono-OFL.txt`](./licenses/JetBrainsMono-OFL.txt)

## Rust (Tauri backend)

| Component | License (SPDX) |
| --- | --- |
| tauri | Apache-2.0 OR MIT |
| tauri-plugin-single-instance | Apache-2.0 OR MIT |
| tauri-plugin-autostart | Apache-2.0 OR MIT |
| tauri-plugin-opener | Apache-2.0 OR MIT |
| tauri-nspanel | Apache-2.0 OR MIT |
| objc2 | Zlib OR Apache-2.0 OR MIT |
| objc2-app-kit | Zlib OR Apache-2.0 OR MIT |
| objc2-foundation | MIT |
| block2 | MIT |
| core-graphics | MIT OR Apache-2.0 |
| core-foundation | MIT OR Apache-2.0 |
| window-vibrancy | Apache-2.0 OR MIT |
| base64 | MIT OR Apache-2.0 |
| serde | MIT OR Apache-2.0 |
| serde_json | MIT OR Apache-2.0 |

## JavaScript / frontend

| Component | License (SPDX) |
| --- | --- |
| @tauri-apps/api | Apache-2.0 OR MIT |
| @tauri-apps/cli | Apache-2.0 OR MIT |
| react | MIT |
| react-dom | MIT |
| tailwindcss | MIT |
| vite | MIT |
| @fontsource/jetbrains-mono | OFL-1.1 |

## Fonts

| Component | License (SPDX) |
| --- | --- |
| JetBrains Mono | OFL-1.1 (SIL Open Font License 1.1) |

The JetBrains Mono font (© 2020 The JetBrains Mono Project Authors) is distributed
under the SIL Open Font License, Version 1.1. Its full text is included at
[`licenses/JetBrainsMono-OFL.txt`](./licenses/JetBrainsMono-OFL.txt). "JetBrains Mono"
is a Reserved Font Name under that license.

## Acknowledgements

ctrl-tab is inspired by [AltTab](https://github.com/lwouis/alt-tab-macos) but is an
independent project: it shares no source code with AltTab and is **not affiliated
with, endorsed by, or derived from** AltTab or Apple. No AltTab or Apple logos or
icons are used. "macOS", "Apple", "Visual Studio Code", "PhpStorm", and "JetBrains"
are trademarks of their respective owners and are referenced for descriptive purposes
only.
