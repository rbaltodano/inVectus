# Security

inVectus is pre-release research software. There is no stable-release support
matrix or guaranteed security response time yet.

Please do not report exploitable vulnerabilities, credentials or private project
data in public issues. Use the repository's GitHub **Security → Report a vulnerability**
option if private vulnerability reporting is enabled. If that option is unavailable,
contact the maintainer through their [GitHub profile](https://github.com/rbaltodano)
to arrange a private channel before sharing sensitive details.

A useful report includes the affected commit, platform, reproducible steps, impact
and a minimal synthetic example. Redact tokens, personal file paths, collection text
and decision notes. Do not attach model files or a private project database.

Core processing is local, but offline operation alone does not make arbitrary input
safe. Model files and native dependencies remain part of the software's trust boundary.
Use verified model sources and review changes before running a fork.
