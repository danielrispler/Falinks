# Claude Code adapter on Linux, 2026-10-14

Run [38541283779](https://github.com/danielrispler/Falinks/actions/runs/38541283779) of `adapter-linux.yml` (#57). It ran `check-claude-adapter` with Claude Code 2.1.287 (glibc build) on GitHub's Ubuntu 24.04 runners and logged in with a `claude setup-token` subscription token.

- `claude-default.json`: the runner's default policy. It fails closed: the sandboxed probe exits with `bwrap: loopback: Failed RTM_NEWADDR: Operation not permitted`.
- `claude-userns.json`: after `sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0`. It passes every required control.

## x86_64 host

```text
Linux 6.17.0-1022-azure x86_64
PRETTY_NAME="Ubuntu 24.04.5 LTS"
lsm=lockdown,capability,landlock,yama,apparmor,ima,evm
kernel.apparmor_restrict_unprivileged_userns = 1
user.max_user_namespaces = 63838
bubblewrap 0.9.0
```

## aarch64 host

```text
Linux 6.17.0-1022-azure aarch64
PRETTY_NAME="Ubuntu 24.04.5 LTS"
lsm=lockdown,capability,landlock,yama,apparmor,ima,evm
kernel.apparmor_restrict_unprivileged_userns = 1
user.max_user_namespaces = 63642
bubblewrap 0.9.0
```
