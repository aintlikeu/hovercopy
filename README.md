# HoverCopy

[Русская версия](README.ru.md)

Copy the word under the mouse cursor with a hotkey — in any window, including text that cannot
be selected: games, images, videos, program interfaces. The text is read from the screen
locally with PaddleOCR models; nothing is sent anywhere. One portable `HoverCopy.exe`
(~28 MB) for Windows 10/11, no installation.

## Usage

1. Run `HoverCopy.exe`. It lives in the system tray.
2. Point at a word and press **Ctrl+Alt+N**.
3. The word is on the clipboard; a short "Скопировано: …" note appears next to the cursor
   (it can be turned off).

One word is copied, without punctuation around it (`"hello",` → `hello`) but with the
punctuation inside (`1,250`, `mr.smith`, `user_42`). For Chinese and Japanese, which have no
spaces, a word is the text between punctuation marks. A press takes about 0.25 s.

## Settings

Tray icon → **Настройки…**: the hotkey, the "Скопировано" note and debug captures. Changes apply
on Save. They are stored in `%APPDATA%\HoverCopy\settings.ini`, which can also be edited by
hand (restart to apply).

## Limitations

- Latin letters, digits, symbols and Chinese are recognized. Cyrillic and Korean are not:
  they are not in the model's alphabet.
- Very small text (about 11 px) on noisy backgrounds is less reliable.
- The interface is in Russian.

## Building

With [Rust](https://rustup.rs): `build.bat` on Windows (needs Visual Studio Build Tools with
"Desktop development with C++"), or `./build.sh` on Linux to cross-compile (needs the
`x86_64-pc-windows-gnu` target and `gcc-mingw-w64-x86-64`). The result is
`dist/HoverCopy.exe`.

Tests: `cargo test --profile fast -- --nocapture` prints recognition accuracy on test images
with different backgrounds, fonts and sizes (`tests/fixtures`, made by
`tools/make_fixtures.py`).

## License

[Apache-2.0](LICENSE). HoverCopy includes the PaddleOCR PP-OCRv4 models, also Apache-2.0; see
[NOTICE](NOTICE).
