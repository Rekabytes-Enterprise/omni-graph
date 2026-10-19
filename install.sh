#!/bin/sh
# Install a verified native release without Node.js, Rust, or administrator access.
set -eu

main() {
    repository=Rekabytes-Enterprise/omni-graph
    case "$(uname -s)" in
        Darwin) os=darwin ;;
        Linux) os=linux ;;
        *) echo 'omni-graph: this installer supports macOS and Linux. On Windows, use npx --yes --package=https://github.com/Rekabytes-Enterprise/omni-graph/releases/latest/download/omni-graph.tgz omni-graph install.' >&2; return 1 ;;
    esac
    case "$(uname -m)" in
        arm64|aarch64) arch=arm64 ;;
        x86_64|amd64) arch=x64 ;;
        *) echo 'omni-graph: unsupported CPU architecture; see the supported platforms in README.md.' >&2; return 1 ;;
    esac
    for dependency in curl tar mktemp awk; do
        command -v "$dependency" >/dev/null 2>&1 || { echo "omni-graph: $dependency is required." >&2; return 1; }
    done
    if command -v sha256sum >/dev/null 2>&1; then
        checksum=sha256sum
    elif command -v shasum >/dev/null 2>&1; then
        checksum=shasum
    else
        echo 'omni-graph: sha256sum or shasum is required to verify the download.' >&2
        return 1
    fi
    version=${OMNI_GRAPH_VERSION:-latest}
    if [ "$version" = latest ]; then
        base="https://github.com/$repository/releases/latest/download"
    else
        case "$version" in v*) ;; *) version="v$version" ;; esac
        case "$version" in *[!a-zA-Z0-9._-]*) echo 'omni-graph: invalid OMNI_GRAPH_VERSION.' >&2; return 1 ;; esac
        base="https://github.com/$repository/releases/download/$version"
    fi
    asset="omni-graph-$os-$arch.tar.gz"
    temp=$(mktemp -d "${TMPDIR:-/tmp}/omni-graph-install.XXXXXXXX")
    trap 'rm -rf "$temp"' EXIT
    trap 'exit 1' HUP INT TERM
    echo "Downloading Omni-graph for $os/$arch..."
    curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 --retry 3 --output "$temp/$asset" "$base/$asset"
    curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 --retry 3 --output "$temp/SHA256SUMS" "$base/SHA256SUMS"
    expected=$(awk -v asset="$asset" '$2 == asset { print $1 }' "$temp/SHA256SUMS")
    case "$expected" in ''|*[!a-f0-9]*) echo 'omni-graph: missing or invalid release checksum.' >&2; return 1 ;; esac
    [ "${#expected}" -eq 64 ] || { echo 'omni-graph: invalid release checksum length.' >&2; return 1; }
    if [ "$checksum" = sha256sum ]; then
        actual=$(sha256sum "$temp/$asset")
    else
        actual=$(shasum -a 256 "$temp/$asset")
    fi
    actual=${actual%% *}
    [ "$actual" = "$expected" ] || { echo 'omni-graph: checksum mismatch; nothing was installed.' >&2; return 1; }
    tar -xzf "$temp/$asset" -C "$temp" omni-graph
    [ -f "$temp/omni-graph" ] && [ ! -L "$temp/omni-graph" ] || { echo 'omni-graph: release archive is missing its executable.' >&2; return 1; }
    chmod +x "$temp/omni-graph"
    "$temp/omni-graph" install
}

main "$@"
