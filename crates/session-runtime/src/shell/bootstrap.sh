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
# Legacy Bash makes async child INT/QUIT dispositions irrevocably ignored.
# Perl only resets those two dispositions and immediately execs the shell; no
# interpreter remains resident. Modern Bash and POSIX dash need no helper.
zmux_legacy_bash=0
case "${BASH_VERSION-}" in
    3.*|4.*)
        zmux_legacy_bash=1
        if [ ! -x /usr/bin/perl ]; then
            printf 'zmux: shell integration unavailable: legacy Bash requires /usr/bin/perl\n' >&2
            exec "$zmux_shell" "$@"
        fi
        ;;
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
        "$zmux_private_root/attention" \
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
# Keep notification ingress outside any nested tmux/screen PTY. Only fixed event
# names cross this private FIFO; the relay retains this session's original TTY,
# never a device pathname that could later belong to another session. The outer
# owner keeps the writer open; interactive descendants inherit neither descriptor.
mkfifo "$zmux_private_root/attention" || exit 70
exec 7>/dev/tty || exit 70
(
    # SSH hangs up the owning process group. This reader's read builtin is
    # interruptible even when Darwin's outer shell defers its foreground trap.
    # Both owners remove only their shared, exact private lease files.
    trap 'zmux_shell_cleanup' 0
    trap 'exit 129' HUP
    trap 'exit 143' TERM
    while IFS= read -r zmux_attention_event; do
        case "$zmux_attention_event" in
            stop) printf '\033]777;notify;Claude;Claude finished its turn\007' >&7 ;;
            failure) printf '\033]777;notify;Claude;Claude stopped with an error\007' >&7 ;;
            attention) printf '\033]777;notify;Claude;Claude needs attention\007' >&7 ;;
        esac
    done <"$zmux_private_root/attention"
) &
exec 8>"$zmux_private_root/attention"
# A shell wait builtin handles owner-only HUP immediately; Darwin need not send
# that HUP to the relay group or the foreground provider. Preserve the real stdin.
exec 9<&0
(
trap - 0 HUP TERM INT QUIT
zmux_exec_shell() {
    if [ "$zmux_legacy_bash" = 1 ]; then
        # The private helper must not load user Perl modules. Preserve their
        # original set/unset values in environment data, never in command argv.
        export ZMUX_PRIVATE_PERL5OPT_SET="${PERL5OPT+x}" ZMUX_PRIVATE_PERL5OPT="${PERL5OPT-}"
        export ZMUX_PRIVATE_PERL5LIB_SET="${PERL5LIB+x}" ZMUX_PRIVATE_PERL5LIB="${PERL5LIB-}"
        unset PERL5OPT PERL5LIB
        exec /usr/bin/perl -f -e '
for my $name ("PERL5OPT", "PERL5LIB") {
    my $set = delete $ENV{"ZMUX_PRIVATE_".$name."_SET"};
    my $value = delete $ENV{"ZMUX_PRIVATE_".$name};
    $ENV{$name} = $value if $set eq "x";
}
$SIG{INT}="DEFAULT"; $SIG{QUIT}="DEFAULT";
exec { $ARGV[0] } @ARGV;
die "zmux: cannot exec interactive shell: $!\n";' -- "$@"
    fi
    exec "$@"
}
case "${zmux_shell##*/}" in
    bash)
        # A private noninteractive helper exports only our function before exec.
        # The real interactive Bash retains its original argv and startup policy.
        # -p prevents the helper from sourcing an inherited BASH_ENV.
        zmux_exec_shell "$zmux_shell" -p -c '. "$ZMUX_SHELL_INTEGRATION_DIRECTORY/claude.sh"; export -f claude; exec "$@"' zmux "$zmux_shell" "$@"
        ;;
    zsh)
        ZMUX_USER_ZDOTDIR=${ZDOTDIR-$HOME}
        ZMUX_USER_ZDOTDIR_SET=${ZDOTDIR+x}
        export ZMUX_USER_ZDOTDIR ZMUX_USER_ZDOTDIR_SET
        ZDOTDIR=$zmux_private_root zmux_exec_shell "$zmux_shell" "$@"
        ;;
    fish)
        zmux_exec_shell "$zmux_shell" --init-command 'source "$ZMUX_SHELL_INTEGRATION_DIRECTORY/claude.fish"' "$@"
        ;;
    sh|dash)
        ZMUX_USER_ENV=${ENV-}
        export ZMUX_USER_ENV
        ENV=$zmux_private_root/env zmux_exec_shell "$zmux_shell" "$@"
        ;;
esac
) <&9 7>&- 8>&- 9>&- &
zmux_shell_child=$!
wait "$zmux_shell_child"
exit $?
