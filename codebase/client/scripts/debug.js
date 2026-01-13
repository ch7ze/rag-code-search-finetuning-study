// Execute immediately since DOM is already loaded in SPA context
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