#!/usr/bin/env python3
import json
import os
import time
import rclpy
from rclpy.node import Node
from rclpy.qos import QoSProfile, ReliabilityPolicy, HistoryPolicy
from std_msgs.msg import String


class DroneAgentWrapper(Node):
    """ROS 2 Node Wrapper for an individual drone agent.

    This node represents the drone inside the ROS 2 graph.
    Upon startup, it broadcasts a registration request containing its ID
    and spawn target coordinates on the `/swarm/register` topic.
    It also publishes periodic heartbeat messages on `/swarm/heartbeat`.
    """

    def __init__(self):
        drone_id = os.environ.get("DRONE_NAME", "drone1")
        super().__init__(f"{drone_id}_agent")

        self.drone_id = drone_id
        self.spawn_x = float(os.environ.get("SPAWN_X", "0.0"))
        self.spawn_y = float(os.environ.get("SPAWN_Y", "0.0"))
        self.spawn_z = float(os.environ.get("SPAWN_Z", "0.3"))

        qos_profile = QoSProfile(
            reliability=ReliabilityPolicy.RELIABLE,
            history=HistoryPolicy.KEEP_LAST,
            depth=10
        )

        # Publishers
        self.registration_pub = self.create_publisher(String, "/swarm/register", qos_profile)
        self.heartbeat_pub = self.create_publisher(String, "/swarm/heartbeat", qos_profile)

        # Timers
        self.registration_timer = self.create_timer(2.0, self.publish_registration)
        self.heartbeat_timer = self.create_timer(1.0, self.publish_heartbeat)

        self.get_logger().info(
            f"[{self.drone_id}] Drone ROS 2 Agent Wrapper started. "
            f"Target position: ({self.spawn_x}, {self.spawn_y}, {self.spawn_z})"
        )

    def publish_registration(self):
        """Sends registration payload to the Rust coordinator."""
        payload = {
            "drone_id": self.drone_id,
            "x": self.spawn_x,
            "y": self.spawn_y,
            "z": self.spawn_z,
        }
        msg = String()
        msg.data = json.dumps(payload)
        self.registration_pub.publish(msg)
        self.get_logger().info(
            f"[{self.drone_id}] Sent registration request to coordinator: {msg.data}"
        )

    def publish_heartbeat(self):
        """Publishes heartbeat message for swarm monitoring."""
        msg = String()
        msg.data = json.dumps({"drone_id": self.drone_id, "status": "online", "timestamp": time.time()})
        self.heartbeat_pub.publish(msg)
        self.get_logger().info(
            f"[{self.drone_id}] Heartbeat published: {msg.data}"
        )


def main():
    rclpy.init()
    node = DroneAgentWrapper()
    try:
        rclpy.spin(node)
    except KeyboardInterrupt:
        pass
    finally:
        node.destroy_node()
        rclpy.shutdown()


if __name__ == "__main__":
    main()
