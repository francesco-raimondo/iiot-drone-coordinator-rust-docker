#!/bin/bash
set -e

source /opt/ros/jazzy/setup.bash

export RMW_IMPLEMENTATION=rmw_fastrtps_cpp
export ROS_DISCOVERY_SERVER="${DISCOVERY_SERVER_IP}:${DISCOVERY_SERVER_PORT}"
export ROS_SUPER_CLIENT=true

export GZ_IP="${GZ_OWN_IP}"
export GZ_PARTITION=swarm

echo "[coordinator] RMW=${RMW_IMPLEMENTATION} ROS_DISCOVERY_SERVER=${ROS_DISCOVERY_SERVER}"

echo "[coordinator] Waiting for discovery-server..."
until getent hosts discovery-server > /dev/null 2>&1; do
  sleep 1
done

echo "[coordinator] Waiting for gazebo service..."
until getent hosts gazebo > /dev/null 2>&1; do
  sleep 1
done

# Sleep to ensure Gazebo user command system plugin is ready to accept create requests
sleep 8

echo "[coordinator] Starting Rust Coordinator binary..."
exec /coordinator/target/release/drone_coordinator
