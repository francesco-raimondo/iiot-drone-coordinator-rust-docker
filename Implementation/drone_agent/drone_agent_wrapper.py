#!/usr/bin/env python3
import json
import math
import os
import time
import rclpy
from rclpy.node import Node
from rclpy.qos import QoSProfile, ReliabilityPolicy, HistoryPolicy
from geometry_msgs.msg import Twist, Point
from nav_msgs.msg import Odometry
from std_msgs.msg import String


class DroneAgentWrapper(Node):
    """ROS 2 Node Wrapper for an individual drone agent.

    This node represents the drone inside the ROS 2 graph.
    It manages registration, heartbeat telemetry, position tracking via odometry,
    and a P-controller velocity loop to navigate to target coordinates (x, y, z)
    and hover in place upon arrival.
    """

    def __init__(self):
        drone_id = os.environ.get("DRONE_NAME", "drone_1")
        super().__init__(f"{drone_id}_agent")

        self.drone_id = drone_id
        self.spawn_x = float(os.environ.get("SPAWN_X", "0.0"))
        self.spawn_y = float(os.environ.get("SPAWN_Y", "0.0"))
        self.spawn_z = float(os.environ.get("SPAWN_Z", "0.3"))

        # Target position initialized to ground spawn coordinates
        self.target_x = self.spawn_x
        self.target_y = self.spawn_y
        self.target_z = self.spawn_z

        # Current estimated position from Gazebo odometry
        self.current_x = self.spawn_x
        self.current_y = self.spawn_y
        self.current_z = self.spawn_z
        self.has_odometry = False
        self.target_reached_logged = False

        # Controller parameters
        self.kp_linear = 1.2
        self.max_linear_vel = 2.0  # m/s
        self.position_tolerance = 0.08  # meters

        qos_profile = QoSProfile(
            reliability=ReliabilityPolicy.RELIABLE,
            history=HistoryPolicy.KEEP_LAST,
            depth=10
        )

        # Publishers
        self.registration_pub = self.create_publisher(String, "/swarm/register", qos_profile)
        self.heartbeat_pub = self.create_publisher(String, "/swarm/heartbeat", qos_profile)
        self.cmd_vel_pub = self.create_publisher(Twist, f"/{self.drone_id}/cmd_vel", 10)

        # Subscribers
        self.odom_sub = self.create_subscription(
            Odometry,
            f"/{self.drone_id}/odometry",
            self.odometry_callback,
            10
        )
        self.goto_sub = self.create_subscription(
            Point,
            f"/{self.drone_id}/goto",
            self.goto_callback,
            10
        )
        self.swarm_goto_sub = self.create_subscription(
            String,
            "/swarm/goto",
            self.swarm_goto_callback,
            qos_profile
        )

        # Timers
        self.registration_timer = self.create_timer(2.0, self.publish_registration)
        self.heartbeat_timer = self.create_timer(1.0, self.publish_heartbeat)
        self.control_timer = self.create_timer(0.05, self.control_loop)  # 20 Hz control loop

        self.get_logger().info(
            f"[{self.drone_id}] Drone ROS 2 Agent Wrapper started. "
            f"Initial spawn/target position: ({self.target_x}, {self.target_y}, {self.target_z})"
        )

    def odometry_callback(self, msg: Odometry):
        """Updates current position from ground-truth Gazebo odometry."""
        self.current_x = msg.pose.pose.position.x
        self.current_y = msg.pose.pose.position.y
        self.current_z = msg.pose.pose.position.z
        self.has_odometry = True

    def goto_callback(self, msg: Point):
        """Sets new 3D target coordinates received on drone-specific topic."""
        self.set_target(msg.x, msg.y, msg.z)

    def swarm_goto_callback(self, msg: String):
        """Processes global swarm target position command JSON payload."""
        try:
            payload = json.loads(msg.data)
            if payload.get("drone_id") == self.drone_id:
                x = float(payload.get("x", self.target_x))
                y = float(payload.get("y", self.target_y))
                z = float(payload.get("z", self.target_z))
                self.set_target(x, y, z)
        except Exception as e:
            self.get_logger().error(f"[{self.drone_id}] Failed to parse swarm goto payload: {e}")

    def set_target(self, x: float, y: float, z: float):
        """Updates target position and resets target reached flag."""
        self.target_x = x
        self.target_y = y
        self.target_z = z
        self.target_reached_logged = False
        self.get_logger().info(
            f"[{self.drone_id}] NEW TARGET RECEIVED: ({self.target_x:.2f}, {self.target_y:.2f}, {self.target_z:.2f})"
        )

    def control_loop(self):
        """P-Controller loop executed at 20 Hz.

        Calculates position error vector towards target position. If distance exceeds tolerance,
        publishes linear velocity commands. Once within tolerance, publishes zero velocity
        to hover steadily at target coordinates.
        """
        if not self.has_odometry:
            return

        dx = self.target_x - self.current_x
        dy = self.target_y - self.current_y
        dz = self.target_z - self.current_z
        distance = math.sqrt(dx * dx + dy * dy + dz * dz)

        cmd = Twist()

        if distance > self.position_tolerance:
            # Proportional velocity calculation
            vx = self.kp_linear * dx
            vy = self.kp_linear * dy
            vz = self.kp_linear * dz

            # Clamp velocities to maximum allowable velocity limit
            speed = math.sqrt(vx * vx + vy * vy + vz * vz)
            if speed > self.max_linear_vel:
                scale = self.max_linear_vel / speed
                vx *= scale
                vy *= scale
                vz *= scale

            cmd.linear.x = vx
            cmd.linear.y = vy
            cmd.linear.z = vz
            self.cmd_vel_pub.publish(cmd)
        else:
            # Target reached: publish 0 velocity to maintain hover position
            cmd.linear.x = 0.0
            cmd.linear.y = 0.0
            cmd.linear.z = 0.0
            self.cmd_vel_pub.publish(cmd)

            if not self.target_reached_logged:
                self.get_logger().info(
                    f"[{self.drone_id}] TARGET REACHED & HOVERING at ({self.current_x:.2f}, {self.current_y:.2f}, {self.current_z:.2f})"
                )
                self.target_reached_logged = True

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

    def publish_heartbeat(self):
        """Publishes heartbeat message for swarm monitoring."""
        msg = String()
        msg.data = json.dumps({
            "drone_id": self.drone_id,
            "status": "online",
            "timestamp": time.time(),
            "x": round(self.current_x, 2),
            "y": round(self.current_y, 2),
            "z": round(self.current_z, 2)
        })
        self.heartbeat_pub.publish(msg)


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
