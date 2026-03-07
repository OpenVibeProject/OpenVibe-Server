const socketProtocol = (window.location.protocol === 'https:' ? 'wss:' : 'ws:');
const socketUrl = `${socketProtocol}//${window.location.host}/monitor`;
const socket = new WebSocket(socketUrl);

const statusBadge = document.getElementById('connectionStatus');
const masterCountEl = document.getElementById('masterCount');
const slaveCountEl = document.getElementById('slaveCount');
const totalConnectionsEl = document.getElementById('totalConnections');
const dataInEl = document.getElementById('dataIn');
const dataOutEl = document.getElementById('dataOut');
const dataTotalEl = document.getElementById('dataTotal');

// Stats state
let stats = {
    masters: 0,
    slaves: 0,
    bytes_in: 0,
    bytes_out: 0
};

// Vis.js Network
const container = document.getElementById('network');
const nodes = new vis.DataSet([]);
const edges = new vis.DataSet([]);
const data = { nodes, edges };

const options = {
    nodes: {
        shape: 'dot',
        size: 20,
        font: {
            color: '#f8fafc',
            face: 'Outfit'
        },
        borderWidth: 2,
        shadow: true
    },
    edges: {
        width: 2,
        color: { color: 'rgba(56, 189, 248, 0.5)', highlight: '#38bdf8' },
        smooth: {
            type: 'continuous'
        }
    },
    physics: {
        stabilization: false,
        barnesHut: {
            gravitationalConstant: -2000,
            springConstant: 0.04,
            springLength: 95
        }
    },
    interaction: {
        hover: true,
        zoomView: true
    }
};

const network = new vis.Network(container, data, options);

// Helper to generate IDs
function getDeviceId(deviceId) {
    return `DEVICE_${deviceId}`;
}

function formatBytes(bytes) {
    if (bytes === 0) return '0 <span class="unit">B</span>';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    const val = parseFloat((bytes / Math.pow(k, i)).toFixed(2));
    return `${val} <span class="unit">${sizes[i]}</span>`;
}

// Track client nodes to remove correct ones
// Map<DeviceId, Array<{id: NodeId, type: 'Master'|'Slave'}>>
const deviceClients = new Map();

function addDeviceNode(deviceId) {
    const id = getDeviceId(deviceId);
    if (!nodes.get(id)) {
        nodes.add({
            id: id,
            label: `Device\n${deviceId}`,
            shape: 'diamond',
            color: { background: '#1e293b', border: '#94a3b8' }, // Dark slate
            font: { color: '#f8fafc' }, // White text
            size: 25
        });
    }
}

function addClientNode(deviceId, type) {
    const deviceNodeId = getDeviceId(deviceId);

    // Ensure device node exists
    if (!nodes.get(deviceNodeId)) {
        addDeviceNode(deviceId);
    }

    const isMaster = type === 'Master';

    if (!isMaster) {
        // Slave connected: Update the central node to be the Slave
        nodes.update({
            id: deviceNodeId,
            label: `Slave\n${deviceId}`,
            color: { background: '#4ade80', border: '#22c55e' }, // Green
            font: { color: '#f8fafc' }, // White text
            shape: 'diamond',
            size: 30
        });

        // Track the "Slave" property on the Device ID implicitly or explicitly
        // We'll track it in deviceClients as a pseudo-node ID
        const clientNodeId = `SLAVE_ON_DEVICE_${deviceId}`;

        if (!deviceClients.has(deviceId)) {
            deviceClients.set(deviceId, []);
        }
        deviceClients.get(deviceId).push({ id: clientNodeId, type });

        stats.slaves++;
    } else {
        // Master connected: Add separate node orbiting the center
        const clientNodeId = `CLIENT_${deviceId}_${type}_${Math.random().toString(36).substr(2, 9)}`;
        const color = '#38bdf8'; // Blue

        nodes.add({
            id: clientNodeId,
            label: 'M',
            group: type,
            color: { background: color, border: color },
            size: 15
        });

        edges.add({
            from: deviceNodeId,
            to: clientNodeId
        });

        if (!deviceClients.has(deviceId)) {
            deviceClients.set(deviceId, []);
        }
        deviceClients.get(deviceId).push({ id: clientNodeId, type });

        stats.masters++;
    }

    updateStats();
}

