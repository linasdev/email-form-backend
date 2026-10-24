#!/usr/bin/env bash
# Installs email-form-backend as a systemd service.
#
#   cargo build --release
#   sudo deploy/install.sh [--now]     install or upgrade
#   sudo deploy/install.sh --uninstall [--purge]
#
# Set DESTDIR to stage the files into another directory without touching the
# running system. systemctl is not called and root is not required in that case.

set -euo pipefail

NAME=email-form-backend
BIN_PATH=/usr/local/bin/$NAME
CONFIG_DIR=/etc/$NAME
UNIT_PATH=/etc/systemd/system/$NAME.service
DESTDIR=${DESTDIR:-}

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
PROJECT_DIR=$(dirname "$SCRIPT_DIR")
BUILT_BINARY=$PROJECT_DIR/target/release/$NAME

mode=install
start_now=false
purge=false

usage() {
    sed -n '2,10p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

for arg in "$@"; do
    case $arg in
        --now) start_now=true ;;
        --uninstall) mode=uninstall ;;
        --purge) purge=true ;;
        -h | --help) usage; exit 0 ;;
        *) echo "unknown option: $arg" >&2; usage >&2; exit 2 ;;
    esac
done

if [[ -z $DESTDIR && $EUID -ne 0 ]]; then
    echo "this script must be run as root (try: sudo $0 $*)" >&2
    exit 1
fi

use_systemctl() {
    [[ -z $DESTDIR ]] && command -v systemctl > /dev/null
}

install_service() {
    if [[ ! -x $BUILT_BINARY ]]; then
        echo "$BUILT_BINARY not found; run 'cargo build --release' first" >&2
        exit 1
    fi

    local was_active=false
    if use_systemctl && systemctl is-active --quiet "$NAME"; then
        was_active=true
    fi

    install -D -m 0755 "$BUILT_BINARY" "$DESTDIR$BIN_PATH"
    echo "installed $BIN_PATH"

    install -d -m 0755 "$DESTDIR$CONFIG_DIR"

    if [[ -e $DESTDIR$CONFIG_DIR/config.toml ]]; then
        echo "kept existing $CONFIG_DIR/config.toml"
    else
        install -m 0644 "$PROJECT_DIR/config.toml" "$DESTDIR$CONFIG_DIR/config.toml"
        echo "installed $CONFIG_DIR/config.toml"
    fi

    if [[ -e $DESTDIR$CONFIG_DIR/environment ]]; then
        echo "kept existing $CONFIG_DIR/environment"
    else
        install -m 0600 /dev/null "$DESTDIR$CONFIG_DIR/environment"
        cat > "$DESTDIR$CONFIG_DIR/environment" << 'EOF'
# Secrets and per-host overrides, one KEY=value per line, no quotes needed.
# Variables are named EMAIL_FORM_<SECTION>__<KEY> (two underscores).
# EMAIL_FORM_SMTP__USERNAME=
# EMAIL_FORM_SMTP__PASSWORD=
# EMAIL_FORM_RECAPTCHA__SECRET=
# RUST_LOG=info
EOF
        echo "installed $CONFIG_DIR/environment (mode 0600)"
    fi

    install -D -m 0644 "$SCRIPT_DIR/$NAME.service" "$DESTDIR$UNIT_PATH"
    echo "installed $UNIT_PATH"

    if ! use_systemctl; then
        return
    fi

    systemctl daemon-reload

    if [[ $start_now == true ]]; then
        systemctl enable --now "$NAME"
        echo "service enabled and started"
    elif [[ $was_active == true ]]; then
        systemctl restart "$NAME"
        echo "service restarted"
    else
        systemctl enable "$NAME"
        echo
        echo "The service is enabled but not started. Next steps:"
        echo "  1. edit $CONFIG_DIR/config.toml (set the [smtp] section)"
        echo "  2. put secrets into $CONFIG_DIR/environment"
        echo "  3. systemctl start $NAME"
        echo "  4. journalctl -u $NAME -f"
    fi
}

uninstall_service() {
    if use_systemctl; then
        systemctl disable --now "$NAME" 2> /dev/null || true
    fi

    rm -f "$DESTDIR$UNIT_PATH" "$DESTDIR$BIN_PATH"
    echo "removed $UNIT_PATH and $BIN_PATH"

    if use_systemctl; then
        systemctl daemon-reload
    fi

    if [[ $purge == true ]]; then
        rm -rf "${DESTDIR:?}${CONFIG_DIR}"
        echo "removed $CONFIG_DIR"
    else
        echo "kept $CONFIG_DIR (use --purge to remove it)"
    fi
}

if [[ $mode == install ]]; then
    install_service
else
    uninstall_service
fi
