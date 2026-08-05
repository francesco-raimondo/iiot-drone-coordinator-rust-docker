#!/usr/bin/env python3
import os
import rclpy
from rclpy.node import Node
from std_msgs.msg import String


class DroneAgent(Node):
    """Nodo di coordinamento minimale per un drone dello swarm.

    Serve soprattutto a verificare che il discovery DDS in unicast
    funzioni: ogni agente deve comparire nel grafo ROS2 degli altri
    container (vedi log 'Nodi visibili nel grafo ROS').
    """

    def __init__(self):
        name = os.environ.get("DRONE_NAME", "drone1")
        super().__init__(f"{name}_agent")
        self.drone_name = name

        self.publisher_ = self.create_publisher(String, "/swarm/heartbeat", 10)
        self.timer = self.create_timer(2.0, self.tick)

        self.get_logger().info(
            f"{name} agent avviato. "
            f"ROS_DISCOVERY_SERVER={os.environ.get('ROS_DISCOVERY_SERVER')}"
        )

    def tick(self):
        msg = String()
        msg.data = f"{self.drone_name} online"
        self.publisher_.publish(msg)

        peers = [n for n in self.get_node_names() if n != self.get_name()]
        self.get_logger().info(f"Nodi visibili nel grafo ROS: {peers}")


def main():
    rclpy.init()
    node = DroneAgent()
    try:
        rclpy.spin(node)
    except KeyboardInterrupt:
        pass
    finally:
        node.destroy_node()
        rclpy.shutdown()


if __name__ == "__main__":
    main()
