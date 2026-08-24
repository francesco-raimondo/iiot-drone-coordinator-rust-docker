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

        # Peer drone tracking for 3D Artificial Potential Fields (APF) Obstacle Avoidance
        self.peer_positions = {}
        self.peer_subs = []
        self.fresh_target_received = False
        for i in range(1, 21):
            peer_id = f"drone_{i}"
            if peer_id != self.drone_id:
                sub = self.create_subscription(
                    Odometry,
                    f"/{peer_id}/odometry",
                    self.make_peer_callback(peer_id),
                    10
                )
                self.peer_subs.append(sub)

        # Timers
        self.registration_timer = self.create_timer(2.0, self.publish_registration)
        self.heartbeat_timer = self.create_timer(1.0, self.publish_heartbeat)
        self.control_timer = self.create_timer(0.05, self.control_loop)  # 20 Hz control loop

        self.get_logger().info(
            f"[{self.drone_id}] Drone ROS 2 Agent Wrapper started with APF 3D Obstacle Avoidance. "
            f"Initial spawn/target position: ({self.target_x}, {self.target_y}, {self.target_z})"
        )

    def make_peer_callback(self, peer_id: str):
        """Creates callback for peer drone odometry tracking."""
        return lambda msg: self.peer_odometry_callback(msg, peer_id)

    def peer_odometry_callback(self, msg: Odometry, peer_id: str):
        """Stores real-time 3D position of peer drones with timestamp for APF repulsion calculation."""
        px = msg.pose.pose.position.x
        py = msg.pose.pose.position.y
        pz = msg.pose.pose.position.z
        self.peer_positions[peer_id] = (px, py, pz, time.time())

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
                self.fresh_target_received = True
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

    def publish_registration(self):
        """Sends registration payload to the Rust coordinator and resets target if on ground."""
        payload = {
            "drone_id": self.drone_id,
            "x": self.spawn_x,
            "y": self.spawn_y,
            "z": self.spawn_z,
        }
        # If drone is at ground level, ensure target is locked to ground until coordinator assigns a new target
        if self.current_z < 1.0 and not self.fresh_target_received:
            self.target_x = self.spawn_x
            self.target_y = self.spawn_y
            self.target_z = self.spawn_z

        msg = String()
        msg.data = json.dumps(payload)
        self.registration_pub.publish(msg)

    def control_loop(self):
        """P-Controller loop with APF 3D Obstacle Avoidance executed at 20 Hz.

        Calculates position error vector towards target position and adds repulsive velocity
        vectors from neighboring drones within safety radius (APF). Once within tolerance,
        publishes zero velocity to hover steadily at target coordinates.
        """
        if not self.has_odometry:
            return

        # If drone is at ground level and hasn't received a fresh formation target from coordinator, stay at ground
        if self.current_z < 1.0 and not self.fresh_target_received:
            cmd = Twist()
            cmd.linear.x = 0.0
            cmd.linear.y = 0.0
            cmd.linear.z = 0.0
            self.cmd_vel_pub.publish(cmd)
            return

        dx = self.target_x - self.current_x
        dy = self.target_y - self.current_y
        dz = self.target_z - self.current_z
        distance = math.sqrt(dx * dx + dy * dy + dz * dz)

        # Altitude Layering check: If flying at layer height (target_z > 4.5) and reached horizontal target
        horizontal_dist = math.sqrt(dx * dx + dy * dy)
        if self.target_z > 4.5 and horizontal_dist < 0.25:
            self.target_z = 4.0
            self.target_reached_logged = False
            self.get_logger().info(
                f"[{self.drone_id}] HORIZONTAL POSITION REACHED: Descending from layer height ({self.current_z:.2f}m) to final hover altitude (4.00m)"
            )
            dz = self.target_z - self.current_z
            distance = math.sqrt(dx * dx + dy * dy + dz * dz)

        # Low-altitude departure check for ground takeoff: ascend to 4.0m once outer slot is reached
        if 1.0 < self.target_z < 2.5 and horizontal_dist < 0.30:
            self.target_z = 4.0
            self.target_reached_logged = False
            self.get_logger().info(
                f"[{self.drone_id}] OUTER SLOT REACHED: Ascending from low transit altitude ({self.current_z:.2f}m) to final hover altitude (4.00m)"
            )
            dz = self.target_z - self.current_z
            distance = math.sqrt(dx * dx + dy * dy + dz * dz)

        # Vertical-First Transit check: If target_z is layer height (>4.5m) and current_z is below layer height,
        # ascend vertically FIRST before applying horizontal velocity!
        vertical_first = False
        if self.target_z > 4.5 and self.current_z < (self.target_z - 0.25):
            vertical_first = True

        cmd = Twist()

        if distance > self.position_tolerance:
            if vertical_first:
                # Ascend vertically first: zero out horizontal attractive velocities until layer height is reached
                vx_att = 0.0
                vy_att = 0.0
                vz_att = self.kp_linear * dz
            else:
                # 1. Attractive velocity vector towards target position
                vx_att = self.kp_linear * dx
                vy_att = self.kp_linear * dy
                vz_att = self.kp_linear * dz

            # 2. Artificial Potential Fields (APF) 3D Repulsive force from neighboring drones
            vx_rep = 0.0
            vy_rep = 0.0
            vz_rep = 0.0

            safety_radius = 1.2  # meters
            k_rep = 1.8          # Repulsive force gain
            now = time.time()

            for peer_id, (px, py, pz, last_seen) in self.peer_positions.items():
                # Ignore stale odometry (>2.0s old) from paused/failed drones
                if now - last_seen > 2.0:
                    continue

                p_dx = self.current_x - px
                p_dy = self.current_y - py
                p_dz = self.current_z - pz
                dist_xy = math.sqrt(p_dx * p_dx + p_dy * p_dy)
                dist = math.sqrt(p_dx * p_dx + p_dy * p_dy + p_dz * p_dz)

                if 0.05 < dist < safety_radius:
                    # F_rep = k_rep * (1/dist - 1/safety_radius) / (dist^2)
                    f_rep = k_rep * ((1.0 / dist) - (1.0 / safety_radius)) / (dist * dist)
                    # Unit direction vector pointing away from peer
                    ux = p_dx / dist
                    uy = p_dy / dist
                    uz = p_dz / dist

                    vx_rep += f_rep * ux
                    vy_rep += f_rep * uy

                    # Tangential local minima avoidance: If peer is directly ahead/behind along X line (|ux| < 0.3)
                    # add a tangential push in X to curve around the blocking drone in 3D space
                    if abs(ux) < 0.3 and abs(p_dy) > 0.1:
                        tangent_dir = 1.0 if (self.current_x - 3.0 + 0.05) >= 0 else -1.0
                        vx_rep += f_rep * 0.8 * tangent_dir

                    # Only apply vertical repulsion if drones are horizontally very close (<0.35m)
                    # to avoid pushing lower-layer drones downward during layered flight
                    if dist_xy < 0.35:
                        vz_rep += f_rep * uz

            # Resultant velocity vector = Attractive + Repulsive
            vx = vx_att + vx_rep
            vy = vy_att + vy_rep
            vz = vz_att + vz_rep

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
    except (KeyboardInterrupt, rclpy.executors.ExternalShutdownException):
        pass
    finally:
        node.destroy_node()
        if rclpy.ok():
            rclpy.shutdown()


if __name__ == "__main__":
    main()
