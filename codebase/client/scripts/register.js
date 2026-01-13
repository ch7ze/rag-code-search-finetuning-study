// Use setTimeout to ensure DOM elements are available after template injection
setTimeout(function() {
    const form = document.getElementById('register-form');
    const messageDiv = document.getElementById('auth-message');
    
    if (!form || !messageDiv) {
        console.error('Register form elements not found');
        return;
    }
    
    form.addEventListener('submit', async function(e) {
        e.preventDefault();
        
        const email = document.getElementById('email').value;
        const displayName = document.getElementById('display-name').value;
        const password = document.getElementById('password').value;
        const passwordConfirm = document.getElementById('password-confirm').value;
        const submitButton = form.querySelector('button[type="submit"]');
        
        // Validate display name
        if (!displayName.trim() || displayName.length > 50) {
            showMessage('Anzeigename muss zwischen 1 und 50 Zeichen lang sein.', 'error');
            return;
        }
        
        // Validate password confirmation
        if (password !== passwordConfirm) {
            showMessage('Die Passwörter stimmen nicht überein.', 'error');
            return;
        }
        
        // Disable submit button
        submitButton.disabled = true;
        submitButton.textContent = 'Wird registriert...';
        
        try {
            console.log('Sending registration request:', { email, display_name: displayName, password: '***' });
            const response = await fetch('/api/register', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({ email, display_name: displayName, password }),
            });
            
            console.log('Response status:', response.status);
            const data = await response.json();
            console.log('Response data:', data);
            
            if (data.success) {
                showMessage(data.message + ' Sie werden automatisch eingeloggt.', 'success');
                // Wait for cookie to be available before redirecting
                await waitForAuthenticationCookie();
                // Use SPA navigation instead of direct URL change
                if (window.navigateTo) {
                    window.navigateTo('/');  // A 5.1 requirement: home is at /
                } else {
                    window.location.href = '/';
                }
            } else {
                showMessage(data.message, 'error');
            }
        } catch (error) {
            console.error('Registration error:', error);
            showMessage('Ein Fehler ist aufgetreten: ' + error.message, 'error');
        } finally {
            // Re-enable submit button
            submitButton.disabled = false;
            submitButton.textContent = 'Registrieren';
        }
    });
    
    function showMessage(message, type) {
        messageDiv.textContent = message;
        messageDiv.className = `auth-message ${type}`;
        messageDiv.style.display = 'block';
    }
    
    // Function to wait for authentication cookie to be available
    // With HTTP-Only Cookies we need to ask the server
    async function waitForAuthenticationCookie(maxRetries = 10, intervalMs = 300) {
        for (let i = 0; i < maxRetries; i++) {
            try {
                const response = await fetch('/api/validate-token', {
                    method: 'GET',
                    credentials: 'include'
                });
                if (response.ok) {
                    console.log(`Authentication verified after ${i + 1} attempts`);
                    return true; // Success
                }
            } catch (error) {
                console.log(`Token validation attempt ${i + 1} failed:`, error);
            }
            
            // Wait before next attempt
            if (i < maxRetries - 1) {
                await new Promise(resolve => setTimeout(resolve, intervalMs));
            }
        }
        
        console.warn('Failed to verify authentication after maximum retries');
        showMessage('Registrierung erfolgreich, aber Weiterleitung fehlgeschlagen. Bitte manuell zur Startseite navigieren.', 'error');
        return false;
    }
}, 100); // Wait 100ms for DOM to be ready