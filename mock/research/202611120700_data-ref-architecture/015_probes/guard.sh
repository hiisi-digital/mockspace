# Sourced by every 015 probe. The probe makes its own scratch directory under a fixed base and refuses to
# run anywhere else; HOME is redirected into it so no global config or cache of the machine is touched.
BASE=/tmp/claude-0/-home-user/8f6b0721-ceea-5423-b931-8e7c87f0af72/scratchpad/015
scratch() { # <name> : sets R to $BASE/<name>, creates it empty, and cd's into it
  case "$1" in ''|*/*|.*) echo "bad scratch name" >&2; exit 2;; esac
  R="$BASE/$1"
  case "$R" in "$BASE"/?*) ;; *) echo "refusing: scratch outside $BASE" >&2; exit 2;; esac
  [ -d "$R" ] && { rm -r -- "$R" || exit 2; }
  mkdir -p "$R" && cd "$R" || exit 2
  [ "$PWD" = "$R" ] || exit 2
  export HOME="$R/home"; mkdir -p "$HOME"
  export GIT_CONFIG_NOSYSTEM=1 GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t GIT_TERMINAL_PROMPT=0
}
# every git call has a timeout and no stdin, so nothing can wait on a terminal
g() { timeout 60 git "$@" </dev/null; }
