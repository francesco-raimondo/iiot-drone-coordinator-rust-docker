#!/bin/bash
set -e

# Source ROS2 setup
if [ -f "/opt/ros/jazzy/setup.bash" ]; then
    source /opt/ros/jazzy/setup.bash
fi

# Define the shared locks directory
LOCKS_DIR="/shared/locks"
mkdir -p "$LOCKS_DIR"

# Loop to find the first available sequential drone ID
DRONE_ID=""
for i in {1..100}; do
    # Try to atomically create a directory for this drone ID.
    # mkdir is atomic in Linux, so only one container can succeed for a given i.
    if mkdir "$LOCKS_DIR/drone_$i" 2>/dev/null; then
        DRONE_ID=$i
        export ROS_NAMESPACE="drone_$i"
        # Write our hostname (container ID) to the lock directory for visibility
        echo "$HOSTNAME" > "$LOCKS_DIR/drone_$i/container_id"
        break
    fi
done

if [ -z "$DRONE_ID" ]; then
    echo "Error: Could not allocate a unique drone ID (1-100)."
    exit 1
fi

echo "Container ID (hostname): $HOSTNAME"
echo "Allocated Drone ID: $DRONE_ID"
echo "Setting ROS2 Namespace to: /$ROS_NAMESPACE"

# Define cleanup function to release the lock when the container stops
cleanup() {
    echo "Releasing lock for drone_$DRONE_ID..."
    rm -rf "$LOCKS_DIR/drone_$DRONE_ID"
}

# Trap termination signals to clean up the lock directory
trap cleanup EXIT INT TERM

# Execute the command passed to docker run
exec "$@"
