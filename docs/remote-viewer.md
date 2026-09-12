# Remote sessions — compute there, plots here

Run `rustlab` on a remote machine and have its plots open in a
`rustlab-viewer` window on your own desktop. Useful when the data lives on a
big box and the screen doesn't.

Works on Linux, macOS and WSL. All you need is SSH.

## Quickstart

Two commands, both on your **local** machine:

```sh
rustlab-viewer &            # opens the (empty) viewer window
rustlab remote user@host    # drops you into a remote REPL
```

```
viewer: forwarding /tmp/rustlab-viewer-501.sock → user@host:/tmp/rustlab-fwd-1001.sock
rustlab 0.3.7 — type 'help' or '?' for help, 'exit' or Ctrl+D to quit
>> A = randn(2000, 2000);      % computed on the remote box
>> plot(svd(A))                % drawn on your desktop
```

The REPL arrives already connected, so the first plot just appears. `Ctrl+D`
ends the session and tears the forward down.

To run a script instead of the REPL:

```sh
rustlab remote user@host --command "rustlab run /data/sim.rlab --plot viewer"
```

## Why a *remote* forward

The viewer is the **server**: `rustlab-viewer` binds a Unix socket and
`rustlab` connects to it. When the compute is remote, the socket therefore has
to travel backwards down the SSH connection — a remote forward (`ssh -R`), not
the local forward (`ssh -L`) most people reach for first.

```
local:  rustlab-viewer ──binds──> /tmp/rustlab-viewer-501.sock
                                        ▲
                                   ssh -R tunnel
                                        │
remote: rustlab ──connects──> /tmp/rustlab-fwd-1001.sock
                              (RUSTLAB_VIEWER_SOCK)
```

`RUSTLAB_VIEWER_SOCK` is the hook that makes it work: it tells `rustlab` to
connect somewhere other than the default `/tmp/rustlab-viewer-<uid>.sock`.

## Doing it by hand

`rustlab remote` is a convenience wrapper around plain SSH. Anything it does,
you can do yourself — useful when you want the forward inside an existing
session, or your setup needs options the wrapper doesn't expose.

Ask it to show its work:

```sh
rustlab remote user@host --remote-socket /tmp/rustlab-fwd.sock --print
```

```sh
ssh -t -o ExitOnForwardFailure=yes -R /tmp/rustlab-fwd.sock:/tmp/rustlab-viewer-501.sock \
    user@host 'RUSTLAB_VIEWER_SOCK='\''/tmp/rustlab-fwd.sock'\'' exec rustlab repl --viewer'
```

Three details in there matter:

- **`-R remote:local`** — the remote path comes first. Reversing it silently
  forwards the wrong way.
- **`ExitOnForwardFailure=yes`** — without it a refused forward is *silent*,
  and you find out much later via a puzzling "could not connect".
- **The variable is set inline**, not with `SetEnv`. `SetEnv` needs
  `AcceptEnv RUSTLAB_VIEWER_SOCK` in the remote `sshd_config`, which you
  probably don't control.

### As an `~/.ssh/config` entry

```
Host gpu-box
    HostName gpu-box.example.com
    User me
    RemoteForward /tmp/rustlab-fwd.sock /tmp/rustlab-viewer-501.sock
    ExitOnForwardFailure yes
```

Then `ssh gpu-box` sets the forward up every time. The environment variable
still has to reach the remote `rustlab` — the reliable way is your remote shell
rc, since `SetEnv` depends on the server's `AcceptEnv`:

```sh
# remote ~/.bashrc or ~/.zshrc
export RUSTLAB_VIEWER_SOCK=/tmp/rustlab-fwd.sock
```

After that, `viewer on` inside a remote REPL connects to your local viewer.

Note the hardcoded `501` — that's *your* local uid (`id -u`). The wrapper looks
this up for you; a config file can't.

## Platform notes

### Linux

Some servers disable Unix-socket forwarding. If the connection dies with
`ExitOnForwardFailure`, ask your admin about these `sshd_config` settings:

```
AllowStreamLocalForwarding yes
StreamLocalBindUnlink yes      # lets sshd replace a leftover socket
```

`StreamLocalBindUnlink` is a nicety — `rustlab remote` clears a stale socket
before connecting, so it works without it.

### macOS

Nothing special, but socket paths are limited to about 104 bytes
(`sockaddr_un`). Keep both paths short and in `/tmp`; a path under a deeply
nested directory will fail to bind. `rustlab remote` checks this and tells you
before SSH gets involved.

### WSL

Run **both** the viewer and `rustlab remote` inside WSL, from the same WSL
shell. The socket lives in the WSL filesystem, and Windows' own `ssh.exe`
cannot reach it — using it is the usual reason a WSL setup won't connect.

The GUI comes from WSLg (Windows 11, or Windows 10 with the WSLg backport). On
the very first launch after boot the display server may not be ready; the
viewer detects this and retries once by itself.

If you are on Windows *without* WSL, the viewer listens on TCP `127.0.0.1:19847`
instead of a Unix socket, so the forward is an ordinary port forward and
`RUSTLAB_VIEWER_SOCK` does not apply:

```sh
ssh -R 19847:localhost:19847 user@host
```

## Troubleshooting

| Symptom | Cause and fix |
|---|---|
| `no viewer listening on /tmp/...` | The local viewer isn't running. `rustlab-viewer &` first. |
| `viewer: not available in this build` | The **remote** `rustlab` was built without the viewer feature. Build it there with `make install`, or `cargo install --path crates/rustlab-cli --features viewer` — a plain `cargo install` leaves it out. |
| `viewer: could not connect — is rustlab-viewer running?` | The message names the socket it tried. If that path looks wrong, `RUSTLAB_VIEWER_SOCK` isn't set (or is stale) on the remote. |
| Connection closes immediately with a forwarding error | A leftover socket on the remote, or the server forbids Unix forwards. `ssh user@host rm -f /tmp/rustlab-fwd-*.sock`, then see the Linux notes above. |
| `viewer on <name>` won't connect over a forward | **Named sessions ignore `RUSTLAB_VIEWER_SOCK`** — the path is derived from the uid and name, so it resolves on the *remote* box and finds nothing. Use plain `viewer on` for forwarded sessions. |
| `path must be shorter than SUN_LEN` | The socket path exceeds the ~104-byte limit. Use a short `/tmp` path. |
| Plots go to the terminal instead | The session isn't connected. `viewer` (bare) reports the connection state and where figures are currently routed. |

## `rustlab remote` options

| Option | Effect |
|---|---|
| `--remote-socket PATH` | Socket to create on the remote. Default `/tmp/rustlab-fwd-<remote-uid>.sock`, keyed on the remote user so two people on one box don't collide. |
| `--command CMD` | What to run remotely. Default `rustlab repl --viewer`. |
| `--ssh-opt OPT` | Passed verbatim to ssh, repeatable: `--ssh-opt -p --ssh-opt 2222`. For anything involved, an `~/.ssh/config` entry is easier. |
| `--print` | Print the ssh command instead of running it. |
| `--no-check` | Skip the "is a viewer listening?" check. |

## See also

- `rustlab repl --viewer` — connect at REPL startup on any machine, not just
  remote ones.
- `rustlab run <script> --plot viewer` — same for scripts.
- [`docs/functions.md`](functions.md) — the `viewer` / `viewer on` / `viewer off`
  reference.
