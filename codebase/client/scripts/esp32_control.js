(function() {
    'use strict';
    
    // Local state for this script execution
    let esp32Websocket = null;
    let esp32Devices = new Map();
    let availableDevices = []; // All discovered devices
    let openTabs = new Set(); // Currently open device tabs
    let currentUser = null;
    let pendingVariableSends = new Set(); // Track which variables are being sent
    let monitorScrollStates = new Map(); // Track auto-scroll state per monitor

// Get device ID from URL parameter
function getDeviceIdFromUrl() {
    const pathParts = window.location.pathname.split('/');
    if (pathParts[1] === 'devices' && pathParts[2]) {
        return pathParts[2];
    }
    return null;
}

// Initialize page immediately (SPA context)
(async function() {
    await initializeAuth();
    await loadAvailableDevices();
    await initializeWebSocket();

    // Check if there's a device ID in URL and open it automatically
    const urlDeviceId = getDeviceIdFromUrl();
    if (urlDeviceId) {
        // Wait a bit for WebSocket to be ready
        setTimeout(() => {
            addDeviceTab(urlDeviceId);
        }, 500);
    }
})();

async function initializeAuth() {
    try {
        const response = await fetch('/api/user-info', {
            credentials: 'include'
        });

        if (response.ok) {
            currentUser = await response.json();
            // User info is now handled by shared navigation in app.js
        } else {
            // Authentication is optional, continue as guest user
            currentUser = {
                success: true,
                authenticated: false,
                user_id: "guest",
                display_name: "Guest User",
                canvas_permissions: {}
            };
        }
    } catch (error) {
        console.error('Auth initialization failed, continuing as guest:', error);
        // Authentication is optional, continue as guest user
        currentUser = {
            success: true,
            authenticated: false,
            user_id: "guest",
            display_name: "Guest User",
            canvas_permissions: {}
        };
    }
}

async function loadAvailableDevices() {
    try {
        const response = await fetch('/api/esp32/discovered', {
            method: 'GET',
            credentials: 'include'
        });

        if (response.ok) {
            const data = await response.json();
            availableDevices = data.devices || [];
            console.log('Loaded available devices:', availableDevices);
            renderDeviceSidebar();

            // Register all devices with 'light' subscription for connection status
            // This will be called after WebSocket is connected
            if (esp32Websocket && esp32Websocket.readyState === WebSocket.OPEN) {
                registerAllDevicesLight();
            }
        } else {
            console.error('Failed to fetch available devices:', response.status);
            availableDevices = [];
        }
    } catch (error) {
        console.error('Error loading available devices:', error);
        availableDevices = [];
    }
}

async function initializeWebSocket() {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = `${protocol}//${window.location.host}/channel`;
    
    try {
        esp32Websocket = new WebSocket(wsUrl);
        
        esp32Websocket.onopen = function() {
            console.log('WebSocket connected');
            // Request list of ESP32 devices
            requestDeviceList();
            // Register all available devices with 'light' subscription for status updates
            registerAllDevicesLight();
        };
        
        esp32Websocket.onmessage = function(event) {
            handleWebSocketMessage(JSON.parse(event.data));
        };
        
        esp32Websocket.onclose = function() {
            console.log('WebSocket disconnected');
            setTimeout(initializeWebSocket, 3000); // Reconnect after 3s
        };
        
        esp32Websocket.onerror = function(error) {
            console.error('WebSocket error:', error);
        };
        
    } catch (error) {
        console.error('WebSocket initialization failed:', error);
        document.getElementById('loading-state').innerHTML = `
            <div class="alert alert-danger">
                <h4>Connection Failed</h4>
                <p>Could not connect to ESP32 service.</p>
                <button class="btn btn-primary" onclick="initializeWebSocket()">Retry</button>
            </div>
        `;
    }
}

async function requestDeviceList() {
    // Show the main layout with sidebar
    if (availableDevices.length > 0) {
        showMainLayout();
    } else {
        showNoDevicesState();
    }
}

function renderDeviceSidebar() {
    const sidebarList = document.getElementById('available-devices-list');
    if (!sidebarList) return;

    if (availableDevices.length === 0) {
        sidebarList.innerHTML = '<p class="text-muted text-center">No devices found</p>';
        return;
    }

    sidebarList.innerHTML = '';
    availableDevices.forEach(device => {
        const deviceItem = document.createElement('div');
        deviceItem.className = 'device-list-item';
        if (openTabs.has(device.deviceId)) {
            deviceItem.classList.add('active');
        }

        const isConnected = esp32Devices.has(device.deviceId) && esp32Devices.get(device.deviceId).connected;

        deviceItem.innerHTML = `
            <span class="status-dot ${getStatusClass(isConnected)}"></span>
            <div class="device-info">
                <div class="device-name">${device.mdnsHostname || device.deviceId}</div>
                <div class="device-mac">${device.macAddress || device.deviceId}</div>
            </div>
        `;

        deviceItem.onclick = () => addDeviceTab(device.deviceId);
        sidebarList.appendChild(deviceItem);
    });
}

async function addDeviceTab(deviceId) {
    if (openTabs.has(deviceId)) {
        // Tab already open, remove it (toggle behavior)
        removeDeviceTab(deviceId);
        return;
    }

    // Add to open tabs
    openTabs.add(deviceId);

    // Update sidebar to show active state
    renderDeviceSidebar();

    // Create device UI if not exists BEFORE registering
    // This ensures the DOM elements exist when events arrive
    if (!esp32Devices.has(deviceId)) {
        await createDeviceUI(deviceId);
    } else {
        // Device already exists, just render
        renderDevices();
    }

    // Upgrade to 'full' subscription for this device (replaces any existing light subscription)
    registerForDevice(deviceId, 'full');

    // Update URL to reflect the newly opened device
    updateUrlForDevice(deviceId);
}

function removeDeviceTab(deviceId) {
    // Remove from open tabs
    openTabs.delete(deviceId);

    // Update sidebar
    renderDeviceSidebar();

    // Render devices to remove the tab
    renderDevices();

    // Update URL to the first remaining tab or stay on /devices
    if (openTabs.size > 0) {
        const firstTab = Array.from(openTabs)[0];
        updateUrlForDevice(firstTab);
    } else {
        // No tabs left, stay on devices page without specific device
        window.history.pushState({}, '', '/devices');
    }

    // Downgrade to 'light' subscription for this device (only connection status)
    // This way we still see the connection status in the sidebar without full event stream
    registerForDevice(deviceId, 'light');
}

function updateUrlForDevice(deviceId) {
    const newUrl = `/devices/${deviceId}`;
    window.history.pushState({deviceId: deviceId}, '', newUrl);
}

function showMainLayout() {
    document.getElementById('loading-state').style.display = 'none';
    document.getElementById('no-devices-state').style.display = 'none';
    document.getElementById('esp32-main-layout').style.display = 'flex';
}

function toggleSidebar() {
    const sidebar = document.getElementById('esp32-sidebar');
    const overlay = document.getElementById('sidebar-overlay');
    sidebar.classList.toggle('show');
    overlay.classList.toggle('show');
}

function closeSidebar() {
    const sidebar = document.getElementById('esp32-sidebar');
    const overlay = document.getElementById('sidebar-overlay');
    sidebar.classList.remove('show');
    overlay.classList.remove('show');
}

// Expose new functions to global scope
window.toggleSidebar = toggleSidebar;
window.closeSidebar = closeSidebar;
window.addDeviceTab = addDeviceTab;
window.removeDeviceTab = removeDeviceTab;
window.updateUrlForDevice = updateUrlForDevice;
window.switchToTab = switchToTab;

function registerForDevice(deviceId, subscriptionType = 'full') {
    console.log(`Attempting to register for device: ${deviceId} with subscription: ${subscriptionType}`);
    if (esp32Websocket && esp32Websocket.readyState === WebSocket.OPEN) {
        console.log('WebSocket is open, sending registration request');
        esp32Websocket.send(JSON.stringify({
            type: 'registerForDevice',
            deviceId: deviceId,
            subscriptionType: subscriptionType
        }));
    } else {
        console.error('WebSocket not ready, readyState:', esp32Websocket?.readyState);
    }
}

// Register all available devices with 'light' subscription for connection status only
function registerAllDevicesLight() {
    console.log('Registering all devices with light subscription for status updates');
    console.log('Available devices:', availableDevices);
    console.log('Open tabs:', Array.from(openTabs));

    availableDevices.forEach(device => {
        // Only register devices that are not already opened in tabs
        if (!openTabs.has(device.deviceId)) {
            console.log('Registering device with light subscription:', device.deviceId);

            // Create full device object if it doesn't exist yet
            // This ensures connection status updates can be received and displayed
            // We need the full object structure because when user opens tab later,
            // it will reuse this object instead of creating a new one
            if (!esp32Devices.has(device.deviceId)) {
                const deviceName = device.mdnsHostname || device.deviceId;
                esp32Devices.set(device.deviceId, {
                    id: device.deviceId,
                    name: deviceName,
                    connected: false,
                    users: [],
                    udpMessages: [],
                    tcpMessages: [],
                    variables: new Map(),
                    startOptions: [],
                    changeableVariables: []
                });
                console.log('Created device object for light subscription:', device.deviceId);
            }

            registerForDevice(device.deviceId, 'light');
        } else {
            console.log('Skipping device (already in tab):', device.deviceId);
        }
    });
}

async function handleWebSocketMessage(message) {
    if (message.deviceId && message.eventsForDevice) {
        await handleDeviceEvents(message.deviceId, message.eventsForDevice);
    } else {
    }
}

async function handleDeviceEvents(deviceId, events) {
    // Ensure device exists in our UI
    if (!esp32Devices.has(deviceId)) {
        await createDeviceUI(deviceId);
    }

    events.forEach(event => {
        processDeviceEvent(deviceId, event);
    });
}

// Get device display name (mDNS hostname if available, fallback to formatted deviceId)
async function getDeviceDisplayName(deviceId) {
    try {
        const response = await fetch('/api/esp32/discovered', {
            method: 'GET',
            credentials: 'include'
        });

        if (response.ok) {
            const data = await response.json();
            const device = data.devices?.find(d => d.deviceId === deviceId);

            if (device && device.mdnsHostname) {
                return device.mdnsHostname;
            }
        }
    } catch (error) {
        console.warn('Could not fetch device display name from API:', error);
    }

    // Fallback to formatted deviceId
    let deviceName = deviceId;
    if (deviceId.startsWith('esp32-')) {
        deviceName = deviceId.replace('esp32-', 'ESP32 ').replace(/-/g, ' ').toUpperCase();
    } else {
        deviceName = deviceId.replace('test-', '').replace(/-/g, ' ').toUpperCase();
    }
    return deviceName;
}

async function createDeviceUI(deviceId) {
    // Try to get the mDNS hostname from discovered devices API
    let deviceName = await getDeviceDisplayName(deviceId);

    const device = {
        id: deviceId,
        name: deviceName,
        connected: false,
        users: [],
        udpMessages: [],
        tcpMessages: [],
        variables: new Map(),
        startOptions: [],
        changeableVariables: [] // Store changeable variables for re-rendering
    };

    esp32Devices.set(deviceId, device);

    // Update sidebar to reflect connection status
    renderDeviceSidebar();

    // Only render if this device is in openTabs
    if (openTabs.has(deviceId)) {
        renderDevices();
    }
}

function processDeviceEvent(deviceId, event) {
    const device = esp32Devices.get(deviceId);
    if (!device) {
        return;
    }

    // Handle new server event format (tagged enum)
    let eventType = null;
    let eventData = null;

    if (event.esp32ConnectionStatus) {
        eventType = 'esp32ConnectionStatus';
        eventData = event.esp32ConnectionStatus;
    } else if (event.esp32UdpBroadcast) {
        eventType = 'esp32UdpBroadcast';
        eventData = event.esp32UdpBroadcast;
    } else if (event.esp32VariableUpdate) {
        eventType = 'esp32VariableUpdate';
        eventData = event.esp32VariableUpdate;
    } else if (event.esp32StartOptions) {
        eventType = 'esp32StartOptions';
        eventData = event.esp32StartOptions;
    } else if (event.event === 'esp32StartOptions') {
        eventType = 'esp32StartOptions';
        eventData = event;
    } else if (event.esp32ChangeableVariables) {
        eventType = 'esp32ChangeableVariables';
        eventData = event.esp32ChangeableVariables;
    } else if (event.event === 'esp32ChangeableVariables') {
        eventType = 'esp32ChangeableVariables';
        eventData = event;
    } else if (event.event === 'esp32ConnectionStatus') {
        eventType = 'esp32ConnectionStatus';
        eventData = event;
    } else if (event.userJoined) {
        eventType = 'userJoined';
        eventData = event.userJoined;
    } else if (event.userLeft) {
        eventType = 'userLeft';
        eventData = event.userLeft;
    } else if (event.event) {
        // Legacy format support
        eventType = event.event;
        eventData = event;
    } else {
        console.log('Unknown ESP32 event format:', event);
        return;
    }

    switch (eventType) {
        case 'esp32ConnectionStatus':
            const wasDisconnected = !device.connected;
            device.connected = eventData.connected;
            updateConnectionStatus(deviceId, eventData.connected);
            // Bei Disconnect alle pending Variable Sends löschen und Controls sperren
            if (!eventData.connected) {
                clearPendingVariableSendsForDevice(deviceId);
            }
            // Bei Connect: Auto-Start prüfen
            if (eventData.connected && wasDisconnected) {
                handleAutoStart(deviceId);
            }
            break;

        case 'esp32UdpBroadcast':
            const newMessage = `[${new Date().toLocaleTimeString()}] ${eventData.message}`;
            device.udpMessages.push(newMessage);
            // Note: Backend now handles message limiting per device (configurable in settings)
            appendToMonitor(deviceId, newMessage);
            break;

        case 'esp32VariableUpdate':
            device.variables.set(eventData.variableName, eventData.variableValue);

            // Extract min/max if present
            const min = eventData.min !== undefined ? eventData.min : null;
            const max = eventData.max !== undefined ? eventData.max : null;
            updateVariableMonitor(deviceId, eventData.variableName, eventData.variableValue, min, max);

            // Nur bei gesendeten Variablen das Textfeld reaktivieren
            const variableKey = `${deviceId}-${eventData.variableName}`;
            if (pendingVariableSends.has(variableKey)) {
                pendingVariableSends.delete(variableKey);
                reactivateVariableInput(deviceId, eventData.variableName, eventData.variableValue);
            }
            break;

        case 'esp32StartOptions':
            device.startOptions = eventData.options;
            updateStartOptions(deviceId, eventData.options);
            break;

        case 'esp32ChangeableVariables':
            // Store the changeable variables in the device object
            device.changeableVariables = eventData.variables;
            updateVariableControls(deviceId, eventData.variables);

            // Update variable monitor with min/max info if available
            eventData.variables.forEach(variable => {
                const min = variable.min !== undefined ? variable.min : null;
                const max = variable.max !== undefined ? variable.max : null;
                updateVariableMonitor(deviceId, variable.name, variable.value, min, max);
            });
            break;

        case 'userJoined':
            if (eventData.userId !== 'ESP32_SYSTEM') {
                device.users.push({
                    userId: eventData.userId,
                    displayName: eventData.displayName,
                    userColor: eventData.userColor
                });
                updateDeviceUsers(deviceId);
            }
            break;

        case 'userLeft':
            if (eventData.userId !== 'ESP32_SYSTEM') {
                device.users = device.users.filter(u => u.userId !== eventData.userId);
                updateDeviceUsers(deviceId);
            }
            break;

        default:
            console.log('Unknown ESP32 event type:', eventType, eventData);
    }
}

function renderDevices() {
    // Clear existing content
    document.getElementById('deviceTabs').innerHTML = '';
    document.getElementById('deviceTabContent').innerHTML = '';
    document.getElementById('esp32-stack').innerHTML = '';

    if (openTabs.size === 0) {
        // Show empty state in tab area
        document.getElementById('deviceTabContent').innerHTML = `
            <div class="text-center p-5">
                <i class="bi bi-arrow-left" style="font-size: 3rem; color: #667eea;"></i>
                <h5 class="mt-3">No devices selected</h5>
                <p class="text-muted">Select a device from the sidebar to start</p>
            </div>
        `;
        return;
    }

    const devicesToShow = Array.from(openTabs)
        .map(deviceId => esp32Devices.get(deviceId))
        .filter(device => device !== undefined);

    devicesToShow.forEach((device, index) => {
        createDeviceTabContent(device, index === 0);
        createDeviceStackContent(device);
    });

    showDevicesContainer();

    // Restore variable controls and start options after re-rendering
    setTimeout(() => {
        devicesToShow.forEach(device => {
            // Restore variable controls if they exist
            if (device.changeableVariables && device.changeableVariables.length > 0) {
                updateVariableControls(device.id, device.changeableVariables);
            }

            // Restore start options if they exist
            if (device.startOptions && device.startOptions.length > 0) {
                updateStartOptions(device.id, device.startOptions);
            }
        });
    }, 50);
}

function createDeviceTabContent(device, isActive) {
    // Create tab
    const tab = document.createElement('li');
    tab.className = 'nav-item';
    tab.innerHTML = `
        <div class="nav-link ${isActive ? 'active' : ''}" id="${device.id}-tab" role="tab">
            <div class="tab-clickable" onclick="switchToTab('${device.id}')">
                <span class="status-dot ${getStatusClass(device.connected)}"></span>
                <span>${device.name}</span>
            </div>
            <button class="tab-close-btn" onclick="event.stopPropagation(); removeDeviceTab('${device.id}')">
                ×
            </button>
        </div>
    `;
    document.getElementById('deviceTabs').appendChild(tab);

    // Create tab content
    const content = document.createElement('div');
    content.className = `tab-pane ${isActive ? 'active' : ''}`;
    content.id = `${device.id}-content`;
    content.setAttribute('role', 'tabpanel');
    content.innerHTML = createDeviceContent(device, 'tab');
    document.getElementById('deviceTabContent').appendChild(content);
}

function switchToTab(deviceId) {
    // Remove active class from all tabs
    document.querySelectorAll('.nav-link').forEach(tab => {
        tab.classList.remove('active');
    });

    // Remove active class from all tab contents
    document.querySelectorAll('.tab-pane').forEach(content => {
        content.classList.remove('active');
    });

    // Add active class to clicked tab
    const activeTab = document.getElementById(`${deviceId}-tab`);
    if (activeTab) {
        activeTab.classList.add('active');
    }

    // Add active class to corresponding content
    const activeContent = document.getElementById(`${deviceId}-content`);
    if (activeContent) {
        activeContent.classList.add('active');
    }

    // Update URL
    updateUrlForDevice(deviceId);
}

function createDeviceStackContent(device) {
    const stackItem = document.createElement('div');
    stackItem.className = 'esp32-device-card mb-4';
    stackItem.innerHTML = `
        <div class="esp32-device-header">
            <div>
                <h5 class="mb-1">${device.name}</h5>
                <div class="connection-status">
                    <span class="status-dot ${getStatusClass(device.connected)}"></span>
                    ${getStatusText(device.connected)}
                </div>
            </div>
            <div class="device-users" id="${device.id}-stack-users"></div>
        </div>
        <div class="p-3">
            ${createDeviceContent(device, 'stack')}
        </div>
    `;
    document.getElementById('esp32-stack').appendChild(stackItem);
}


function createDeviceContent(device, suffix = '') {
    const idPrefix = suffix ? `${device.id}-${suffix}` : device.id;

    return `
        <div class="device-layout" id="${idPrefix}-layout">
            <div class="main-container" id="${idPrefix}-main">
                <div class="left-panel">
                    <!-- Control Panel -->
                    <div class="start-options-area">
                        <h6><i class="bi bi-play-circle"></i> Device Control</h6>
                        <div class="row align-items-end">
                            <div class="col-md-4">
                                <label class="form-label">Start Option</label>
                                <select class="form-select" id="${idPrefix}-start-select">
                                    <option value="">Select option...</option>
                                </select>
                            </div>
                            <div class="col-md-4">
                                <div class="form-check mb-2">
                                    <input class="form-check-input" type="checkbox" id="${idPrefix}-auto-start">
                                    <label class="form-check-label" for="${idPrefix}-auto-start">Auto Start</label>
                                </div>
                            </div>
                            <div class="col-md-4">
                                <button class="btn btn-success me-2" onclick="sendStartOption('${device.id}')">
                                    <i class="bi bi-play"></i> Start
                                </button>
                                <button class="btn btn-danger" onclick="sendReset('${device.id}')">
                                    <i class="bi bi-arrow-clockwise"></i> Reset
                                </button>
                            </div>
                        </div>
                    </div>

                    <!-- Variable Controls -->
                    <div class="variable-control">
                        <h6><i class="bi bi-sliders"></i> Variable Control</h6>
                        <div id="${idPrefix}-variables">
                            <p class="text-muted">No variables available</p>
                        </div>
                    </div>

                    <!-- Variable Monitor -->
                    <div class="variable-monitor-section">
                        <h6><i class="bi bi-link-45deg"></i> Variable Monitor</h6>
                        <div class="monitor-area" id="${idPrefix}-variable-monitor">
                            <div id="${idPrefix}-variable-monitor-text"></div>
                        </div>
                    </div>

                    <!-- Progress Bar Monitor -->
                    <div class="progress-bar-monitor-section">
                        <h6><i class="bi bi-bar-chart"></i> Progress Monitor</h6>
                        <div id="${idPrefix}-progress-bars">
                            <p class="text-muted">No progress data available</p>
                        </div>
                    </div>
                </div>

                <div class="panel-resizer" id="${idPrefix}-resizer"></div>

                <div class="right-panel">
                    <!-- UDP Monitor -->
                    <div class="udp-monitor-section">
                        <div class="monitor-area" id="${idPrefix}-udp-monitor"></div>
                    </div>
                </div>
            </div>
        </div>
    `;
}

function getStatusClass(connected) {
    return connected ? 'status-connected' : 'status-disconnected';
}

function getStatusText(connected) {
    return connected ? 'Connected' : 'Disconnected';
}

function showNoDevicesState() {
    document.getElementById('loading-state').style.display = 'none';
    document.getElementById('esp32-main-layout').style.display = 'none';
    document.getElementById('no-devices-state').style.display = 'block';
}

function showDevicesContainer() {
    document.getElementById('loading-state').style.display = 'none';
    document.getElementById('no-devices-state').style.display = 'none';

    // Hide all layouts first
    document.getElementById('esp32-tabs').style.display = 'none';
    document.getElementById('esp32-stack').style.display = 'none';

    // Determine layout based on screen dimensions
    const width = window.innerWidth;
    const height = window.innerHeight;
    const aspectRatio = width / height;

    // Layout logic:
    // 1. Very narrow (< 800px width): Stack layout
    // 2. Wide screens or foldables unfolded (>= 800px width): Use tabs with landscape/portrait logic
    // 3. For tabs: aspectRatio > 1.0 OR width > 1400 = landscape, otherwise portrait

    if (width < 800) {
        // Use stack layout for narrow screens (including folded phones)
        document.getElementById('esp32-tabs').style.display = 'none';
        document.getElementById('esp32-stack').style.display = 'block';
    } else {
        // Use tabs layout for wide screens (including unfolded foldables)
        document.getElementById('esp32-tabs').style.display = 'block';
        document.getElementById('esp32-stack').style.display = 'none';

        // Determine landscape vs portrait for tabs
        // Landscape: true aspect ratio landscape OR very wide screens (like unfolded foldables)
        const isLandscape = aspectRatio > 1.0 || width > 1400;
        applyDynamicLayout(isLandscape);
    }
}

function getCurrentActiveLayout() {
    // Determine active layout based on new logic
    const width = window.innerWidth;

    if (width < 800) {
        return 'stack';  // Narrow screens including folded phones
    } else {
        return 'tab';   // Wide screens including unfolded foldables (corrected from 'tabs' to 'tab')
    }
}

function applyDynamicLayout(isLandscape) {
    // Add or remove CSS class based on orientation
    const containers = document.querySelectorAll('.main-container');


    containers.forEach((container, index) => {
        // Force remove both classes first
        container.classList.remove('landscape-layout', 'portrait-layout');

        if (isLandscape) {
            container.classList.add('landscape-layout');
        } else {
            container.classList.add('portrait-layout');
        }

        // Debug: Log current classes
    });

    // Force CSS refresh by triggering a reflow
    containers.forEach(container => {
        container.style.display = 'none';
        container.offsetHeight; // Trigger reflow
        container.style.display = '';
    });

    // Additional debugging: Check if CSS file is loaded
    const cssLinks = document.querySelectorAll('link[href*="esp32_control.css"]');
    console.log(`ESP32 CSS DEBUG: Found ${cssLinks.length} CSS links for esp32_control.css`);

    // Force CSS reload if needed
    if (cssLinks.length > 0) {
        cssLinks.forEach(link => {
            const href = link.href;
            link.href = href + '?v=' + Date.now();
            console.log(`ESP32 CSS DEBUG: Reloaded CSS with cache buster`);
        });
    }
}


function handleAutoStart(deviceId) {
    console.log(`ESP32 DEBUG: handleAutoStart called for device ${deviceId}`);

    // Check if auto-start checkbox is checked
    if (!isAutoStartEnabled(deviceId)) {
        console.log(`ESP32 DEBUG: Auto-start not enabled for device ${deviceId}`);
        return;
    }

    // Get selected start option
    const suffixes = ['tab', 'stack'];
    let selectedValue = null;

    for (const suffix of suffixes) {
        const selectId = `${deviceId}-${suffix}-start-select`;
        const selectEl = document.getElementById(selectId);

        if (selectEl && selectEl.value) {
            selectedValue = selectEl.value;
            console.log(`ESP32 DEBUG: Found selected value '${selectedValue}' for auto-start`);
            break;
        }
    }

    // If no option selected, don't auto-start
    if (!selectedValue) {
        console.log(`ESP32 DEBUG: No start option selected, skipping auto-start`);
        return;
    }

    // Send start option automatically
    console.log(`ESP32 DEBUG: Auto-starting with option '${selectedValue}'`);
    sendStartOptionWithValue(deviceId, selectedValue);
}

function isAutoStartEnabled(deviceId) {
    // Try to find auto-start checkbox from any layout (tab, stack)
    const suffixes = ['tab', 'stack'];

    for (const suffix of suffixes) {
        const checkboxId = `${deviceId}-${suffix}-auto-start`;
        const checkboxEl = document.getElementById(checkboxId);

        if (checkboxEl) {
            return checkboxEl.checked;
        }
    }

    return false;
}

function updateConnectionStatus(deviceId, connected) {
    console.log(`ESP32 DEBUG: updateConnectionStatus called for device ${deviceId} connected: ${connected}`);

    // Update sidebar
    renderDeviceSidebar();

    // Update status dots in tab buttons
    const escapedDeviceId = CSS.escape(deviceId);
    const tabStatusElements = document.querySelectorAll(`[id="${escapedDeviceId}-tab"] .status-dot`);
    console.log(`ESP32 DEBUG: Found ${tabStatusElements.length} tab status dot elements for device ${deviceId}`);
    tabStatusElements.forEach(el => {
        el.className = `status-dot ${getStatusClass(connected)}`;
        console.log(`ESP32 DEBUG: Updated tab status element class to: status-dot ${getStatusClass(connected)}`);
    });

    // Update connection status in tab content (if tab layout exists)
    const tabContentElement = document.getElementById(`${deviceId}-content`);
    if (tabContentElement) {
        const tabContentStatus = tabContentElement.querySelector('.connection-status');
        if (tabContentStatus) {
            tabContentStatus.innerHTML = `<span class="status-dot ${getStatusClass(connected)}"></span> ${getStatusText(connected)}`;
            console.log(`ESP32 DEBUG: Updated tab connection status text to: ${getStatusText(connected)}`);
        }
    }

    // Update connection status in stack layout - find by users div ID
    const stackUsersDiv = document.getElementById(`${deviceId}-stack-users`);
    if (stackUsersDiv) {
        const stackCard = stackUsersDiv.closest('.esp32-device-card');
        if (stackCard) {
            const stackConnectionStatus = stackCard.querySelector('.connection-status');
            if (stackConnectionStatus) {
                stackConnectionStatus.innerHTML = `<span class="status-dot ${getStatusClass(connected)}"></span> ${getStatusText(connected)}`;
                console.log(`ESP32 DEBUG: Updated stack connection status text to: ${getStatusText(connected)}`);
            }
        }
    }

    // Variable Controls auch entsprechend dem Connection Status aktualisieren
    updateVariableControlsConnectionState(deviceId, connected);
}


// Simple append - just add new message to bottom
// Smart append with PlatformIO-style auto-scroll behavior
function appendToMonitor(deviceId, message) {
    const suffixes = ['tab', 'stack'];

    suffixes.forEach(suffix => {
        const monitorId = `${deviceId}-${suffix}-udp-monitor`;
        const monitorEl = document.getElementById(monitorId);

        if (monitorEl) {
            // Initialize scroll listener on first use
            if (!monitorScrollStates.has(monitorId)) {
                initializeMonitorScrollTracking(monitorId, monitorEl);
            }

            // Get scroll state info
            const scrollState = monitorScrollStates.get(monitorId);

            // Remove old messages if limit exceeded
            while (monitorEl.children.length >= 500) {
                monitorEl.removeChild(monitorEl.firstChild);
            }

            // Add new message
            const messageDiv = document.createElement('div');
            messageDiv.textContent = message;
            monitorEl.appendChild(messageDiv);

            // Only auto-scroll if enabled
            if (scrollState.autoScroll) {
                scrollState.isProgrammaticScroll = true;
                monitorEl.scrollTop = monitorEl.scrollHeight;
            }
        }
    });
}

// Initialize scroll tracking for a monitor
function initializeMonitorScrollTracking(monitorId, monitorEl) {
    // Create state object
    const scrollState = {
        autoScroll: true,
        isProgrammaticScroll: false
    };
    monitorScrollStates.set(monitorId, scrollState);

    // Track user scroll events
    monitorEl.addEventListener('scroll', () => {
        // Ignore programmatic scrolls
        if (scrollState.isProgrammaticScroll) {
            scrollState.isProgrammaticScroll = false;
            return;
        }

        // This is a user-initiated scroll
        // Update auto-scroll state based on current position
        const isAtBottom = isScrolledToBottom(monitorEl);
        scrollState.autoScroll = isAtBottom;
    }, { passive: true });
}

// Check if element is scrolled to bottom (with small tolerance)
function isScrolledToBottom(element) {
    const threshold = 30; // pixels tolerance
    const isAtBottom = element.scrollHeight - element.scrollTop - element.clientHeight <= threshold;
    return isAtBottom;
}

function updateVariableMonitor(deviceId, name, value, min = null, max = null) {

    // Update all variable monitor variants (tab, stack)
    const suffixes = ['tab', 'stack'];
    let updated = false;

    suffixes.forEach(suffix => {
        const timestamp = new Date().toLocaleTimeString();
        const variableId = `${deviceId}-${suffix}-variable-${name}`;

        // Variables mit min/max: Progress Bar Monitor
        if (min !== null && max !== null) {
            const progressContainerId = `${deviceId}-${suffix}-progress-bars`;
            const progressContainer = document.getElementById(progressContainerId);

            if (progressContainer) {
                updateProgressBar(progressContainer, name, value, min, max);
                updated = true;
            }
        } else {
            // Variables ohne min/max: Textdarstellung im Text-Bereich
            const textContainerId = `${deviceId}-${suffix}-variable-monitor-text`;
            const textContainer = document.getElementById(textContainerId);

            if (textContainer) {
                let existingDiv = document.getElementById(variableId);
                const content = `[${timestamp}] ${name}: ${value}`;

                if (existingDiv) {
                    // Update existing text
                    existingDiv.textContent = content;
                } else {
                    // Create new text entry
                    existingDiv = document.createElement('div');
                    existingDiv.id = variableId;
                    existingDiv.className = 'variable-text-entry';
                    existingDiv.textContent = content;
                    textContainer.appendChild(existingDiv);
                }
                updated = true;
            }
        }
    });

    if (updated) {
    } else {
    }
}

function updateProgressBar(container, name, value, min, max) {
    const progressBarId = `progress-bar-${name}`;
    let progressItem = container.querySelector(`[data-progress-id="${progressBarId}"]`);

    // Berechne Prozentsatz
    const range = max - min;
    const normalizedValue = value - min;
    const percentage = range > 0 ? (normalizedValue / range) * 100 : 0;
    const clampedPercentage = Math.max(0, Math.min(100, percentage));

    if (!progressItem) {
        // Remove "no data" message if exists
        const noDataMsg = container.querySelector('.text-muted');
        if (noDataMsg) {
            noDataMsg.remove();
        }

        // Create new progress bar item
        progressItem = document.createElement('div');
        progressItem.className = 'progress-bar-item';
        progressItem.setAttribute('data-progress-id', progressBarId);
        progressItem.innerHTML = `
            <div class="progress-bar-container">
                <div class="progress-bar-label">${name}</div>
                <div class="progress-bar-track">
                    <div class="progress-bar-fill" style="width: ${clampedPercentage}%"></div>
                    <div class="progress-bar-values">
                        <span class="progress-min">${min}</span>
                        <span class="progress-value">${value}</span>
                        <span class="progress-max">${max}</span>
                    </div>
                </div>
            </div>
        `;
        container.appendChild(progressItem);
    } else {
        // Update existing progress bar
        const fillEl = progressItem.querySelector('.progress-bar-fill');
        const valueEl = progressItem.querySelector('.progress-value');
        const minEl = progressItem.querySelector('.progress-min');
        const maxEl = progressItem.querySelector('.progress-max');

        if (fillEl) fillEl.style.width = `${clampedPercentage}%`;
        if (valueEl) valueEl.textContent = value;
        if (minEl) minEl.textContent = min;
        if (maxEl) maxEl.textContent = max;
    }
}

function updateStartOptions(deviceId, options) {
    console.log(`ESP32 DEBUG: updateStartOptions called for device ${deviceId} with options:`, options);

    // Update all layout variants (tab, stack)
    const suffixes = ['tab', 'stack'];
    let updated = false;

    suffixes.forEach(suffix => {
        const selectId = `${deviceId}-${suffix}-start-select`;
        const selectEl = document.getElementById(selectId);
        console.log(`ESP32 DEBUG: Element with ID '${selectId}' found:`, selectEl);

        if (selectEl) {
            selectEl.innerHTML = '<option value="">Select option...</option>';
            console.log(`ESP32 DEBUG: Adding ${options.length} options to ${suffix} select`);
            options.forEach(option => {
                const optionEl = document.createElement('option');
                optionEl.value = option;
                optionEl.textContent = option;
                selectEl.appendChild(optionEl);
            });
            console.log(`ESP32 DEBUG: Updated ${suffix} select with options:`, options);
            updated = true;
        }
    });

    if (!updated) {
        console.error(`ESP32 DEBUG: Cannot update start options - no select elements found for device ${deviceId}`);
    } else {
        console.log(`ESP32 DEBUG: Successfully updated start options for device ${deviceId}`);
    }
}

function updateVariableControls(deviceId, variables) {
    // Update all layout variants (tab, stack)
    const suffixes = ['tab', 'stack'];
    let updated = false;

    suffixes.forEach(suffix => {
        const containerId = `${deviceId}-${suffix}-variables`;
        const containerEl = document.getElementById(containerId);
        console.log(`ESP32 DEBUG: Variable container with ID '${containerId}' found:`, containerEl);

        if (containerEl) {
            console.log(`ESP32 DEBUG: Updating ${suffix} variables container with:`, variables);
            updateVariableControlsForContainer(containerEl, variables, deviceId);
            updated = true;
        }
    });

    if (!updated) {
        console.error(`ESP32 DEBUG: Cannot update variable controls - no containers found for device ${deviceId}`);
    }
}

function updateVariableControlsForContainer(containerEl, variables, deviceId) {
    if (variables.length === 0) {
        containerEl.innerHTML = '<p class="text-muted">No variables available</p>';
        return;
    }

    containerEl.innerHTML = '';
    variables.forEach(variable => {
        const variableEl = document.createElement('div');
        variableEl.className = 'variable-item';
        variableEl.innerHTML = `
            <div class="variable-name">${variable.name}</div>
            <div class="variable-input-row">
                <input type="number"
                       class="form-control variable-value"
                       data-variable-name="${variable.name}"
                       data-original-value="${variable.value}"
                       value="${variable.value}"
                       min="0"
                       oninput="handleVariableChange(this, '${deviceId}', '${variable.name}')"
                       onkeypress="handleVariableKeyPress(event, '${deviceId}', '${variable.name}')">
                <button class="btn btn-sm variable-send-btn"
                        data-variable-name="${variable.name}"
                        onclick="sendVariable('${deviceId}', '${variable.name}')">
                    <i class="bi bi-send"></i>
                </button>
            </div>
        `;
        containerEl.appendChild(variableEl);
    });
}

function updateDeviceUsers(deviceId) {
    const device = esp32Devices.get(deviceId);
    ['tabs', 'stack'].forEach(layout => {
        const usersEl = document.getElementById(`${deviceId}-${layout}-users`);
        if (usersEl) {
            if (device.users.length === 0) {
                usersEl.innerHTML = '';
            } else {
                usersEl.innerHTML = device.users.map(user => `
                    <span class="user-indicator" style="background-color: ${user.userColor}"></span>
                    ${user.displayName}
                `).join(', ');
            }
        }
    });
}

// Event handlers
function sendStartOption(deviceId) {
    console.log(`ESP32 DEBUG: sendStartOption called for device ${deviceId}`);

    // Try to find select element from any layout (tab, stack)
    const suffixes = ['tab', 'stack'];
    let selectedValue = null;
    let foundElement = null;

    for (const suffix of suffixes) {
        const selectId = `${deviceId}-${suffix}-start-select`;
        const selectEl = document.getElementById(selectId);
        console.log(`ESP32 DEBUG: Checking ${selectId}, found:`, selectEl);

        if (selectEl && selectEl.value) {
            selectedValue = selectEl.value;
            foundElement = selectEl;
            console.log(`ESP32 DEBUG: Found selected value '${selectedValue}' in ${suffix} layout`);
            break;
        }
    }

    if (foundElement && selectedValue) {
        sendStartOptionWithValue(deviceId, selectedValue);
    } else {
        console.error(`ESP32 DEBUG: Cannot send start option - no element found or no value selected`);
    }
}

function sendStartOptionWithValue(deviceId, startOption) {
    if (esp32Websocket && esp32Websocket.readyState === WebSocket.OPEN) {
        console.log(`ESP32 DEBUG: Sending start option: ${startOption} to device ${deviceId}`);
        esp32Websocket.send(JSON.stringify({
            type: 'deviceEvent',
            deviceId: deviceId,
            eventsForDevice: [{
                event: 'esp32Command',
                deviceId: deviceId,
                command: {
                    startOption: startOption
                }
            }]
        }));
    } else {
        console.error(`ESP32 DEBUG: Cannot send start option - WebSocket not open`);
    }
}

function sendReset(deviceId) {
    if (esp32Websocket) {
        esp32Websocket.send(JSON.stringify({
            type: 'deviceEvent',
            deviceId: deviceId,
            eventsForDevice: [{
                event: 'esp32Command',
                deviceId: deviceId,
                command: {
                    reset: true
                }
            }]
        }));
    }
}

// Expose functions to global scope for HTML onclick handlers
window.sendReset = sendReset;
window.sendStartOption = sendStartOption;
window.sendVariable = sendVariable;
window.handleVariableChange = handleVariableChange;
window.handleVariableKeyPress = handleVariableKeyPress;
window.refreshDevices = refreshDevices;
window.initializeWebSocket = initializeWebSocket;

function sendVariable(deviceId, variableName) {

    // Get the currently active layout based on screen width
    const activeLayout = getCurrentActiveLayout();

    // Try the active layout first
    const activeContainerId = `${deviceId}-${activeLayout}-variables`;
    const activeContainer = document.getElementById(activeContainerId);

    let inputEl = null;
    let buttonEl = null;

    if (activeContainer) {
        inputEl = activeContainer.querySelector(`input[data-variable-name="${variableName}"]`);
        buttonEl = activeContainer.querySelector(`button[data-variable-name="${variableName}"]`);

        if (inputEl && buttonEl) {
        }
    }

    // Fallback: try other layouts if active layout failed
    if (!inputEl || !buttonEl) {
        const fallbackSuffixes = ['tab', 'stack'].filter(s => s !== activeLayout);

        for (const suffix of fallbackSuffixes) {
            const containerId = `${deviceId}-${suffix}-variables`;
            const container = document.getElementById(containerId);

            if (container) {
                inputEl = container.querySelector(`input[data-variable-name="${variableName}"]`);
                buttonEl = container.querySelector(`button[data-variable-name="${variableName}"]`);
                if (inputEl && buttonEl) {
                    break;
                }
            }
        }
    }

    if (inputEl && buttonEl && esp32Websocket) {
        const rawValue = inputEl.value;
        const value = parseInt(rawValue) || 0;

        // Textfeld deaktivieren während des Sendens
        inputEl.disabled = true;
        // Button bleibt rot bis ACK ankommt

        // Variable als "wird gesendet" markieren
        const variableKey = `${deviceId}-${variableName}`;
        pendingVariableSends.add(variableKey);

        const message = {
            type: 'deviceEvent',
            deviceId: deviceId,
            eventsForDevice: [{
                event: 'esp32Command',
                deviceId: deviceId,
                command: {
                    setVariable: {
                        name: variableName,
                        value: value
                    }
                }
            }]
        };

        esp32Websocket.send(JSON.stringify(message));

    } else {
        console.error(`Cannot send variable - inputEl: ${!!inputEl}, buttonEl: ${!!buttonEl}, websocket: ${!!esp32Websocket}`);
    }
}

function handleVariableChange(inputEl, deviceId, variableName) {
    const originalValue = inputEl.getAttribute('data-original-value');
    const currentValue = inputEl.value;

    // Finde den entsprechenden Button
    const variableItem = inputEl.closest('.variable-item');
    const button = variableItem.querySelector(`button[data-variable-name="${variableName}"]`);

    if (currentValue !== originalValue) {
        button.classList.add('changed');
    } else {
        button.classList.remove('changed');
    }
}

function reactivateVariableInput(deviceId, variableName, newValue) {
    // Update ALL layouts to keep them in sync
    const suffixes = ['tab', 'stack'];

    for (const suffix of suffixes) {
        const containerId = `${deviceId}-${suffix}-variables`;
        const container = document.getElementById(containerId);

        if (container) {
            const inputEl = container.querySelector(`input[data-variable-name="${variableName}"]`);
            const buttonEl = container.querySelector(`button[data-variable-name="${variableName}"]`);

            if (inputEl && buttonEl) {
                // Textfeld wieder aktivieren
                inputEl.disabled = false;

                // Wert NICHT ändern - User könnte schon wieder etwas getippt haben
                // Nur original-value aktualisieren damit Button-Status richtig ist
                inputEl.setAttribute('data-original-value', newValue.toString());

                // Button-Status basierend auf aktuellem Wert prüfen
                if (inputEl.value === newValue.toString()) {
                    buttonEl.classList.remove('changed');
                } else {
                    buttonEl.classList.add('changed');
                }
            }
        }
    }
}

function clearPendingVariableSendsForDevice(deviceId) {
    // Alle pending Sends für dieses Device löschen
    const keysToDelete = Array.from(pendingVariableSends).filter(key => key.startsWith(deviceId + '-'));
    keysToDelete.forEach(key => pendingVariableSends.delete(key));

    // Alle Variable Controls für dieses Device sperren und auf blass rot setzen
    updateVariableControlsConnectionState(deviceId, false);
}

function updateVariableControlsConnectionState(deviceId, connected) {
    const suffixes = ['tab', 'stack'];

    for (const suffix of suffixes) {
        const containerId = `${deviceId}-${suffix}-variables`;
        const container = document.getElementById(containerId);

        if (container) {
            const inputElements = container.querySelectorAll('input[data-variable-name]');
            const buttonElements = container.querySelectorAll('button[data-variable-name]');

            inputElements.forEach(input => {
                input.disabled = !connected;
            });

            buttonElements.forEach(button => {
                if (connected) {
                    button.classList.remove('disconnected');
                } else {
                    button.classList.add('disconnected');
                    button.classList.remove('changed');
                }
            });
        }
    }
}

function handleVariableKeyPress(event, deviceId, variableName) {
    if (event.key === 'Enter') {
        sendVariable(deviceId, variableName);
    }
}

function refreshDevices() {
    location.reload();
}


// Handle window resize for responsive layout
window.addEventListener('resize', function() {
    if (esp32Devices.size > 0) {
        showDevicesContainer();
    }
});

// Initialize panel resizers for all devices
function initializePanelResizers() {
    // Find all resizers in the document
    const resizers = document.querySelectorAll('.panel-resizer');

    resizers.forEach(resizer => {
        let isResizing = false;
        let startX = 0;
        let startLeftWidth = 0;
        let leftPanel = null;
        let mainContainer = null;

        const onMouseDown = (e) => {
            isResizing = true;
            startX = e.clientX;

            // Find the parent container and left panel
            mainContainer = resizer.closest('.main-container');
            leftPanel = mainContainer.querySelector('.left-panel');

            if (leftPanel) {
                startLeftWidth = leftPanel.offsetWidth;
            }

            resizer.classList.add('resizing');
            document.body.style.cursor = 'col-resize';
            document.body.style.userSelect = 'none';

            e.preventDefault();
        };

        const onMouseMove = (e) => {
            if (!isResizing || !leftPanel || !mainContainer) return;

            const deltaX = e.clientX - startX;
            const newLeftWidth = startLeftWidth + deltaX;
            const containerWidth = mainContainer.offsetWidth;

            // Calculate percentage
            const newLeftPercent = (newLeftWidth / containerWidth) * 100;

            // Apply min/max constraints (20% to 80%)
            if (newLeftPercent >= 20 && newLeftPercent <= 80) {
                leftPanel.style.flex = `0 0 ${newLeftPercent}%`;
            }

            e.preventDefault();
        };

        const onMouseUp = () => {
            if (isResizing) {
                isResizing = false;
                resizer.classList.remove('resizing');
                document.body.style.cursor = '';
                document.body.style.userSelect = '';
            }
        };

        resizer.addEventListener('mousedown', onMouseDown);
        document.addEventListener('mousemove', onMouseMove);
        document.addEventListener('mouseup', onMouseUp);
    });
}

// Call initializer after devices are rendered
const originalRenderDevices = renderDevices;
renderDevices = function() {
    originalRenderDevices();
    // Initialize resizers after a short delay to ensure DOM is ready
    setTimeout(initializePanelResizers, 100);
};

})(); // End IIFE