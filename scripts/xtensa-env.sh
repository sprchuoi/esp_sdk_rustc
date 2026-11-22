# Put the espup Xtensa toolchain on PATH. Source this, do not execute.
#
# espup writes $HOME/export-esp.sh by default; if it was installed with
# --export-file elsewhere, set ESPUP_EXPORT_FILE. As a last resort, locate the
# toolchain in the rustup directory so CI/containers without the export file
# still build.
_xtensa_export="${ESPUP_EXPORT_FILE:-$HOME/export-esp.sh}"
if [ -f "$_xtensa_export" ]; then
  set -a
  # shellcheck disable=SC1090
  . "$_xtensa_export"
  set +a
else
  for _dir in "$HOME"/.rustup/toolchains/esp/xtensa-esp-elf/*/xtensa-esp-elf/bin; do
    [ -d "$_dir" ] && PATH="$_dir:$PATH"
  done
  for _dir in "$HOME"/.rustup/toolchains/esp/xtensa-esp-elf-clang/*/esp-clang/lib; do
    [ -d "$_dir" ] && LIBCLANG_PATH="$_dir"
  done
  export PATH LIBCLANG_PATH
fi
unset _xtensa_export _dir
