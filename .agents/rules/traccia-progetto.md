---
trigger: always_on
---

Project title: Resilient Multi-Agent Drone Coordinator in Rust with Docker-Based Deployment
Description: The project consists in designing and implementing a fault-tolerant coordination system for a swarm of simulated drones using ROS2 and Gazebo, with the coordination logic written in Rust as an actor-based runtime. Each drone agent is modelled as an independent actor communicating via asynchronous message passing; the coordinator implements a consensus-based leader election to maintain formation even when one agent fails.
The system is packaged as a set of Docker containers — one per drone agent, one for the coordinator, one for the Gazebo simulation environment — interconnected through a virtual Docker network. Your existing familiarity with Docker and virtual network setups is the key advantage here: the deployment layer is a first-class part of the deliverable. A FastAPI backend exposes the coordinator's runtime state through a REST interface. Note: for multi-drone scenarios in Gazebo, using a lightweight world with minimal rendering is recommended to keep the simulation manageable on your machine.
The critical subsystem — the leader election and recovery protocol — is the verified component, modelled in UPPAAL or verified with Kani.
Expected deliverables: - Rust actor-based coordinator - ROS2 node wrappers for each drone agent - Gazebo simulation scenario - Docker Compose deployment - FastAPI observability backend - Formal model of the leader election with verified recovery bound - A public GitHub repository with documentation
