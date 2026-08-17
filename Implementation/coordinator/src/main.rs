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
    x: Option<f64>,
    y: Option<f64>,
    z: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SwarmGotoPayload {
    drone_id: String,
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Debug, Serialize, Deserialize)]
struct SwarmFormationPayload {
    formation: String,
}

#[derive(Debug, Clone)]
struct ManagedDrone {
    drone_id: String,
    state: DroneState,
    last_heartbeat: Instant,
    x: f64,
    y: f64,
    z: f64,
    target_position: Option<(f64, f64, f64)>,
}

struct SwarmCoordinatorState {
    drones: HashMap<String, ManagedDrone>,
    current_leader_id: Option<String>,
    formation: Option<String>,
    formation_center: Option<(f64, f64)>,
    formation_positions: HashMap<String, (f64, f64, f64)>,
}

impl SwarmCoordinatorState {
    fn new() -> Self {
        SwarmCoordinatorState {
            drones: HashMap::new(),
            current_leader_id: None,
            formation: None,
            formation_center: None,
            formation_positions: HashMap::new(),
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

    // Spawn listener thread for ROS 2 topic `/swarm/goto`
    let swarm_state_goto = Arc::clone(&swarm_state);
    thread::spawn(move || {
        listen_swarm_goto(swarm_state_goto);
    });

    // Spawn listener thread for ROS 2 topic `/swarm/formation`
    let swarm_state_form = Arc::clone(&swarm_state);
    thread::spawn(move || {
        listen_swarm_formation(swarm_state_form);
    });

    println!("[coordinator] Coordinator running and listening on ROS 2 topics /swarm/register, /swarm/heartbeat, /swarm/goto & /swarm/formation...");

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
            let new_leader_elected = perform_leader_selection_if_needed(&mut guard);
            let active_formation = guard.formation.clone();

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

            drop(guard);

            if new_leader_elected && active_formation.is_some() {
                println!(
                    "[coordinator] AUTOMATIC RE-FORMATION: New Leader elected while active formation '{}'! Recalculating formation around new Leader...",
                    active_formation.unwrap()
                );
                trigger_line_formation(Arc::clone(&swarm_state));
            }
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

fn perform_leader_selection_if_needed(state: &mut SwarmCoordinatorState) -> bool {
    let mut newly_selected = false;
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
            newly_selected = true;
        }
    }
    newly_selected
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
                        if let Some(x) = payload.x { drone.x = x; }
                        if let Some(y) = payload.y { drone.y = y; }
                        if let Some(z) = payload.z { drone.z = z; }

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

fn listen_swarm_formation(swarm_state: Arc<Mutex<SwarmCoordinatorState>>) {
    let mut child = Command::new("ros2")
        .args(&["topic", "echo", "/swarm/formation", "std_msgs/msg/String"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to execute ros2 topic echo for /swarm/formation.");

    let stdout = child.stdout.take().expect("Failed to capture stdout from ros2 topic echo");
    let reader = BufReader::new(stdout);

    println!("[coordinator] ROS 2 topic listener attached to /swarm/formation");

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

                if let Ok(payload) = serde_json::from_str::<SwarmFormationPayload>(&unescaped_json) {
                    if payload.formation.to_lowercase() == "line" {
                        trigger_line_formation(Arc::clone(&swarm_state));
                    } else {
                        println!("[coordinator] Unknown formation requested: {}", payload.formation);
                    }
                }
            }
        }
    }
}

fn trigger_line_formation(swarm_state: Arc<Mutex<SwarmCoordinatorState>>) {
    let mut guard = swarm_state.lock().unwrap();

    let leader_id = match &guard.current_leader_id {
        Some(id) => id.clone(),
        None => {
            println!("[coordinator] WARNING: Cannot trigger line formation: No active leader elected!");
            return;
        }
    };

    // Determine or anchor the fixed formation center (X_C, Y_C)
    let (center_x, center_y) = match guard.formation_center {
        Some(center) => center,
        None => {
            let (lx, ly) = match guard.drones.get(&leader_id) {
                Some(leader_drone) => (leader_drone.x, leader_drone.y),
                None => (0.0, 0.0),
            };
            guard.formation_center = Some((lx, ly));
            (lx, ly)
        }
    };

    let formation_z = 4.0;
    let safety_distance = 1.5;

    // Collect active followers with their current Y coordinate: (drone_id, current_y)
    let mut followers: Vec<(String, f64)> = guard
        .drones
        .iter()
        .filter(|(id, drone)| *id != &leader_id && drone.state.s1 == Fsm1State::Active)
        .map(|(id, drone)| (id.clone(), drone.y))
        .collect();

    // Sort followers by current Y coordinate in ascending order (left to right)
    followers.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    println!(
        "[coordinator] FORMATION: Triggering LINE formation anchored at ({:.2}, {:.2}) around Leader {} with {} active followers...",
        center_x, center_y, leader_id, followers.len()
    );

    let mut new_commands = Vec::new();

    // 1. Leader target position: (center_x, center_y, formation_z)
    let leader_target = (center_x, center_y, formation_z);
    let mut leader_flight_z = formation_z;
    if let Some(leader_drone) = guard.drones.get_mut(&leader_id) {
        let dist = ((leader_drone.x - center_x).powi(2) + (leader_drone.y - center_y).powi(2)).sqrt();
        if dist > 1.0 {
            leader_flight_z = 4.8;
        }
        leader_drone.target_position = Some(leader_target);
    }
    guard.formation_positions.insert(leader_id.clone(), leader_target);
    new_commands.push((leader_id.clone(), (center_x, center_y, leader_flight_z)));

    // 2. Compute symmetric follower target Y offsets relative to formation center Y
    let num_followers = followers.len();
    let mut target_y_list: Vec<f64> = Vec::with_capacity(num_followers);
    for idx in 0..num_followers {
        let pair_index = ((idx / 2) + 1) as f64;
        let sign = if idx % 2 == 0 { -1.0 } else { 1.0 };
        let offset_y = sign * pair_index * safety_distance;
        target_y_list.push(center_y + offset_y);
    }
    target_y_list.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    // 3. Separate airborne followers with saved targets and unassigned ground followers
    let mut final_assignments: Vec<(String, f64, bool)> = Vec::new();
    let mut occupied_y_slots: Vec<f64> = Vec::new();
    let mut airborne_with_target: Vec<(String, f64)> = Vec::new();
    let mut unassigned_ground: Vec<String> = Vec::new();

    for (fid, _y) in followers.iter() {
        if let Some(drone) = guard.drones.get(fid) {
            if drone.z > 2.0 && drone.target_position.is_some() {
                let saved_y = drone.target_position.unwrap().1;
                airborne_with_target.push((fid.clone(), saved_y));
                occupied_y_slots.push(saved_y);
            } else {
                unassigned_ground.push(fid.clone());
            }
        }
    }

    if !unassigned_ground.is_empty() && !airborne_with_target.is_empty() {
        // Airborne followers keep their exact saved target positions (NO movement for hovering drones)
        for (fid, saved_y) in airborne_with_target {
            final_assignments.push((fid, saved_y, false));
        }

        // Find missing slots in target_y_list not occupied by airborne followers
        let mut missing_slots: Vec<f64> = Vec::new();
        for &ty in &target_y_list {
            let is_occupied = occupied_y_slots.iter().any(|&oy| (oy - ty).abs() < 0.1);
            if !is_occupied {
                missing_slots.push(ty);
            }
        }
        missing_slots.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        // Assign missing outer slot to repaired ground drone(s)
        for (i, fid) in unassigned_ground.iter().enumerate() {
            let slot = if i < missing_slots.len() { missing_slots[i] } else { target_y_list[0] };
            final_assignments.push((fid.clone(), slot, true));
        }
    } else {
        for (idx, (follower_id, _curr_y)) in followers.iter().enumerate() {
            let is_g = guard.drones.get(follower_id).map_or(false, |d| d.z <= 2.0);
            final_assignments.push((follower_id.clone(), target_y_list[idx], is_g));
        }
    }

    // 4. Send goto commands to all followers
    for (follower_id, target_y, is_ground) in final_assignments {
        let saved_follower_target = (center_x, target_y, formation_z);

        // Ground/repaired drones fly at low transit altitude 1.5m to avoid hovering drones
        let curr_y = guard.drones.get(&follower_id).map_or(0.0, |d| d.y);
        let dist_y = (curr_y - target_y).abs();
        let flight_z = if is_ground {
            1.5
        } else if dist_y > 1.0 {
            4.8
        } else {
            4.0
        };
        let flight_target = (center_x, target_y, flight_z);

        if let Some(follower_drone) = guard.drones.get_mut(&follower_id) {
            follower_drone.target_position = Some(saved_follower_target);
        }
        guard.formation_positions.insert(follower_id.clone(), saved_follower_target);
        new_commands.push((follower_id, flight_target));
    }

    guard.formation = Some("line".to_string());
    drop(guard);

    // 3. Issue ros2 topic pub /swarm/goto commands for all drones in formation
    for (drone_id, (tx, ty, tz)) in new_commands {
        println!(
            "[coordinator] FORMATION LINE: Assigning target ({:.2}, {:.2}, {:.2}) to {}",
            tx, ty, tz, drone_id
        );
        send_swarm_goto(&drone_id, tx, ty, tz);
        thread::sleep(Duration::from_millis(400));
    }
}

fn listen_swarm_goto(swarm_state: Arc<Mutex<SwarmCoordinatorState>>) {
    let mut child = Command::new("ros2")
        .args(&["topic", "echo", "/swarm/goto", "std_msgs/msg/String"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to execute ros2 topic echo for /swarm/goto.");

    let stdout = child.stdout.take().expect("Failed to capture stdout from ros2 topic echo");
    let reader = BufReader::new(stdout);

    println!("[coordinator] ROS 2 topic listener attached to /swarm/goto");

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

                if let Ok(payload) = serde_json::from_str::<SwarmGotoPayload>(&unescaped_json) {
                    let mut guard = swarm_state.lock().unwrap();
                    if let Some(drone) = guard.drones.get_mut(&payload.drone_id) {
                        drone.target_position = Some((payload.x, payload.y, payload.z));
                        println!(
                            "[coordinator] Recorded target_position for {}: ({}, {}, {})",
                            payload.drone_id, payload.x, payload.y, payload.z
                        );
                    }
                }
            }
        }
    }
}

