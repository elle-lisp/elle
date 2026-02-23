;; CSS stylesheet generation

(var generate-css
  (fn ()
    "/* Elle Documentation Site Stylesheet */

:root {
  color-scheme: light dark;
  --bg: #ffffff;
  --fg: #1a1a2e;
  --bg-secondary: #f8f9fa;
  --code-bg: #f5f5f5;
  --code-fg: #e83e8c;
  --accent: #6c5ce7;
  --accent-hover: #5a4bd1;
  --border: #e0e0e0;
  --shadow: rgba(0,0,0,0.1);
  --note-info-bg: #e3f2fd;
  --note-info-border: #2196f3;
  --note-warning-bg: #fff3e0;
  --note-warning-border: #ff9800;
  --note-tip-bg: #e8f5e9;
  --note-tip-border: #4caf50;
}

@media (prefers-color-scheme: dark) {
  :root {
    --bg: #1a1a2e;
    --fg: #e0e0e0;
    --bg-secondary: #16213e;
    --code-bg: #2d2d44;
    --code-fg: #f78da7;
    --accent: #a29bfe;
    --accent-hover: #b8b3ff;
    --border: #3d3d5c;
    --shadow: rgba(0,0,0,0.3);
    --note-info-bg: #1a237e;
    --note-info-border: #5c6bc0;
    --note-warning-bg: #3e2723;
    --note-warning-border: #ff8f00;
    --note-tip-bg: #1b5e20;
    --note-tip-border: #66bb6a;
  }
}

* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

html {
  scroll-behavior: smooth;
}

body {
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
  background-color: var(--bg);
  color: var(--fg);
  line-height: 1.6;
  transition: background-color 0.3s ease, color 0.3s ease;
}

/* Layout */
body {
  display: flex;
  min-height: 100vh;
}

.sidebar {
  width: 250px;
  background-color: var(--bg-secondary);
  border-right: 1px solid var(--border);
  padding: 2rem 1rem;
  position: fixed;
  height: 100vh;
  overflow-y: auto;
  transition: background-color 0.3s ease;
}

.site-title {
  font-size: 1.5rem;
  font-weight: bold;
  margin-bottom: 2rem;
  color: var(--accent);
}

.sidebar ul {
  list-style: none;
}

.sidebar li {
  margin-bottom: 0.5rem;
}

.sidebar a {
  display: block;
  padding: 0.5rem 1rem;
  color: var(--fg);
  text-decoration: none;
  border-radius: 4px;
  transition: background-color 0.2s ease, color 0.2s ease;
}

.sidebar a:hover {
  background-color: var(--bg);
  color: var(--accent);
}

.sidebar a.active {
  background-color: var(--accent);
  color: white;
}

.content {
  margin-left: 250px;
  flex: 1;
  padding: 3rem;
  max-width: 900px;
}

/* Typography */
h1 {
  font-size: 2.5rem;
  margin-bottom: 1.5rem;
  margin-top: 0;
  color: var(--accent);
}

h2 {
  font-size: 2rem;
  margin-top: 2rem;
  margin-bottom: 1rem;
  color: var(--accent);
  border-bottom: 2px solid var(--border);
  padding-bottom: 0.5rem;
}

h3 {
  font-size: 1.5rem;
  margin-top: 1.5rem;
  margin-bottom: 0.75rem;
  color: var(--fg);
}

p {
  margin-bottom: 1rem;
}

/* Links */
a {
  color: var(--accent);
  text-decoration: none;
  transition: color 0.2s ease;
}

a:hover {
  color: var(--accent-hover);
  text-decoration: underline;
}

/* Code */
code {
  background-color: var(--code-bg);
  color: var(--code-fg);
  padding: 0.2em 0.4em;
  border-radius: 3px;
  font-family: 'Courier New', Courier, monospace;
  font-size: 0.9em;
}

pre {
  background-color: var(--code-bg);
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 1rem;
  overflow-x: auto;
  margin-bottom: 1rem;
}

pre code {
  background-color: transparent;
  color: var(--fg);
  padding: 0;
  border-radius: 0;
}

/* Lists */
ul, ol {
  margin-left: 2rem;
  margin-bottom: 1rem;
}

li {
  margin-bottom: 0.5rem;
}

/* Blockquotes */
blockquote {
  border-left: 4px solid var(--accent);
  padding-left: 1rem;
  margin-left: 0;
  margin-bottom: 1rem;
  color: var(--fg);
  font-style: italic;
}

/* Tables */
table {
  width: 100%;
  border-collapse: collapse;
  margin-bottom: 1rem;
  border: 1px solid var(--border);
}

thead {
  background-color: var(--bg-secondary);
}

th {
  padding: 0.75rem;
  text-align: left;
  font-weight: bold;
  border-bottom: 2px solid var(--border);
}

td {
  padding: 0.75rem;
  border-bottom: 1px solid var(--border);
}

tbody tr:nth-child(even) {
  background-color: var(--bg-secondary);
}

/* Notes/Callouts */
.note {
  padding: 1rem;
  margin-bottom: 1rem;
  border-left: 4px solid;
  border-radius: 4px;
  background-color: var(--bg-secondary);
}

.note-info {
  border-left-color: var(--note-info-border);
  background-color: var(--note-info-bg);
}

.note-warning {
  border-left-color: var(--note-warning-border);
  background-color: var(--note-warning-bg);
}

.note-tip {
  border-left-color: var(--note-tip-border);
  background-color: var(--note-tip-bg);
}

/* Responsive */
@media (max-width: 768px) {
  body {
    flex-direction: column;
  }
  
  .sidebar {
    width: 100%;
    height: auto;
    position: relative;
    border-right: none;
    border-bottom: 1px solid var(--border);
    padding: 1rem;
  }
  
  .site-title {
    margin-bottom: 1rem;
  }
  
  .content {
    margin-left: 0;
    padding: 1.5rem;
  }
  
  h1 {
    font-size: 1.75rem;
  }
  
  h2 {
    font-size: 1.5rem;
  }
  
  h3 {
    font-size: 1.25rem;
  }
}

/* Utility */
.container {
  max-width: 900px;
  margin: 0 auto;
}
"
))
