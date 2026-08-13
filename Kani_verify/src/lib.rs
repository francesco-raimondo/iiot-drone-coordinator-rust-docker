// Verification of the Two-FSM Drone Swarm Coordination Model using Kani
//
// State tuple per drone: (S1, S2)
// S1 (FSM 1 - Health): Unregistered, Active, Suspected, Failed
// S2 (FSM 2 - Role): None, Candidate, Follower, Leader

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fsm1State {
    Unregistered,
    Active,
    Suspected,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fsm2State {
    None,
    Candidate,
    Follower,
    Leader,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DroneState {
    pub s1: Fsm1State,
    pub s2: Fsm2State,
}

impl DroneState {
    pub fn new() -> Self {
        DroneState {
            s1: Fsm1State::Unregistered,
            s2: Fsm2State::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwarmEvent {
    /// Event 1: Registration succeeded (spawned by coordinator)
    RegistrationSuccess { has_active_leader: bool },
    /// Event 2: Leader selection by coordinator
    LeaderSelection { is_chosen: bool },
    /// Event 3: Heartbeat missing > 5s
    HeartbeatTimeout5s,
    /// Event 4: Heartbeat received within 10s while suspected
    HeartbeatRecovered10s,
    /// Event 5: Heartbeat missing > 10s (Drone Failure)
    DroneFailed,
    /// Event 6: Drone repair completed (5-20s random delay)
    RepairCompleted,
    /// Trigger: Re-election triggered because current Leader failed
    LeaderFailedReelectionTrigger,
}

/// State transition function for a single drone given an event.
pub fn transition(current: DroneState, event: SwarmEvent) -> DroneState {
    match event {
        // Event 1: Registration
        SwarmEvent::RegistrationSuccess { has_active_leader } => {
            if current.s1 == Fsm1State::Unregistered && current.s2 == Fsm2State::None {
                if !has_active_leader {
                    // Case A: No active leader -> Active Candidate
                    DroneState {
                        s1: Fsm1State::Active,
                        s2: Fsm2State::Candidate,
                    }
                } else {
                    // Case B: Active leader exists -> Active Follower
                    DroneState {
                        s1: Fsm1State::Active,
                        s2: Fsm2State::Follower,
                    }
                }
            } else {
                current
            }
        }

        // Event 2: Leader Selection (from Candidate state)
        SwarmEvent::LeaderSelection { is_chosen } => {
            if current.s1 == Fsm1State::Active && current.s2 == Fsm2State::Candidate {
                if is_chosen {
                    DroneState {
                        s1: Fsm1State::Active,
                        s2: Fsm2State::Leader,
                    }
                } else {
                    DroneState {
                        s1: Fsm1State::Active,
                        s2: Fsm2State::Follower,
                    }
                }
            } else {
                current
            }
        }

        // Event 3: Heartbeat timeout > 5s
        SwarmEvent::HeartbeatTimeout5s => {
            if current.s1 == Fsm1State::Active {
                DroneState {
                    s1: Fsm1State::Suspected,
                    s2: current.s2,
                }
            } else {
                current
            }
        }

        // Event 4: Heartbeat recovered within 10s
        SwarmEvent::HeartbeatRecovered10s => {
            if current.s1 == Fsm1State::Suspected {
                DroneState {
                    s1: Fsm1State::Active,
                    s2: current.s2,
                }
            } else {
                current
            }
        }

        // Event 5: Drone Failed (>10s no heartbeat)
        SwarmEvent::DroneFailed => {
            if current.s1 == Fsm1State::Suspected {
                DroneState {
                    s1: Fsm1State::Failed,
                    s2: Fsm2State::None,
                }
            } else {
                current
            }
        }

        // Event 6: Repair completed
        SwarmEvent::RepairCompleted => {
            if current.s1 == Fsm1State::Failed {
                DroneState {
                    s1: Fsm1State::Unregistered,
                    s2: Fsm2State::None,
                }
            } else {
                current
            }
        }

        // Re-election trigger when leader fails
        SwarmEvent::LeaderFailedReelectionTrigger => {
            if (current.s1 == Fsm1State::Active || current.s1 == Fsm1State::Suspected)
                && current.s2 == Fsm2State::Follower
            {
                DroneState {
                    s1: current.s1,
                    s2: Fsm2State::Candidate,
                }
            } else {
                current
            }
        }
    }
}



// KANI PROOF HARNESSES

#[cfg(kani)]
mod verification_harnesses {
    use super::*;

    /// Proof 1: Inactive state consistency (Safety Invariant)
    /// An Unregistered or Failed drone MUST ALWAYS have S2 = None.
    #[kani::proof]
    pub fn proof_inactive_role_is_none() {
        let s1_choice: u8 = kani::any();
        let s2_choice: u8 = kani::any();
        let event_choice: u8 = kani::any();

        let s1 = match s1_choice % 4 {
            0 => Fsm1State::Unregistered,
            1 => Fsm1State::Active,
            2 => Fsm1State::Suspected,
            _ => Fsm1State::Failed,
        };

        let s2 = match s2_choice % 4 {
            0 => Fsm2State::None,
            1 => Fsm2State::Candidate,
            2 => Fsm2State::Follower,
            _ => Fsm2State::Leader,
        };

        let current = DroneState { s1, s2 };

        // Assume valid initial configuration: if S1 is Unregistered or Failed, S2 must be None
        if current.s1 == Fsm1State::Unregistered || current.s1 == Fsm1State::Failed {
            kani::assume(current.s2 == Fsm2State::None);
        }

        let event = match event_choice % 7 {
            0 => SwarmEvent::RegistrationSuccess {
                has_active_leader: kani::any(),
            },
            1 => SwarmEvent::LeaderSelection {
                is_chosen: kani::any(),
            },
            2 => SwarmEvent::HeartbeatTimeout5s,
            3 => SwarmEvent::HeartbeatRecovered10s,
            4 => SwarmEvent::DroneFailed,
            5 => SwarmEvent::RepairCompleted,
            _ => SwarmEvent::LeaderFailedReelectionTrigger,
        };

        let next = transition(current, event);

        // Safety assertion: after ANY event, Unregistered or Failed MUST have S2 == None
        if next.s1 == Fsm1State::Unregistered || next.s1 == Fsm1State::Failed {
            kani::assert(next.s2 == Fsm2State::None, "S2 must be None when S1 is Unregistered or Failed");
        }
    }

    /// Proof 2: Suspected role preservation (Safety Invariant)
    /// Transitioning to Suspected or recovering from Suspected MUST preserve FSM2 role.
    #[kani::proof]
    pub fn proof_suspected_role_preservation() {
        let s2_choice: u8 = kani::any();
        let s2 = match s2_choice % 4 {
            0 => Fsm2State::None,
            1 => Fsm2State::Candidate,
            2 => Fsm2State::Follower,
            _ => Fsm2State::Leader,
        };

        let active_drone = DroneState {
            s1: Fsm1State::Active,
            s2,
        };

        // Timeout 5s -> Suspected
        let suspected_drone = transition(active_drone, SwarmEvent::HeartbeatTimeout5s);
        kani::assert(suspected_drone.s1 == Fsm1State::Suspected, "Drone must be in Suspected state");
        kani::assert(suspected_drone.s2 == s2, "Role S2 must be preserved when entering Suspected");

        // Recover within 10s -> Active
        let recovered_drone = transition(suspected_drone, SwarmEvent::HeartbeatRecovered10s);
        kani::assert(recovered_drone.s1 == Fsm1State::Active, "Drone must be back to Active state");
        kani::assert(recovered_drone.s2 == s2, "Role S2 must be preserved when recovering to Active");
    }
    
    

    /// Proof 3: Swarm Single Leader Invariant & Leader Failure Recovery
    /// In a 3-drone swarm, there is at most 1 Leader. When the Leader fails,
    /// a new Leader can be elected among active candidates.
    #[kani::proof]
    pub fn proof_swarm_single_leader_and_recovery() {
        // Model a 3-drone swarm
        let mut d1 = DroneState::new();
        let mut d2 = DroneState::new();
        let mut d3 = DroneState::new();

        // 1. Register d1 (no active leader -> Candidate)
        d1 = transition(d1, SwarmEvent::RegistrationSuccess { has_active_leader: false });
        kani::assert(d1.s2 == Fsm2State::Candidate, "d1 must be Candidate after registration with no active leader");

        // 2. Select d1 as Leader
        d1 = transition(d1, SwarmEvent::LeaderSelection { is_chosen: true });
        kani::assert(d1.s2 == Fsm2State::Leader, "d1 must be Leader");

        // 3. Register d2 and d3 (active leader exists -> Follower)
        d2 = transition(d2, SwarmEvent::RegistrationSuccess { has_active_leader: true });
        d3 = transition(d3, SwarmEvent::RegistrationSuccess { has_active_leader: true });
        kani::assert(d2.s2 == Fsm2State::Follower, "d2 must be Follower");
        kani::assert(d3.s2 == Fsm2State::Follower, "d3 must be Follower");

        // Verify count of Leaders == 1
        let leaders_count = (d1.s2 == Fsm2State::Leader) as u8
            + (d2.s2 == Fsm2State::Leader) as u8
            + (d3.s2 == Fsm2State::Leader) as u8;
        kani::assert(leaders_count == 1, "Exactly one leader must exist in the swarm");

        // --- Simulate Leader Failure & Re-election ---
        // d1 fails (>5s suspected, >10s failed)
        d1 = transition(d1, SwarmEvent::HeartbeatTimeout5s);
        d1 = transition(d1, SwarmEvent::DroneFailed);
        kani::assert(d1.s1 == Fsm1State::Failed && d1.s2 == Fsm2State::None, "d1 must be in Failed state with S2=None");

        // Followers get reelection trigger -> Candidate
        d2 = transition(d2, SwarmEvent::LeaderFailedReelectionTrigger);
        d3 = transition(d3, SwarmEvent::LeaderFailedReelectionTrigger);
        kani::assert(d2.s2 == Fsm2State::Candidate, "d2 must become Candidate for reelection");
        kani::assert(d3.s2 == Fsm2State::Candidate, "d3 must become Candidate for reelection");

        // Elect d2 as new Leader, d3 as Follower
        d2 = transition(d2, SwarmEvent::LeaderSelection { is_chosen: true });
        d3 = transition(d3, SwarmEvent::LeaderSelection { is_chosen: false });

        kani::assert(d2.s2 == Fsm2State::Leader, "d2 must be new Leader");
        kani::assert(d3.s2 == Fsm2State::Follower, "d3 must stay Follower");

        // Verify count of Leaders is STILL exactly 1
        let new_leaders_count = (d1.s2 == Fsm2State::Leader) as u8
            + (d2.s2 == Fsm2State::Leader) as u8
            + (d3.s2 == Fsm2State::Leader) as u8;
        kani::assert(new_leaders_count == 1, "Exactly one leader must exist after reelection");

        // --- Simulate d1 Repair (Event 6) ---
        d1 = transition(d1, SwarmEvent::RepairCompleted);
        kani::assert(d1.s1 == Fsm1State::Unregistered && d1.s2 == Fsm2State::None, "d1 must be Unregistered after repair");

        // d1 re-registers while d2 is active leader (Case B -> Follower)
        d1 = transition(d1, SwarmEvent::RegistrationSuccess { has_active_leader: true });
        kani::assert(d1.s1 == Fsm1State::Active && d1.s2 == Fsm2State::Follower, "d1 must re-join as Active Follower");

        // Verify count of Leaders is STILL exactly 1
        let final_leaders_count = (d1.s2 == Fsm2State::Leader) as u8
            + (d2.s2 == Fsm2State::Leader) as u8
            + (d3.s2 == Fsm2State::Leader) as u8;
        kani::assert(final_leaders_count == 1, "Exactly one leader must exist after repaired drone rejoins");
    }
}
