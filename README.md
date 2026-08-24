# 🛸 IIoT Drone Swarm Coordinator with Verified Rust FSM & ROS 2

A distributed and highly resilient swarm coordination system for IIoT (*Industrial Internet of Things*) drones, built in **Rust** and **ROS 2 Jazzy**, integrated with the **Gazebo Sim** 3D simulator, and formally verified using the **Kani** model checker.

The project features a Web Observability & Control Dashboard built with **FastAPI**, enabling real-time monitoring of the drone swarm, triggering geometric flight formations, and simulating drone agent failures and repairs within a containerized **Docker Compose** environment.

---

## 📌 Table of Contents
- [What is the Project](#-what-is-the-project)
- [🛠️ Technologies Used](#️-technologies-used)
- [🏗️ System Architecture](#️-system-architecture)
  - [Dual Finite State Machines (Dual FSM)](#dual-finite-state-machines-dual-fsm)
  - [Formal Verification with Kani](#formal-verification-with-kani)
  - [Leader Election & Dynamic Formation Algorithm](#leader-election--dynamic-formation-algorithm)
  - [3D Obstacle Avoidance (APF & Layering)](#3d-obstacle-avoidance-apf--layering)
- [📋 System Requirements](#-system-requirements)
- [🚀 How to Launch the Simulation](#-how-to-launch-the-simulation)
  - [1. X11 Display Configuration (for Gazebo GUI)](#1-x11-display-configuration-for-gazebo-gui)
  - [2. Launching Docker Containers](#2-launching-docker-containers)
  - [3. Opening the Web Dashboard](#3-opening-the-web-dashboard)
- [🎮 How to Run the Simulation (Step-by-Step)](#-how-to-run-the-simulation-step-by-step)
  - [A. Triggering Line Formation](#a-triggering-line-formation)
  - [B. Simulating Drone Failure (Pause Container)](#b-simulating-drone-failure-pause-container)
  - [C. Simulating Drone Repair (Unpause Container)](#c-simulating-drone-repair-unpause-container)
- [🛑 How to Teardown & Stop Everything](#-how-to-teardown--stop-everything)

---

## 🔍 What is the Project

This project implements a full end-to-end autonomous coordination solution for industrial IIoT drone swarms. The swarm capability set includes:
1. **Dynamic Registration**: Automatic onboarding of new drones entering the ROS 2 graph.
2. **Autonomous Leader Election**: Spatial density & proximity-driven election algorithm (Bully-variant) to reduce collisions.
3. **Health Telemetry & FSM Tracking**: Heartbeat monitoring with multi-tier timeout handling (5s Suspected, 10s Failed).
4. **Complex Flight Formations**: Synchronized formation maneuvers (e.g., *Line Formation*) centered around the elected Leader.
5. **3D Flight Safety**: Collision avoidance using Artificial Potential Fields (APF) and altitude layering.
6. **Fault Tolerance**: Instant re-election of a new Leader upon failure or disconnection, real-time mid-flight formation adjustment (*Ripple Shift*), and seamless re-integration of repaired drones.

---

## 🛠️ Technologies Used

| Component | Technology | Description |
| :--- | :--- | :--- |
| **Core Coordinator** | **Rust** (`std::thread`, Serde) | Multithreaded, high-performance, resilient swarm coordinator core. |
| **Formal Verification** | **Kani Rust Model Checker** | Proof harnesses verifying safety invariants of the dual FSM logic. |
| **Middleware & Messaging** | **ROS 2 Jazzy Jalisco** | Inter-process & container communication via ROS 2 pub/sub topics. |
| **DDS Discovery** | **Fast-DDS Discovery Server** | Unicast DDS discovery (UDP/TCP port 11811) overcoming Docker multicast limits. |
| **3D Simulation** | **Gazebo Sim (Harmonic/Ignition)** | Physics-based 3D simulator rendering quadcopter drones (X3 UAV models). |
| **Drone Agent Controller** | **Python 3** (`rclpy`) | Onboard agent node with 20 Hz P-Controller & 3D APF obstacle avoidance. |
| **Observability Backend** | **Python** (**FastAPI**, Docker SDK) | REST API backend inspecting container status and executing Docker management tasks. |
| **Frontend Dashboard** | **HTML5 / CSS3 / Vanilla JS** | Real-time web user interface for swarm status tracking and interactive control. |
| **Containerization** | **Docker & Docker Compose** | Microservices architecture supporting dynamic scaling (`docker compose up --scale drone=N`). |

---

## 🏗️ System Architecture

The architecture consists of isolated microservices connected via a dedicated Docker bridge network (`swarm_net`: `172.28.0.0/16`).

```
                              ┌───────────────────────────────┐
                              │     Fast-DDS Discovery        │
                              │     Server (172.28.0.10)      │
                              └──────────────┬────────────────┘
                                             │
      ┌──────────────────────────────────────┼──────────────────────────────────────┐
      │                                      │                                      │
┌─────▼──────────────┐             ┌─────────▼──────────┐                 ┌─────────▼──────────┐
│  Rust Coordinator  │             │   Gazebo Sim 3D    │                 │   FastAPI Server   │
│   (172.28.0.30)    │             │   (172.28.0.20)    │                 │   (172.28.0.40)    │
│  - Dual FSM Kani   │             │  - Spawn X3 Drones │                 │  - Port 8000 Web UI│
│  - Leader Election │             │  - Gazebo Odometry │                 │  - Docker Sock API │
└─────┬──────────────┘             └─────────▲──────────┘                 └─────────▲──────────┘
      │                                      │                                      │
      │   ROS 2 Topics:                      │                                      │
      │   /swarm/register                    │ /drone_N/cmd_vel                     │ HTTP REST / Pause
      │   /swarm/heartbeat                   │ /drone_N/odometry                    │ / Unpause Drone
      │   /swarm/goto                        │                                      │
      │   /swarm/formation                   │                                      │
      └───────────────────────────┬──────────┴──────────────────────────────────────┘
                                  │
                  ┌───────────────┴───────────────┐
                  │    Drone Agent Containers     │
                  │ (drone_1, drone_2, drone_N)   │
                  └───────────────────────────────┘
```

### Dual Finite State Machines (Dual FSM)

Every drone in the swarm is represented by a dual-state tuple `(S1, S2)` maintained by the Rust coordinator:

- **FSM 1 (Health State - S1)**:
  - `Unregistered`: Drone newly spawned or initializing.
  - `Active`: Operational drone emitting periodic heartbeats.
  - `Suspected`: No heartbeat received for over **5 seconds**.
  - `Failed`: No heartbeat received for over **10 seconds** (drone considered crashed/offline).
- **FSM 2 (Swarm Role - S2)**:
  - `None`: No role assigned (when S1 is `Unregistered` or `Failed`).
  - `Candidate`: Eligible to become Leader during an election process.
  - `Follower`: Gregarious drone following the Leader's trajectory and formation directives.
  - `Leader`: Active leader drone governing the swarm.

### Formal Verification with Kani

Located in the `Kani_verify/` directory, proof harnesses written for the **Kani** model checker mathematically verify the following safety invariants:
1. **Inactive Role Invariant**: An `Unregistered` or `Failed` drone must ALWAYS have S2 = `None`.
2. **Suspected Role Preservation**: Entering `Suspected` or recovering back to `Active` preserves the S2 role unchanged.
3. **Single Leader & Recovery Invariant**: In any valid swarm state, **at most one active Leader** exists. Upon Leader failure, re-election guarantees a single new Leader without violating the invariant when repaired drones rejoin.

### Leader Election & Dynamic Formation Algorithm

- **Density & Proximity Leader Election**: When the active Leader fails, the Rust coordinator inspects the vacant $Y$ coordinate of the lost Leader and selects a replacement candidate based on wing density (left vs right side candidate count) to minimize total swarm movement.
- **Ripple Shift Re-Formation**: When a drone or Leader fails during active line formation, the swarm executes a sequential shift (*Ripple Shift*): drones on the opposite wing stay completely stationary, while drones on the affected side translate inward to fill the gap safely.

### 3D Obstacle Avoidance (APF & Layering)

- **3D Artificial Potential Fields (APF)**: Each drone agent computes a repulsive velocity vector once another drone is approaching to avoid collisions
- **Directional Altitude Layering**: During formation reorganization, drones crossing paths adjust altitude (e.g., 4.8m vs 5.6m) to prevent mid-air collisions.

---

## 📋 System Requirements

- **Operating System**: Linux (Ubuntu 22.04 / 24.04 recommended).
- **Docker**: Docker Engine 20.10+ with permissions to run without `sudo` (user added to the `docker` group).
- **Docker Compose**: Docker Compose v2 (`docker compose`).
- **X11 Display Server**: Required for rendering the Gazebo 3D GUI window (`xhost` installed).
- **Recommended Hardware**: Quad-Core CPU+, 8 GB RAM.

---

## 🚀 How to Launch the Simulation

### 1. X11 Display Configuration (for Gazebo GUI)

To allow the Gazebo Docker container to open its 3D GUI window on your Linux host display, run the following command in your terminal:

```bash
xhost +local:root
```

### 2. Launching Docker Containers

Navigate to the `Implementation/` directory and spin up the microservices, specifying the desired number of drones using `--scale drone=N` (for example, **3 drones**):

```bash
cd Implementation
docker compose up --build --scale drone=3
```

> 💡 **Note**: On first run, Docker will download base ROS 2 images and compile both the Rust coordinator and FastAPI server. Wait until all containers report healthy status in the terminal logs.

### 3. Opening the Web Dashboard

Once containers are running, open your web browser and navigate to:

```text
http://localhost:8000
```

---

## 🎮 How to Run the Simulation (Step-by-Step)

### A. Triggering Line Formation

1. Open the Web Dashboard (`http://localhost:8000`). You will see the total drone count (e.g., 3 drones: `drone_1`, `drone_2`, `drone_3`), individual status cards, and the **Active Leader** indicator (e.g., `Leader: drone_1`).
2. In the **Swarm Controls** panel, click **"Trigger Line Formation"**.
3. **Observed Behavior**:
   - The Rust coordinator assigns target coordinates (X_C, Y_C, Z=4.0m) to the Leader and symmetric 2.0m-spaced targets along the Y-axis to Follower drones.
   - In the Gazebo 3D window, drones take off from the ground and align into a line formation centered around the Leader.

---

### B. Simulating Drone Failure (Pause Container)

1. On the Web Dashboard, locate the card for the **Leader** (or any other drone).
2. Click the **"Pause"** button on the target drone's card (e.g., `drone_1`).
3. **Observed Behavior**:
   - The Docker container is paused, simulating hardware failure or communication loss.
   - **Phase 1 (5s Timeout)**: After 5s without heartbeats, the drone's S1 state transitions from `Active` to `Suspected` (highlighted in orange/yellow).
   - **Phase 2 (10s Timeout)**: After 10s, S1 transitions to `Failed` (highlighted in red) and the drone is despawned from Gazebo.
   - **Re-election & Re-formation**: If the failed drone was the Leader, the `LeaderFailedReelectionTrigger` event fires. A new Leader is elected, and remaining drones execute a *Ripple Shift* to close the formation gap without collisions.

---

### C. Simulating Drone Repair (Unpause Container)

1. On the Web Dashboard, click **"Unpause"** next to the paused drone (e.g., `drone_1`).
2. **Observed Behavior**:
   - The drone container resumes operation.
   - The drone agent sends a registration request to the coordinator.
   - The coordinator triggers Event 6 (`RepairCompleted`): S1 transitions `Failed` to `Unregistered` to turn `Active` as a `Follower`.
   - **Automatic Re-integration**: The swarm detects the recovered drone and re-integrates it into the active formation, assigning it an outer slot.

---

## 🛑 How to Teardown & Stop Everything

To stop all services, clean up Docker containers, networks, and volumes:

1. Press `Ctrl + C` in the terminal where Docker Compose is running.
2. Execute the teardown command inside `Implementation/`:

```bash
docker compose down
```

3. (Optional) To clean up named volumes (e.g., generated SDF model files):

```bash
docker compose down -v
```

4. Restore standard X11 display security rules on your host machine:

```bash
xhost -local:root
```

---

*Developed for Industrial IoT & Autonomous Systems Architecture.*
