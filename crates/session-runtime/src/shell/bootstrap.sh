# Runs under a private session-owned shell, locally or on the SSH host. No user
# startup files or Claude settings are written. The outer owner removes its exact
# files after the interactive shell exits; the local Rust lease also cleans on kill.
zmux_private_root=$1
shift
zmux_shell=$1
shift
case "${zmux_shell##*/}" in
    bash|zsh|sh|dash|fish) ;;
    *) printf 'zmux: automatic Claude notifications unavailable for this shell\n' >&2
       exec "$zmux_shell" "$@" ;;
esac
umask 077
if [ -z "$zmux_private_root" ]; then
    # Keep shell lifecycle storage independent of remote transfer staging.
    # TMPDIR can be scoped to a single upload operation by a managed host.
    zmux_private_root=$(mktemp -d "/tmp/zmux-shell.XXXXXXXX") || exit 70
fi
ZMUX_SHELL_INTEGRATION_DIRECTORY=$zmux_private_root
export ZMUX_SHELL_INTEGRATION_DIRECTORY
zmux_shell_cleanup() {
    rm -f "$zmux_private_root/claude.sh" "$zmux_private_root/claude.zsh" "$zmux_private_root/claude.fish" \
        "$zmux_private_root/.zshenv" "$zmux_private_root/.zprofile" \
        "$zmux_private_root/.zshrc" "$zmux_private_root/env" \
        "$zmux_private_root/plugin/.claude-plugin/plugin.json" \
        "$zmux_private_root/plugin/hooks/hooks.json"
    rmdir "$zmux_private_root/plugin/.claude-plugin" "$zmux_private_root/plugin/hooks" \
        "$zmux_private_root/plugin" "$zmux_private_root" 2>/dev/null || :
}
trap 'zmux_shell_cleanup' 0
trap 'exit 129' HUP
trap 'exit 143' TERM
mkdir "$zmux_private_root/plugin" "$zmux_private_root/plugin/.claude-plugin" \
    "$zmux_private_root/plugin/hooks" || exit 70
# ZMUX_GENERATED_FILES
case "${zmux_shell##*/}" in
    bash)
        # A private noninteractive helper exports only our function before exec.
        # The real interactive Bash retains its original argv and startup policy.
        # -p prevents the helper from sourcing an inherited BASH_ENV.
        "$zmux_shell" -p -c '. "$ZMUX_SHELL_INTEGRATION_DIRECTORY/claude.sh"; export -f claude; exec "$@"' zmux "$zmux_shell" "$@"
        ;;
    zsh)
        ZMUX_USER_ZDOTDIR=${ZDOTDIR-$HOME}
        ZMUX_USER_ZDOTDIR_SET=${ZDOTDIR+x}
        export ZMUX_USER_ZDOTDIR ZMUX_USER_ZDOTDIR_SET
        ZDOTDIR=$zmux_private_root "$zmux_shell" "$@"
        ;;
    fish)
        "$zmux_shell" --init-command 'source "$ZMUX_SHELL_INTEGRATION_DIRECTORY/claude.fish"' "$@"
        ;;
    sh|dash)
        ZMUX_USER_ENV=${ENV-}
        export ZMUX_USER_ENV
        ENV=$zmux_private_root/env "$zmux_shell" "$@"
        ;;
esac
exit $?
