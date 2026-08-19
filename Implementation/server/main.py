import os
import re
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


def parse_coordinator_status_log(log_text: str) -> Dict[str, Any]:
    """Parse the latest status line from the coordinator container log output.

    Sample log line:
    [coordinator] Swarm status: 3 drone(s) | Leader: Some("drone_1") | States: [drone_1: (Active, Leader), drone_2: (Active, Follower)]
    """
    latest_status_line = None
    for line in log_text.splitlines():
        if "[coordinator] Swarm status:" in line:
            latest_status_line = line

    if not latest_status_line:
        return {"leader_id": None, "drones": {}}

    # Extract leader ID
    leader_match = re.search(r'Leader:\s*(?:Some\("([^"]+)"\)|None)', latest_status_line)
    leader_id = leader_match.group(1) if leader_match and leader_match.group(1) else None

    # Extract drone states list: [drone_1: (Active, Leader), drone_2: (Active, Follower)]
    drones_map = {}
    states_section_match = re.search(r"States:\s*\[(.*?)\]", latest_status_line)
    if states_section_match:
        states_str = states_section_match.group(1)
        # Match pattern drone_id: (State1, State2)
        pattern = r'([a-zA-Z0-9_\-]+):\s*\(([^,]+),\s*([^)]+)\)'
        for match in re.finditer(pattern, states_str):
            drone_id = match.group(1).strip()
            fsm_s1 = match.group(2).strip()
            fsm_s2 = match.group(3).strip()
            drones_map[drone_id] = {
                "drone_id": drone_id,
                "fsm_s1": fsm_s1,
                "fsm_s2": fsm_s2,
            }

    return {"leader_id": leader_id, "drones": drones_map}


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
    """Fetch current swarm status by inspecting Docker containers and coordinator logs."""
    client = get_docker_client()

    # Find coordinator container
    coordinator_container = None
    all_containers = client.containers.list(all=True)
    for container in all_containers:
        if "coordinator" in container.name:
            coordinator_container = container
            break

    log_status = {"leader_id": None, "drones": {}}
    if coordinator_container:
        try:
            logs = coordinator_container.logs(tail=100).decode("utf-8", errors="ignore")
            log_status = parse_coordinator_status_log(logs)
        except Exception as log_err:
            print(f"[server] Warning: Could not read coordinator logs: {log_err}")

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

        # Get FSM status if reported by coordinator
        fsm_info = log_status["drones"].get(drone_id, {})
        fsm_s1 = fsm_info.get("fsm_s1", "Active" if container.status == "running" else "Unknown")
        fsm_s2 = fsm_info.get("fsm_s2", "Follower")

        if log_status["leader_id"] and log_status["leader_id"] == drone_id:
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
            "is_leader": log_status["leader_id"] == drone_id,
        })

    # Sort drones by drone_id for stable UI ordering
    drone_list.sort(key=lambda d: d["drone_id"])

    return {
        "total_drones": len(drone_list),
        "leader_id": log_status.get("leader_id"),
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

