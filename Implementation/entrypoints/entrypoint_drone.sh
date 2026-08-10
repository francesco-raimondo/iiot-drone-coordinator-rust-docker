#!/bin/bash
set -e

source /opt/ros/jazzy/setup.bash

# DDS discovery settings
export RMW_IMPLEMENTATION=rmw_fastrtps_cpp
export ROS_DISCOVERY_SERVER="${DISCOVERY_SERVER_IP}:${DISCOVERY_SERVER_PORT}"
export ROS_SUPER_CLIENT=true

# Detect container IP address dynamically if not set
if [ -z "$GZ_OWN_IP" ]; then
  GZ_OWN_IP=$(hostname -I | awk '{print $1}')
fi

# Detect drone ID and numeric index dynamically
if [ -z "$DRONE_NAME" ]; then
  CONTAINER_HOST=$(hostname)
  ALIAS_NAME=$(grep -E 'drone[-_]?[0-9]+' /etc/hosts | awk '{print $2}' | head -n 1 || true)
  if [ -n "$ALIAS_NAME" ]; then
    INDEX=$(echo "$ALIAS_NAME" | grep -oE '[0-9]+$' || true)
  fi

  if [ -z "$INDEX" ]; then
    INDEX=$(echo "$CONTAINER_HOST" | grep -oE '[0-9]+$' || true)
  fi

  if [ -n "$INDEX" ]; then
    DRONE_NAME="drone${INDEX}"
  else
    LAST_OCTET=$(echo "$GZ_OWN_IP" | awk -F'.' '{print $NF}')
    INDEX="$LAST_OCTET"
    DRONE_NAME="drone_${CONTAINER_HOST}"
  fi
else
  INDEX=$(echo "$DRONE_NAME" | grep -oE '[0-9]+$' || echo "1")
fi

# Calculate spawn X coordinate dynamically if not set explicitly (spacing 1.5 meters)
if [ -z "$SPAWN_X" ]; then
  OFFSET=$(( INDEX > 0 ? INDEX - 1 : 0 ))
  SPAWN_X=$(python3 -c "print(float($OFFSET * 1.5))")
fi

SPAWN_Y="${SPAWN_Y:-0.0}"
SPAWN_Z="${SPAWN_Z:-0.3}"
WORLD_NAME="${WORLD_NAME:-empty}"

export DRONE_NAME
export SPAWN_X
export SPAWN_Y
export SPAWN_Z
export GZ_OWN_IP
export GZ_IP="${GZ_OWN_IP}"
export GZ_PARTITION=swarm

echo "[$DRONE_NAME] RMW=${RMW_IMPLEMENTATION} ROS_DISCOVERY_SERVER=${ROS_DISCOVERY_SERVER}"
echo "[$DRONE_NAME] GZ_IP=${GZ_IP} Target Spawn: (${SPAWN_X}, ${SPAWN_Y}, ${SPAWN_Z})"

echo "[$DRONE_NAME] Waiting for discovery server and coordinator..."
until getent hosts discovery-server > /dev/null 2>&1; do
  sleep 1
done

echo "[$DRONE_NAME] Launching ROS 2 drone agent wrapper..."
exec python3 /drone_ws/drone_agent_wrapper.py

