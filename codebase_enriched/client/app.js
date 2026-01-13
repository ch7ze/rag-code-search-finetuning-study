// Information about available pages
const pages = {
    'index': {
        title: 'Home',
        template: 'index.html',
        defaultPath: 'index.html',
        scripts: ['index.js'],
        styles: [],
        requiresAuth: false
    },
    'login': {
        title: 'Login',
        template: 'login.html',
        defaultPath: 'login.html',
        scripts: ['login.js'],
        styles: [],
        requiresAuth: false
    },
    'register': {
        title: 'Registrierung',
        template: 'register.html',
        defaultPath: 'register.html',
        scripts: ['register.js'],
        styles: [],
        requiresAuth: false
    },
    'debug': {
        title: 'Debug',
        template: 'debug.html',
        defaultPath: 'debug.html',
        scripts: ['debug.js'],
        styles: [],
        requiresAuth: false
    },
    'esp32_control': {
        title: 'ESP32 Control',
        template: 'esp32_control.html',
        defaultPath: '/devices/:id',
        scripts: ['esp32_control.js'],
        styles: ['esp32_control.css'],
        requiresAuth: false
    },
    'docs': {
        title: 'Dokumentation',
        template: 'docs.html',
        defaultPath: 'docs.html',
        scripts: ['docs.js'],
        styles: [],
        requiresAuth: false
    },
    'admin': {
        title: 'Administrator',
        template: 'admin.html',
        defaultPath: 'admin.html',
        scripts: ['admin.js'],
        styles: [],
        requiresAuth: false
    },
    'settings': {
        title: 'Settings',
        template: 'settings.html',
        defaultPath: 'settings.html',
        scripts: [],
        styles: [],
        requiresAuth: false
    }
};

// Cache templates to avoid repeated fetches
const templateCache = {};

// Function for loading a template and extracting scripts
async function loadTemplate(templateName) {
  // Return cached template if available
  if (templateCache[templateName]) {
    return templateCache[templateName];
  }
  
  try {
    const response = await fetch(`/templates/${templateName}`);
    if (!response.ok) {
      throw new Error(`HTTP error! status: ${response.status}`);
    }
    const template = await response.text();
    
    // Extract and store scripts separately
    const scriptRegex = /<script\b[^<]*(?:(?!<\/script>)<[^<]*)*<\/script>/gi;
    const scripts = template.match(scriptRegex) || [];
    const templateWithoutScripts = template.replace(scriptRegex, '');
    
    // Cache both template and scripts
    templateCache[templateName] = {
      html: templateWithoutScripts,
      scripts: scripts.map(script => {
        // Extract script content between tags
        const scriptContent = script.replace(/<script[^>]*>/, '').replace(/<\/script>/, '');
        return scriptContent;
      })
    };
    
    return templateCache[templateName];
  } catch (error) {
    console.error(`Error loading template ${templateName}:`, error);
    return { html: '<p>Error loading content</p>', scripts: [] };
  }
}

// Authentication utility functions - HTTP-Only Cookie compatible
async function isAuthenticated() {
    // With HTTP-Only cookies we cannot read the cookie directly
    // We need to ask the server if the cookie is valid
    try {
        const response = await fetch('/api/validate-token', {
            method: 'GET',
            credentials: 'include'
        });
        return response.ok;
    } catch (error) {
        console.error('Token validation error:', error);
        return false;
    }
}

// Function to update global navigation based on authentication status
function updateGlobalNavigation(authenticated) {
    const loginLink = document.querySelector('a[href="login.html"]');
    const registerLink = document.querySelector('a[href="register.html"]');
    
    if (loginLink && registerLink) {
        if (authenticated) {
            // Hide login and register links when authenticated
            loginLink.style.display = 'none';
            registerLink.style.display = 'none';
        } else {
            // Show login and register links when not authenticated
            loginLink.style.display = '';
            registerLink.style.display = '';
        }
    }
}

