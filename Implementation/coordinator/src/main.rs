use Kani_verify::{transition, DroneState, Fsm1State, Fsm2State, SwarmEvent};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Serialize, Deserialize)]
struct RegistrationPayload {
    drone_id: String,
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Debug, Serialize, Deserialize)]
struct HeartbeatPayload {
    drone_id: String,
    status: String,
    timestamp: f64,
}

#[derive(Debug, Clone)]
struct ManagedDrone {
    drone_id: String,
    state: DroneState,
    last_heartbeat: Instant,
    x: f64,
    y: f64,
    z: f64,
}

struct SwarmCoordinatorState {
    drones: HashMap<String, ManagedDrone>,
    current_leader_id: Option<String>,
}

impl SwarmCoordinatorState {
    fn new() -> Self {
        SwarmCoordinatorState {
            drones: HashMap::new(),
            current_leader_id: None,
        }
    }
}

fn main() {
    println!("[coordinator] Starting Rust Swarm Coordinator with Verified FSM...");

    let swarm_state = Arc::new(Mutex::new(SwarmCoordinatorState::new()));

    // Spawn listener thread for ROS 2 topic `/swarm/register`
    let swarm_state_reg = Arc::clone(&swarm_state);
    thread::spawn(move || {
        listen_and_spawn_drones(swarm_state_reg);
    });

    // Spawn listener thread for ROS 2 topic `/swarm/heartbeat`
    let swarm_state_hb = Arc::clone(&swarm_state);
    thread::spawn(move || {
        listen_heartbeats(swarm_state_hb);
    });

    println!("[coordinator] Coordinator running and listening on ROS 2 topics /swarm/register & /swarm/heartbeat...");

    // Main loop keeps coordinator active, checks heartbeat timeouts (5s Event 3, 10s Event 5), and logs status
    loop {
        thread::sleep(Duration::from_secs(2));

        if let Ok(mut guard) = swarm_state.lock() {
            let mut suspected_drones = Vec::new();
            let mut failed_drones = Vec::new();

            for (id, drone) in guard.drones.iter_mut() {
                // Event 5: Timeout > 10s from Suspected -> Failed
                if drone.state.s1 == Fsm1State::Suspected && drone.last_heartbeat.elapsed() > Duration::from_secs(10) {
                    drone.state = transition(drone.state, SwarmEvent::DroneFailed);
                    failed_drones.push((id.clone(), drone.state));
                }
                // Event 3: Timeout > 5s from Active -> Suspected
                else if drone.state.s1 == Fsm1State::Active && drone.last_heartbeat.elapsed() > Duration::from_secs(5) {
                    drone.state = transition(drone.state, SwarmEvent::HeartbeatTimeout5s);
                    suspected_drones.push((id.clone(), drone.state));
                }
            }

            for (id, state) in suspected_drones {
                println!(
                    "[coordinator] WARNING: No heartbeat from {} for >5s! Event 3 applied -> New state: ({:?}, {:?})",
                    id, state.s1, state.s2
                );
            }

            let mut leader_failed = false;
            for (id, state) in failed_drones {
                println!(
                    "[coordinator] CRITICAL: No heartbeat from {} for >10s! Event 5 (DroneFailed) applied -> New state: ({:?}, {:?})",
                    id, state.s1, state.s2
                );

                // Despawn failed drone from Gazebo
                despawn_drone_from_gazebo(&id);

                if guard.current_leader_id.as_deref() == Some(&id) {
                    leader_failed = true;
                }
            }

            // Handle Leader failure & re-election trigger if current leader failed
            if leader_failed {
                handle_leader_failure(&mut guard);
            }

            // Check if we need to select a leader among Candidate drones
            perform_leader_selection_if_needed(&mut guard);

            let drone_summary: Vec<String> = guard
                .drones
                .values()
                .map(|d| format!("{}: ({:?}, {:?})", d.drone_id, d.state.s1, d.state.s2))
                .collect();

            println!(
                "[coordinator] Swarm status: {} drone(s) | Leader: {:?} | States: [{}]",
                guard.drones.len(),
                guard.current_leader_id,
                drone_summary.join(", ")
            );
        }
    }
}

fn handle_leader_failure(state: &mut SwarmCoordinatorState) {
    if let Some(old_leader_id) = state.current_leader_id.take() {
        println!(
            "[coordinator] LEADER FAILURE DETECTED: Leader {} failed! Triggering reelection among active followers...",
            old_leader_id
        );

        // Apply LeaderFailedReelectionTrigger to all active/suspected Followers
        for (id, drone) in state.drones.iter_mut() {
            if drone.state.s2 == Fsm2State::Follower {
                drone.state = transition(drone.state, SwarmEvent::LeaderFailedReelectionTrigger);
                println!(
                    "[coordinator] Reelection trigger applied to {}. New state: ({:?}, {:?})",
                    id, drone.state.s1, drone.state.s2
                );
            }
        }
    }
}

