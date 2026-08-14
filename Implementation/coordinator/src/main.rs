use Kani_verify::{transition, DroneState, Fsm2State, SwarmEvent};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug, Serialize, Deserialize)]
struct RegistrationPayload {
    drone_id: String,
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Debug, Clone)]
struct ManagedDrone {
    drone_id: String,
    state: DroneState,
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
    let swarm_state_clone = Arc::clone(&swarm_state);
    thread::spawn(move || {
        listen_and_spawn_drones(swarm_state_clone);
    });

    println!("[coordinator] Coordinator running and listening on ROS 2 topic /swarm/register...");

    // Main loop keeps coordinator active and performs periodic status logging & leader selection
    loop {
        thread::sleep(std::time::Duration::from_secs(3));
        if let Ok(mut guard) = swarm_state.lock() {
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
                    // for all drones checks wether the id is equal to the chosen leader id and applies the transition
                    let is_chosen = id == &chosen_leader_id;
                    drone.state = transition(drone.state, SwarmEvent::LeaderSelection { is_chosen });
                }
            }
            // sets the new leader
            state.current_leader_id = Some(chosen_leader_id);
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

                    if !guard.drones.contains_key(&drone_id) {
                        println!(
                            "[coordinator] New drone detected: {} at ({}, {}, {}). Initial state: (Unregistered, None)",
                            drone_id, payload.x, payload.y, payload.z
                        );

                        // Initial state: (Unregistered, None)
                        let new_drone = ManagedDrone {
                            drone_id: drone_id.clone(),
                            state: DroneState::new(),
                            x: payload.x,
                            y: payload.y,
                            z: payload.z,
                        };
                        guard.drones.insert(drone_id.clone(), new_drone);
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