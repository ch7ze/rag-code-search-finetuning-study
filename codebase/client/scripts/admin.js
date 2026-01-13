// Use setTimeout to ensure DOM elements are available after template injection
setTimeout(function() {
    const messageDiv = document.getElementById('admin-message');
    const refreshButton = document.getElementById('refresh-users');
    const searchInput = document.getElementById('user-search');
    const usersTable = document.getElementById('users-table');
    const usersTbody = document.getElementById('users-tbody');
    const totalUsersSpan = document.getElementById('total-users');
    const adminUsersSpan = document.getElementById('admin-users');
    
    let allUsers = [];
    let filteredUsers = [];
    
    // Check if user has admin privileges
    async function checkAdminAccess() {
        try {
            const response = await fetch('/api/user-info', {
                credentials: 'include'
            });
            
            if (!response.ok) {
                throw new Error('Nicht eingeloggt');
            }
            
            const data = await response.json();
            
            // Check if user is admin (from database)
            const adminCheckResponse = await fetch('/api/admin/users', {
                credentials: 'include'
            });
            
            if (adminCheckResponse.status === 403) {
                showMessage('You do not have administrator privileges.', 'error');
                return false;
            } else if (!adminCheckResponse.ok) {
                throw new Error('Admin access could not be verified');
            }
            
            return true;
        } catch (error) {
            showMessage('Error checking authorization: ' + error.message, 'error');
            return false;
        }
    }
    
    // Load users from admin API
    async function loadUsers() {
        if (!(await checkAdminAccess())) {
            return;
        }
        
        refreshButton.disabled = true;
        refreshButton.textContent = 'Loading...';
        
        try {
            const response = await fetch('/api/admin/users', {
                credentials: 'include'
            });
            
            if (!response.ok) {
                throw new Error(`HTTP ${response.status}: ${response.statusText}`);
            }
            
            const data = await response.json();
            
            if (data.success) {
                allUsers = data.users;
                filteredUsers = [...allUsers];
                updateStats();
                renderUsers();
                showMessage('User data loaded successfully.', 'success');
                setTimeout(() => hideMessage(), 3000);
            } else {
                throw new Error(data.message || 'Unbekannter Fehler');
            }
        } catch (error) {
            showMessage('Error loading users: ' + error.message, 'error');
            usersTbody.innerHTML = '<tr><td colspan="5" class="loading">Fehler beim Laden der Daten</td></tr>';
        } finally {
            refreshButton.disabled = false;
            refreshButton.textContent = 'Refresh';
        }
    }
    
    // Update statistics
    function updateStats() {
        const totalUsers = allUsers.length;
        const adminUsers = allUsers.filter(user => user.is_admin).length;
        
        totalUsersSpan.textContent = totalUsers;
        adminUsersSpan.textContent = adminUsers;
    }
    
    // Render users table
    function renderUsers() {
        if (filteredUsers.length === 0) {
            usersTbody.innerHTML = '<tr><td colspan="5" class="loading">Keine Benutzer gefunden</td></tr>';
            return;
        }
        
        const html = filteredUsers.map(user => {
            const createdDate = new Date(user.created_at).toLocaleString('de-DE');
            const adminBadge = user.is_admin ? 
                '<span class="admin-badge yes">Ja</span>' : 
                '<span class="admin-badge no">Nein</span>';
            
            return `
                <tr data-user-id="${user.id}">
                    <td><strong>${escapeHtml(user.display_name)}</strong></td>
                    <td>${adminBadge}</td>
                    <td><span class="date-display">${createdDate}</span></td>
                    <td>
                        <div class="user-actions">
                            <button class="admin-button warning" onclick="toggleAdmin('${user.id}', ${!user.is_admin})">
                                ${user.is_admin ? 'Remove Admin' : 'Make Admin'}
                            </button>
                            <button class="admin-button danger" onclick="deleteUser('${user.id}', '${escapeHtml(user.display_name)}')">
                                Delete
                            </button>
                        </div>
                    </td>
                </tr>
            `;
        }).join('');
        
        usersTbody.innerHTML = html;
    }
    
    // Filter users based on search
    function filterUsers() {
        const query = searchInput.value.toLowerCase().trim();
        
        if (query === '') {
            filteredUsers = [...allUsers];
        } else {
            filteredUsers = allUsers.filter(user => 
                user.display_name.toLowerCase().includes(query)
            );
        }
        
        renderUsers();
    }
    
    // Delete user
    window.deleteUser = async function(userId, userName) {
        if (!confirm(`Do you really want to delete the user "${userName}"? This action cannot be undone.`)) {
            return;
        }
        
        try {
            const response = await fetch(`/api/admin/users/${userId}`, {
                method: 'POST',
                credentials: 'include'
            });
            
            const data = await response.json();
            
            if (data.success) {
                showMessage(`User "${userName}" was successfully deleted.`, 'success');
                loadUsers(); // Reload user list
            } else {
                throw new Error(data.message || 'Unbekannter Fehler');
            }
        } catch (error) {
            showMessage('Error deleting user: ' + error.message, 'error');
        }
    };
    
    // Toggle admin status
    window.toggleAdmin = async function(userId, makeAdmin) {
        const action = makeAdmin ? 'Admin-Rechte verleihen' : 'Admin-Rechte entziehen';
        
        if (!confirm(`Do you really want to ${action} this user?`)) {
            return;
        }
        
        try {
            const response = await fetch(`/api/admin/users/${userId}/admin`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({ is_admin: makeAdmin }),
                credentials: 'include'
            });
            
            const data = await response.json();
            
            if (data.success) {
                showMessage(`${data.message}`, 'success');
                loadUsers(); // Reload user list
            } else {
                throw new Error(data.message || 'Unbekannter Fehler');
            }
        } catch (error) {
            showMessage('Error changing admin status: ' + error.message, 'error');
        }
    };
    
    // Helper functions
    function showMessage(message, type) {
        messageDiv.textContent = message;
        messageDiv.className = `auth-message ${type}`;
        messageDiv.style.display = 'block';
    }
    
    function hideMessage() {
        messageDiv.style.display = 'none';
    }
    
    function escapeHtml(text) {
        const map = {
            '&': '&amp;',
            '<': '&lt;',
            '>': '&gt;',
            '"': '&quot;',
            "'": '&#39;'
        };
        return text.replace(/[&<>"']/g, function(m) { return map[m]; });
    }
    
    // Event listeners
    if (refreshButton) {
        refreshButton.addEventListener('click', loadUsers);
    }
    
    if (searchInput) {
        searchInput.addEventListener('input', filterUsers);
        searchInput.addEventListener('keyup', filterUsers);
    }
    
    // Initial load
    loadUsers();
    
}, 100); // Wait 100ms for DOM to be ready