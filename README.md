# Resilient Multi-Agent Drone Coordinator

Fault-tolerant coordination system for a swarm of simulated drones using ROS 2 and Gazebo,
with coordination logic written in Rust and Docker-based deployment.

---

## Prerequisites

| Tool | Version | Notes |
|------|---------|-------|
| Docker | ≥ 24.x | [Install](https://docs.docker.com/engine/install/) |
| Docker Compose | ≥ 2.x | Included with Docker Desktop; standalone: `apt install docker-compose-plugin` |
| `xhost` | any | Required to forward the Gazebo GUI to the host display (`apt install x11-xserver-utils`) |
| Rust + Kani | nightly | Only needed to run formal verification proofs locally (see [Kani Verification](#kani-verification)) |

---

## Running the System

### 1. Allow Docker containers to access the host display (Gazebo GUI)

```bash
xhost +local:root
```

> Run this once per session, before starting the containers.

### 2. Start the swarm

From the `Implementation/` directory:

```bash
cd Implementation/

# Start with 3 drone agents (recommended)
docker compose up --scale drone=3

# Or build images first if running for the first time
docker compose up --build --scale drone=3
```

The following containers will start:

| Container | Role | Address |
|-----------|------|---------|
| `discovery-server` | Fast-DDS unicast discovery for ROS 2 | `172.28.0.10` |
| `gazebo` | Gazebo Harmonic simulation | `172.28.0.20` |
| `coordinator` | Rust swarm coordinator | `172.28.0.30` |
| `drone` (×N) | ROS 2 drone agent nodes | dynamic |
| `server` | FastAPI observability backend | `172.28.0.40:8000` |

The observability dashboard is available at **http://localhost:8000** once all containers are up.

### 3. Stop the swarm

```bash
docker compose down
```

To also remove built images:

```bash
docker compose down --rmi all --volumes
```

---

## Kani Verification

The formal model of the leader election FSM is verified with [Kani](https://model-checking.github.io/kani/).

### Install Kani (one-time)

```bash
cargo install --locked kani-verifier
cargo kani setup
```

### Run the proof harnesses

```bash
cd Kani_verify/
cargo kani
```

Three proof harnesses will be verified:

| Harness | Property |
|---------|----------|
| `proof_inactive_role_is_none` | `Unregistered`/`Failed` drones always have role `None` |
| `proof_suspected_role_preservation` | Role (FSM2) is preserved across suspicion and recovery |
| `proof_swarm_single_leader_and_recovery` | Single-leader invariant holds before and after leader failure + re-election |
