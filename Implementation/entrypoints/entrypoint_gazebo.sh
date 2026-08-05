#!/bin/bash
set -e

source /opt/ros/jazzy/setup.bash

# --------------------------------------------------------------------
# 1) DDS discovery in unicast (Fast-DDS Discovery Server)
#    Ogni nodo ROS2 (compreso il bridge lanciato qui) deve puntare
#    esplicitamente al server, altrimenti Fast-DDS ricade sul discovery
#    SIMPLE (multicast) che su rete bridge Docker è inaffidabile.
# --------------------------------------------------------------------
export RMW_IMPLEMENTATION=rmw_fastrtps_cpp
export ROS_DISCOVERY_SERVER="${DISCOVERY_SERVER_IP}:${DISCOVERY_SERVER_PORT}"
export ROS_SUPER_CLIENT=true   # vede l'intero grafo, non solo il server

# --------------------------------------------------------------------
# 2) Gazebo Transport (gz-transport)
#    Su una stessa rete bridge Docker Linux, il discovery multicast di Gazebo
#    sulla porta 11319 ed interfaccia di rete di default funziona nativamente.
# --------------------------------------------------------------------
export GZ_IP="${GZ_OWN_IP}"
export GZ_PARTITION=swarm

echo "[gazebo] RMW=${RMW_IMPLEMENTATION} ROS_DISCOVERY_SERVER=${ROS_DISCOVERY_SERVER}"
echo "[gazebo] GZ_IP=${GZ_IP}"

echo "[gazebo] attendo che discovery-server (${DISCOVERY_SERVER_IP}:${DISCOVERY_SERVER_PORT}) sia risolvibile..."
until getent hosts discovery-server > /dev/null 2>&1; do
  sleep 1
done
sleep 2

echo "[gazebo] avvio Gazebo Harmonic con interfaccia grafica..."
gz sim -r -v 3 /worlds/empty_discovery.sdf &
GZ_PID=$!

# piccolo margine per lasciare inizializzare gz-transport prima del bridge
sleep 6

echo "[gazebo] avvio bridge /clock verso ROS2..."
ros2 run ros_gz_bridge parameter_bridge \
    /clock@rosgraph_msgs/msg/Clock[gz.msgs.Clock &
BRIDGE_PID=$!

wait $GZ_PID $BRIDGE_PID