fn send_swarm_goto(drone_id: &str, x: f64, y: f64, z: f64) {
    let json_str = format!(r#"{{"drone_id": "{}", "x": {}, "y": {}, "z": {}}}"#, drone_id, x, y, z);
    let data_arg = format!("data: '{}'", json_str);
    let output = Command::new("ros2")
        .args(&[
            "topic",
            "pub",
            "-t",
            "3",
            "/swarm/goto",
            "std_msgs/msg/String",
            &data_arg,
        ])
        .output();

    match output {
        Ok(out) => {
            if out.status.success() {
                println!(
                    "[coordinator] RESILIENCE SUCCESS: Resent goto target ({}, {}, {}) to {}",
                    x, y, z, drone_id
                );
            } else {
                let err_msg = String::from_utf8_lossy(&out.stderr);
                println!(
                    "[coordinator] WARNING: Resend goto for {} finished with status {}: {}",
                    drone_id, out.status, err_msg
                );
            }
        }
        Err(e) => {
            println!(
                "[coordinator] ERROR: Failed to invoke ros2 topic pub for {}: {}",
                drone_id, e
            );
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
                                drone.x = payload.x;
                                drone.y = payload.y;
                                drone.z = payload.z;
                                drone.target_position = None;
                                println!(
                                    "[coordinator] Event 6 (RepairCompleted) applied for {}. Reset position to ground ({}, {}, {}). New state: ({:?}, {:?})",
                                    drone_id, payload.x, payload.y, payload.z, drone.state.s1, drone.state.s2
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
                                target_position: None,
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

                            let has_active_formation = guard.formation.is_some();
                            let saved_target = guard.drones.get(&drone_id).and_then(|d| d.target_position);
                            drop(guard);

                            if has_active_formation {
                                println!(
                                    "[coordinator] AUTOMATIC RE-FORMATION: Drone {} recovered! Re-integrating drone into active formation...",
                                    drone_id
                                );
                                trigger_line_formation(Arc::clone(&swarm_state));
                            } else if let Some((tx, ty, tz)) = saved_target {
                                println!(
                                    "[coordinator] RESILIENCE: Drone {} recovered! Re-sending target position ({}, {}, {})...",
                                    drone_id, tx, ty, tz
                                );
                                send_swarm_goto(&drone_id, tx, ty, tz);
                            }
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