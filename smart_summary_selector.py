"""
Intelligent automatic decision system for LLM-generated summaries.
Calculates a "Documentation Quality Score" for each function and decides
whether it needs an LLM-generated summary.
"""

import re
from typing import Dict, List

class SmartSummarySelector:
    """
    Automatically decides which functions need LLM-generated summaries
    based on multiple criteria.
    """

    # Configuration
    THRESHOLD = 40  # Functions scoring below 40 get LLM summaries

    # Generic function names that need more context
    GENERIC_NAMES = [
        'new', 'create', 'init', 'initialize', 'setup', 'start', 'stop',
        'handle', 'process', 'execute', 'run', 'update', 'get', 'set',
        'parse', 'validate', 'check', 'send', 'receive', 'load', 'save',
        'add', 'remove', 'delete', 'connect', 'disconnect'
    ]

    # Keywords that indicate good documentation
    QUALITY_KEYWORDS = [
        'create', 'validate', 'process', 'handle', 'connect', 'authenticate',
        'hash', 'encrypt', 'decode', 'encode', 'serialize', 'deserialize',
        'register', 'unregister', 'subscribe', 'publish', 'broadcast'
    ]

    def calculate_documentation_score(self, chunk: Dict) -> int:
        """
        Calculate documentation quality score (0-100).
        Higher score = better documented, doesn't need LLM summary.

        Scoring breakdown:
        - Docstring presence: 0-40 points
        - Function name quality: 0-20 points
        - Code complexity: 0-20 points
        - Context availability: 0-20 points
        """
        score = 0

        # 1. DOCSTRING ANALYSIS (0-40 points)
        docstring = chunk.get('docstring', '').strip()
        if docstring:
            # Has docstring - base 20 points
            score += 20

            # Length bonus (longer = more detailed)
            doc_length = len(docstring.split())
            if doc_length > 30:
                score += 20  # Very detailed
            elif doc_length > 15:
                score += 15  # Good detail
            elif doc_length > 5:
                score += 10  # Minimal detail
            else:
                score += 5   # Too short
        else:
            # No docstring at all - 0 points
            score += 0

        # 2. FUNCTION NAME QUALITY (0-20 points)
        name = chunk.get('name', '').lower()

        # Penalty for generic names
        is_generic = any(generic in name for generic in self.GENERIC_NAMES)
        if is_generic:
            score += 5  # Generic name, needs more context
        else:
            score += 15  # Descriptive name

        # Bonus for compound names (e.g., "create_jwt_token")
        underscores = name.count('_')
        camel_case_parts = len(re.findall(r'[A-Z][a-z]*', name))
        if underscores >= 2 or camel_case_parts >= 3:
            score += 5  # Multi-word name = self-documenting

        # 3. CODE COMPLEXITY (0-20 points)
        code = chunk.get('code', '')
        lines = code.split('\n')
        code_lines = [l for l in lines if l.strip() and not l.strip().startswith('//')]

        complexity_score = 20
        if len(code_lines) > 100:
            complexity_score = 5   # Very complex - needs summary
        elif len(code_lines) > 50:
            complexity_score = 10  # Complex - likely needs summary
        elif len(code_lines) > 20:
            complexity_score = 15  # Medium - maybe needs summary
        else:
            complexity_score = 20  # Simple - less critical

        score += complexity_score

        # 4. CONTEXT AVAILABILITY (0-20 points)
        context = chunk.get('context', '').strip()

        # Check if signature has type hints (Rust/TypeScript)
        has_types = '->' in context or ': ' in context
        if has_types:
            score += 10

        # Check if we have parameter information
        if '(' in context and ')' in context:
            params_str = context[context.find('('):context.find(')')+1]
            param_count = params_str.count(',') + (1 if params_str.strip() != '()' else 0)
            if param_count > 3:
                score -= 5  # Many params = complex, needs summary
            else:
                score += 10  # Few params = simpler

        return min(100, max(0, score))  # Clamp to 0-100

    def needs_llm_summary(self, chunk: Dict) -> bool:
        """
        Decide if this chunk needs an LLM-generated summary.
        Returns True if documentation score is below threshold.
        """
        score = self.calculate_documentation_score(chunk)
        return score < self.THRESHOLD

    def analyze_chunk_batch(self, chunks: List[Dict]) -> Dict:
        """
        Analyze a batch of chunks and return statistics.

        Returns:
            {
                'total': int,
                'needs_summary': int,
                'well_documented': int,
                'avg_score': float,
                'chunks_needing_summary': List[Dict]
            }
        """
        results = {
            'total': len(chunks),
            'needs_summary': 0,
            'well_documented': 0,
            'avg_score': 0,
            'chunks_needing_summary': []
        }

        scores = []
        for chunk in chunks:
            score = self.calculate_documentation_score(chunk)
            scores.append(score)

            if score < self.THRESHOLD:
                results['needs_summary'] += 1
                results['chunks_needing_summary'].append({
                    'location': chunk['location'],
                    'name': chunk['name'],
                    'score': score,
                    'has_docstring': bool(chunk.get('docstring', '').strip()),
                    'code_length': len(chunk['code'].split('\n'))
                })
            else:
                results['well_documented'] += 1

        results['avg_score'] = sum(scores) / len(scores) if scores else 0

        return results

    def get_explanation(self, chunk: Dict) -> str:
        """
        Get human-readable explanation of why a function needs/doesn't need summary.
        """
        score = self.calculate_documentation_score(chunk)
        needs_summary = score < self.THRESHOLD

        reasons = []

        # Docstring analysis
        docstring = chunk.get('docstring', '').strip()
        if not docstring:
            reasons.append("❌ No docstring")
        else:
            doc_words = len(docstring.split())
            if doc_words < 5:
                reasons.append(f"⚠️  Short docstring ({doc_words} words)")
            elif doc_words < 15:
                reasons.append(f"✓ Minimal docstring ({doc_words} words)")
            else:
                reasons.append(f"✓✓ Good docstring ({doc_words} words)")

        # Name analysis
        name = chunk.get('name', '').lower()
        is_generic = any(g in name for g in self.GENERIC_NAMES)
        if is_generic:
            reasons.append(f"⚠️  Generic name: '{name}'")
        else:
            reasons.append(f"✓ Descriptive name: '{name}'")

        # Complexity
        lines = len([l for l in chunk['code'].split('\n') if l.strip()])
        if lines > 50:
            reasons.append(f"⚠️  Complex ({lines} lines)")
        elif lines > 20:
            reasons.append(f"→ Medium complexity ({lines} lines)")
        else:
            reasons.append(f"✓ Simple ({lines} lines)")

        decision = "🤖 NEEDS LLM SUMMARY" if needs_summary else "✓ Well documented"

        return f"{decision} (Score: {score}/100)\n  " + "\n  ".join(reasons)


