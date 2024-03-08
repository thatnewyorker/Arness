# (WIP) Arness: NES Emulator in Rust

Welcome to Arness, a high-performance Nintendo Entertainment System (NES) emulator written in Rust. Arness aims to provide an accurate and enjoyable emulation experience, focusing on compatibility, performance, and user-friendly features.

## (Planned) Features

- **High Compatibility:** Runs a wide range of NES games with high accuracy.
- **Optimized Performance:** Leverages Rust's efficiency to ensure smooth gameplay even on pitiful hardware.
- **Save States:** Save your progress at any point and return to it instantly.
- **Cross-Platform Support:** Works on Windows, macOS, and Linux.
- **Customizable Controls:** Easily configure keyboard or gamepad inputs to your preference.
- **Open Source:** Fully open-source under the MIT License. Contributions are welcome!

## Getting Started

### Prerequisites

- Rust (latest stable version)
- SDL2 (for graphics and audio output)

### Installation

1. Clone the repository:

```bash
git clone https://github.com/thatnewyorker/Arness.git
cd arness
cargo build --release
cargo run --release path/to/your/game.rom
```

Replace path/to/your/game.rom with the path to the NES ROM file you wish to play.

Usage

After launching a game, use the configured input methods to control the game. You can access the emulator settings and configure controls, video options, and more by editing the config.toml file (see Configuration section below).
Configuration

Arness can be customized through a config.toml file located in the root directory. This file allows you to set various options related to video, audio, and input controls. See the config.example.toml file for a template and instructions.

Contributing

Contributions to Arness are warmly welcomed. Whether you're fixing bugs, adding new features, or improving documentation, your help is appreciated. Please check the CONTRIBUTING.md file for more details on how to contribute.

License

Arness is released under the MIT License. See the LICENSE file for more details.
Acknowledgments

    Thanks to all the contributors who have helped make Arness better.
    Special thanks to the Rust programming community for their invaluable resources and support.

Disclaimer

Arness is a project developed for educational purposes and personal use. It does not include any copyrighted Nintendo software or games. Users are responsible for obtaining NES ROMs from legal sources.

Enjoy your journey back to the classic NES era with Arness!
