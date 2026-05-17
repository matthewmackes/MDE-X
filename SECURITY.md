# Security Policy

## Supported versions

| Version | Supported |
|---|---|
| 1.x (latest) | ✅ Security fixes |
| < 1.0 | ❌ No support |

The latest minor release on the `main` branch receives security fixes.
Older minors may not. Always upgrade to the latest tagged release before
filing a security report.

## Reporting a vulnerability

**Please do not open a public GitHub issue for security vulnerabilities.**

Instead, email **matthewmackes@gmail.com** with:

- A description of the vulnerability
- Reproduction steps (proof-of-concept if possible)
- The affected version(s)
- Your assessment of severity and any suggested mitigation
- Whether you'd like to be credited in the release notes

You should expect:

- An acknowledgement within **3 business days**
- An assessment + remediation timeline within **10 business days**
- A coordinated public disclosure once a fix is shipped

Critical vulnerabilities (remote code execution, privilege escalation
beyond what Mackes' admin-session model already authorizes) will be
patched in a point release with a CVE if applicable.

## Threat model

Mackes Shell is a system-administration tool that holds elevated
privileges (root, via sudo timestamp cache) for the duration of its
unlocked session. The threat model assumes:

- A trusted local user (the operator running Mackes).
- Untrusted network peers — the mesh provides confidentiality + identity
  but not trust transitivity (peers can do anything to peers that the
  ACL allows; the ACL is the perimeter).
- Untrusted package contents fetched at install time (Guacamole `.war`,
  RPM Fusion repos, Flathub apps) are validated by their respective
  signing keys / hashes.

Out of scope:

- Vulnerabilities in upstream packages (xfce4, ansible-core, headscale,
  etc.) — report those upstream.
- Local privilege escalation between mackes-shell users on the same
  machine — this is the OS's perimeter, not ours.

## Hardening tips

- Lock the admin session when stepping away (click the header
  Lock/Unlock button or close Mackes; the session auto-locks on close).
- Keep `mackes-ansible-pull.timer` enabled so configuration drift is
  corrected within 30 min.
- Audit `~/QNM-Shared/.qnm-sync/ansible-runs/` for unexpected pull
  results.
- Run `mackes status` regularly to check the per-peer posture.
