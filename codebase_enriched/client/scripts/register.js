/**
 * User registration form handler with automatic login and JWT cookie validation.
 * Waits for DOM availability, validates registration form (email, display name, password confirmation),
 * sends registration request to /api/register, polls for HttpOnly authentication cookie readiness,
 * and redirects to home page on successful registration. Includes client-side validation and retry logic.
 *
 * Validation:
 * - Display name: 1-50 characters, non-empty
 * - Password confirmation: must match password field
 * - Email: validated by backend
 *
 * Flow:
 * 1. Validates display name length (1-50 chars)
 * 2. Checks password confirmation match
 * 3. Sends POST /api/register with credentials
 * 4. Waits for auth cookie via polling /api/validate-token (max 10 retries, 300ms interval)
 * 5. Redirects to home page using SPA navigation or fallback URL change
 *
 * Keywords: user registration, signup form, account creation, password validation, display name,
 * automatic login, cookie polling, JWT validation, registration handler, form validation
 */
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
    
    /**
     * Displays registration feedback message with success or error styling.
     * Shows messages in auth-message div with appropriate CSS classes for visual feedback.
     * Used for registration success, validation errors, network failures.
     *
     * @param {string} message - Message text to display to user
     * @param {string} type - Message type: 'success' (green) or 'error' (red)
     *
     * Keywords: show message, display feedback, registration message, validation error,
     * success notification, user feedback, form error message
     */
    function showMessage(message, type) {
        messageDiv.textContent = message;
        messageDiv.className = `auth-message ${type}`;
        messageDiv.style.display = 'block';
    }

    /**
     * Polls server to verify HttpOnly authentication cookie availability after registration.
     * Since HttpOnly cookies cannot be read by JavaScript, polls /api/validate-token endpoint
     * with exponential backoff (300ms intervals) to detect when backend sets auth cookie.
     * Prevents premature redirects before cookie is ready. Max 10 retries = 3 seconds total.
     *
     * Use case: After registration, backend sets HttpOnly auth_token cookie. Frontend needs to
     * wait until cookie is set before redirecting to authenticated pages, otherwise user sees
     * login page again due to missing cookie.
     *
     * @param {number} maxRetries - Maximum polling attempts before giving up (default: 10)
     * @param {number} intervalMs - Milliseconds between retry attempts (default: 300ms)
     * @returns {Promise<boolean>} - True if cookie verified within retries, false if timeout
     *
     * Keywords: cookie polling, HttpOnly cookie, authentication verification, JWT validation,
     * cookie readiness check, async cookie wait, registration cookie, token polling
     */
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