// Function for rendering the page based on the current URL
async function renderPage() {
    console.log('renderPage() called');
    const contentContainer = document.getElementById('content-container');
    
    // Analyze URL to determine the current page
    const url = new URL(window.location.href);
    let path = url.pathname.split('/').pop() || 'index.html';
    
    // Handle root path
    if (url.pathname === '/') {
        path = 'index.html';
    }
    
    let pageName = Object.keys(pages).find(page => 
        pages[page].defaultPath === path) || 'index';
    
    // Handle special URL cases
    if (path === 'login') {
        pageName = 'login';
    } else if (path === 'register') {
        pageName = 'register';
    } else if (path === 'docs') {
        pageName = 'docs';
    } else if (path === 'debug') {
        pageName = 'debug';
    } else if (path === 'admin') {
        pageName = 'admin';
    } else if (path === 'settings') {
        pageName = 'settings';
    }


    // Handle device pages - redirect to ESP32 control
    if (url.pathname.startsWith('/devices')) {
        pageName = 'esp32_control';
    }
    
    // Debug logging
    console.log('SPA Navigation Debug:', {
        pathname: url.pathname,
        path: path,
        pageName: pageName,
        pageExists: !!pages[pageName]
    });
    
    // Page information
    const pageInfo = pages[pageName];
    
    if (!pageInfo) {
        console.error('Page not found:', pageName, 'Available pages:', Object.keys(pages));
        // Fallback to index
        pageName = 'index';
    }
    
    // Get page info (after potential fallback)
    const finalPageInfo = pages[pageName];
    if (!finalPageInfo) {
        console.error('Even fallback page not found!');
        return;
    }
    
    // Check authentication requirements
    const authenticated = await isAuthenticated();
    
    // Since all pages are now accessible without authentication,
    // we no longer redirect to login. Users can access all functionality
    // as guest users.
    
    // Set document title
    document.title = finalPageInfo.title;
    
    // Update global navigation based on authentication status
    updateGlobalNavigation(authenticated);
    
    // Load CSS
    const existingStyles = document.querySelectorAll('link[data-dynamic-style]');
    existingStyles.forEach(style => style.remove());
    
    finalPageInfo.styles.forEach(style => {
        const styleLink = document.createElement('link');
        styleLink.rel = 'stylesheet';
        styleLink.href = `/styles/${style}`;
        styleLink.setAttribute('data-dynamic-style', 'true');
        document.head.appendChild(styleLink);
    });
    
    // Load template and insert into container
    const templateData = await loadTemplate(finalPageInfo.template);
    contentContainer.innerHTML = templateData.html;
    
    // Execute template scripts immediately after DOM injection
    if (templateData.scripts && templateData.scripts.length > 0) {
        templateData.scripts.forEach((scriptContent, index) => {
            try {
                // Create a new Function to execute the script in global scope
                const executeScript = new Function(scriptContent);
                executeScript();
                console.log(`Template script ${index + 1} executed successfully`);
            } catch (error) {
                console.error(`Error executing template script ${index + 1}:`, error);
            }
        });
    }
    
    // Load and execute scripts in sequence to maintain order
    const existingScripts = document.querySelectorAll('script[data-dynamic-script]');
    existingScripts.forEach(script => script.remove());    // Load scripts in sequence
    async function loadScriptsSequentially() {
        for (const scriptSrc of finalPageInfo.scripts) {
            await new Promise((resolve, reject) => {
                const scriptElement = document.createElement('script');
                scriptElement.src = `/scripts/${scriptSrc}`;
                scriptElement.setAttribute('data-dynamic-script', 'true');
                scriptElement.onload = () => resolve();
                scriptElement.onerror = (e) => reject(e);
                document.body.appendChild(scriptElement);
                console.log(`Script ${scriptSrc} loaded`);
            });
        }
    }

    loadScriptsSequentially().catch(err => {
        console.error("Failed to load scripts:", err);
    });
}

// Add this function to handle SPA navigation with reliable canvas cleanup
async function navigateTo(url) {
  console.log(`NavigateTo called: ${window.location.pathname} → ${url}`);
  
  // Canvas cleanup when leaving canvas
  const currentPath = window.location.pathname;
  const currentCanvasMatch = currentPath.match(/^\/canvas\/([^\/]+)$/);
  const newCanvasMatch = url.match(/^\/canvas\/([^\/]+)$/);
  
  const currentCanvasId = currentCanvasMatch ? currentCanvasMatch[1] : null;
  const newCanvasId = newCanvasMatch ? newCanvasMatch[1] : null;
  
  // Synchronous canvas cleanup - wait for server confirmation before navigation
  if (currentCanvasId && (!newCanvasId || newCanvasId !== currentCanvasId)) {
    console.log(`Canvas cleanup needed: ${currentPath} → ${url}`);
    try {
      if (window.unregisterFromCanvas) {
        console.log('Waiting for canvas unregistration to complete...');
        await window.unregisterFromCanvas();
        console.log(`Canvas unregistered: ${currentCanvasId}`);
      } else {
        console.warn('unregisterFromCanvas not available');
      }
    } catch (error) {
      console.error('Canvas unregistration failed:', error);
      // Continue navigation even if cleanup fails to avoid hanging the UI
    }
  }
  
  // Update browser history
  history.pushState(null, null, url);
  // Render the new page
  renderPage();
  
  // SPA user refresh: update user list when navigating to canvas
  if (newCanvasMatch) {
    const targetCanvasId = newCanvasMatch[1];
    console.log('SPA Navigation: Refreshing user list for canvas:', targetCanvasId);
    
    // Check if this is a canvas-to-canvas navigation (different canvas)
    const isCanvasToCanvasNavigation = currentCanvasId && targetCanvasId !== currentCanvasId;
    if (isCanvasToCanvasNavigation) {
      console.log('Canvas-to-Canvas navigation detected:', currentCanvasId, '→', targetCanvasId);
    }
    
    // Give DOM time to render, then refresh user list
    setTimeout(() => {
      if (window.refreshCanvasUsers) {
        window.refreshCanvasUsers(true); // bypassThrottle = true for SPA navigation
        console.log('SPA Navigation: User list refreshed for canvas:', targetCanvasId);
      } else {
        console.warn('SPA Navigation: refreshCanvasUsers not available yet, retrying...');
        // Retry once for edge cases where scripts are still loading
        setTimeout(() => {
          if (window.refreshCanvasUsers) {
            window.refreshCanvasUsers(true);
            console.log('SPA Navigation: User list refreshed (retry)');
          }
        }, 500);
      }
    }, 200); // 200ms delay for DOM rendering
  }
}

// Store the original async function
const _navigateToAsync = navigateTo;

// Make navigateTo and renderPage globally available
// Wrapper for backward compatibility - can be called async or sync
window.navigateTo = (url) => {
  _navigateToAsync(url).catch(error => {
    console.error('Navigation failed:', error);
    renderPage(); // Fallback
  });
};
window.renderPage = renderPage;

// Add event delegation for SPA links with async navigation
document.addEventListener('click', function(e) {
  // Find closest anchor tag
  const link = e.target.closest('a.spa-link');
  if (link) {
    e.preventDefault();
    // Handle async navigation with error handling using original async function
    _navigateToAsync(link.getAttribute('href')).catch(error => {
      console.error('Navigation failed:', error);
      // Fallback: still try to render page even if cleanup failed
      renderPage();
    });
  }
});

// Initially render the page
document.addEventListener('DOMContentLoaded', renderPage);

// For browser back button support
window.addEventListener('popstate', renderPage);