fn perform_leader_selection_if_needed(state: &mut SwarmCoordinatorState) {
    // Only perform leader selection if no active leader exists
    if state.current_leader_id.is_none() {
        let candidates: Vec<String> = state
            .drones
            .iter()
            .filter(|(_, d)| d.state.s2 == Fsm2State::Candidate)
            .map(|(id, _)| id.clone())
            .collect();

        if !candidates.is_empty() {
            // Randomly select a leader among candidates
            use std::collections::hash_map::RandomState;
            use std::hash::{BuildHasher, Hasher};
            let random_index = (RandomState::new().build_hasher().finish() as usize) % candidates.len();
            let chosen_leader_id = candidates[random_index].clone();

            println!(
                "[coordinator] Selecting leader randomly among candidates {:?} -> Chosen: {}",
                candidates, chosen_leader_id
            );

            // Apply Event 2 (LeaderSelection) for all candidates
            for (id, drone) in state.drones.iter_mut() {
                if drone.state.s2 == Fsm2State::Candidate {
                    let is_chosen = id == &chosen_leader_id;
                    drone.state = transition(drone.state, SwarmEvent::LeaderSelection { is_chosen });
                }
            }
            // sets the new leader
            state.current_leader_id = Some(chosen_leader_id);
        }
    }
}

