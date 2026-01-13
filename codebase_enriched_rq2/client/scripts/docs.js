// Load initial documentation immediately
loadDocumentation('');

// Add click handlers for sidebar navigation
document.querySelectorAll('.doc-link').forEach(link => {
    link.addEventListener('click', function(e) {
        e.preventDefault();
        
        // Remove active class from all links
        document.querySelectorAll('.doc-link').forEach(l => l.classList.remove('active'));
        // Add active class to clicked link
        this.classList.add('active');
        
        // Load documentation
        const docPath = this.getAttribute('data-doc');
        loadDocumentation(docPath);
    });
});

/**
 * Loads and displays documentation content from API endpoint.
 * Fetches HTML from /api/docs or /api/docs/{docPath}, parses response, removes navigation div,
 * and inserts content into #docs-content element. Shows loading message during fetch and error message on failure.
 * @param {string} docPath - Documentation path (empty string for root, specific path for sections)
 */
function loadDocumentation(docPath) {
    const apiUrl = docPath ? `/api/docs/${docPath}` : '/api/docs';
    
    document.getElementById('docs-content').innerHTML = '<p>Laden der Dokumentation...</p>';
    
    fetch(apiUrl)
        .then(response => {
            if (!response.ok) {
                throw new Error(`HTTP ${response.status}: ${response.statusText}`);
            }
            return response.text();
        })
        .then(html => {
            // Extract content from the documentation HTML
            const parser = new DOMParser();
            const doc = parser.parseFromString(html, 'text/html');
            const body = doc.body;
            
            // Remove navigation div (zurück zur dokumentation links)
            const navDiv = body.querySelector('div[style*="margin-bottom: 20px"]');
            if (navDiv) {
                navDiv.remove();
            }
            
            // Insert the content
            document.getElementById('docs-content').innerHTML = body.innerHTML;
        })
        .catch(error => {
            console.error('Documentation load error:', error);
            document.getElementById('docs-content').innerHTML = 
                '<p>Fehler beim Laden der Dokumentation: ' + error.message + '</p>';
        });
}