/**
 * Login form handler for user authentication with JWT token-based session management.
 * Waits for DOM availability in SPA context, handles login form submission, sends credentials
 * to /api/login endpoint, manages authentication cookies (HttpOnly), and redirects authenticated
 * users to home page. Includes loading states, error handling, and automatic page re-rendering.
 *
 * Flow:
 * 1. Waits 100ms for DOM template injection to complete
 * 2. Captures form submit event with email and password
 * 3. Disables submit button during authentication
 * 4. Sends POST request to /api/login with credentials
 * 5. On success: stores JWT cookie, shows success message, redirects to home
 * 6. On failure: displays error message, re-enables form
 *
 * Keywords: user login, authentication form, JWT login, credential submission, login handler,
 * session authentication, cookie-based auth, SPA login, form validation, auth redirect
 */
setTimeout(function() {
    const form = document.getElementById('login-form');
    const messageDiv = document.getElementById('auth-message');
    
    if (!form || !messageDiv) {
        console.error('Login form elements not found');
        return;
    }
    
    form.addEventListener('submit', async function(e) {
        e.preventDefault();
        
        const email = document.getElementById('email').value;
        const password = document.getElementById('password').value;
        const submitButton = form.querySelector('button[type="submit"]');
        
        // Disable submit button
        submitButton.disabled = true;
        submitButton.textContent = 'Wird eingeloggt...';
        
        try {
            const response = await fetch('/api/login', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({ email, password }),
                credentials: 'include'
            });
            
            const data = await response.json();
            
            if (data.success) {
                showMessage(data.message, 'success');
                // Small delay to ensure cookie is set, then trigger page re-render
                setTimeout(() => {
                    // Trigger a page re-render which will handle authentication redirect automatically
                    if (window.renderPage) {
                        window.renderPage();
                    } else {
                        window.location.reload();
                    }
                }, 500);
            } else {
                showMessage(data.message, 'error');
            }
        } catch (error) {
            showMessage('Ein Fehler ist aufgetreten. Bitte versuchen Sie es erneut.', 'error');
        } finally {
            // Re-enable submit button
            submitButton.disabled = false;
            submitButton.textContent = 'Einloggen';
        }
    });
    
    /**
     * Displays authentication feedback message to user with styling.
     * Shows success or error messages in the auth-message div with appropriate CSS classes
     * for visual distinction. Used for login success, authentication errors, network failures.
     *
     * @param {string} message - Message text to display to user
     * @param {string} type - Message type: 'success' (green) or 'error' (red)
     *
     * Keywords: show message, display feedback, authentication message, login error,
     * success notification, user feedback, form validation message
     */
    function showMessage(message, type) {
        messageDiv.textContent = message;
        messageDiv.className = `auth-message ${type}`;
        messageDiv.style.display = 'block';
    }
    
}, 100); // Wait 100ms for DOM to be ready