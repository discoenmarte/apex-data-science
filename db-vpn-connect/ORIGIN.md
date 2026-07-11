# Origin

This directory is vendored from the coworker's repository:

    git@github.com:borls-co/db-vpn-connect.git

It has been absorbed into the APEX DS Pipeline monorepo so the whole pipeline
is self-contained and runnable from one clone. Local modifications made for the
APEX setup:

- `client/entrypoint.sh` — added `--no-http-keepalive` (fixes FortiGate dropping
  the idle connection while waiting for the emailed 2FA code).
- `Makefile` — added `vpn-connect-auto` target (non-interactive connect helper).
- `vpn-connect-auto.exp` — expect script: VPN password via env, 2FA code via drop-file.
- `.env/.client.example` — template (real secrets live in `.client`, gitignored).

To pull upstream updates, add it back as a remote in a separate checkout; do not
re-init git inside this folder (it is part of the monorepo now).
