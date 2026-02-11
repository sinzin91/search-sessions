# Security Policy

## Reporting Vulnerabilities

If you discover a security vulnerability, please report it responsibly:

**GitHub Security Advisory:** [Create a private security advisory](https://github.com/sinzin91/search-sessions/security/advisories/new)

This is the preferred method — it allows for private discussion and coordinated disclosure.

### What to Include

- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix (optional)

### Response Timeline

This is an open source project maintained on a best-effort basis. There are no SLAs. I'll acknowledge reports as soon as possible and work to address confirmed vulnerabilities.

## Scope

This tool reads local session files from your machine. It does not:
- Make network requests
- Upload or transmit session data
- Require authentication

Security concerns are primarily around:
- Path traversal in file operations
- Information disclosure in error messages
- Dependency vulnerabilities
