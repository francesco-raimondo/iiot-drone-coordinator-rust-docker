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

# Detect drone ID and numeric index dynamically using Docker DNS resolution
if [ -z "$DRONE_NAME" ]; then
  MY_IP="$GZ_OWN_IP"
  INDEX=""

  # Query Docker embedded DNS for hostnames matching drone-N or implementation-drone-N
  for i in $(seq 1 50); do
    RESOLVED_IP=$(getent hosts "drone-$i" | awk '{print $1}' || true)
    if [ -z "$RESOLVED_IP" ]; then
      RESOLVED_IP=$(getent hosts "implementation-drone-$i" | awk '{print $1}' || true)
    fi

    if [ "$RESOLVED_IP" = "$MY_IP" ]; then
      INDEX="$i"
      break
    fi
  done

  if [ -n "$INDEX" ]; then
    DRONE_NAME="drone_${INDEX}"
  else
    DRONE_NAME="drone_1"
    INDEX=1
  fi
else
  # Extract numeric index from explicit DRONE_NAME and normalize to drone_<INDEX>
  INDEX=$(echo "$DRONE_NAME" | grep -oE '[0-9]+$' || echo "1")
  DRONE_NAME="drone_${INDEX}"
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

echo "[$DRONE_NAME] Launching ROS 2 bridge for /${DRONE_NAME}/cmd_vel and /${DRONE_NAME}/odometry..."
export GZ_PARTITION=swarm
ros2 run ros_gz_bridge parameter_bridge \
    "/model/${DRONE_NAME}/cmd_vel@geometry_msgs/msg/Twist@gz.msgs.Twist" \
    "/model/${DRONE_NAME}/odometry@nav_msgs/msg/Odometry[gz.msgs.Odometry" \
    --ros-args \
    -r "/model/${DRONE_NAME}/cmd_vel:=/${DRONE_NAME}/cmd_vel" \
    -r "/model/${DRONE_NAME}/odometry:=/${DRONE_NAME}/odometry" &

echo "[$DRONE_NAME] Launching ROS 2 drone agent wrapper..."
exec python3 /drone_ws/drone_agent_wrapper.py


