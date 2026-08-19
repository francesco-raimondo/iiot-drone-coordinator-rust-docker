document.addEventListener('DOMContentLoaded', () => {
    const statTotal = document.getElementById('stat-total');
    const statActive = document.getElementById('stat-active');
    const statFailed = document.getElementById('stat-failed');
    const statLeader = document.getElementById('stat-leader');
    const dronesContainer = document.getElementById('drones-container');
    const btnLineFormation = document.getElementById('btn-line-formation');
    const toastContainer = document.getElementById('toast-container');

    let isFetching = false;

    // Toast Notification helper
    function showToast(message, isError = false) {
        const toast = document.createElement('div');
        toast.className = 'toast';
        if (isError) {
            toast.style.backgroundColor = '#ef4444';
        }
        toast.textContent = message;
        toastContainer.appendChild(toast);

        setTimeout(() => {
            toast.style.opacity = '0';
            toast.style.transition = 'opacity 0.3s ease';
            setTimeout(() => toast.remove(), 300);
        }, 3000);
    }

    // Fetch and render swarm status
    async function fetchSwarmStatus() {
        if (isFetching) return;
        isFetching = true;

        try {
            const response = await fetch('/api/swarm/status');
            if (!response.ok) {
                throw new Error(`HTTP error! status: ${response.status}`);
            }

            const data = await response.json();
            renderDashboard(data);
        } catch (error) {
            console.error('Failed to fetch swarm status:', error);
        } finally {
            isFetching = false;
        }
    }

    // Render summary statistics & drone grid
    function renderDashboard(data) {
        const drones = data.drones || [];
        const total = data.total_drones || drones.length;
        const leaderId = data.leader_id || 'None';

        let activeCount = 0;
        let failedCount = 0;

        drones.forEach(drone => {
            if (drone.is_paused || drone.fsm_s1 === 'Failed') {
                failedCount++;
            } else {
                activeCount++;
            }
        });

        // Update stats
        statTotal.textContent = total;
        statActive.textContent = activeCount;
        statFailed.textContent = failedCount;
        statLeader.textContent = leaderId ? leaderId.replace('_', ' ').toUpperCase() : 'None';

        if (drones.length === 0) {
            dronesContainer.innerHTML = `
                <div class="empty-state">
                    <span class="material-symbols-outlined" style="font-size: 3rem; margin-bottom: 0.5rem;">radar</span>
                    <h3>No Drone Agents Detected</h3>
                    <p>Start drone containers using <code>docker compose up --scale drone=N</code></p>
                </div>
            `;
            return;
        }

        // Generate drone cards
        dronesContainer.innerHTML = drones.map(drone => {
            const isLeader = drone.is_leader || drone.fsm_s2 === 'Leader';
            const isPaused = drone.is_paused;
            const fsmState1 = drone.fsm_s1 || 'Unknown';
            const fsmState2 = drone.fsm_s2 || 'Follower';

            // Determine status badge class
            let statusBadgeClass = 'active';
            if (isPaused || fsmState1 === 'Failed') {
                statusBadgeClass = 'failed';
            } else if (fsmState1 === 'Suspected') {
                statusBadgeClass = 'suspected';
            }

            return `
                <div class="drone-card ${isLeader ? 'is-leader-card' : ''}">
                    <div>
                        <div class="drone-header">
                            <div class="drone-title">
                                <span class="material-symbols-outlined">flight</span>
                                ${drone.drone_id.replace('_', ' ').toUpperCase()}
                            </div>
                        </div>
                        <div class="drone-container-name">${drone.container_name}</div>
                        <div class="badge-group">
                            <span class="pill-badge ${statusBadgeClass}">
                                <span class="material-symbols-outlined" style="font-size: 0.9rem;">
                                    ${isPaused ? 'pause_circle' : 'sensors'}
                                </span>
                                ${isPaused ? 'PAUSED' : fsmState1.toUpperCase()}
                            </span>
                            <span class="pill-badge ${isLeader ? 'leader' : 'follower'}">
                                <span class="material-symbols-outlined" style="font-size: 0.9rem;">
                                    ${isLeader ? 'stars' : 'navigation'}
                                </span>
                                ${fsmState2.toUpperCase()}
                            </span>
                        </div>
                    </div>
                    <div class="drone-actions">
                        ${!isPaused ? `
                            <button class="btn btn-danger btn-pause" data-id="${drone.drone_id}">
                                <span class="material-symbols-outlined">pause</span>
                                Simulate Failure
                            </button>
                        ` : `
                            <button class="btn btn-success btn-unpause" data-id="${drone.drone_id}">
                                <span class="material-symbols-outlined">play_arrow</span>
                                Repair Drone
                            </button>
                        `}
                    </div>
                </div>
            `;
        }).join('');

        // Attach event listeners to buttons
        document.querySelectorAll('.btn-pause').forEach(btn => {
            btn.addEventListener('click', (e) => handleDroneAction(e.currentTarget.dataset.id, 'pause'));
        });

        document.querySelectorAll('.btn-unpause').forEach(btn => {
            btn.addEventListener('click', (e) => handleDroneAction(e.currentTarget.dataset.id, 'unpause'));
        });
    }

    // Handle pause/unpause API calls
    async function handleDroneAction(droneId, action) {
        try {
            const response = await fetch(`/api/drone/${droneId}/${action}`, { method: 'POST' });
            const result = await response.json();

            if (!response.ok) {
                throw new Error(result.detail || 'Action failed');
            }

            showToast(`Drone ${droneId} ${action === 'pause' ? 'paused (simulated failure)' : 'repaired'}.`);
            fetchSwarmStatus();
        } catch (error) {
            showToast(`Error: ${error.message}`, true);
        }
    }

    // Handle line formation button click
    btnLineFormation.addEventListener('click', async () => {
        btnLineFormation.disabled = true;
        btnLineFormation.innerHTML = `<span class="material-symbols-outlined spinning">sync</span> Requesting Line Formation...`;

        try {
            const response = await fetch('/api/swarm/formation/line', { method: 'POST' });
            const result = await response.json();

            if (!response.ok) {
                throw new Error(result.detail || 'Failed to trigger formation');
            }

            showToast('Line formation command sent to coordinator!');
            fetchSwarmStatus();
        } catch (error) {
            showToast(`Error: ${error.message}`, true);
        } finally {
            btnLineFormation.disabled = false;
            btnLineFormation.innerHTML = `<span class="material-symbols-outlined">grid_view</span> Create Line Formation`;
        }
    });

    // Initial fetch and 1.5s interval polling
    fetchSwarmStatus();
    setInterval(fetchSwarmStatus, 1500);
});
