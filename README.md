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
  --map-control-endpoint https://control-plane.example.com \
  --jason-controller-endpoint https://control-plane.example.com \
  --access-token "$MITHRAN_TOKEN" \
  --scope audience:jason-controller

jason doctor
jason status
jason run --repo mithran-hq/demo --issue 123
jason watch run_123
jason logs run_123
jason artifacts run_123
```

The run id is the hosted controller task id returned by `jason run`.

`--map-control-endpoint` names the MAP protocol endpoint. In MAP 1.0 that
endpoint is served by `mithran-control-plane`; `mithran-map-control` is retired.
`--jason-controller-endpoint` may point at the control-plane Jason-facing
gateway rather than a direct private controller URL.

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

## Verification

Normal CI runs the public client tests:

```sh
cargo test
```

When the private controller checkout is available as `../jason`, this smoke
starts a real local `jason-controller` process and drives the public client
against it:

```sh
scripts/smoke_spawned_controller.sh
```

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
