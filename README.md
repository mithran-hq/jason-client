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
jason task list
```

## Boundary

Public:

- hosted controller client;
- login-state discovery;
- task/session/status inspection.

Private:

- Jason controller;
- runtime-control;
- Firecracker workers;
- worker credentials.
