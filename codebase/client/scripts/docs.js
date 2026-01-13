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