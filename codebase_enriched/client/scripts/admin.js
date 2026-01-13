/**
 * Admin panel for user management with search, filtering, and privilege control.
 * Provides administrator interface to view all users, search by display name, toggle admin rights,
 * delete users, and view user statistics. Includes admin access verification, real-time search,
 * and secure API operations with CSRF protection. Waits for DOM template injection in SPA context.
 *
 * Features:
 * - Load and display all users from /api/admin/users
 * - Real-time search filtering by display name
 * - User statistics (total users, admin count)
 * - Toggle admin privileges via /api/admin/users/{id}/admin
 * - Delete users with confirmation via /api/admin/users/{id}
 * - Admin access verification (403 check)
 *
 * Keywords: admin panel, user management, admin privileges, user administration, delete users,
 * toggle admin, user search, admin interface, user statistics, privilege management
 */
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

    /**
     * Verifies current user has administrator privileges by checking API access.
     * Fetches user info from /api/user-info and attempts admin API call to /api/admin/users.
     * Returns false on 403 (Forbidden) or authentication failure. Used as authorization guard
     * before allowing admin operations like user management, privilege changes, or deletions.
     *
     * @returns {Promise<boolean>} - True if user has admin access, false otherwise
     *
     * Keywords: check admin access, verify admin privileges, authorization check, admin verification,
     * permission check, admin guard, access control, privilege verification
     */
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
    
    /**
     * Loads all users from admin API endpoint with authorization check.
     * Fetches complete user list from /api/admin/users, updates local state (allUsers, filteredUsers),
     * renders user table, and updates statistics. Requires admin privileges. Shows loading state
     * on refresh button during fetch. Handles errors with user-friendly messages.
     *
     * @returns {Promise<void>}
     *
     * Keywords: load users, fetch user list, admin API, get all users, user data loading,
     * admin user list, refresh users, load user table
     */
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
    
    /**
     * Calculates and updates user statistics display in admin panel.
     * Counts total users and admin users from allUsers array, updates DOM elements
     * #total-users and #admin-users with current counts. Called after loading users.
     *
     * Keywords: update statistics, user count, admin count, calculate stats, user metrics
     */
    function updateStats() {
        const totalUsers = allUsers.length;
        const adminUsers = allUsers.filter(user => user.is_admin).length;
        
        totalUsersSpan.textContent = totalUsers;
        adminUsersSpan.textContent = adminUsers;
    }
    
    /**
     * Renders filtered user list as HTML table with action buttons.
     * Generates table rows from filteredUsers array with display name, admin badge,
     * creation date, and action buttons (toggle admin, delete). Includes XSS protection
     * via escapeHtml. Shows empty state if no users match filter.
     *
     * Keywords: render users, display user table, user list rendering, admin table,
     * generate user rows, user UI, table rendering
     */
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
    
    /**
     * Filters user list based on search input query in real-time.
     * Case-insensitive search matches against user display names. Updates filteredUsers
     * array and re-renders table. Empty query shows all users. Triggered on input events.
     *
     * Keywords: filter users, search users, user search, search by name, filter list,
     * real-time search, user filtering, search functionality
     */
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
    
    /**
     * Deletes user account with confirmation dialog and admin privileges check.
     * Shows confirmation dialog before deletion. Sends POST request to /api/admin/users/{userId}
     * to permanently delete user. Reloads user list on success. Irreversible operation.
     *
     * @param {string} userId - Unique user ID to delete
     * @param {string} userName - User display name for confirmation message
     * @returns {Promise<void>}
     *
     * Keywords: delete user, remove user, user deletion, permanent delete, admin delete,
     * delete account, remove account, user removal
     */
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
    
    /**
     * Toggles admin privileges for user with confirmation dialog.
     * Grants or revokes administrator rights via POST /api/admin/users/{userId}/admin.
     * Shows confirmation before privilege change. Reloads user list to reflect new status.
     *
     * @param {string} userId - Unique user ID to modify
     * @param {boolean} makeAdmin - True to grant admin rights, false to revoke
     * @returns {Promise<void>}
     *
     * Keywords: toggle admin, grant admin, revoke admin, admin privileges, change privileges,
     * admin rights, make admin, remove admin, privilege management
     */
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
    
    /**
     * Displays admin panel feedback message with success or error styling.
     * Shows messages in admin-message div with appropriate CSS classes for visual feedback.
     *
     * @param {string} message - Message text to display
     * @param {string} type - Message type: 'success' or 'error'
     *
     * Keywords: show message, admin message, display notification, feedback message
     */
    function showMessage(message, type) {
        messageDiv.textContent = message;
        messageDiv.className = `auth-message ${type}`;
        messageDiv.style.display = 'block';
    }

    /**
     * Hides currently displayed admin message.
     * Used to clear messages after timeout or user action.
     *
     * Keywords: hide message, clear notification, dismiss message
     */
    function hideMessage() {
        messageDiv.style.display = 'none';
    }

    /**
     * Escapes HTML special characters to prevent XSS attacks in user-generated content.
     * Converts &, <, >, ", ' to HTML entities for safe display in DOM.
     *
     * @param {string} text - Text containing potential HTML/script
     * @returns {string} - Safely escaped text
     *
     * Keywords: escape HTML, XSS prevention, sanitize input, HTML entities, security,
     * prevent injection, safe HTML
     */
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