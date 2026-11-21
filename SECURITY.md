# Security policy

Dede TV Core is pre-release and must not be exposed directly to the internet.

Security reports: use GitHub private vulnerability reporting if enabled, or
contact the repository owner privately. Do not publish working exploits
before coordination.

Boundaries:
- This scheduler does not implement authentication or serve HTTP traffic.
- Administrative APIs must independently authenticate and authorize requests.
- Media folders must be trusted and mounted with minimal permissions.
- External metadata extraction (ffprobe) must impose resource/time limits.
- The program is currently a prototype and has not been independently audited.
