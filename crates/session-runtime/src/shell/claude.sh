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

# Startup files such as macOS /etc/zprofile may rebuild PATH after the outer
# bootstrap prepends our session helpers. Reassert the private helper path after
# user startup, while retaining every other entry and any browser overrides.
zmux_refresh_openers() {
    [ -n "${ZMUX_OPEN_URL_NONCE-}" ] || return 0
    zmux_open_bin="$ZMUX_SHELL_INTEGRATION_DIRECTORY/bin"
    [ -x "$zmux_open_bin/open" ] || return 0
    case "${PATH-}" in
        "$zmux_open_bin"|"$zmux_open_bin":*) return 0 ;;
    esac
    PATH="$zmux_open_bin${PATH:+:$PATH}"
    export PATH
}
zmux_refresh_openers

# Bash has no post-startup rc hook. A prompt hook covers later startup changes
# without editing any user file or replacing their own prompt command.
if [ -n "${BASH_VERSION-}" ]; then
    case "${PROMPT_COMMAND-}" in
        *zmux_refresh_openers*) ;;
        '') PROMPT_COMMAND=zmux_refresh_openers ;;
        *) PROMPT_COMMAND="zmux_refresh_openers;${PROMPT_COMMAND}" ;;
    esac
    export PROMPT_COMMAND
fi
