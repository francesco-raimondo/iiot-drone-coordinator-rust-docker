use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug, Serialize, Deserialize)]
struct RegistrationPayload {
    drone_id: String,
    x: f64,
    y: f64,
    z: f64,
}

fn main() {
    println!("[coordinator] Starting Rust Swarm Coordinator...");

    let spawned_drones = Arc::new(Mutex::new(HashSet::<String>::new()));

    // Spawn listener thread for ROS 2 topic `/swarm/register`
    let spawned_drones_clone = Arc::clone(&spawned_drones);
    thread::spawn(move || {
        listen_and_spawn_drones(spawned_drones_clone);
    });

    println!("[coordinator] Coordinator running and listening on ROS 2 topic /swarm/register...");
    
    // Main loop keeps coordinator active
    loop {
        thread::sleep(std::time::Duration::from_secs(5));
        if let Ok(guard) = spawned_drones.lock() {
            println!(
                "[coordinator] Swarm status: {} drone(s) spawned {:?}",
                guard.len(),
                *guard
            );
        }
    }
}

fn listen_and_spawn_drones(spawned_drones: Arc<Mutex<HashSet<String>>>) {
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
                let json_str = cleaned.trim_start_matches("data:").trim().trim_matches('\'').trim_matches('"');
                // Unescape JSON string if ROS quotes it
                let unescaped_json = json_str.replace("\\\"", "\"");
                
                if let Ok(payload) = serde_json::from_str::<RegistrationPayload>(&unescaped_json) {
                    let drone_id = payload.drone_id.clone();
                    let mut guard = spawned_drones.lock().unwrap();
                    if !guard.contains(&drone_id) {
                        println!(
                            "[coordinator] New drone detected: {} at ({}, {}, {}). Initiating Gazebo spawn...",
                            drone_id, payload.x, payload.y, payload.z
                        );
                        guard.insert(drone_id.clone());
                        drop(guard);

                        spawn_drone_in_gazebo(&payload);
                    }
                }
            }
        }
    }
}

fn spawn_drone_in_gazebo(payload: &RegistrationPayload) {
    let world = std::env::var("WORLD_NAME").unwrap_or_else(|_| "empty".to_string());
    println!(
        "[coordinator] Executing spawn for {} in world '{}'...",
        payload.drone_id, world
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
            "https://fuel.gazebosim.org/1.0/OpenRobotics/models/X3 UAV",
        ])
        .output();

    match output {
        Ok(out) => {
            if out.status.success() {
                println!("[coordinator] SUCCESS: Drone {} spawned in Gazebo!", payload.drone_id);
            } else {
                let err_msg = String::from_utf8_lossy(&out.stderr);
                println!(
                    "[coordinator] WARNING: Spawn command for {} finished with status {}: {}",
                    payload.drone_id, out.status, err_msg
                );
            }
        }
        Err(e) => {
            println!(
                "[coordinator] ERROR: Failed to invoke ros2 run ros_gz_sim create for {}: {}",
                payload.drone_id, e
            );
        }
    }
}