fn listen_heartbeats(swarm_state: Arc<Mutex<SwarmCoordinatorState>>) {
    let mut child = Command::new("ros2")
        .args(&["topic", "echo", "/swarm/heartbeat", "std_msgs/msg/String"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to execute ros2 topic echo for /swarm/heartbeat.");

    let stdout = child.stdout.take().expect("Failed to capture stdout from ros2 topic echo");
    let reader = BufReader::new(stdout);

    println!("[coordinator] ROS 2 topic listener attached to /swarm/heartbeat");

    for line in reader.lines() {
        if let Ok(line_content) = line {
            let cleaned = line_content.trim();
            if cleaned.starts_with("data:") {
                let json_str = cleaned
                    .trim_start_matches("data:")
                    .trim()
                    .trim_matches('\'')
                    .trim_matches('"');
                let unescaped_json = json_str.replace("\\\"", "\"");

                if let Ok(payload) = serde_json::from_str::<HeartbeatPayload>(&unescaped_json) {
                    let mut guard = swarm_state.lock().unwrap();
                    if let Some(drone) = guard.drones.get_mut(&payload.drone_id) {
                        drone.last_heartbeat = Instant::now();

                        // If drone was Suspected, apply Event 4 (HeartbeatRecovered10s)
                        if drone.state.s1 == Fsm1State::Suspected {
                            drone.state = transition(drone.state, SwarmEvent::HeartbeatRecovered10s);
                            println!(
                                "[coordinator] INFO: Heartbeat recovered for {}! Event 4 applied -> New state: ({:?}, {:?})",
                                payload.drone_id, drone.state.s1, drone.state.s2
                            );
                        }
                    }
                }
            }
        }
    }
}

fn listen_and_spawn_drones(swarm_state: Arc<Mutex<SwarmCoordinatorState>>) {
    let mut child = Command::new("ros2")
        .args(&["topic", "echo", "/swarm/register", "std_msgs/msg/String"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to execute ros2 topic echo command. Ensure ROS 2 environment is sourced.");

    let stdout = child.stdout.take().expect("Failed to capture stdout from ros2 topic echo");
    let reader = BufReader::new(stdout);

    println!("[coordinator] ROS 2 topic listener attached to /swarm/register");

    for line in reader.lines() {
        if let Ok(line_content) = line {
            let cleaned = line_content.trim();
            if cleaned.starts_with("data:") {
                let json_str = cleaned
                    .trim_start_matches("data:")
                    .trim()
                    .trim_matches('\'')
                    .trim_matches('"');
                // Unescape JSON string if ROS quotes it
                let unescaped_json = json_str.replace("\\\"", "\"");

                if let Ok(payload) = serde_json::from_str::<RegistrationPayload>(&unescaped_json) {
                    let drone_id = payload.drone_id.clone();
                    let mut guard = swarm_state.lock().unwrap();

                    // Check if drone is completely new OR if it is a Repaired drone in Failed state
                    let is_new = !guard.drones.contains_key(&drone_id);
                    let is_failed = guard
                        .drones
                        .get(&drone_id)
                        .map(|d| d.state.s1 == Fsm1State::Failed)
                        .unwrap_or(false);

                    if is_new || is_failed {
                        if is_failed {
                            if let Some(drone) = guard.drones.get_mut(&drone_id) {
                                // Event 6: RepairCompleted (Failed -> Unregistered)
                                drone.state = transition(drone.state, SwarmEvent::RepairCompleted);
                                drone.last_heartbeat = Instant::now();
                                println!(
                                    "[coordinator] Event 6 (RepairCompleted) applied for {}. New state: ({:?}, {:?})",
                                    drone_id, drone.state.s1, drone.state.s2
                                );
                            }
                        } else {
                            println!(
                                "[coordinator] New drone detected: {} at ({}, {}, {}). Initial state: (Unregistered, None)",
                                drone_id, payload.x, payload.y, payload.z
                            );

                            let new_drone = ManagedDrone {
                                drone_id: drone_id.clone(),
                                state: DroneState::new(),
                                last_heartbeat: Instant::now(),
                                x: payload.x,
                                y: payload.y,
                                z: payload.z,
                            };
                            guard.drones.insert(drone_id.clone(), new_drone);
                        }

                        let has_active_leader = guard.current_leader_id.is_some();
                        drop(guard);

                        // Perform Gazebo Spawn
                        let spawn_success = spawn_drone_in_gazebo(&payload);

                        if spawn_success {
                            let mut guard = swarm_state.lock().unwrap();
                            if let Some(drone) = guard.drones.get_mut(&drone_id) {
                                // Event 1: RegistrationSuccess
                                drone.state = transition(
                                    drone.state,
                                    SwarmEvent::RegistrationSuccess { has_active_leader },
                                );
                                drone.last_heartbeat = Instant::now();
                                println!(
                                    "[coordinator] Event 1 (RegistrationSuccess) applied for {}. New state: ({:?}, {:?})",
                                    drone_id, drone.state.s1, drone.state.s2
                                );
                            }
                            // Trigger leader selection immediately if appropriate
                            perform_leader_selection_if_needed(&mut guard);
                        }
                    }
                }
            }
        }
    }
}

fn spawn_drone_in_gazebo(payload: &RegistrationPayload) -> bool {
    let world = std::env::var("WORLD_NAME").unwrap_or_else(|_| "empty".to_string());
    let template_path = std::env::var("DRONE_MODEL_PATH")
        .unwrap_or_else(|_| "/app/models/x3_uav/model.sdf".to_string());

    let robot_namespace = format!("/model/{}", payload.drone_id);

    let sdf_template = match fs::read_to_string(&template_path) {
        Ok(content) => content,
        Err(e) => {
            println!(
                "[coordinator] ERROR: Failed to read model template '{}': {}",
                template_path, e
            );
            return false;
        }
    };

    let sdf_instance = sdf_template.replace("__ROBOT_NAMESPACE__", &robot_namespace);

    let instance_path = format!("/app/spawned/{}_model.sdf", payload.drone_id);
    if let Err(e) = fs::write(&instance_path, sdf_instance) {
        println!(
            "[coordinator] ERROR: Failed to write per-drone model file '{}': {}",
            instance_path, e
        );
        return false;
    }

    let model_path = instance_path;

    println!(
        "[coordinator] Executing spawn for {} in world '{}' using model '{}' (namespace '{}')...",
        payload.drone_id, world, model_path, robot_namespace
    );

    let output = Command::new("ros2")
        .args(&[
            "run",
            "ros_gz_sim",
            "create",
            "-world",
            &world,
            "-name",
            &payload.drone_id,
            "-x",
            &payload.x.to_string(),
            "-y",
            &payload.y.to_string(),
            "-z",
            &payload.z.to_string(),
            "-file",
            &model_path,
        ])
        .output();

    match output {
        Ok(out) => {
            if out.status.success() {
                println!("[coordinator] SUCCESS: Drone {} spawned in Gazebo!", payload.drone_id);
                true
            } else {
                let err_msg = String::from_utf8_lossy(&out.stderr);
                println!(
                    "[coordinator] WARNING: Spawn command for {} finished with status {}: {}",
                    payload.drone_id, out.status, err_msg
                );
                false
            }
        }
        Err(e) => {
            println!(
                "[coordinator] ERROR: Failed to invoke ros2 run ros_gz_sim create for {}: {}",
                payload.drone_id, e
            );
            false
        }
    }
}

fn despawn_drone_from_gazebo(drone_id: &str) {
    let world = std::env::var("WORLD_NAME").unwrap_or_else(|_| "empty".to_string());

    println!(
        "[coordinator] Executing Gazebo despawn for failed drone {} in world '{}'...",
        drone_id, world
    );

    let service_topic = format!("/world/{}/remove", world);
    let req_body = format!("name: \"{}\", type: MODEL", drone_id);

    let output = Command::new("gz")
        .args(&[
            "service",
            "-s",
            &service_topic,
            "--reqtype",
            "gz.msgs.Entity",
            "--reptype",
            "gz.msgs.Boolean",
            "--timeout",
            "2000",
            "--req",
            &req_body,
        ])
        .output();

    match output {
        Ok(out) => {
            if out.status.success() {
                println!("[coordinator] SUCCESS: Drone {} despawned from Gazebo!", drone_id);
            } else {
                let err_msg = String::from_utf8_lossy(&out.stderr);
                println!(
                    "[coordinator] WARNING: Despawn command for {} finished with status {}: {}",
                    drone_id, out.status, err_msg
                );
            }
        }
        Err(e) => {
            println!(
                "[coordinator] ERROR: Failed to invoke gz service remove for {}: {}",
                drone_id, e
            );
        }
    }
}