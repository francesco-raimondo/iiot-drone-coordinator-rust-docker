#!/bin/bash
set -e

source /opt/ros/jazzy/setup.bash

# --------------------------------------------------------------------
# 1) DDS discovery in unicast (Fast-DDS Discovery Server)
#    Every ROS2 node (including the bridge launched here) must point
#    explicitly to the server, otherwise Fast-DDS falls back to SIMPLE
#    discovery (multicast) which is unreliable on Docker bridge networks.
# --------------------------------------------------------------------
export RMW_IMPLEMENTATION=rmw_fastrtps_cpp
export ROS_DISCOVERY_SERVER="${DISCOVERY_SERVER_IP}:${DISCOVERY_SERVER_PORT}"
export ROS_SUPER_CLIENT=true   # sees the entire graph, not just the server

# --------------------------------------------------------------------
# 2) Gazebo Transport (gz-transport)
#    On the same Docker bridge network on Linux, Gazebo's multicast discovery
#    on port 11319 and default network interface works natively.
# --------------------------------------------------------------------
export GZ_IP="${GZ_OWN_IP}"
export GZ_PARTITION=swarm

echo "[gazebo] RMW=${RMW_IMPLEMENTATION} ROS_DISCOVERY_SERVER=${ROS_DISCOVERY_SERVER}"
echo "[gazebo] GZ_IP=${GZ_IP}"

echo "[gazebo] waiting for discovery-server (${DISCOVERY_SERVER_IP}:${DISCOVERY_SERVER_PORT}) to be resolvable..."
until getent hosts discovery-server > /dev/null 2>&1; do
  sleep 1
done
sleep 2

echo "[gazebo] launching Gazebo Harmonic with graphical interface..."
gz sim -r -v 3 /worlds/empty_discovery.sdf &
GZ_PID=$!

# small margin to allow gz-transport to initialize before the bridge
sleep 6

echo "[gazebo] launching /clock bridge to ROS2..."
ros2 run ros_gz_bridge parameter_bridge \
    /clock@rosgraph_msgs/msg/Clock[gz.msgs.Clock &
BRIDGE_PID=$!

wait $GZ_PID $BRIDGE_PID
