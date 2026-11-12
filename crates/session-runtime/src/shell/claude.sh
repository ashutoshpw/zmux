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
    if [ "${ZMUX_LOCAL_CLAUDE_LIFECYCLE-}" = 1 ]; then
        if [ -x "${ZMUX_CLI_PATH-}" ] && [ -n "${ZMUX_SURFACE_ID-}" ]; then
            "$ZMUX_CLI_PATH" hooks claude auto \
                --root "$ZMUX_SHELL_INTEGRATION_DIRECTORY" \
                --surface "$ZMUX_SURFACE_ID" \
                --notice-plugin "$ZMUX_SHELL_INTEGRATION_DIRECTORY/plugin" \
                -- "$@"
            return $?
        fi
        printf 'zmux: automatic local Claude lifecycle unavailable; launching with terminal notices only\n' >&2
    fi
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

# OSC 133;A is the shell-owned prompt boundary consumed by the terminal's
# program-status lifetime policy. Keep this separate from provider identity.
zmux_program_status_prompt() {
    printf '\033]133;A\007'
}

# Bash's private rc wrapper calls this again after the user's selected startup
# file, so a later PROMPT_COMMAND assignment cannot suppress the prompt marker.
zmux_install_program_status_prompt() {
    [ -n "${BASH_VERSION-}" ] || return 0
    case "$(declare -p PROMPT_COMMAND 2>/dev/null)" in
        *zmux_program_status_prompt*) ;;
        *)
            case "$(declare -p PROMPT_COMMAND 2>/dev/null)" in
                'declare -a PROMPT_COMMAND='*|'declare -ar PROMPT_COMMAND='*)
                    # Keep Bash's array syntax inside eval so POSIX shells can
                    # parse this file even though the branch is Bash-only.
                    eval 'PROMPT_COMMAND=(zmux_refresh_openers zmux_program_status_prompt "${PROMPT_COMMAND[@]}")'
                    ;;
                *)
                    PROMPT_COMMAND="zmux_refresh_openers;zmux_program_status_prompt${PROMPT_COMMAND:+;${PROMPT_COMMAND}}"
                    ;;
            esac
            ;;
    esac
    export PROMPT_COMMAND
}
if [ -n "${BASH_VERSION-}" ]; then
    zmux_install_program_status_prompt
fi

# Zsh's hook API composes with user precmd functions and preserves ordering.
if [ -n "${ZSH_VERSION-}" ]; then
    autoload -Uz add-zsh-hook
    add-zsh-hook precmd zmux_program_status_prompt
fi

if [ -r "${ZMUX_SHELL_INTEGRATION_DIRECTORY-}/provider.sh" ]; then
    . "$ZMUX_SHELL_INTEGRATION_DIRECTORY/provider.sh"
fi
