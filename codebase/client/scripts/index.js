// Execute immediately since DOM is already loaded in SPA context
(function() {
    // Load ESP32 devices and canvas list (user info is now handled by shared navigation)
    loadEsp32DevicesList();
    loadCanvasList();
    
    // A 5.4: Canvas Management Event Listeners
    setupCanvasManagement();
    
    // ESP32 Discovery Event Listeners  
    setupEsp32Discovery();
    
    // WebSocket Integration for live ESP32 updates
    setupWebSocketForESP32Discovery();
    
    // Drawing functionality will be initialized on canvas detail pages only
    
    // ============================================================================
    // A 5.4: Canvas Management Functions
    // ============================================================================
    
    async function loadCanvasList() {
        const canvasListElement = document.getElementById('canvas-list');
        canvasListElement.innerHTML = '<div class="loading">Lade Zeichenflächen...</div>';
        
        try {
            const response = await fetch('/api/devices', {
                method: 'GET',
                credentials: 'include'
            });
            
            if (response.ok) {
                const data = await response.json();
                displayCanvasList(data.devices || []);
            } else {
                canvasListElement.innerHTML = '<div class="loading">Fehler beim Laden der Zeichenflächen</div>';
            }
        } catch (error) {
            console.error('Error loading canvas list:', error);
            canvasListElement.innerHTML = '<div class="loading">Fehler beim Laden der Zeichenflächen</div>';
        }
    }
    
    // ESP32 Discovery Functions
    async function loadEsp32DevicesList() {
        const esp32ListElement = document.getElementById('esp32-list');
        esp32ListElement.innerHTML = '<div class="loading">Suche nach ESP32 Geräten...</div>';
        
        try {
            const response = await fetch('/api/esp32/discovered', {
                method: 'GET',
                credentials: 'include'
            });
            
            if (response.ok) {
                const data = await response.json();
                displayEsp32DevicesList(data.devices || []);
            } else {
                esp32ListElement.innerHTML = '<div class="loading">Fehler beim Laden der ESP32 Geräte</div>';
            }
        } catch (error) {
            console.error('Error loading ESP32 devices:', error);
            esp32ListElement.innerHTML = '<div class="loading">Fehler beim Laden der ESP32 Geräte</div>';
        }
    }
    
    function displayEsp32DevicesList(devicesList) {
        const esp32ListElement = document.getElementById('esp32-list');
        
        if (devicesList.length === 0) {
            esp32ListElement.innerHTML = '<div class="loading">Keine ESP32 Geräte gefunden. Stellen Sie sicher, dass Geräte im Netzwerk verfügbar sind.</div>';
            return;
        }
        
        let html = '';
        devicesList.forEach(device => {
            console.log('Device data:', device);
            console.log('MAC Address:', device.macAddress);
            console.log('mDNS Hostname:', device.mdnsHostname);

            // Use mDNS hostname for display name, fallback to deviceId
            const displayName = device.mdnsHostname || device.deviceId;

            html += `
                <div class="esp32-device" data-device-id="${device.deviceId}">
                    <h4>${displayName}</h4>
                    <div class="esp32-device-info">
                        <span><strong>IP:</strong> ${device.deviceIp}</span>
                        <span><strong>TCP Port:</strong> ${device.tcpPort}</span>
                        <span><strong>UDP Port:</strong> ${device.udpPort}</span>
                        <span><strong>MAC:</strong> ${device.macAddress || 'UNDEFINED'}</span>
                    </div>
                    <div class="esp32-actions">
                        <a href="/devices/${device.deviceId}" class="action-button edit-button spa-link">Öffnen</a>
                    </div>
                </div>
            `;
        });
        
        console.log('Setting ESP32 device list HTML...');
        esp32ListElement.innerHTML = html;
        console.log('ESP32 device list HTML set. Device count:', devicesList.length);
    }
    
    function setupEsp32Discovery() {
        // Refresh ESP32 Devices Button
        document.getElementById('refresh-esp32-btn').addEventListener('click', loadEsp32DevicesList);
    }
    
    // WebSocket setup for live ESP32 device discovery
    function setupWebSocketForESP32Discovery() {
        const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        const wsUrl = `${protocol}//${window.location.host}/channel`;
        let websocket = null;
        
        function connectWebSocket() {
            try {
                websocket = new WebSocket(wsUrl);
                
                websocket.onopen = function() {
                    console.log('ESP32 Discovery WebSocket connected');
                    // Register for system events to receive ESP32 discovery
                    websocket.send(JSON.stringify({
                        type: 'registerForDevice',
                        deviceId: 'system'
                    }));
                };
                
                websocket.onmessage = function(event) {
                    try {
                        const message = JSON.parse(event.data);
                        handleESP32DiscoveryMessage(message);
                    } catch (error) {
                        console.error('Error parsing WebSocket message:', error);
                    }
                };
                
                websocket.onclose = function() {
                    console.log('ESP32 Discovery WebSocket disconnected, reconnecting...');
                    setTimeout(connectWebSocket, 3000);
                };
                
                websocket.onerror = function(error) {
                    console.error('ESP32 Discovery WebSocket error:', error);
                };
                
            } catch (error) {
                console.error('Failed to create ESP32 Discovery WebSocket:', error);
                setTimeout(connectWebSocket, 3000);
            }
        }
        
        function handleESP32DiscoveryMessage(message) {
            if (message.deviceId === 'system' && message.eventsForDevice) {
                message.eventsForDevice.forEach(event => {
                    if (event.event === 'esp32DeviceDiscovered') {
                        console.log('New ESP32 device discovered via WebSocket:', event.deviceId);
                        // Reload the device list to show the new device
                        loadEsp32DevicesList();

                        // Show notification
                        showESP32DiscoveryNotification(event.deviceId, event.deviceIp, event.mdnsHostname);
                    }
                });
            }
        }
        
        function showESP32DiscoveryNotification(deviceId, deviceIp, mdnsHostname) {
            // Use mDNS hostname for display name, fallback to deviceId
            const displayName = mdnsHostname || deviceId;

            // Create notification element
            const notification = document.createElement('div');
            notification.style.cssText = `
                position: fixed;
                top: 20px;
                right: 20px;
                background: #d4edda;
                color: #155724;
                border: 1px solid #c3e6c3;
                padding: 12px 16px;
                border-radius: 8px;
                box-shadow: 0 2px 10px rgba(0,0,0,0.1);
                z-index: 1000;
                font-size: 14px;
                max-width: 300px;
                animation: slideInRight 0.3s ease-out;
            `;
            notification.innerHTML = `
                <div style="font-weight: bold; margin-bottom: 4px;">🔍 ESP32 Gerät gefunden!</div>
                <div><strong>${displayName}</strong></div>
                <div style="font-size: 12px; opacity: 0.8;">IP: ${deviceIp}</div>
            `;
            
            // Add animation keyframes if not already present
            if (!document.querySelector('#esp32-notification-styles')) {
                const style = document.createElement('style');
                style.id = 'esp32-notification-styles';
                style.textContent = `
                    @keyframes slideInRight {
                        from {
                            transform: translateX(100%);
                            opacity: 0;
                        }
                        to {
                            transform: translateX(0);
                            opacity: 1;
                        }
                    }
                `;
                document.head.appendChild(style);
            }
            
            document.body.appendChild(notification);
            
            // Auto-remove after 5 seconds
            setTimeout(() => {
                if (notification.parentElement) {
                    notification.style.animation = 'slideInRight 0.3s ease-out reverse';
                    setTimeout(() => notification.remove(), 300);
                }
            }, 5000);
        }
        
        // Start WebSocket connection
        connectWebSocket();
    }
    
    function displayCanvasList(canvasList) {
        const canvasListElement = document.getElementById('canvas-list');
        
        if (canvasList.length === 0) {
            canvasListElement.innerHTML = '<div class="loading">Keine Zeichenflächen gefunden. Erstellen Sie Ihre erste!</div>';
            return;
        }
        
        canvasListElement.innerHTML = canvasList.map(canvas => createCanvasCard(canvas)).join('');
    }
    
    function createCanvasCard(canvas) {
        const permissionNames = {
            'R': 'Read-Only',
            'W': 'Write',
            'V': 'Voice (moderated write)',
            'M': 'Moderator',
            'O': 'Owner'
        };
        
        const permissionName = permissionNames[canvas.your_permission] || canvas.your_permission;
        const canModerate = ['M', 'O'].includes(canvas.your_permission);
        const canEdit = ['W', 'V', 'M', 'O'].includes(canvas.your_permission);
        const canView = ['R', 'W', 'V', 'M', 'O'].includes(canvas.your_permission);
        const isOwner = canvas.your_permission === 'O';
        
        const moderatedBadge = canvas.is_moderated ? '<span class="moderated-badge">MODERIERT</span>' : '';
        
        let actionButtons = '';
        if (canView) {
            const buttonText = canEdit ? 'Öffnen' : 'Anzeigen';
            actionButtons += `<a href="/devices/${canvas.id}" class="action-button edit-button spa-link">${buttonText}</a>`;
        }
        if (canModerate) {
            const toggleText = canvas.is_moderated ? 'Demoderation' : 'Moderieren';
            actionButtons += `<button class="action-button edit-button" onclick="toggleModeration('${canvas.id}', ${!canvas.is_moderated})">${toggleText}</button>`;
        }
        if (isOwner) {
            actionButtons += `<button class="action-button edit-button" onclick="managePermissions('${canvas.id}')">Berechtigungen</button>`;
            actionButtons += `<button class="action-button delete-button" onclick="deleteCanvas('${canvas.id}', '${canvas.name}')">Löschen</button>`;
        }
        
        return `
            <div class="canvas-card">
                <h3>${canvas.name} ${moderatedBadge}</h3>
                <p><strong>Berechtigung:</strong> <span class="permission-badge permission-${canvas.your_permission}">${canvas.your_permission}</span> ${permissionName}</p>
                <p><strong>Erstellt:</strong> ${new Date(canvas.created_at).toLocaleDateString()}</p>
                <div class="canvas-actions">
                    ${actionButtons}
                </div>
            </div>
        `;
    }
    
    function setupCanvasManagement() {
        // Create Canvas Button
        document.getElementById('create-canvas-btn').addEventListener('click', () => {
            document.getElementById('create-canvas-modal').style.display = 'flex';
            document.getElementById('new-canvas-name').focus();
        });
        
        // Refresh Canvas List Button
        document.getElementById('refresh-canvas-btn').addEventListener('click', loadCanvasList);
        
        // Modal Controls
        document.getElementById('cancel-create-canvas').addEventListener('click', () => {
            document.getElementById('create-canvas-modal').style.display = 'none';
            document.getElementById('new-canvas-name').value = '';
        });
        
        document.getElementById('confirm-create-canvas').addEventListener('click', createNewCanvas);
        
        // Allow Enter key in modal
        document.getElementById('new-canvas-name').addEventListener('keypress', (e) => {
            if (e.key === 'Enter') {
                createNewCanvas();
            }
        });
        
        // Close modal on outside click
        document.getElementById('create-canvas-modal').addEventListener('click', (e) => {
            if (e.target === e.currentTarget) {
                document.getElementById('create-canvas-modal').style.display = 'none';
                document.getElementById('new-canvas-name').value = '';
            }
        });
    }
    
    async function createNewCanvas() {
        const nameInput = document.getElementById('new-canvas-name');
        const name = nameInput.value.trim();
        
        if (!name) {
            alert('Bitte geben Sie einen Namen für die Zeichenfläche ein.');
            return;
        }
        
        if (name.length > 100) {
            alert('Der Name darf maximal 100 Zeichen lang sein.');
            return;
        }
        
        try {
            const response = await fetch('/api/devices', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json'
                },
                credentials: 'include',
                body: JSON.stringify({ name })
            });
            
            const data = await response.json();
            
            if (response.ok && data.success) {
                // Close modal
                document.getElementById('create-canvas-modal').style.display = 'none';
                nameInput.value = '';
                
                // Reload canvas list
                loadCanvasList();
            } else {
                alert('Fehler beim Erstellen der Zeichenfläche: ' + data.message);
            }
        } catch (error) {
            console.error('Error creating canvas:', error);
            alert('Fehler beim Erstellen der Zeichenfläche.');
        }
    }
    
    // Global functions for button actions
    
    window.toggleModeration = async function(canvasId, setModerated) {
        try {
            const response = await fetch(`/api/devices/${canvasId}`, {
                method: 'PUT',
                headers: {
                    'Content-Type': 'application/json'
                },
                credentials: 'include',
                body: JSON.stringify({ is_moderated: setModerated })
            });
            
            const data = await response.json();
            
            if (response.ok && data.success) {
                loadCanvasList(); // Reload to show updated status
                const action = setModerated ? 'moderiert' : 'demoderiiert';
                alert(`Zeichenfläche wurde erfolgreich ${action}.`);
            } else {
                alert('Fehler beim Ändern des Moderations-Status: ' + data.message);
            }
        } catch (error) {
            console.error('Error toggling moderation:', error);
            alert('Fehler beim Ändern des Moderations-Status.');
        }
    };
    
    window.managePermissions = function(canvasId) {
        openPermissionModal(canvasId);
    };
    
    window.deleteCanvas = async function(canvasId, canvasName) {
        if (!confirm(`Möchten Sie die Zeichenfläche "${canvasName}" wirklich löschen?\n\nDiese Aktion kann nicht rückgängig gemacht werden und alle zugehörigen Daten gehen verloren.`)) {
            return;
        }
        
        try {
            const response = await fetch(`/api/devices/${canvasId}`, {
                method: 'DELETE',
                credentials: 'include'
            });
            
            const data = await response.json();
            
            if (response.ok && data.success) {
                alert('Zeichenfläche wurde erfolgreich gelöscht.');
                loadCanvasList(); // Reload to remove deleted canvas
            } else {
                alert('Fehler beim Löschen der Zeichenfläche: ' + data.message);
            }
        } catch (error) {
            console.error('Error deleting canvas:', error);
            alert('Fehler beim Löschen der Zeichenfläche.');
        }
    };
    
    // ============================================================================
    // PERMISSION MANAGEMENT FUNCTIONS
    // ============================================================================
    
    let currentCanvasId = null;
    let selectedUserId = null;
    let userSearchTimeout = null;
    let userCache = new Map(); // Cache für User ID -> Display Name Mapping
    
    async function openPermissionModal(canvasId) {
        console.log('Opening permission modal for canvas:', canvasId);
        currentCanvasId = canvasId;
        
        // Modal anzeigen with accessibility setup
        const modal = document.getElementById('permission-modal');
        if (modal) {
            modal.style.display = 'flex';
            const originalFocus = setupModalAccessibility('permission-modal');
            modal.dataset.originalFocus = originalFocus;
            console.log('Permission modal displayed with accessibility features');
        } else {
            console.error('Permission modal element not found!');
            return;
        }
        
        // Canvas-Informationen laden
        await loadCanvasInfoForModal(canvasId);
        
        // Initiale Benutzerliste laden (vor Berechtigungen für userCache)
        await loadInitialUserList();
        
        // Bestehende Berechtigungen laden
        await loadExistingPermissions(canvasId);
        
        // Event Listeners einrichten
        setupPermissionModalEventListeners();
        
        // User search input fokussieren and setup accessibility
        const searchInput = document.getElementById('user-search-input');
        if (searchInput) {
            // Focus with delay to ensure modal is fully rendered
            setTimeout(() => {
                searchInput.focus();
                console.log('Search input focused');
            }, 100);
            
            // Add ARIA attributes for better accessibility
            searchInput.setAttribute('aria-label', 'Benutzer für Berechtigung suchen');
            searchInput.setAttribute('aria-describedby', 'user-search-help');
            
            // Add helper text for screen readers
            if (!document.getElementById('user-search-help')) {
                const helpText = document.createElement('div');
                helpText.id = 'user-search-help';
                helpText.className = 'sr-only';
                helpText.textContent = 'Geben Sie mindestens 2 Zeichen ein, um nach Benutzern zu suchen';
                searchInput.parentElement.appendChild(helpText);
            }
        } else {
            console.error('user-search-input element not found!');
        }
    }
    
    async function loadCanvasInfoForModal(canvasId) {
        try {
            const response = await fetch(`/api/devices/${canvasId}`, {
                method: 'GET',
                credentials: 'include'
            });
            
            if (response.ok) {
                const data = await response.json();
                if (data.success && data.canvas) {
                    const canvas = data.canvas;
                    document.getElementById('permission-modal-canvas-info').innerHTML = `
                        Canvas: <strong>${canvas.name}</strong> (ID: ${canvas.id})
                        ${canvas.is_moderated ? '<span class="moderated-badge">MODERIERT</span>' : ''}
                    `;
                }
            }
        } catch (error) {
            console.error('Error loading canvas info:', error);
        }
    }
    
    async function loadExistingPermissions(canvasId) {
        const permissionsContainer = document.getElementById('existing-permissions');
        permissionsContainer.innerHTML = '<div class="loading-permissions"><span class="loading-spinner"></span>Lade Berechtigungen...</div>';
        
        try {
            const response = await fetch(`/api/devices/${canvasId}`, {
                method: 'GET',
                credentials: 'include'
            });
            
            if (response.ok) {
                const data = await response.json();
                console.log('Canvas API Response:', data);
                
                if (data.success && data.canvas && data.canvas.all_permissions) {
                    console.log('Permissions data structure:', data.canvas.all_permissions);
                    console.log('Type of permissions:', typeof data.canvas.all_permissions);
                    displayExistingPermissions(data.canvas.all_permissions);
                } else {
                    console.log('No permissions found in API response');
                    permissionsContainer.innerHTML = '<div class="loading-permissions">Keine Berechtigungen sichtbar</div>';
                }
            } else {
                console.error('Canvas API Error:', response.status, response.statusText);
                permissionsContainer.innerHTML = '<div class="loading-permissions">Fehler beim Laden der Berechtigungen</div>';
            }
        } catch (error) {
            console.error('Error loading permissions:', error);
            permissionsContainer.innerHTML = '<div class="loading-permissions">Fehler beim Laden der Berechtigungen</div>';
        }
    }
    
    function displayExistingPermissions(permissions) {
        const permissionsContainer = document.getElementById('existing-permissions');
        
        console.log('displayExistingPermissions called with:', permissions);
        console.log('Type of permissions parameter:', typeof permissions);
        
        if (!permissions || permissions.length === 0) {
            permissionsContainer.innerHTML = '<div class="loading-permissions">Keine Berechtigungen vorhanden</div>';
            return;
        }
        
        const permissionNames = {
            'R': 'Read-Only',
            'W': 'Write',
            'V': 'Voice (moderated write)',
            'M': 'Moderator',
            'O': 'Owner'
        };
        
        const html = permissions.map((permissionData) => {
            const userId = permissionData.user_id;
            const permission = permissionData.permission || 'R';
            
            console.log(`Processing userId: ${userId}, permission: ${permission}`);
            
            console.log(`Extracted permission for ${userId}:`, permission);
            
            return `
                <div class="permission-item" data-user-id="${userId}">
                    <div class="permission-user-info">
                        <div class="permission-user-name">${userCache.get(userId) || `User ${userId}`}</div>
                    </div>
                    <div class="permission-level">
                        <span class="permission-badge permission-${permission}">${permission}</span>
                        ${permissionNames[permission] || permission}
                    </div>
                    <div class="permission-item-actions">
                        <button class="action-button edit-button" onclick="editPermission('${userId}', '${permission}')">Ändern</button>
                        <button class="action-button delete-button" onclick="removePermission('${userId}')">Entfernen</button>
                    </div>
                </div>
            `;
        }).join('');
        
        permissionsContainer.innerHTML = html;
        
        // User-Cache wird bereits beim Laden des Modals gefüllt - keine zusätzlichen API-Calls nötig
    }
    
    // loadUserInfoForPermission entfernt - wird durch User-Cache ersetzt
    
    // Lädt die ersten Benutzer beim Öffnen des Modals
    async function loadInitialUserList() {
        const userList = document.getElementById('user-list');
        
        try {
            userList.innerHTML = '<div class="loading">Lade Benutzer...</div>';
            
            const response = await fetch('/api/users/list?limit=20', {
                method: 'GET',
                credentials: 'include'
            });
            
            if (response.ok) {
                const data = await response.json();
                if (data.success && data.users) {
                    // User-Cache füllen
                    data.users.forEach(user => {
                        userCache.set(user.user_id, user.display_name);
                    });
                    await displayUserList(data.users);
                } else {
                    userList.innerHTML = '<div class="no-users">Keine Benutzer gefunden</div>';
                }
            } else {
                userList.innerHTML = '<div class="no-users">Fehler beim Laden der Benutzer</div>';
            }
        } catch (error) {
            console.error('Error loading initial user list:', error);
            userList.innerHTML = '<div class="no-users">Fehler beim Laden der Benutzer</div>';
        }
    }
    
    // Get current user ID from API
    async function getCurrentUserId() {
        try {
            const response = await fetch('/api/user-info', {
                method: 'GET',
                credentials: 'include'
            });
            if (response.ok) {
                const data = await response.json();
                return data.success ? data.user_id : 'guest';
            }
        } catch (error) {
            console.error('Error getting current user ID:', error);
        }
        return 'guest'; // Default to guest user if no authentication
    }

    // Zeigt die Benutzerliste an (ohne aktuellen Benutzer)
    async function displayUserList(users) {
        const userList = document.getElementById('user-list');
        
        // Filter current user out
        const currentUserId = await getCurrentUserId();
        const filteredUsers = currentUserId ? users.filter(user => user.user_id !== currentUserId) : users;
        
        if (filteredUsers.length === 0) {
            userList.innerHTML = '<div class="no-users">Keine anderen Benutzer gefunden</div>';
            return;
        }
        
        const html = filteredUsers.map(user => `
            <div class="user-list-item" data-user-id="${user.user_id}" data-display-name="${user.display_name}">
                <div class="user-name">${user.display_name}</div>
            </div>
        `).join('');
        
        userList.innerHTML = html;
        
        // Event delegation für User-Auswahl
        userList.onclick = (e) => {
            const userItem = e.target.closest('.user-list-item');
            if (userItem) {
                const userId = userItem.dataset.userId;
                const displayName = userItem.dataset.displayName;
                window.selectUser(userId, displayName);
            }
        };
    }
    
    function setupPermissionModalEventListeners() {
        console.log('Setting up permission modal event listeners...');
        
        // Close modal
        const closeBtn = document.getElementById('close-permission-modal');
        if (closeBtn) {
            closeBtn.onclick = closePermissionModal;
            console.log('Close button listener attached');
        } else {
            console.error('close-permission-modal button not found!');
        }
        
        // User search
        const searchInput = document.getElementById('user-search-input');
        if (searchInput) {
            searchInput.oninput = handleUserSearch;
            searchInput.onblur = () => {
                // Delay hiding results to allow click
                setTimeout(() => {
                    const searchResults = document.getElementById('user-search-results');
                    if (searchResults) {
                        searchResults.style.display = 'none';
                    }
                }, 300);
            };
            console.log('Search input listeners attached');
        } else {
            console.error('user-search-input element not found!');
        }
        
        // Grant permission button
        const grantBtn = document.getElementById('grant-permission-btn');
        if (grantBtn) {
            grantBtn.onclick = grantPermission;
            console.log('Grant button listener attached');
        } else {
            console.error('grant-permission-btn button not found!');
        }
        
        // Permission select change handler for explanation
        const permissionSelect = document.getElementById('permission-select');
        if (permissionSelect) {
            permissionSelect.onchange = function() {
                const selectedPermission = this.value;
                const permissionInfo = getPermissionInfo(selectedPermission);
                const explanationElement = document.getElementById('permission-explanation-text');
                if (explanationElement) {
                    explanationElement.textContent = permissionInfo.description;
                }
            };
            // Trigger initial explanation
            permissionSelect.dispatchEvent(new Event('change'));
        }
        
        // Cancel grant button  
        const cancelBtn = document.getElementById('cancel-grant-btn');
        if (cancelBtn) {
            cancelBtn.onclick = cancelPermissionGrant;
            console.log('Cancel button listener attached');
        } else {
            console.error('cancel-grant-btn button not found!');
        }
        
        // Close modal on outside click and keyboard navigation
        const modal = document.getElementById('permission-modal');
        if (modal) {
            modal.onclick = (e) => {
                if (e.target === e.currentTarget) {
                    closePermissionModal();
                }
            };
            
            // Enhanced keyboard navigation
            modal.onkeydown = (e) => {
                if (e.key === 'Escape') {
                    closePermissionModal();
                } else if (e.key === 'Tab') {
                    handleModalTabNavigation(e, modal);
                }
            };
            
            console.log('Modal outside click and keyboard listeners attached');
        } else {
            console.error('permission-modal element not found!');
        }
        
        console.log('Permission modal event listeners setup complete');
    }
    
    function handleUserSearch(e) {
        const query = e.target.value.trim();
        console.log('User search triggered with query:', query);
        
        if (userSearchTimeout) {
            clearTimeout(userSearchTimeout);
        }
        
        if (query.length === 0) {
            // Zeige initiale Benutzerliste
            loadInitialUserList();
            return;
        }
        
        if (query.length < 2) {
            // Query zu kurz, aber nicht leer - zeige "keine Ergebnisse"
            const userList = document.getElementById('user-list');
            userList.innerHTML = '<div class="no-users">Mindestens 2 Zeichen eingeben</div>';
            return;
        }
        
        console.log('Setting timeout for search...');
        userSearchTimeout = setTimeout(() => {
            searchUsers(query);
        }, 300);
    }
    
    async function searchUsers(query) {
        try {
            const response = await fetch(`/api/users/search?q=${encodeURIComponent(query)}`, {
                method: 'GET',
                credentials: 'include'
            });
            
            if (response.ok) {
                const data = await response.json();
                if (data.success) {
                    // User-Cache auch mit Suchergebnissen füllen
                    data.users.forEach(user => {
                        userCache.set(user.user_id, user.display_name);
                    });
                    await displayUserList(data.users);
                } else {
                    const userList = document.getElementById('user-list');
                    userList.innerHTML = '<div class="no-users">Suchfehler: ' + data.message + '</div>';
                }
            } else {
                const userList = document.getElementById('user-list');
                userList.innerHTML = '<div class="no-users">Suchfehler: Server nicht erreichbar</div>';
            }
        } catch (error) {
            console.error('Error searching users:', error);
            const userList = document.getElementById('user-list');
            userList.innerHTML = '<div class="no-users">Suchfehler: ' + error.message + '</div>';
        }
    }

    window.selectUser = function(userId, displayName) {
        console.log('selectUser called with:', { userId, displayName });
        selectedUserId = userId;
        
        // Search results verstecken
        const searchResults = document.getElementById('user-search-results');
        if (searchResults) {
            searchResults.style.display = 'none';
            console.log('Search results hidden');
        } else {
            console.error('user-search-results element not found');
        }
        
        // Grant section anzeigen
        const grantSection = document.getElementById('permission-grant-section');
        if (grantSection) {
            grantSection.style.display = 'block';
            console.log('Grant section shown');
        } else {
            console.error('permission-grant-section element not found');
        }
        
        const userDisplay = document.getElementById('selected-user-display');
        if (userDisplay) {
            userDisplay.textContent = displayName;
            console.log('User display updated');
        } else {
            console.error('selected-user-display element not found');
        }
        
        // Search input leeren
        const searchInput = document.getElementById('user-search-input');
        if (searchInput) {
            searchInput.value = '';
            console.log('Search input cleared');
        } else {
            console.error('user-search-input element not found');
        }
    }
    
    async function grantPermission() {
        if (!selectedUserId || !currentCanvasId) {
            showErrorMessage('Fehler: Kein Benutzer oder Canvas ausgewählt');
            return;
        }
        
        const permission = document.getElementById('permission-select').value;
        const grantButton = document.getElementById('grant-permission-btn');
        
        // Add loading state
        grantButton.classList.add('loading');
        grantButton.disabled = true;
        grantButton.textContent = '⏳ Erteilt...';
        
        try {
            const response = await fetch(`/api/device-permissions/${currentCanvasId}`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json'
                },
                credentials: 'include',
                body: JSON.stringify({ 
                    user_id: selectedUserId,
                    permission: permission
                })
            });
            
            const data = await response.json();
            
            if (response.ok && data.success) {
                // Erfolgsmeldung
                showSuccessMessage(`Berechtigung ${permission} erfolgreich erteilt`);
                
                // Berechtigungsliste neu laden
                await loadExistingPermissions(currentCanvasId);
                
                // Grant section verstecken
                cancelPermissionGrant();
                
                // User cache mit neuem User erweitern falls nötig
                if (!userCache.has(selectedUserId)) {
                    userCache.set(selectedUserId, `User ${selectedUserId}`);
                }
            } else {
                showErrorMessage('Fehler beim Erteilen der Berechtigung: ' + (data.message || 'Unbekannter Fehler'));
            }
        } catch (error) {
            console.error('Error granting permission:', error);
            showErrorMessage('Fehler beim Erteilen der Berechtigung: Netzwerkfehler');
        } finally {
            // Remove loading state
            grantButton.classList.remove('loading');
            grantButton.disabled = false;
            grantButton.textContent = 'Grant Permission';
        }
    }
    
    function cancelPermissionGrant() {
        selectedUserId = null;
        document.getElementById('permission-grant-section').style.display = 'none';
        document.getElementById('user-search-input').value = '';
        console.log('Permission grant cancelled');
    }
    
    function closePermissionModal() {
        const modal = document.getElementById('permission-modal');
        if (modal) {
            modal.style.display = 'none';
            
            // Reset state
            currentCanvasId = null;
            selectedUserId = null;
            userCache.clear();
            
            // Return focus to original element
            const originalFocus = modal.dataset.originalFocus;
            if (originalFocus) {
                const originalElement = document.querySelector(`[data-focus-id="${originalFocus}"]`);
                if (originalElement) {
                    originalElement.focus();
                }
            }
            
            console.log('Permission modal closed');
        }
    }
    
    // Helper functions for permission management
    function getPermissionInfo(permission) {
        const permissionInfos = {
            'R': {
                name: 'Read-Only',
                description: 'Kann Canvas nur anzeigen, nicht bearbeiten'
            },
            'W': {
                name: 'Write',
                description: 'Kann Canvas anzeigen und bearbeiten'
            },
            'V': {
                name: 'Voice',
                description: 'Kann auch in moderierten Canvas zeichnen'
            },
            'M': {
                name: 'Moderator',
                description: 'Kann moderieren und Berechtigungen verwalten'
            },
            'O': {
                name: 'Owner',
                description: 'Vollzugriff auf Canvas und alle Einstellungen'
            }
        };
        
        return permissionInfos[permission] || { name: permission, description: 'Unbekannte Berechtigung' };
    }
    
    function setupModalAccessibility(modalId) {
        // Basic modal accessibility setup
        const modal = document.getElementById(modalId);
        if (modal) {
            modal.setAttribute('role', 'dialog');
            modal.setAttribute('aria-modal', 'true');
            return document.activeElement?.dataset?.focusId || 'unknown';
        }
        return null;
    }
    
    function handleModalTabNavigation(e, modal) {
        // Basic tab trap implementation
        const focusableElements = modal.querySelectorAll(
            'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
        );
        const firstElement = focusableElements[0];
        const lastElement = focusableElements[focusableElements.length - 1];
        
        if (e.shiftKey) {
            if (document.activeElement === firstElement) {
                lastElement.focus();
                e.preventDefault();
            }
        } else {
            if (document.activeElement === lastElement) {
                firstElement.focus();
                e.preventDefault();
            }
        }
    }
    
    function showSuccessMessage(message) {
        // Simple success message implementation
        console.log('Success:', message);
        alert(message); // Replace with better UI notification
    }
    
    function showErrorMessage(message) {
        // Simple error message implementation
        console.error('Error:', message);
        alert(message); // Replace with better UI notification
    }
    
    // Global functions that need to be accessible from HTML onclick handlers
    window.editPermission = function(userId, currentPermission) {
        console.log('editPermission called for user:', userId, 'with permission:', currentPermission);
        // Implementation for editing permissions
        alert('Edit permission functionality not yet implemented');
    };
    
    window.removePermission = async function(userId) {
        if (!confirm('Möchten Sie diese Berechtigung wirklich entfernen?')) {
            return;
        }
        
        try {
            const response = await fetch(`/api/device-permissions/${currentCanvasId}`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json'
                },
                credentials: 'include',
                body: JSON.stringify({ 
                    user_id: userId,
                    permission: 'REMOVE'
                })
            });
            
            const data = await response.json();
            
            if (response.ok && data.success) {
                showSuccessMessage('Berechtigung erfolgreich entfernt');
                await loadExistingPermissions(currentCanvasId);
            } else {
                showErrorMessage('Fehler beim Entfernen der Berechtigung: ' + (data.message || 'Unbekannter Fehler'));
            }
        } catch (error) {
            console.error('Error removing permission:', error);
            showErrorMessage('Fehler beim Entfernen der Berechtigung');
        }
    };
})();