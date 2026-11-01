@echo off
rem Builds dist\HoverCopy.exe on Windows.
rem Needs Rust (https://rustup.rs) with the MSVC toolchain:
rem Visual Studio Build Tools, workload "Desktop development with C++".
cd /d "%~dp0"
cargo build --release || exit /b 1
if not exist dist mkdir dist
copy /y target\release\HoverCopy.exe dist\ >nul || exit /b 1
echo Built dist\HoverCopy.exe
