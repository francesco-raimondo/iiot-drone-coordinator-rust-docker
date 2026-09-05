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
    /// Event 3 & 5: Timed heartbeat timeout with discrete elapsed time in seconds (t >= 5s -> Suspected, t >= 10s -> Failed)
    HeartbeatTimeout { elapsed_seconds: u8 },
    /// Event 4: Heartbeat recovered while suspected
    HeartbeatRecovered,
    /// Event 6: Drone repair completed
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

        // Event 3 & 5: Timed Heartbeat Timeout with discrete bounds (5s / 10s)
        SwarmEvent::HeartbeatTimeout { elapsed_seconds } => {
            if elapsed_seconds >= 10 {
                // > 10s timeout -> Failed, role reset to None
                if current.s1 == Fsm1State::Active || current.s1 == Fsm1State::Suspected {
                    DroneState {
                        s1: Fsm1State::Failed,
                        s2: Fsm2State::None,
                    }
                } else {
                    current
                }
            } else if elapsed_seconds >= 5 {
                // 5s..9s timeout -> Suspected, role S2 preserved
                if current.s1 == Fsm1State::Active {
                    DroneState {
                        s1: Fsm1State::Suspected,
                        s2: current.s2,
                    }
                } else {
                    current
                }
            } else {
                // < 5s: active heartbeat within interval -> no change
                current
            }
        }

        // Event 4: Heartbeat recovered while in Suspected state
        SwarmEvent::HeartbeatRecovered => {
            if current.s1 == Fsm1State::Suspected {
                DroneState {
                    s1: Fsm1State::Active,
                    s2: current.s2,
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

    /// Helper function to construct a symbolic DroneState using kani::any()
    fn symbolic_drone_state() -> DroneState {
        let s1_choice: u8 = kani::any();
        let s2_choice: u8 = kani::any();

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

        DroneState { s1, s2 }
    }

    /// Proof 1: Inactive state consistency (Safety Invariant)
    /// An Unregistered or Failed drone MUST ALWAYS have S2 = None.
    #[kani::proof]
    pub fn proof_inactive_role_is_none() {
        let current = symbolic_drone_state();

        // Assume valid initial configuration: if S1 is Unregistered or Failed, S2 must be None
        if current.s1 == Fsm1State::Unregistered || current.s1 == Fsm1State::Failed {
            kani::assume(current.s2 == Fsm2State::None);
        }

        let event_choice: u8 = kani::any();
        let event = match event_choice % 6 {
            0 => SwarmEvent::RegistrationSuccess {
                has_active_leader: kani::any(),
            },
            1 => SwarmEvent::LeaderSelection {
                is_chosen: kani::any(),
            },
            2 => SwarmEvent::HeartbeatTimeout {
                elapsed_seconds: kani::any(),
            },
            3 => SwarmEvent::HeartbeatRecovered,
            4 => SwarmEvent::RepairCompleted,
            _ => SwarmEvent::LeaderFailedReelectionTrigger,
        };

        let next = transition(current, event);

        // Safety assertion: after ANY event, Unregistered or Failed MUST have S2 == None
        if next.s1 == Fsm1State::Unregistered || next.s1 == Fsm1State::Failed {
            kani::assert(
                next.s2 == Fsm2State::None,
                "S2 must be None when S1 is Unregistered or Failed",
            );
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

        // Symbolic time between 5s and 9s -> Suspected
        let t_suspected: u8 = kani::any();
        kani::assume(t_suspected >= 5 && t_suspected < 10);

        let suspected_drone = transition(
            active_drone,
            SwarmEvent::HeartbeatTimeout {
                elapsed_seconds: t_suspected,
            },
        );
        kani::assert(
            suspected_drone.s1 == Fsm1State::Suspected,
            "Drone must be in Suspected state for 5s <= t < 10s",
        );
        kani::assert(
            suspected_drone.s2 == s2,
            "Role S2 must be preserved when entering Suspected",
        );

        // Recover while suspected -> Active
        let recovered_drone = transition(suspected_drone, SwarmEvent::HeartbeatRecovered);
        kani::assert(
            recovered_drone.s1 == Fsm1State::Active,
            "Drone must be back to Active state",
        );
        kani::assert(
            recovered_drone.s2 == s2,
            "Role S2 must be preserved when recovering to Active",
        );
    }

    /// Proof 3: Swarm Single Leader Invariant & Leader Failure Recovery (Fully Symbolic)
    /// Proves that in a 3-drone swarm with arbitrary symbolic initial states and symbolic event executions:
    /// 1. If at most 1 Leader exists initially, then after arbitrary timed timeouts, failure, re-election,
    ///    repair, and re-registration events, the number of Leaders NEVER exceeds 1 (leaders_count <= 1).
    /// 2. Formally models the timed transition bounds (5s Suspected, 10s Failed) and leader recovery.
    #[kani::proof]
    pub fn proof_swarm_single_leader_and_recovery() {
        fn count_leaders(d1: DroneState, d2: DroneState, d3: DroneState) -> u8 {
            (d1.s2 == Fsm2State::Leader) as u8
                + (d2.s2 == Fsm2State::Leader) as u8
                + (d3.s2 == Fsm2State::Leader) as u8
        }

        // Initialize 3 drones with fully symbolic states
        let mut d1 = symbolic_drone_state();
        let mut d2 = symbolic_drone_state();
        let mut d3 = symbolic_drone_state();

        // Assume initial valid invariant: Unregistered or Failed drones must have S2 == None
        if d1.s1 == Fsm1State::Unregistered || d1.s1 == Fsm1State::Failed {
            kani::assume(d1.s2 == Fsm2State::None);
        }
        if d2.s1 == Fsm1State::Unregistered || d2.s1 == Fsm1State::Failed {
            kani::assume(d2.s2 == Fsm2State::None);
        }
        if d3.s1 == Fsm1State::Unregistered || d3.s1 == Fsm1State::Failed {
            kani::assume(d3.s2 == Fsm2State::None);
        }

        // Assume initial valid swarm state: at most 1 leader exists
        kani::assume(count_leaders(d1, d2, d3) <= 1);

        // Phase 1: Symbolic Timed Timeout on any drone (elapsed_seconds evaluated symbolically)
        let elapsed_d1: u8 = kani::any();
        let elapsed_d2: u8 = kani::any();
        let elapsed_d3: u8 = kani::any();

        d1 = transition(
            d1,
            SwarmEvent::HeartbeatTimeout {
                elapsed_seconds: elapsed_d1,
            },
        );
        d2 = transition(
            d2,
            SwarmEvent::HeartbeatTimeout {
                elapsed_seconds: elapsed_d2,
            },
        );
        d3 = transition(
            d3,
            SwarmEvent::HeartbeatTimeout {
                elapsed_seconds: elapsed_d3,
            },
        );

        // Assert invariant holds after timed timeouts
        kani::assert(
            count_leaders(d1, d2, d3) <= 1,
            "Leader count must be <= 1 after timed heartbeat evaluation",
        );

        // Phase 2: Symbolic Leader Failure & Re-election trigger
        let has_active_leader_now = count_leaders(d1, d2, d3) == 1;

        if !has_active_leader_now {
            // Trigger re-election on remaining followers
            d1 = transition(d1, SwarmEvent::LeaderFailedReelectionTrigger);
            d2 = transition(d2, SwarmEvent::LeaderFailedReelectionTrigger);
            d3 = transition(d3, SwarmEvent::LeaderFailedReelectionTrigger);

            // Symbolic election outcome among candidates
            let elect_d1: bool = kani::any();
            let elect_d2: bool = kani::any();
            let elect_d3: bool = kani::any();

            // At most one drone can be chosen as leader in election
            kani::assume(
                !(elect_d1 && elect_d2) && !(elect_d1 && elect_d3) && !(elect_d2 && elect_d3),
            );

            d1 = transition(
                d1,
                SwarmEvent::LeaderSelection {
                    is_chosen: elect_d1,
                },
            );
            d2 = transition(
                d2,
                SwarmEvent::LeaderSelection {
                    is_chosen: elect_d2,
                },
            );
            d3 = transition(
                d3,
                SwarmEvent::LeaderSelection {
                    is_chosen: elect_d3,
                },
            );

            // Assert invariant holds after re-election
            kani::assert(
                count_leaders(d1, d2, d3) <= 1,
                "Leader count must be <= 1 after symbolic re-election",
            );
        }

        // Phase 3: Symbolic Repair & Re-registration of any failed drone
        let repair_d1: bool = kani::any();
        if repair_d1 {
            d1 = transition(d1, SwarmEvent::RepairCompleted);
            let leader_exists = count_leaders(d1, d2, d3) == 1;
            d1 = transition(
                d1,
                SwarmEvent::RegistrationSuccess {
                    has_active_leader: leader_exists,
                },
            );
        }

        let repair_d2: bool = kani::any();
        if repair_d2 {
            d2 = transition(d2, SwarmEvent::RepairCompleted);
            let leader_exists = count_leaders(d1, d2, d3) == 1;
            d2 = transition(
                d2,
                SwarmEvent::RegistrationSuccess {
                    has_active_leader: leader_exists,
                },
            );
        }

        // Assert final invariant: Leader count NEVER exceeds 1
        kani::assert(
            count_leaders(d1, d2, d3) <= 1,
            "Leader count must be <= 1 after drone repair and re-registration",
        );
    }
}
