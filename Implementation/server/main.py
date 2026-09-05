import json
import os
import re
import urllib.request
from typing import Any, Dict, List, Optional
import docker
from fastapi import FastAPI, HTTPException
from fastapi.responses import FileResponse
from fastapi.staticfiles import StaticFiles

app = FastAPI(title="Drone Coordinator Observability Backend")

# Mount static files directory for frontend assets
STATIC_DIR = os.path.join(os.path.dirname(__file__), "static")
os.makedirs(STATIC_DIR, exist_ok=True)
app.mount("/static", StaticFiles(directory=STATIC_DIR), name="static")


def get_docker_client() -> docker.DockerClient:
    """Initialize and return the Docker client instance."""
    try:
        return docker.from_env()
    except Exception as err:
        raise HTTPException(
            status_code=500, detail=f"Failed to connect to Docker daemon: {err}"
        )


COORDINATOR_HTTP_URL = os.environ.get("COORDINATOR_HTTP_URL", "http://coordinator:8080/status")


def fetch_coordinator_status_http() -> Dict[str, Any]:
    """Query current swarm status directly from Rust coordinator HTTP REST API."""
    try:
        req = urllib.request.Request(COORDINATOR_HTTP_URL, headers={"Accept": "application/json"})
        with urllib.request.urlopen(req, timeout=2.0) as response:
            if response.status == 200:
                body = response.read().decode("utf-8")
                return json.loads(body)
    except Exception as err:
        # Fallback to localhost if running outside Docker network
        try:
            fallback_url = "http://localhost:8080/status"
            req = urllib.request.Request(fallback_url, headers={"Accept": "application/json"})
            with urllib.request.urlopen(req, timeout=1.0) as response:
                if response.status == 200:
                    body = response.read().decode("utf-8")
                    return json.loads(body)
        except Exception:
            pass
        print(f"[server] Warning: Could not reach coordinator REST API: {err}")

    return {"leader_id": None, "drones": {}}


@app.get("/")
def read_root():
    """Serve main HTML dashboard."""
    index_path = os.path.join(STATIC_DIR, "index.html")
    if os.path.exists(index_path):
        return FileResponse(index_path)
    return {"message": "ok, funziono"}


@app.get("/home")
def read_home():
    """Legacy home endpoint."""
    return {"message": "ok, funziono"}


@app.get("/api/swarm/status")
def get_swarm_status():
    """Fetch current swarm status by querying coordinator REST API and inspecting Docker containers."""
    client = get_docker_client()
    coordinator_status = fetch_coordinator_status_http()

    all_containers = client.containers.list(all=True)

    # Discover drone containers
    drone_containers = [
        c for c in all_containers if "drone" in c.name and "coordinator" not in c.name
    ]

    drone_list = []
    found_drone_ids = set()

    for container in drone_containers:
        raw_name = container.name
        # Format container name to drone_id (e.g. implementation-drone-1 -> drone_1)
        drone_id_match = re.search(r"drone[-_]?(\d+)", raw_name)
        if drone_id_match:
            drone_num = drone_id_match.group(1)
            drone_id = f"drone_{drone_num}"
        else:
            drone_id = raw_name

        found_drone_ids.add(drone_id)

        # Get FSM status if reported by coordinator REST API
        fsm_info = coordinator_status.get("drones", {}).get(drone_id, {})
        fsm_s1 = fsm_info.get("fsm_s1", "Active" if container.status == "running" else "Unknown")
        fsm_s2 = fsm_info.get("fsm_s2", "Follower")

        if coordinator_status.get("leader_id") and coordinator_status["leader_id"] == drone_id:
            fsm_s2 = "Leader"

        is_paused = container.status == "paused"
        if is_paused:
            fsm_s1 = "Failed" if fsm_s1 != "Suspected" else "Suspected"

        drone_list.append({
            "drone_id": drone_id,
            "container_name": raw_name,
            "container_status": container.status,
            "fsm_s1": fsm_s1,
            "fsm_s2": fsm_s2,
            "is_paused": is_paused,
            "is_leader": coordinator_status.get("leader_id") == drone_id,
        })

    # Sort drones by drone_id for stable UI ordering
    drone_list.sort(key=lambda d: d["drone_id"])

    return {
        "total_drones": len(drone_list),
        "leader_id": coordinator_status.get("leader_id"),
        "drones": drone_list,
    }



@app.post("/api/drone/{drone_id}/pause")
def pause_drone(drone_id: str):
    """Pause target drone container to simulate agent failure."""
    client = get_docker_client()
    target_container = None

    for container in client.containers.list(all=True):
        if drone_id in container.name or container.name.endswith(drone_id.replace("_", "-")):
            target_container = container
            break

    if not target_container:
        raise HTTPException(
            status_code=44, detail=f"Drone container for '{drone_id}' not found."
        )

    try:
        target_container.pause()
        return {"status": "success", "message": f"Paused {target_container.name}"}
    except Exception as err:
        raise HTTPException(status_code=500, detail=str(err))


@app.post("/api/drone/{drone_id}/unpause")
def unpause_drone(drone_id: str):
    """Unpause target drone container to simulate repair."""
    client = get_docker_client()
    target_container = None

    for container in client.containers.list(all=True):
        if drone_id in container.name or container.name.endswith(drone_id.replace("_", "-")):
            target_container = container
            break

    if not target_container:
        raise HTTPException(
            status_code=404, detail=f"Drone container for '{drone_id}' not found."
        )

    try:
        target_container.unpause()
        return {"status": "success", "message": f"Unpaused {target_container.name}"}
    except Exception as err:
        raise HTTPException(status_code=500, detail=str(err))


@app.post("/api/swarm/formation/line")
def trigger_line_formation():
    """Trigger line formation command inside coordinator container."""
    client = get_docker_client()
    coordinator_container = None

    for container in client.containers.list():
        if "coordinator" in container.name:
            coordinator_container = container
            break

    if not coordinator_container:
        raise HTTPException(
            status_code=404, detail="Coordinator container is not running."
        )

    cmd = [
        "bash",
        "-c",
        'export ROS_DISCOVERY_SERVER=172.28.0.10:11811 && source /opt/ros/jazzy/setup.bash && ros2 topic pub --once /swarm/formation std_msgs/msg/String "data: \'{\\"formation\\": \\"line\\"}\'"'
    ]

    try:
        exec_result = coordinator_container.exec_run(cmd)
        output_str = exec_result.output.decode("utf-8", errors="ignore")
        print(f"[server] Formation command result (code {exec_result.exit_code}): {output_str}")
        return {
            "status": "success",
            "exit_code": exec_result.exit_code,
            "output": output_str,
        }
    except Exception as err:
        raise HTTPException(status_code=500, detail=str(err))

