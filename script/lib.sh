# Shared library for nit scripts
# Source this file: source "$(dirname "$0")/lib.sh"

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NIT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
BIN_DIR="${NIT_ROOT}/bin"

# Discover all executable nit implementations (full paths)
discover_implementations() {
    local impls=()
    for impl in "${BIN_DIR}"/nit-*; do
        [[ -x "$impl" ]] && impls+=("$impl")
    done
    echo "${impls[@]}"
}

# Discover implementation names (e.g., "rust", "zig")
discover_impl_names() {
    for impl in "${BIN_DIR}"/nit-*; do
        [[ -x "$impl" ]] && basename "$impl" | sed 's/nit-//'
    done
}

# Discover implementation directories (full paths to nit-* dirs)
discover_impl_dirs() {
    for dir in "${NIT_ROOT}/nit-"*; do
        [[ -d "$dir" ]] && echo "$dir"
    done
}

# Get build command for implementation type
get_build_cmd() {
    local impl="$1"
    case "$impl" in
        rust)    echo "cargo build --release" ;;
        zig)     echo "zig build --release=fast" ;;
        crystal) echo "shards build --release" ;;
        *)       return 1 ;;
    esac
}

# Get test command for implementation type
get_test_cmd() {
    local impl="$1"
    case "$impl" in
        rust)    echo "cargo test" ;;
        zig)     echo "zig build test" ;;
        crystal) echo "crystal spec" ;;
        *)       return 1 ;;
    esac
}

# Get implementation directory for type
get_impl_dir() {
    local impl="$1"
    echo "${NIT_ROOT}/nit-${impl}"
}

# Get binary output path for implementation type (after release build)
get_binary_path() {
    local impl="$1"
    case "$impl" in
        rust)    echo "${NIT_ROOT}/nit-rust/target/release/nit" ;;
        zig)     echo "${NIT_ROOT}/nit-zig/zig-out/bin/nit" ;;
        crystal) echo "${NIT_ROOT}/nit-crystal/bin/nit" ;;
        *)       return 1 ;;
    esac
}