# Example usage
if __name__ == "__main__":
    # Test the selector
    selector = SmartSummarySelector()

    # Example 1: Poorly documented function (should need summary)
    chunk1 = {
        'name': 'new',
        'docstring': '',
        'code': '''pub fn new(email: String, display_name: String, password: &str) -> Result<Self, Box<dyn std::error::Error>> {
    let password_hash = hash(password, DEFAULT_COST)?;
    Ok(Self {
        id: Uuid::new_v4().to_string(),
        email,
        display_name,
        password_hash,
        created_at: Utc::now(),
        is_admin: false,
    })
}''',
        'context': 'pub fn new(email: String, display_name: String, password: &str) -> Result<Self, Box<dyn std::error::Error>>',
        'location': 'database.rs:DatabaseUser::new'
    }

    # Example 2: Well documented function (shouldn't need summary)
    chunk2 = {
        'name': 'validate_jwt',
        'docstring': 'Validates a JWT token and returns the claims. Website feature: Checks if a user is still logged in.',
        'code': '''pub fn validate_jwt(token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(JWT_SECRET),
        &Validation::default(),
    )
    .map(|data| data.claims)
}''',
        'context': 'pub fn validate_jwt(token: &str) -> Result<Claims, jsonwebtoken::errors::Error>',
        'location': 'auth.rs:validate_jwt'
    }

    # Example 3: Complex function with minimal docs (should need summary)
    chunk3 = {
        'name': 'handle_client_message',
        'docstring': 'Handle incoming client message',
        'code': '''async fn handle_client_message(
    message_text: &str,
    device_store: &SharedDeviceStore,
    db: &Arc<DatabaseManager>,
    esp32_manager: &Arc<crate::esp32_manager::Esp32Manager>,
    esp32_discovery: &Arc<tokio::sync::Mutex<crate::esp32_discovery::Esp32Discovery>>,
    uart_connection: &Arc<tokio::sync::Mutex<crate::uart_connection::UartConnection>>,
    user_id: &str,
    display_name: &str,
    client_id: &str,
    tx: &mpsc::UnboundedSender<ServerMessage>,
    registered_devices: &mut Vec<String>,
) -> Result<(), String> {
    // 50+ lines of complex message routing logic
    ''' + '\n    // ...\n' * 50 + '''}''',
        'context': 'async fn handle_client_message(...)',
        'location': 'websocket.rs:handle_client_message'
    }

    print("="*80)
    print("AUTOMATIC LLM SUMMARY DECISION SYSTEM")
    print("="*80)
    print()

    for i, chunk in enumerate([chunk1, chunk2, chunk3], 1):
        print(f"Example {i}: {chunk['location']}")
        print(selector.get_explanation(chunk))
        print()

    print("="*80)
    print("BATCH ANALYSIS")
    print("="*80)

    results = selector.analyze_chunk_batch([chunk1, chunk2, chunk3])
    print(f"Total functions: {results['total']}")
    print(f"Need LLM summary: {results['needs_summary']} ({results['needs_summary']/results['total']*100:.1f}%)")
    print(f"Well documented: {results['well_documented']} ({results['well_documented']/results['total']*100:.1f}%)")
    print(f"Average score: {results['avg_score']:.1f}/100")
    print()

    print("Functions needing LLM summary:")
    for chunk in results['chunks_needing_summary']:
        print(f"  • {chunk['location']} (score: {chunk['score']})")
