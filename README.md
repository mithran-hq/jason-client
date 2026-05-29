# Jason Client

`jason` is the hosted Jason client distributed by `Aegis.pkg` for MAP 1.0.
It talks to the hosted Jason controller after the user signs in through MAP or
Aegis.

This repository contains the public client only. The scheduler, controller,
runtime-control integration, worker provisioning, and credential materialization
remain private MAP infrastructure.

## Quick Start

```sh
map login save \
  --map-control-endpoint https://map.example.com \
  --jason-controller-endpoint https://jason.example.com \
  --access-token "$MITHRAN_TOKEN" \
  --scope audience:jason-controller

jason doctor
jason status
jason run --repo mithran-hq/demo --issue 123
jason watch run_123
jason logs run_123
jason artifacts run_123
```

## Commands

The public client uses one user-facing concept: a run.

```text
jason run --repo <owner/repo> --issue <number-or-url>
jason run --repo <owner/repo> --prompt <text>
jason status [run-id]
jason watch <run-id>
jason logs <run-id>
jason artifacts <run-id>
jason cancel <run-id>
jason doctor
jason version
```

`status` without a run id reports hosted controller status. `status <run-id>`
reports one run.

## Boundary

Public:

- hosted controller client;
- login-state discovery;
- run/status/log/artifact inspection.

Private:

- Jason controller;
- runtime-control;
- Firecracker workers;
- worker credentials.
