// Use setTimeout to ensure DOM elements are available after template injection
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
    
    function showMessage(message, type) {
        messageDiv.textContent = message;
        messageDiv.className = `auth-message ${type}`;
        messageDiv.style.display = 'block';
    }
    
}, 100); // Wait 100ms for DOM to be ready