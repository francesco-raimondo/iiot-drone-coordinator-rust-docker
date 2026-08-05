#!/bin/bash
set -e

source /opt/ros/jazzy/setup.bash

# --- stessa configurazione DDS/GZ del container gazebo, vedi commenti lì ---
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

echo "[$DRONE_NAME] attendo che il container gazebo sia risolvibile in rete..."
until getent hosts gazebo > /dev/null 2>&1; do
  sleep 1
done

# margine per dare tempo a `gz sim` di aprire il servizio /world/<world>/create
echo "[$DRONE_NAME] attendo avvio del world server..."
sleep 10

echo "[$DRONE_NAME] spawn modello X3 UAV in (${SPAWN_X}, ${SPAWN_Y}, ${SPAWN_Z})..."
ros2 run ros_gz_sim create \
    -world "${WORLD_NAME}" \
    -name "${DRONE_NAME}" \
    -x "${SPAWN_X}" -y "${SPAWN_Y}" -z "${SPAWN_Z}" \
    -file "https://fuel.gazebosim.org/1.0/OpenRobotics/models/X3 UAV" \
    || echo "[$DRONE_NAME] ATTENZIONE: spawn fallito, vedi note su cache Fuel/rete"

echo "[$DRONE_NAME] avvio nodo di coordinamento..."
exec python3 /drone_ws/drone_node.py
