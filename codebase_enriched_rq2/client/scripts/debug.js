/**
 * Debug tool for cookie inspection and JWT token validation testing.
 * Displays all browser cookies, validates authentication token via API, and provides utilities
 * to clear cookies or retest token status. Used for debugging authentication issues, cookie problems,
 * session management, and JWT token verification during development.
 *
 * Features:
 * - Display all document cookies in browser
 * - Validate JWT auth token via /api/validate-token endpoint
 * - Clear all browser cookies for testing
 * - Real-time token status checking
 *
 * Keywords: cookie debugging, JWT validation, authentication testing, token verification,
 * session debugging, clear cookies, cookie inspector, auth debugging tool
 */
(async function() {
    const cookieInfo = document.getElementById('cookie-info');
    const tokenValidation = document.getElementById('token-validation');
    const clearBtn = document.getElementById('clear-cookies');
    const testBtn = document.getElementById('test-token');
    
    // Show all cookies
    cookieInfo.textContent = document.cookie || 'Keine Cookies gefunden';
    
    // Test token validation
    try {
        const response = await fetch('/api/validate-token', {
            method: 'GET',
            credentials: 'include'
        });
        tokenValidation.textContent = `Status: ${response.status} (${response.ok ? 'Gültig' : 'Ungültig'})`;
    } catch (error) {
        tokenValidation.textContent = `Fehler: ${error.message}`;
    }
    
    clearBtn.addEventListener('click', function() {
        // Clear all cookies
        document.cookie.split(";").forEach(function(c) { 
            document.cookie = c.replace(/^ +/, "").replace(/=.*/, "=;expires=" + new Date().toUTCString() + ";path=/"); 
        });
        location.reload();
    });
    
    testBtn.addEventListener('click', async function() {
        try {
            const response = await fetch('/api/validate-token', {
                method: 'GET',
                credentials: 'include'
            });
            alert(`Token-Status: ${response.status} (${response.ok ? 'Gültig' : 'Ungültig'})`);
        } catch (error) {
            alert(`Fehler: ${error.message}`);
        }
    });
})();