function removeClientNode(deviceId, type) {
    const clients = deviceClients.get(deviceId) || [];
    const index = clients.findIndex(c => c.type === type);

    if (index !== -1) {
        const client = clients[index];

        if (type === 'Slave') {
            // Slave disconnected: Revert central node to Device placeholder
            nodes.update({
                id: getDeviceId(deviceId),
                label: `Device\n${deviceId}`,
                color: { background: '#1e293b', border: '#94a3b8' },
                font: { color: '#f8fafc' },
                shape: 'diamond',
                size: 25
            });
            stats.slaves--;
        } else {
            // Master disconnected
            nodes.remove(client.id);
            stats.masters--;
        }

        clients.splice(index, 1);
        updateStats();

        // If no more clients, remove device node? 
        if (clients.length === 0) {
            nodes.remove(getDeviceId(deviceId));
            deviceClients.delete(deviceId);
        }
    }
}

function updateStats() {
    masterCountEl.innerText = stats.masters;
    slaveCountEl.innerText = stats.slaves;
    totalConnectionsEl.innerText = stats.masters + stats.slaves;
    dataInEl.innerHTML = formatBytes(stats.bytes_in);
    dataOutEl.innerHTML = formatBytes(stats.bytes_out);
    dataTotalEl.innerHTML = formatBytes(stats.bytes_in + stats.bytes_out);
}

socket.onopen = () => {
    statusBadge.innerText = '⬤‎‎ ‎‎   Connected';
    statusBadge.classList.add('connected');
    statusBadge.classList.remove('disconnected');
};

socket.onclose = () => {
    statusBadge.innerText = '⬤‎ ‎ ‎‎  Disconnected';
    statusBadge.classList.add('disconnected');
    statusBadge.classList.remove('connected');
};

socket.onmessage = (event) => {
    try {
        const msg = JSON.parse(event.data);
        console.log('Received:', msg);

        switch (msg.type) {
            case 'Init':
                handleInit(msg.payload);
                break;
            case 'ClientConnected':
                handleConnect(msg.payload);
                break;
            case 'ClientDisconnected':
                handleDisconnect(msg.payload);
                break;
            case 'StatsUpdate':
                stats.masters = msg.payload.master_count;
                stats.slaves = msg.payload.slave_count;
                stats.bytes_in = msg.payload.bytes_in;
                stats.bytes_out = msg.payload.bytes_out;
                updateStats();
                break;
        }
    } catch (e) {
        console.error('Error parsing message:', e);
    }
};

function handleInit(payload) {
    // Clear existing
    nodes.clear();
    edges.clear();
    deviceClients.clear();
    stats.masters = 0;
    stats.slaves = 0;
    stats.bytes_in = payload.bytes_in || 0;
    stats.bytes_out = payload.bytes_out || 0;
    updateStats();

    // Payload: { masters: [[deviceId, count], ...], slaves: [[deviceId, count], ...] }

    if (payload.masters) {
        payload.masters.forEach(([deviceId, count]) => {
            for (let i = 0; i < count; i++) addClientNode(deviceId, 'Master');
        });
    }

    if (payload.slaves) {
        payload.slaves.forEach(([deviceId, count]) => {
            for (let i = 0; i < count; i++) addClientNode(deviceId, 'Slave');
        });
    }
}

function handleConnect(payload) {
    // payload: { device_id, client_type }
    addClientNode(payload.device_id, payload.client_type);
}

function handleDisconnect(payload) {
    // payload: { device_id, client_type }
    removeClientNode(payload.device_id, payload.client_type);
}
