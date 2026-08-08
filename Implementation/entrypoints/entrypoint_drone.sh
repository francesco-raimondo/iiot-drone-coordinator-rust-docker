#!/bin/bash
set -e

source /opt/ros/jazzy/setup.bash

# DDS and Gazebo discovery settings
export RMW_IMPLEMENTATION=rmw_fastrtps_cpp
export ROS_DISCOVERY_SERVER="${DISCOVERY_SERVER_IP}:${DISCOVERY_SERVER_PORT}"
export ROS_SUPER_CLIENT=true

export GZ_IP="${GZ_OWN_IP}"
export GZ_PARTITION=swarm

DRONE_NAME="${DRONE_NAME:-drone1}"
SPAWN_X="${SPAWN_X:-0}"
SPAWN_Y="${SPAWN_Y:-0}"
SPAWN_Z="${SPAWN_Z:-0.3}"
WORLD_NAME="${WORLD_NAME:-empty}"

echo "[$DRONE_NAME] RMW=${RMW_IMPLEMENTATION} ROS_DISCOVERY_SERVER=${ROS_DISCOVERY_SERVER}"
echo "[$DRONE_NAME] GZ_IP=${GZ_IP}"

echo "[$DRONE_NAME] Waiting for discovery server and coordinator..."
until getent hosts discovery-server > /dev/null 2>&1; do
  sleep 1
done

echo "[$DRONE_NAME] Launching ROS 2 drone agent wrapper..."
exec python3 /drone_ws/drone_agent_wrapper.py
