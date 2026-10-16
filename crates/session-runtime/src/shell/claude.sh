# Original zmux session-only Claude completion integration (MIT).
# Plugins are repeatable and additive; existing user settings/hooks stay loaded.
claude() {
    if [ "${ZMUX_CLAUDE_INTEGRATION_DISABLED:-0}" = 1 ] ||
        [ ! -r "$ZMUX_SHELL_INTEGRATION_DIRECTORY/plugin/hooks/hooks.json" ] ||
        [ ! -p "$ZMUX_SHELL_INTEGRATION_DIRECTORY/attention" ]; then
        command \claude "$@"
        return $?
    fi
    case "${1-}" in
        --help|-h|--version|-v|auth|doctor|install|update|upgrade|mcp|plugin|plugins)
            command \claude "$@"
            return $?
            ;;
    esac
    command \claude --plugin-dir "$ZMUX_SHELL_INTEGRATION_DIRECTORY/plugin" "$@"
}
